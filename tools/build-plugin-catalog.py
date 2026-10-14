#!/usr/bin/env python3
"""Build RackForge's official plugin catalog from the plugins' GitHub releases.

Reads ``config/plugin-catalog.toml`` and writes ``<output>/v1/index.json`` in
the catalog v1 format of docs/architecture/plugin-repositories.md. Every
listed package is downloaded and checked -- a safe archive, the plugin it
claims to be, a portable component -- before its length and SHA-256 are
written down, because the signature over this file is a promise about those
bytes. Signing is a separate step (``rackforge-store sign``), so the secret
key never reaches this script.

A plugin whose releases cannot be read or checked is left out and reported;
``--strict`` makes that an error instead.
"""

from __future__ import annotations

import argparse
import datetime
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import sys
import tempfile
import tomllib
from typing import Callable
import urllib.request
import zipfile


ROOT = Path(__file__).resolve().parents[1]
_FETCH_SPEC = importlib.util.spec_from_file_location(
    "fetch_official_plugins", ROOT / "tools" / "fetch-official-plugins.py"
)
assert _FETCH_SPEC is not None and _FETCH_SPEC.loader is not None
_FETCH = importlib.util.module_from_spec(_FETCH_SPEC)
_FETCH_SPEC.loader.exec_module(_FETCH)
download = _FETCH.download
sha256 = _FETCH.sha256
manifest_value = _FETCH.manifest_value

CATALOG_SCHEMA_VERSION = 1
CONFIG_SCHEMA_VERSION = 1
SEMVER = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$")


class CatalogError(RuntimeError):
    pass


def version_key(version: str) -> tuple:
    """Semver precedence, enough for ordering a plugin's releases."""
    match = SEMVER.match(version)
    if not match:
        raise CatalogError(f"{version!r} is not a semantic version")
    major, minor, patch, pre = match.groups()
    # A release outranks any prerelease of the same version.
    pre_key: tuple = (1,) if pre is None else (0, *(
        (0, int(part)) if part.isdigit() else (1, part) for part in pre.split(".")
    ))
    return (int(major), int(minor), int(patch), pre_key)


def read_config(path: Path) -> dict:
    config = tomllib.loads(path.read_text(encoding="utf-8"))
    if config.get("schema_version") != CONFIG_SCHEMA_VERSION:
        raise CatalogError(f"unsupported catalog configuration schema in {path}")
    for field in ("repository_id", "name", "plugins"):
        if field not in config:
            raise CatalogError(f"{path} does not declare {field}")
    ids = [plugin["id"] for plugin in config["plugins"]]
    if len(ids) != len(set(ids)):
        raise CatalogError(f"{path} lists a plugin twice")
    return config


def github_releases(repository: str) -> list[dict]:
    """The releases a repository has published, newest first."""
    request = urllib.request.Request(
        f"https://api.github.com/repos/{repository}/releases?per_page=30",
        headers={
            "User-Agent": "RackForge-catalog/1",
            "Accept": "application/vnd.github+json",
        },
    )
    token = os.environ.get("GITHUB_TOKEN")
    if token:
        # Anonymous requests are rate limited hard enough to fail a busy day.
        request.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def inspect_package(path: Path, plugin_id: str) -> dict:
    """What a package says about itself, once it has proved it is `plugin_id`."""
    with zipfile.ZipFile(path) as archive:
        names = set()
        for info in archive.infolist():
            name = PurePosixPath(info.filename.replace("\\", "/"))
            if name.is_absolute() or ".." in name.parts:
                raise CatalogError(f"unsafe path in {path.name}: {info.filename}")
            names.add(name.as_posix().rstrip("/"))
        if "rackforge-plugin.toml" not in names:
            raise CatalogError(f"{path.name} has no rackforge-plugin.toml")
        manifest = archive.read("rackforge-plugin.toml").decode("utf-8")
        declared_id = manifest_value(manifest, "id")
        version = manifest_value(manifest, "version")
        if declared_id != plugin_id:
            raise CatalogError(f"{path.name} is {declared_id}, not {plugin_id}")
        version_key(version)
        # The catalog lists portable packages: one artifact every host can
        # install, native builds riding inside it for the hosts that run them.
        if not re.search(r"^\[component\]\s*$", manifest, re.MULTILINE):
            raise CatalogError(f"{path.name} has no portable component")
        if "metadata/runtime.json" in names:
            runtime = json.loads(archive.read("metadata/runtime.json"))
            if runtime.get("id") != declared_id or runtime.get("version") != version:
                raise CatalogError("runtime metadata does not match the plugin manifest")
        description = re.search(r'^description\s*=\s*"([^"]+)"\s*$', manifest, re.MULTILINE)
        license_id = None
        if "LICENSE" in names:
            spdx = re.search(
                r"SPDX-License-Identifier:\s*(\S+)",
                archive.read("LICENSE").decode("utf-8", errors="replace"),
            )
            license_id = spdx.group(1) if spdx else None
        return {
            "name": manifest_value(manifest, "name"),
            "version": version,
            "description": description.group(1) if description else None,
            "license": license_id,
        }


def plugin_entry(
    plugin: dict,
    keep: int,
    releases_of: Callable[[str], list[dict]],
    fetch: Callable[[str, Path], None],
) -> dict:
    """One plugin of the catalog: its `keep` newest stable releases."""
    listed: dict[str, dict] = {}
    newest: dict | None = None
    for release in releases_of(plugin["repository"]):
        if release.get("draft") or release.get("prerelease"):
            continue
        asset = next(
            (item for item in release.get("assets", []) if item.get("name") == plugin["asset"]),
            None,
        )
        if asset is None:
            continue
        url = str(asset["browser_download_url"])
        with tempfile.TemporaryDirectory(prefix="rackforge-catalog-") as temporary:
            staged = Path(temporary) / plugin["asset"]
            fetch(url, staged)
            package = inspect_package(staged, plugin["id"])
            digest = sha256(staged)
            size = staged.stat().st_size
        # A package's version is what its manifest says; a repository that
        # releases more than one plugin (RackForge itself, for Concert Grand)
        # tags its own version, not the plugin's.
        version = package["version"]
        if version in listed:
            continue
        listed[version] = {
            "version": version,
            "published_at": str(release.get("published_at") or release.get("created_at")),
            "artifacts": [
                {"platform": "wasm-v1", "url": url, "size": size, "sha256": digest}
            ],
        }
        if newest is None or version_key(version) > version_key(newest["version"]):
            newest = package
        if len(listed) == keep:
            break
    if newest is None:
        raise CatalogError(f"no release of {plugin['repository']} carries {plugin['asset']}")
    summary = plugin.get("summary") or newest["description"]
    license_id = plugin.get("license") or newest["license"]
    if not summary:
        raise CatalogError(f"{plugin['id']} needs a summary: its manifest has no description")
    if not license_id:
        raise CatalogError(f"{plugin['id']} needs a license: its package names no SPDX identifier")
    return {
        "id": plugin["id"],
        "name": newest["name"],
        "summary": summary,
        "license": license_id,
        "homepage": f"https://github.com/{plugin['repository']}",
        "releases": sorted(
            listed.values(), key=lambda item: version_key(item["version"]), reverse=True
        ),
    }


def build_catalog(
    config: dict,
    releases_of: Callable[[str], list[dict]] = github_releases,
    fetch: Callable[[str, Path], None] = download,
    now: datetime.datetime | None = None,
) -> tuple[dict, list[str]]:
    """The catalog, and the plugins that had to be left out of it."""
    keep = int(config.get("releases", 3))
    if keep < 1:
        raise CatalogError("a catalog lists at least one release of each plugin")
    plugins = []
    failures = []
    for plugin in config["plugins"]:
        try:
            plugins.append(plugin_entry(plugin, keep, releases_of, fetch))
            print(
                f"CATALOG_PLUGIN id={plugin['id']} "
                f"versions={','.join(item['version'] for item in plugins[-1]['releases'])}"
            )
        except Exception as error:  # Reported per plugin; see --strict.
            failures.append(f"{plugin['id']}: {error}")
    moment = (now or datetime.datetime.now(datetime.timezone.utc)).astimezone(
        datetime.timezone.utc
    )
    catalog = {
        "schema_version": CATALOG_SCHEMA_VERSION,
        "repository_id": config["repository_id"],
        "name": config["name"],
        "generated_at": moment.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "plugins": plugins,
    }
    return catalog, failures


def write_catalog(catalog: dict, output: Path) -> Path:
    destination = output / "v1" / "index.json"
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(catalog, indent=2) + "\n", encoding="utf-8")
    return destination


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--config", type=Path, default=ROOT / "config" / "plugin-catalog.toml")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--strict",
        action="store_true",
        help="fail when any plugin has to be left out of the catalog",
    )
    arguments = parser.parse_args()
    catalog, failures = build_catalog(read_config(arguments.config))
    for failure in failures:
        prefix = "::warning title=Plugin left out of the catalog::" if os.environ.get(
            "GITHUB_ACTIONS"
        ) else ""
        print(f"{prefix}CATALOG_PLUGIN_FAILED {failure}", file=sys.stderr)
    if not catalog["plugins"]:
        print("CATALOG_EMPTY every plugin failed", file=sys.stderr)
        return 1
    destination = write_catalog(catalog, arguments.output)
    print(f"CATALOG_WRITTEN path={destination} plugins={len(catalog['plugins'])}")
    return 1 if failures and arguments.strict else 0


if __name__ == "__main__":
    raise SystemExit(main())
