"""Assemble a RackForge release from the artifacts of one 'Build main artifacts' run.

    python tools/assemble-release.py <run-id> <tag> <notes.md> [--notes-header] [--publish]

Downloads every artifact of the run, renames them to the asset names the
previous releases used, zips the VST3 bundles, writes SHA256SUMS.txt, copies
THIRD_PARTY_NOTICES.md, and (with --publish) creates the GitHub release.

With --notes-header, <notes.md> carries only what changed in this release and
the opening — commit, editions, and the bundled package list with versions —
is written from the pins and checked against the artifact being published.

Everything above happens in main(); importing this file does nothing, so the
checks it performs can be tested without a run to download.
"""
import hashlib
import importlib.util
import io
import os
import re
import shutil
import subprocess
import sys
import tarfile
import zipfile

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def edition_suffix(name: str) -> str:
    return "-Minimal" if "-Minimal-" in name else ""


def zip_dir(source: str, target: str, extra: dict[str, str]) -> None:
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as z:
        for root, _dirs, files in os.walk(source):
            for file in files:
                path = os.path.join(root, file)
                z.write(path, os.path.relpath(path, source))
        present = set(z.namelist())
        for arcname, path in extra.items():
            if arcname not in present:
                z.write(path, arcname)


def official_pins() -> tuple:
    """The packages the build was told to carry, read from the pins themselves."""
    path = os.path.join(REPO, "tools", "fetch-official-plugins.py")
    spec = importlib.util.spec_from_file_location("rackforge_official_pins", path)
    if spec is None or spec.loader is None:
        raise SystemExit(f"cannot read the official plugin pins at {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.OFFICIAL_PLUGINS


def upstream_repository(url: str) -> str:
    match = re.match(r"(https://github\.com/[^/]+/[^/]+)/releases/download/", url)
    if not match:
        raise SystemExit(f"cannot read a repository from {url}")
    return match.group(1)


def packaged_plugins(archive: str) -> dict[str, str]:
    """Every .rfplugin inside a release archive, and the version it declares."""
    found = {}
    with tarfile.open(archive, "r:gz") as tar:
        for member in tar.getmembers():
            if not member.name.endswith(".rfplugin"):
                continue
            source = tar.extractfile(member)
            if source is None:
                continue
            with zipfile.ZipFile(io.BytesIO(source.read())) as package:
                manifest = package.read("rackforge-plugin.toml").decode("utf-8")
            declared = re.search(r'^\s*version\s*=\s*"([^"]+)"\s*$', manifest, re.MULTILINE)
            if not declared:
                raise SystemExit(f"{member.name} declares no version")
            found[os.path.basename(member.name)] = declared.group(1)
    return found


def check_pins_against_artifact(pins: tuple, assets: str) -> None:
    """Refuses to describe a release as carrying something it does not carry.

    The opening of these notes is a list of versions, and until now it was
    copied in by hand from one release to the next. A version list is the
    kind of claim nobody re-reads: it is believed, and it is what a player
    checks their install against. So it is written from the pins and then
    read back out of the package the build actually produced.

    One Standard archive is enough. Every platform bundles the packages the
    same fetch produced, and CI has already verified that each archive holds
    the pinned set by name; what is missing from that check, and what this
    adds, is the version inside each package.
    """
    archive = os.path.join(assets, "RackForge-Linux-x86_64.tar.gz")
    if not os.path.isfile(archive):
        raise SystemExit("--notes-header needs the Standard Linux archive to check against")
    packaged = packaged_plugins(archive)
    if "RF-Concert-Grand.rfplugin" not in packaged:
        raise SystemExit("the Standard archive carries no Concert Grand")
    for plugin in pins:
        name, pinned = str(plugin["filename"]), str(plugin["version"])
        if name not in packaged:
            raise SystemExit(f"the Standard archive is missing {name}, which the pins carry")
        if packaged[name] != pinned:
            raise SystemExit(
                f"{name} is {packaged[name]} in the archive and {pinned} in the pins"
            )
    unexpected = sorted(
        set(packaged) - {str(p["filename"]) for p in pins} - {"RF-Concert-Grand.rfplugin"}
    )
    if unexpected:
        raise SystemExit("the Standard archive carries unpinned packages: " + ", ".join(unexpected))


def run_commit(run_id: str) -> str:
    """The commit the artifacts were built from, according to the run itself."""
    return subprocess.run(
        ["gh", "run", "view", run_id, "--json", "headSha", "-q", ".headSha"],
        check=True,
        capture_output=True,
        text=True,
        cwd=REPO,
    ).stdout.strip()[:7]


def notes_header(pins: tuple, tag: str, commit: str) -> str:
    bundled = [
        f"[{str(plugin['filename']).removesuffix('.rfplugin')} {plugin['version']}]"
        f"({upstream_repository(str(plugin['url']))})"
        for plugin in pins
    ]
    return "\n".join(
        [
            f"RackForge {tag} Preview, built and verified by GitHub Actions from commit `{commit}`.",
            "",
            "Standard carries Concert Grand and every officially pinned plugin; Minimal carries"
            " none and is otherwise the same host. Both are published for all five platforms —"
            " see `docs/RELEASE_EDITIONS.md`.",
            "",
            "Bundled in Standard: Concert Grand from this repository, "
            + ", ".join(bundled[:-1])
            + f" and {bundled[-1]}."
            " Each is pinned by version, URL and SHA-256 and verified inside the generated"
            " Android, Linux and Raspberry Pi distributions. Every Minimal artifact was checked"
            " to contain no plugin package at all.",
            "",
        ]
    )


def main() -> None:
    run_id, tag, notes = sys.argv[1], sys.argv[2], sys.argv[3]
    publish = "--publish" in sys.argv
    generate_header = "--notes-header" in sys.argv
    work = os.path.join(os.environ["TEMP"], f"rf-release-{tag}")
    downloads = os.path.join(work, "artifacts")
    assets = os.path.join(work, "assets")
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(downloads)
    os.makedirs(assets)

    subprocess.run(["gh", "run", "download", run_id, "-D", downloads], check=True, cwd=REPO)

    notices = os.path.join(REPO, "THIRD_PARTY_NOTICES.md")
    for artifact in sorted(os.listdir(downloads)):
        folder = os.path.join(downloads, artifact)
        suffix = edition_suffix(artifact)
        if artifact.startswith("RackForge-VST3-Windows-x86_64"):
            # The bundle directory plus the loose files, with the notices, as before.
            zip_dir(folder, os.path.join(assets, f"RackForge-VST3-Windows-x86_64{suffix}.zip"), {"THIRD_PARTY_NOTICES.md": notices})
        elif artifact.startswith("RackForge-Windows-x86_64"):
            shutil.copy2(os.path.join(folder, "rackforge.exe"), os.path.join(assets, f"RackForge-Windows-x86_64{suffix}.exe"))
        elif artifact.startswith("RackForge-Linux-x86_64"):
            shutil.copy2(os.path.join(folder, "RackForge-Linux-x86_64.tar.gz"), os.path.join(assets, f"RackForge-Linux-x86_64{suffix}.tar.gz"))
        elif artifact.startswith("RackForge-RaspberryPi-arm64"):
            shutil.copy2(os.path.join(folder, "RackForge-RaspberryPi-arm64.tar.gz"), os.path.join(assets, f"RackForge-RaspberryPi-arm64{suffix}.tar.gz"))
        elif artifact.startswith("RackForge-Android-arm64"):
            shutil.copy2(os.path.join(folder, "RackForge-debug.apk"), os.path.join(assets, f"RackForge-Android-arm64{suffix}.apk"))
        else:
            print("skipping unknown artifact", artifact)

    shutil.copy2(notices, os.path.join(assets, "THIRD_PARTY_NOTICES.md"))
    with open(os.path.join(assets, "SHA256SUMS.txt"), "w", newline="\n") as sums:
        for name in sorted(os.listdir(assets)):
            if name in ("SHA256SUMS.txt", "THIRD_PARTY_NOTICES.md"):
                continue
            digest = hashlib.sha256(open(os.path.join(assets, name), "rb").read()).hexdigest()
            sums.write(f"{digest} {name}\n")

    for name in sorted(os.listdir(assets)):
        print(f"{os.path.getsize(os.path.join(assets, name)):>12} {name}")

    if generate_header:
        pins = official_pins()
        check_pins_against_artifact(pins, assets)
        composed = os.path.join(work, f"release-notes-{tag}.md")
        with open(composed, "w", encoding="utf-8", newline="\n") as file:
            file.write(notes_header(pins, tag, run_commit(run_id)))
            file.write("\n" + open(notes, encoding="utf-8").read().lstrip("\n"))
        notes = composed
        print("notes written to", notes)

    if publish:
        files = [os.path.join(assets, n) for n in sorted(os.listdir(assets))]
        subprocess.run(["gh", "release", "create", tag, "--target", "main", "--title", f"RackForge {tag} Preview", "--notes-file", notes, *files], check=True, cwd=REPO)
        print("published", tag)
    else:
        print("assets ready in", assets, "(no --publish)")


if __name__ == "__main__":
    main()
