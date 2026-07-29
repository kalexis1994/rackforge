"""Build a same-size, non-executable firmware diagnostic.

The diagnostic changes only a 19-byte marker inside a verified erased-flash
gap in the exact stock 1.2.1 image. It does not alter the header, image length,
vector table, executable instructions, dispatcher, or control flow.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from patch_firmware import (
    APPLICATION_BASE,
    HEADER_LEN,
    PID_61,
    PAYLOAD_CHECKSUM_OFFSET,
    STOCK_SHA256,
    VID,
    CandidateError,
    parse_image,
    sha256,
)

MARKER_ADDRESS = 0x08035CC0
MARKER = b"RACKFORGE-DIAG-v1\x00\x00"
ERASED_GAP_START = 0x08035C0C
ERASED_GAP_END = 0x08035D98


def build_inert_candidate(stock: bytes) -> tuple[bytes, dict]:
    stock_info = parse_image(stock)
    if sha256(stock) != STOCK_SHA256:
        raise CandidateError("input is not the exact verified stock 1.2.1 image")
    if stock_info["vid"] != VID or stock_info["pid"] != PID_61:
        raise CandidateError("input is not the 61-key KeyLab Essential mk3 image")
    if not (
        ERASED_GAP_START
        <= MARKER_ADDRESS
        < MARKER_ADDRESS + len(MARKER)
        <= ERASED_GAP_END
    ):
        raise CandidateError("diagnostic marker is outside the verified erased gap")

    marker_offset = HEADER_LEN + MARKER_ADDRESS - APPLICATION_BASE
    marker_end = marker_offset + len(MARKER)
    original = stock[marker_offset:marker_end]
    if original != b"\xFF" * len(MARKER):
        raise CandidateError("diagnostic marker target is not erased flash")

    candidate = bytearray(stock)
    candidate[marker_offset:marker_end] = MARKER
    candidate = bytes(candidate)
    candidate_info = parse_image(candidate, verify_payload_checksum=False)

    if candidate[:HEADER_LEN] != stock[:HEADER_LEN]:
        raise CandidateError("diagnostic unexpectedly changed the Arturia header")
    if len(candidate) != len(stock):
        raise CandidateError("diagnostic unexpectedly changed the image length")
    stable_metadata = (
        "vid",
        "pid",
        "payload_len",
        "application_base",
        "application_end",
    )
    if any(candidate_info[key] != stock_info[key] for key in stable_metadata):
        raise CandidateError("diagnostic unexpectedly changed parsed image metadata")

    changed_offsets = [
        index for index, (before, after) in enumerate(zip(stock, candidate)) if before != after
    ]
    expected_offsets = list(range(marker_offset, marker_end))
    if changed_offsets != expected_offsets:
        raise CandidateError("diagnostic changed bytes outside the marker")

    manifest = {
        "status": "OFFLINE_VALIDATED_NOT_HARDWARE_TESTED_DO_NOT_FLASH",
        "hardware_result": (
            "This renamed marker variant has not been installed; the pre-rename "
            "diagnostic failed and official 1.2.1 was restored"
        ),
        "purpose": "Archived payload-integrity diagnostic",
        "stock_sha256": sha256(stock),
        "candidate_sha256": sha256(candidate),
        "image_length": len(candidate),
        "header_identical_to_stock": True,
        "payload_checksum_valid": candidate_info["payload_checksum_valid"],
        "stored_payload_checksum": (
            f"0x{candidate[PAYLOAD_CHECKSUM_OFFSET]:02X}"
        ),
        "required_payload_checksum": (
            f"0x{candidate_info['calculated_payload_checksum']:02X}"
        ),
        "root_cause": (
            "The diagnostic preserves checksum 0xD9, but the RackForge marker "
            "requires 0x1C"
        ),
        "executable_code_changed": False,
        "control_flow_changed": False,
        "changed_byte_count": len(changed_offsets),
        "marker": {
            "address": f"0x{MARKER_ADDRESS:08X}",
            "file_offset": f"0x{marker_offset:08X}",
            "bytes": MARKER.hex(" ").upper(),
            "ascii": MARKER.rstrip(b"\x00").decode("ascii"),
            "original_bytes": original.hex(" ").upper(),
            "verified_erased_gap": (
                f"0x{ERASED_GAP_START:08X}..0x{ERASED_GAP_END - 1:08X}"
            ),
        },
    }
    return candidate, manifest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--stock", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()

    candidate, manifest = build_inert_candidate(args.stock.read_bytes())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(candidate)
    args.manifest.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
