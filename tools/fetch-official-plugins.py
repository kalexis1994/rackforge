#!/usr/bin/env python3
"""Fetch and validate the external plugins shipped with RackForge builds.

Every artifact is pinned by URL, version, identity and SHA-256.  Builds never
consume a moving ``latest`` release: updating a bundled plugin is a reviewed
source change and remains reproducible after the upstream repository moves on.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import tempfile
import time
import urllib.request
import zipfile


MAX_ARCHIVE_BYTES = 512 * 1024 * 1024

OFFICIAL_PLUGINS = (
    {
        "filename": "RF-Concert-Grand.rfplugin",
        "plugin_id": "org.rackforge.concert-grand",
        "version": "0.171.39",
        "url": (
            "https://github.com/kalexis1994/rackforge-plugin-concert-grand/"
            "releases/download/v0.171.39/RF-Concert-Grand.rfplugin"
        ),
        "sha256": "9e8c5b6c9bc35e39d9c951ac02cd92dfb1fcde0c3c02ba82c9ea426014462d84",
        # The startup instrument is carried separately by native hosts.
        # Its source and release lifecycle are owned by the plugin repository.
        "default": True,
        "extract_directory": "concert-grand",
        "required": (
            "rackforge-plugin.toml", "component.wasm", "LICENSE", "NOTICE.md",
            "metadata/runtime.json", "metadata/parameters.json", "metadata/presets.json",
            "branding/icon.png", "branding/banner.png", "branding/splash.png", "web/play.html",
            "native/linux-aarch64/librackforge_concert_grand.so",
        ),
    },
    {
        "filename": "RF-106.rfplugin",
        "plugin_id": "org.rackforge.rf-106",
        "version": "0.2.21",
        "url": (
            "https://github.com/kalexis1994/rackforge-plugin-rf-106/"
            "releases/download/v0.2.21/RF-106.rfplugin"
        ),
        "sha256": "8f2cd0b97579abeb3941a5eb47f8a9c2926603a6b67fed025752c13552799295",
        "required": (
            "rackforge-plugin.toml",
            "component.wasm",
            "LICENSE",
            "NOTICE.md",
            "metadata/runtime.json",
            "metadata/parameters.json",
            "metadata/presets.json",
            "branding/icon.png",
            "branding/banner.png",
            "branding/splash.png",
            "web/play.html",
        ),
    },
    {
        "filename": "RF-5.rfplugin",
        "plugin_id": "org.rackforge.rf-5",
        "version": "0.1.22",
        "url": (
            "https://github.com/kalexis1994/rackforge-plugin-rf-5/"
            "releases/download/v0.1.22/RF-5.rfplugin"
        ),
        "sha256": "a0e4d02cb87e91b9f68d04c0d938de8ff7787fa15437ba927924cadbf443a5d3",
        "required": (
            "rackforge-plugin.toml",
            "component.wasm",
            "LICENSE",
            "NOTICE.md",
            "metadata/runtime.json",
            "metadata/parameters.json",
            "metadata/presets.json",
            "branding/icon.png",
            "branding/banner.png",
            "branding/splash.png",
            "web/play.html",
            "web/config.html",
            "web/app.js",
            "web/app_bg.wasm",
            "web/styles.css",
        ),
    },
    {
        "filename": "RF-7.rfplugin",
        "plugin_id": "org.rackforge.rf7",
        "version": "0.5.4",
        "url": (
            "https://github.com/kalexis1994/RF-7/"
            "releases/download/v0.5.4/RF-7.rfplugin"
        ),
        "sha256": "42097dafd6b6cd8eee71684c6522fa84a65333304f64f8193300e72fdef3dd18",
        "required": (
            "rackforge-plugin.toml",
            "component.wasm",
            "LICENSE",
            "NOTICE.md",
            "metadata/runtime.json",
            "metadata/parameters.json",
            "metadata/presets.json",
            "branding/icon.png",
            "branding/banner.png",
            "branding/splash.png",
            "web/play.html",
            "web/config.html",
            "web/app.js",
            "web/app_bg.wasm",
            "web/style.css",
        ),
    },
    {
        "filename": "RF-Organ.rfplugin",
        "plugin_id": "org.rackforge.organ",
        "version": "0.53.1",
        "url": (
            "https://github.com/kalexis1994/RF-Organ/"
            "releases/download/v0.53.1/RF-Organ.rfplugin"
        ),
        "sha256": "b8a08c563cbf0b161cff1879870f57a2927313776e854d2535f96886cf8e0e42",
        "required": (
            "rackforge-plugin.toml",
            "component.wasm",
            "LICENSE",
            "NOTICE.md",
            "metadata/runtime.json",
            "metadata/parameters.json",
            "metadata/presets.json",
            "branding/icon.png",
            "branding/banner.png",
            "branding/splash.png",
            "web/play.html",
            "web/bootstrap.js",
            "web/app.js",
            "web/app_bg.wasm",
            "web/style.css",
            "web/fonts/oswald.woff2",
            "web/fonts/playfair.woff2",
            "web/fonts/OFL.txt",
        ),
    },
    {
        "filename": "RF-Tines.rfplugin",
        "plugin_id": "org.rackforge.rftines",
        "version": "0.2.11",
        "url": (
            "https://github.com/kalexis1994/RF-Tines/"
            "releases/download/v0.2.11/RF-Tines.rfplugin"
        ),
        "sha256": "d7c51d892b82bd61d3579154f4bd9a25b4bac70a699c35ddb142059fb8211e71",
        "required": (
            "rackforge-plugin.toml",
            "component.wasm",
            "LICENSE",
            "NOTICE.md",
            "metadata/runtime.json",
            "metadata/parameters.json",
            "metadata/presets.json",
            "branding/icon.png",
            "branding/banner.png",
            "branding/splash.png",
            "web/play.html",
            "web/config.html",
            "web/app.js",
            "web/app_bg.wasm",
            "web/config.css",
            "web/config.mjs",
            "web/play-programs.mjs",
            "web/rftines-format.mjs",
            "web/style.css",
        ),
    },
    {
        "filename": "RF-EPiano.rfplugin",
        "plugin_id": "org.rackforge.rf-epiano",
        "version": "0.1.5",
        "url": (
            "https://github.com/kalexis1994/rackforge-plugin-rf-epiano/"
            "releases/download/v0.1.5/RF-EPiano.rfplugin"
        ),
        "sha256": "30e0fefc76991de8b25c099f1a8577d2473965a1509bd4272397a1f233009333",
        "required": (
            "rackforge-plugin.toml",
            "component.wasm",
            "metadata/runtime.json",
            "metadata/parameters.json",
            "metadata/presets.json",
            "branding/icon.png",
            "branding/banner.png",
            "branding/splash.png",
            "web/play.html",
            "web/play.js",
            "web/play.css",
            "notices/NOTICE.txt",
            "notices/OpenWurli-GPL-3.0.txt",
        ),
    },
    {
        "filename": "RF-Musette.rfplugin",
        "plugin_id": "org.rackforge.musette",
        "version": "0.13.26",
        "url": (
            "https://github.com/kalexis1994/RF-Musette/"
            "releases/download/v0.13.26/RF-Musette.rfplugin"
        ),
        "sha256": "0e54b3f7fcd4b5cc74f658d2a643b2b7c04659688c28b79babd5866ea2627e76",
        "required": (
            "rackforge-plugin.toml",
            "component.wasm",
            "LICENSE",
            "NOTICE.md",
            "metadata/runtime.json",
            "metadata/parameters.json",
            "metadata/presets.json",
            "metadata/control-layout.json",
            "branding/icon.png",
            "branding/banner.png",
            "branding/splash.png",
            "web/play.html",
            "web/config.html",
            "web/app.js",
            "web/app_bg.wasm",
            "web/styles.css",
            "web/fonts/OFL-GrandHotel.txt",
            "web/fonts/OFL-JosefinSans.txt",
        ),
    },
    {
        "filename": "RF-D.rfplugin",
        "plugin_id": "org.rackforge.lab.rf-d-voice",
        "version": "0.8.7",
        "url": (
            "https://github.com/kalexis1994/rackforge-plugin-rf-d/"
            "releases/download/v0.8.7/RF-D.rfplugin"
        ),
        "sha256": "4a50cac37b1df517df8a6fe6c8aba174e2f51290a5f821a8cda9513884d591a0",
        "required": (
            "rackforge-plugin.toml",
            "component.wasm",
            "metadata/control-layout.json",
            "metadata/runtime.json",
            "metadata/parameters.json",
            "metadata/presets.json",
            "branding/icon.png",
            "branding/banner.png",
            "branding/splash.png",
            "web/play.html",
            "web/config.html",
            "web/panel.js",
            "web/programs.js",
            "web/panel.css",
            "web/voice.css",
        ),
    },
    {
        "filename": "RF-Soundfonts.rfplugin",
        "plugin_id": "org.rackforge.rf-soundfonts",
        "version": "0.7.0",
        "url": (
            "https://github.com/kalexis1994/rackforge-plugin-rf-soundfonts/"
            "releases/download/v0.7.0/RF-Soundfonts.rfplugin"
        ),
        "sha256": "21c6f40c01f18b9a8f5cdda70a37cd15ef128440d5359d7e2c8efe42c9609c5b",
        # The demo downloads its extracted packages at startup. The factory
        # SoundFont expands to 118 MB and can be installed separately.
        "browser_demo": False,
        "required": (
            "rackforge-plugin.toml",
            "component.wasm",
            "LICENSE",
            "THIRD_PARTY_NOTICES.md",
            "licenses/YDP-Grand-Piano.txt",
            "licenses/Strait-OFL.txt",
            "assets/ydp-grand-piano.sf2",
            "metadata/runtime.json",
            "metadata/parameters.json",
            "metadata/presets.json",
            "branding/icon.png",
            "branding/banner.png",
            "branding/splash.png",
            "web/play.html",
            "web/config.html",
            "web/app.js",
            "web/styles.css",
            "web/Strait-Regular.ttf",
            "web/bank-presentations.json",
            "web/banks/ydp-grand-piano/presentation.json",
            "web/banks/ydp-grand-piano/artwork.png",
        ),
    },
    {
        "filename": "RF-Limiter.rfplugin",
        "plugin_id": "org.rackforge.rf-limiter",
        "version": "0.2.1",
        "url": (
            "https://github.com/kalexis1994/rackforge-plugin-rf-limiter/"
            "releases/download/v0.2.1/RF-Limiter.rfplugin"
        ),
        "sha256": "6a897c0b102071358fde3bcfb854c75d1616ed27f80ec4a63c6effa08f3c1b38",
        "required": (
            "rackforge-plugin.toml",
            "component.wasm",
            "LICENSE",
            "NOTICE.md",
            "metadata/runtime.json",
            "metadata/parameters.json",
            "metadata/presets.json",
            "branding/icon.png",
            "branding/banner.png",
            "branding/splash.png",
            "web/play.html",
            "web/play.js",
        ),
    },
    {
        "filename": "RF-EQ.rfplugin",
        "plugin_id": "org.rackforge.rf-eq",
        "version": "0.2.1",
        "url": (
            "https://github.com/kalexis1994/rackforge-plugin-rf-eq/"
            "releases/download/v0.2.1/RF-EQ.rfplugin"
        ),
        "sha256": "0e685ff712255f397f60c263fa8d51450a74d370bbe9a1ba5bd6728cc7541231",
        "required": (
            "rackforge-plugin.toml",
            "component.wasm",
            "LICENSE",
            "NOTICE.md",
            "metadata/runtime.json",
            "metadata/parameters.json",
            "metadata/presets.json",
            "branding/icon.png",
            "branding/banner.png",
            "branding/splash.png",
            "web/play.html",
            "web/play.js",
        ),
    },
    {
        "filename": "RF-Comp.rfplugin",
        "plugin_id": "org.rackforge.rf-comp",
        "version": "0.2.1",
        "url": (
            "https://github.com/kalexis1994/rackforge-plugin-rf-comp/"
            "releases/download/v0.2.1/RF-Comp.rfplugin"
        ),
        "sha256": "b0cb348660111bdfcc6600feed409da025f789ee25c4e9b5efe925fd5ffbc439",
        "required": (
            "rackforge-plugin.toml",
            "component.wasm",
            "LICENSE",
            "NOTICE.md",
            "metadata/runtime.json",
            "metadata/parameters.json",
            "metadata/presets.json",
            "branding/icon.png",
            "branding/banner.png",
            "branding/splash.png",
            "web/play.html",
            "web/play.js",
        ),
    },
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download(url: str, destination: Path) -> None:
    last_error: Exception | None = None
    for attempt in range(4):
        try:
            if attempt == 3:
                print(f"OFFICIAL_PLUGIN_API_FALLBACK url={url}", flush=True)
                request = release_asset_request(url)
            else:
                request = urllib.request.Request(
                    url, headers={"User-Agent": "RackForge-build/1"}
                )
            with urllib.request.urlopen(request, timeout=30) as response:
                length = response.headers.get("Content-Length")
                if length is not None and int(length) > MAX_ARCHIVE_BYTES:
                    raise RuntimeError("official plugin exceeds the package size limit")
                total = 0
                with destination.open("wb") as output:
                    while chunk := response.read(1024 * 1024):
                        total += len(chunk)
                        if total > MAX_ARCHIVE_BYTES:
                            raise RuntimeError("official plugin exceeds the package size limit")
                        output.write(chunk)
                if total == 0:
                    raise RuntimeError("official plugin download was empty")
            return
        except Exception as error:  # Network failures are retried uniformly.
            last_error = error
            destination.unlink(missing_ok=True)
            if attempt < 2:
                time.sleep(2 ** attempt)
    raise RuntimeError(f"could not download {url}: {last_error}")


def manifest_value(text: str, field: str) -> str:
    match = re.search(rf'^\s*{re.escape(field)}\s*=\s*"([^"]+)"\s*$', text, re.MULTILINE)
    if not match:
        raise RuntimeError(f"plugin manifest does not declare {field}")
    return match.group(1)


def validate_archive(path: Path, plugin: dict[str, object]) -> None:
    actual = sha256(path)
    expected = str(plugin["sha256"])
    if actual != expected:
        raise RuntimeError(f"checksum mismatch for {path.name}: expected {expected}, got {actual}")
    validate_contents(path, plugin, str(plugin["version"]))


def validate_contents(path: Path, plugin: dict[str, object], version: str) -> None:
    """Everything about a package except its checksum.

    Separate because an update has no hash to check against yet: it is
    computing the one the pin will carry. The package still has to be the
    plugin it claims to be, with every file the build depends on.
    """
    with zipfile.ZipFile(path) as archive:
        names = set()
        for info in archive.infolist():
            name = PurePosixPath(info.filename.replace("\\", "/"))
            if name.is_absolute() or ".." in name.parts:
                raise RuntimeError(f"unsafe path in {path.name}: {info.filename}")
            names.add(name.as_posix().rstrip("/"))
        for required in plugin["required"]:
            if required not in names:
                raise RuntimeError(f"{path.name} is missing {required}")

        manifest = archive.read("rackforge-plugin.toml").decode("utf-8")
        plugin_id = manifest_value(manifest, "id")
        declared = manifest_value(manifest, "version")
        if plugin_id != plugin["plugin_id"] or declared != version:
            raise RuntimeError(
                f"unexpected package identity {plugin_id} {declared}; "
                f"expected {plugin['plugin_id']} {version}"
            )
        runtime = json.loads(archive.read("metadata/runtime.json"))
        if runtime.get("id") != plugin_id or runtime.get("version") != declared:
            raise RuntimeError("runtime metadata does not match the plugin manifest")


def fetch(output: Path, plugin: dict[str, object]) -> None:
    destination = output / str(plugin["filename"])
    if destination.is_file() and sha256(destination) == plugin["sha256"]:
        validate_archive(destination, plugin)
        print(f"OFFICIAL_PLUGIN_READY path={destination} cached=true")
        return

    with tempfile.TemporaryDirectory(prefix="rackforge-official-plugin-") as temporary:
        staged = Path(temporary) / destination.name
        download(str(plugin["url"]), staged)
        validate_archive(staged, plugin)
        output.mkdir(parents=True, exist_ok=True)
        replacement = destination.with_suffix(destination.suffix + ".new")
        shutil.copyfile(staged, replacement)
        os.replace(replacement, destination)
    print(f"OFFICIAL_PLUGIN_READY path={destination} cached=false")


def extract(output: Path, destination_root: Path, plugin: dict[str, object]) -> None:
    archive_path = output / str(plugin["filename"])
    directory = str(plugin.get("extract_directory", plugin["plugin_id"]))
    if PurePosixPath(directory).name != directory or directory in ("", ".", "..") or "\\" in directory:
        raise RuntimeError(f"invalid plugin extraction directory: {directory}")
    destination = destination_root / directory
    if destination.exists():
        raise RuntimeError(f"official plugin destination already exists: {destination}")
    destination.mkdir(parents=True)
    with zipfile.ZipFile(archive_path) as archive:
        for info in archive.infolist():
            relative = PurePosixPath(info.filename.replace("\\", "/"))
            target = destination.joinpath(*relative.parts)
            if info.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(info) as source, target.open("wb") as sink:
                shutil.copyfileobj(source, sink)
    print(f"OFFICIAL_PLUGIN_EXTRACTED id={plugin['plugin_id']} path={destination}")


def upstream_repository(url: str) -> tuple[str, str]:
    """The owner and repository a pinned download URL belongs to."""
    match = re.match(r"https://github\.com/([^/]+)/([^/]+)/releases/download/", url)
    if not match:
        raise RuntimeError(f"cannot read a repository from {url}")
    return match.group(1), match.group(2)


def github_api_request(url: str, accept: str) -> urllib.request.Request:
    request = urllib.request.Request(
        url, headers={"User-Agent": "RackForge-build/1", "Accept": accept}
    )
    token = os.environ.get("GITHUB_TOKEN")
    if token:
        # The asset endpoint can redirect to a separate CDN origin.
        request.add_unredirected_header("Authorization", f"Bearer {token}")
    return request


def release_asset_request(url: str) -> urllib.request.Request:
    owner, repository = upstream_repository(url)
    tag = url.split("/releases/download/", 1)[1].split("/", 1)[0]
    releases = f"https://api.github.com/repos/{owner}/{repository}/releases"
    request = github_api_request(f"{releases}/tags/{tag}", "application/vnd.github+json")
    with urllib.request.urlopen(request, timeout=30) as response:
        release = json.load(response)
    for asset in release["assets"]:
        if asset["browser_download_url"] == url:
            asset_id = int(asset["id"])
            return github_api_request(f"{releases}/assets/{asset_id}", "application/octet-stream")
    raise RuntimeError(f"the release does not list {url}")


def latest_release(owner: str, repository: str) -> dict[str, object]:
    """What the upstream repository calls its newest release."""
    request = github_api_request(
        f"https://api.github.com/repos/{owner}/{repository}/releases/latest",
        "application/vnd.github+json",
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def released_version(release: dict[str, object]) -> str:
    return str(release.get("tag_name", "")).lstrip("v")


def check_latest() -> int:
    """Reports pins the upstream repositories have moved past.

    Pinning is what makes a build reproducible and a package verifiable, but
    a pin has no way to say that it has fallen behind: RackForge shipped an
    instrument its own plugin had already replaced, and the only place that
    showed was a player's screen. This is the missing voice. It changes
    nothing; it only says a newer release exists.
    """
    behind = 0
    for plugin in OFFICIAL_PLUGINS:
        owner, repository = upstream_repository(str(plugin["url"]))
        try:
            release = latest_release(owner, repository)
        except Exception as error:
            print(f"OFFICIAL_PLUGIN_CHECK_FAILED id={plugin['plugin_id']} error={error}")
            behind += 1
            continue
        newest = released_version(release)
        pinned = str(plugin["version"])
        if newest and newest != pinned:
            behind += 1
            print(
                f"OFFICIAL_PLUGIN_BEHIND id={plugin['plugin_id']} "
                f"pinned={pinned} released={newest} "
                f"url=https://github.com/{owner}/{repository}/releases/tag/v{newest}"
            )
        else:
            print(f"OFFICIAL_PLUGIN_CURRENT id={plugin['plugin_id']} version={pinned}")
    return behind


def rewrite_pin(
    source: Path,
    plugin: dict[str, object],
    version: str,
    url: str,
    digest: str,
) -> None:
    """Moves one plugin's pin, and only that plugin's."""
    text = source.read_text(encoding="utf-8")
    anchor = f'"plugin_id": "{plugin["plugin_id"]}"'
    start = text.index(anchor)
    end = text.index("\n    },", start)
    block = text[start:end]
    updated = re.sub(r'"version": "[^"]+"', f'"version": "{version}"', block, count=1)
    # Wrapped the way the pins above it are written: the repository on one
    # line, the release path on the next. A tool that reformats the file it
    # edits makes a diff nobody wants to read.
    split = url.find("/releases/")
    if split > 0:
        wrapped = (
            '"url": (\n'
            f'            "{url[: split + 1]}"\n'
            f'            "{url[split + 1 :]}"\n'
            "        )"
        )
    else:
        wrapped = '"url": (\n' f'            "{url}"\n' "        )"
    # The URL may be wrapped in parentheses or sit on one line; RF-7's did,
    # and a pattern for the wrapped form alone moved its version and digest
    # and left the URL on the old release.
    updated, urls = re.subn(
        r'"url": (?:\(\s*\n(?:\s*"[^"]*"\s*\n)+\s*\)|"[^"]*")',
        lambda _: wrapped,
        updated,
        count=1,
    )
    if urls != 1:
        raise RuntimeError(f"could not find the URL in the pin for {plugin['plugin_id']}")
    updated = re.sub(r'"sha256": "[^"]+"', f'"sha256": "{digest}"', updated, count=1)
    if updated == block:
        raise RuntimeError(f"could not rewrite the pin for {plugin['plugin_id']}")
    source.write_text(text[:start] + updated + text[end:], encoding="utf-8")


def update_pins(source: Path) -> None:
    """Moves every pin to the newest release its repository has published."""
    for plugin in OFFICIAL_PLUGINS:
        owner, repository = upstream_repository(str(plugin["url"]))
        release = latest_release(owner, repository)
        newest = released_version(release)
        if not newest or newest == str(plugin["version"]):
            print(
                f"OFFICIAL_PLUGIN_CURRENT id={plugin['plugin_id']} "
                f"version={plugin['version']}"
            )
            continue
        assets = {
            str(asset.get("name")): str(asset.get("browser_download_url"))
            for asset in release.get("assets", [])
        }
        url = assets.get(str(plugin["filename"]))
        if not url:
            raise RuntimeError(
                f"release v{newest} of {repository} carries no {plugin['filename']}"
            )
        with tempfile.TemporaryDirectory(prefix="rackforge-pin-") as temporary:
            staged = Path(temporary) / str(plugin["filename"])
            download(url, staged)
            # Checked before it is written down, never after: a pin is a
            # promise about bytes nobody has looked at yet.
            validate_contents(staged, plugin, newest)
            digest = sha256(staged)
        rewrite_pin(source, plugin, newest, url, digest)
        print(
            f"OFFICIAL_PLUGIN_PINNED id={plugin['plugin_id']} "
            f"{plugin['version']} -> {newest} sha256={digest}"
        )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--output-directory",
        type=Path,
        default=Path("dist/bundled-plugins/official"),
    )
    parser.add_argument("--extract-directory", type=Path)
    parser.add_argument("--plugin-id", action="append", default=[],
                        help="fetch only these pinned plugin IDs (repeatable)")
    parser.add_argument(
        "--browser-demo",
        action="store_true",
        help="fetch the packages loaded at browser demo startup",
    )
    parser.add_argument(
        "--check-latest",
        action="store_true",
        help="report pins the upstream repositories have moved past; changes nothing",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="move every pin to the newest published release, verifying it first",
    )
    arguments = parser.parse_args()
    unknown = set(arguments.plugin_id) - {str(p["plugin_id"]) for p in OFFICIAL_PLUGINS}
    if unknown:
        parser.error("unknown pinned plugin IDs: " + ", ".join(sorted(unknown)))
    if arguments.check_latest:
        raise SystemExit(1 if check_latest() else 0)
    if arguments.update:
        update_pins(Path(__file__).resolve())
        return
    output = arguments.output_directory.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for plugin in OFFICIAL_PLUGINS:
        if arguments.plugin_id and plugin["plugin_id"] not in arguments.plugin_id:
            continue
        if arguments.browser_demo and not plugin.get("browser_demo", True):
            continue
        # Keep the default package outside the other official packages:
        # existing hosts embed one default plus this directory exactly once.
        package_output = output.parent if plugin.get("default") else output
        fetch(package_output, plugin)
        if arguments.extract_directory is not None:
            extract(package_output, arguments.extract_directory.resolve(), plugin)


if __name__ == "__main__":
    main()
