#!/usr/bin/env python3
"""Verify the instrument-package boundary of a RackForge release edition."""

from __future__ import annotations

import argparse
import sys
import tarfile
import zipfile
from pathlib import Path, PurePosixPath


def plugin_names_from_archive(path: Path) -> list[str]:
    if path.suffix.lower() in {".apk", ".zip"}:
        with zipfile.ZipFile(path) as archive:
            members = archive.namelist()
    elif path.name.lower().endswith((".tar.gz", ".tgz")):
        with tarfile.open(path, "r:gz") as archive:
            members = archive.getnames()
    else:
        raise ValueError(f"unsupported release archive: {path}")
    return [
        PurePosixPath(member.replace("\\", "/")).name
        for member in members
        if member.lower().endswith(".rfplugin")
    ]


def plugin_names_from_manifest(path: Path) -> list[str]:
    return [
        Path(line.strip()).name
        for line in path.read_text(encoding="utf-8-sig").splitlines()
        if line.strip()
    ]


def verify(edition: str, actual: list[str], expected: list[str]) -> None:
    duplicates = sorted({name for name in actual if actual.count(name) > 1})
    if duplicates:
        raise ValueError(f"duplicate plugin packages: {', '.join(duplicates)}")

    actual_set = set(actual)
    if edition == "minimal":
        if expected:
            raise ValueError("Minimal verification cannot declare expected plugins")
        if actual_set:
            raise ValueError(
                "Minimal artifact contains plugin packages: "
                + ", ".join(sorted(actual_set))
            )
        return

    expected_set = set(expected)
    if not expected_set:
        raise ValueError("Standard verification requires expected plugin packages")
    missing = sorted(expected_set - actual_set)
    unexpected = sorted(actual_set - expected_set)
    if missing or unexpected:
        details = []
        if missing:
            details.append("missing: " + ", ".join(missing))
        if unexpected:
            details.append("unexpected: " + ", ".join(unexpected))
        raise ValueError("Standard plugin set does not match (" + "; ".join(details) + ")")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--edition", choices=("standard", "minimal"), required=True)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--archive", type=Path)
    source.add_argument("--manifest", type=Path)
    parser.add_argument("--expect-plugin", action="append", default=[])
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    source = args.archive or args.manifest
    if not source.is_file():
        print(f"release edition source does not exist: {source}", file=sys.stderr)
        return 2
    try:
        actual = (
            plugin_names_from_archive(source)
            if args.archive
            else plugin_names_from_manifest(source)
        )
        verify(args.edition, actual, args.expect_plugin)
    except (OSError, ValueError, tarfile.TarError, zipfile.BadZipFile) as error:
        print(f"release edition verification failed: {error}", file=sys.stderr)
        return 1
    print(
        f"RELEASE_EDITION_OK edition={args.edition} "
        f"plugins={','.join(sorted(actual)) or 'none'} source={source}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
