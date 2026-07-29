"""Repack a validated 61-key candidate into the official .kle3 container."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import zipfile

from patch_firmware import PID_61, parse_image

OFFICIAL_PACKAGE_SHA256 = (
    "c57604bebb688f3c03d508ff5a24171fb142331edd64a1cdde815aad06dbfc93"
)
TARGET_PRODUCT_ID_DECIMAL = str(PID_61)
EXPECTED_ENTRIES = {
    "info.json",
    "Kle3_fw_1__fw1_2_1_746__2025_08_27.bin",
    "Kle3_fw_2__fw1_2_1_746__2025_08_27.bin",
    "Kle3_fw_3__fw1_2_1_746__2025_08_27.bin",
}
DEFAULT_STATUS = "REJECTED_ON_HARDWARE_DO_NOT_FLASH"
INTEGRITY_PROBE_STATUS = "PASSED_ON_HARDWARE_ARCHIVE_ONLY"
INTEGRITY_PROBE_SHA256 = (
    "d7a1a0d6d86a97f9d9b94fc6415d82504c3fb6465ea522c679faaaea829a78d8"
)
DISPLAY_HOOK_STATUS = "INSTALLED_ONCE_INEFFECTIVE_ARCHIVE_ONLY"
DISPLAY_HOOK_SHA256 = (
    "1566c0f9a6aa491d4251cea3f4955a4112de68a4505e7130642383adc7c937df"
)
RAW_SYSEX_HOOK_STATUS = "INSTALLED_ONCE_BROKE_MIDI_ARCHIVE_ONLY"
RAW_SYSEX_HOOK_SHA256 = (
    "00ce83922290bf11b69d872b5aa4e54a175dcd19f5772e86f40810a920446e29"
)
ALLOWED_STATUSES = {
    DEFAULT_STATUS,
    INTEGRITY_PROBE_STATUS,
    DISPLAY_HOOK_STATUS,
    RAW_SYSEX_HOOK_STATUS,
}


class PackageError(ValueError):
    pass


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def build_package(
    official_package: bytes,
    candidate: bytes,
    status: str = DEFAULT_STATUS,
) -> tuple[bytes, dict]:
    if status not in ALLOWED_STATUSES:
        raise PackageError(f"unsupported package status: {status}")
    if _sha256(official_package) != OFFICIAL_PACKAGE_SHA256:
        raise PackageError("input is not the pinned official 1.2.1 package")
    candidate_info = parse_image(candidate)
    if candidate_info["pid"] != PID_61:
        raise PackageError("candidate is not a 61-key firmware image")
    if (
        status == INTEGRITY_PROBE_STATUS
        and _sha256(candidate) != INTEGRITY_PROBE_SHA256
    ):
        raise PackageError("archived integrity probe does not match its pinned hash")
    if status == DISPLAY_HOOK_STATUS and _sha256(candidate) != DISPLAY_HOOK_SHA256:
        raise PackageError("archived display hook does not match its pinned hash")
    if (
        status == RAW_SYSEX_HOOK_STATUS
        and _sha256(candidate) != RAW_SYSEX_HOOK_SHA256
    ):
        raise PackageError("one-time raw SysEx hook does not match its pinned hash")

    from io import BytesIO

    source_buffer = BytesIO(official_package)
    output_buffer = BytesIO()
    with zipfile.ZipFile(source_buffer, "r") as source:
        names = set(source.namelist())
        if names != EXPECTED_ENTRIES:
            raise PackageError(f"unexpected .kle3 entries: {sorted(names)}")
        info = json.loads(source.read("info.json"))
        targets = [
            image["file_name"]
            for image in info.get("images", [])
            if image.get("productid") == TARGET_PRODUCT_ID_DECIMAL
        ]
        if len(targets) != 1:
            raise PackageError("info.json does not identify exactly one 61-key image")
        target = targets[0]

        with zipfile.ZipFile(output_buffer, "w") as output:
            for entry in source.infolist():
                data = candidate if entry.filename == target else source.read(entry.filename)
                cloned = zipfile.ZipInfo(entry.filename, entry.date_time)
                cloned.compress_type = entry.compress_type
                cloned.comment = entry.comment
                cloned.extra = entry.extra
                cloned.internal_attr = entry.internal_attr
                cloned.external_attr = entry.external_attr
                cloned.create_system = entry.create_system
                output.writestr(cloned, data)

    packaged = output_buffer.getvalue()
    with zipfile.ZipFile(BytesIO(packaged), "r") as check:
        if set(check.namelist()) != EXPECTED_ENTRIES:
            raise PackageError("generated package entry set changed")
        if check.read(target) != candidate:
            raise PackageError("generated package does not contain the candidate")
        with zipfile.ZipFile(BytesIO(official_package), "r") as source:
            for name in EXPECTED_ENTRIES - {target}:
                if check.read(name) != source.read(name):
                    raise PackageError(f"non-target package entry changed: {name}")

    manifest = {
        "status": status,
        "official_package_sha256": _sha256(official_package),
        "candidate_image_sha256": _sha256(candidate),
        "candidate_package_sha256": _sha256(packaged),
        "replaced_entry": target,
        "preserved_entries": sorted(EXPECTED_ENTRIES - {target}),
    }
    return packaged, manifest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--official-package", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument(
        "--status",
        choices=sorted(ALLOWED_STATUSES),
        default=DEFAULT_STATUS,
    )
    args = parser.parse_args()

    if (
        args.status == INTEGRITY_PROBE_STATUS
        and args.output.suffix.lower() == ".kle3"
    ):
        parser.error("the passed integrity probe may only use an archived extension")
    if args.status == DISPLAY_HOOK_STATUS and args.output.suffix.lower() == ".kle3":
        parser.error("the ineffective display hook may only use an archived extension")
    if args.status == RAW_SYSEX_HOOK_STATUS and args.output.suffix.lower() == ".kle3":
        parser.error("the raw SysEx hook broke MIDI and may only use an archived extension")

    packaged, manifest = build_package(
        args.official_package.read_bytes(),
        args.candidate.read_bytes(),
        args.status,
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(packaged)
    args.manifest.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
