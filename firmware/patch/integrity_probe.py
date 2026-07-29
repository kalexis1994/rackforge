"""Build a checksum-correct, non-executable firmware integrity probe.

This probe changes a marker inside a verified erased-flash gap and refreshes
both Arturia additive checksums. It does not alter vectors, instructions, the
dispatcher, or control flow. The output is an offline artifact, not a flasher.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from patch_firmware import (
    APPLICATION_BASE,
    HEADER_LEN,
    PID_61,
    STOCK_SHA256,
    VID,
    CandidateError,
    parse_image,
    refresh_integrity_checksums,
    sha256,
)

MARKER_ADDRESS = 0x08035CC0
MARKER = b"RACKFORGE-CKSUM-v1\x00"
ERASED_GAP_START = 0x08035C0C
ERASED_GAP_END = 0x08035D98
STATUS = "OFFLINE_VALIDATED_NOT_HARDWARE_TESTED_DO_NOT_FLASH"


def build_integrity_probe(stock: bytes) -> tuple[bytes, dict]:
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
        raise CandidateError("probe marker is outside the verified erased gap")

    marker_offset = HEADER_LEN + MARKER_ADDRESS - APPLICATION_BASE
    marker_end = marker_offset + len(MARKER)
    original = stock[marker_offset:marker_end]
    if original != b"\xFF" * len(MARKER):
        raise CandidateError("probe marker target is not erased flash")

    header = bytearray(stock[:HEADER_LEN])
    payload = bytearray(stock[HEADER_LEN:])
    payload_marker_offset = marker_offset - HEADER_LEN
    payload[payload_marker_offset : payload_marker_offset + len(MARKER)] = MARKER
    refresh_integrity_checksums(header, payload)
    candidate = bytes(header + payload)
    candidate_info = parse_image(candidate)

    changed_offsets = [
        index for index, (before, after) in enumerate(zip(stock, candidate)) if before != after
    ]
    expected_changed_offsets = {
        0x0A,
        0x3F,
        *range(marker_offset, marker_end),
    }
    if set(changed_offsets) != expected_changed_offsets:
        raise CandidateError("probe changed bytes outside checksums and marker")
    if len(candidate) != len(stock):
        raise CandidateError("probe unexpectedly changed the image length")

    manifest = {
        "status": STATUS,
        "purpose": "Minimal checksum-correct no-code-change boot probe",
        "stock_sha256": sha256(stock),
        "candidate_sha256": sha256(candidate),
        "image_length": len(candidate),
        "executable_code_changed": False,
        "control_flow_changed": False,
        "changed_byte_count": len(changed_offsets),
        "integrity": {
            "payload_checksum_valid": candidate_info["payload_checksum_valid"],
            "stock_payload_checksum": f"0x{stock_info['payload_checksum']:02X}",
            "candidate_payload_checksum": (
                f"0x{candidate_info['payload_checksum']:02X}"
            ),
            "header_checksum_valid": sum(candidate[:HEADER_LEN]) & 0xFF == 0,
        },
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

    candidate, manifest = build_integrity_probe(args.stock.read_bytes())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(candidate)
    args.manifest.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
