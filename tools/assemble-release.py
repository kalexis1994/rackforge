"""Assemble a RackForge release from the artifacts of one 'Build main artifacts' run.

    python tools/assemble-release.py <run-id> <tag> <notes.md> [--publish]

Downloads every artifact of the run, renames them to the asset names the
previous releases used, zips the VST3 bundles, writes SHA256SUMS.txt, copies
THIRD_PARTY_NOTICES.md, and (with --publish) creates the GitHub release.
"""
import hashlib
import os
import shutil
import subprocess
import sys
import zipfile

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
run_id, tag, notes = sys.argv[1], sys.argv[2], sys.argv[3]
publish = "--publish" in sys.argv
work = os.path.join(os.environ["TEMP"], f"rf-release-{tag}")
downloads = os.path.join(work, "artifacts")
assets = os.path.join(work, "assets")
shutil.rmtree(work, ignore_errors=True)
os.makedirs(downloads)
os.makedirs(assets)

subprocess.run(["gh", "run", "download", run_id, "-D", downloads], check=True, cwd=REPO)


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

if publish:
    files = [os.path.join(assets, n) for n in sorted(os.listdir(assets))]
    subprocess.run(["gh", "release", "create", tag, "--target", "main", "--title", f"RackForge {tag} Preview", "--notes-file", notes, *files], check=True, cwd=REPO)
    print("published", tag)
else:
    print("assets ready in", assets, "(no --publish)")
