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
        "filename": "RF-106.rfplugin",
        "plugin_id": "org.rackforge.rf-106",
        "version": "0.2.12",
        "url": (
            "https://github.com/kalexis1994/rackforge-plugin-rf-106/"
            "releases/download/v0.2.12/RF-106.rfplugin"
        ),
        "sha256": "6fa0ece966773f6a56616b0fba9fe390b1a61e6b666627615d36a02d4e93e304",
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
        "version": "0.1.14",
        "url": (
            "https://github.com/kalexis1994/rackforge-plugin-rf-5/"
            "releases/download/v0.1.14/RF-5.rfplugin"
        ),
        "sha256": "a791b911e003b5b0c6fee451abdad63f26f88cb5eecaaa836dfdd96dbfe87662",
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
            "web/app.js",
            "web/app_bg.wasm",
            "web/styles.css",
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
    for attempt in range(3):
        try:
            request = urllib.request.Request(
                url,
                headers={"User-Agent": "RackForge-build/1"},
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
    destination = destination_root / str(plugin["plugin_id"])
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


def latest_release(owner: str, repository: str) -> dict[str, object]:
    """What the upstream repository calls its newest release."""
    request = urllib.request.Request(
        f"https://api.github.com/repos/{owner}/{repository}/releases/latest",
        headers={
            "User-Agent": "RackForge-build/1",
            "Accept": "application/vnd.github+json",
        },
    )
    token = os.environ.get("GITHUB_TOKEN")
    if token:
        # Anonymous requests are rate limited hard enough to fail a busy day.
        request.add_header("Authorization", f"Bearer {token}")
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
    updated = re.sub(
        r'"url": \(\s*\n(?:\s*"[^"]*"\s*\n)+\s*\)',
        lambda _: wrapped,
        updated,
        count=1,
    )
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
    if arguments.check_latest:
        raise SystemExit(1 if check_latest() else 0)
    if arguments.update:
        update_pins(Path(__file__).resolve())
        return
    output = arguments.output_directory.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for plugin in OFFICIAL_PLUGINS:
        fetch(output, plugin)
        if arguments.extract_directory is not None:
            extract(output, arguments.extract_directory.resolve(), plugin)


if __name__ == "__main__":
    main()
