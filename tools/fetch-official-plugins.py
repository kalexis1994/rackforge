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
        "version": "0.2.9",
        "url": (
            "https://github.com/kalexis1994/rackforge-plugin-rf-106/"
            "releases/download/v0.2.9/RF-106.rfplugin"
        ),
        "sha256": "32bc827c770471c50e13e4443059d44f7fb9b6c30da8c477a0b20266d5fb6815",
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
        version = manifest_value(manifest, "version")
        if plugin_id != plugin["plugin_id"] or version != plugin["version"]:
            raise RuntimeError(
                f"unexpected package identity {plugin_id} {version}; "
                f"expected {plugin['plugin_id']} {plugin['version']}"
            )
        runtime = json.loads(archive.read("metadata/runtime.json"))
        if runtime.get("id") != plugin_id or runtime.get("version") != version:
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


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--output-directory",
        type=Path,
        default=Path("dist/bundled-plugins/official"),
    )
    parser.add_argument("--extract-directory", type=Path)
    arguments = parser.parse_args()
    output = arguments.output_directory.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for plugin in OFFICIAL_PLUGINS:
        fetch(output, plugin)
        if arguments.extract_directory is not None:
            extract(output, arguments.extract_directory.resolve(), plugin)


if __name__ == "__main__":
    main()
