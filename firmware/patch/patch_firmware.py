"""Build and validate an offline RackForge firmware candidate.

This tool only reads and writes local files. It contains no USB, MIDI, DFU,
flash erase, download, reset, or bootloader operation.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
from pathlib import Path

STOCK_SHA256 = "819258d9efe53e5e5026489f097e3e0dc9f132fd051ee00d28614000d2965269"
HEADER_LEN = 64
PAYLOAD_CHECKSUM_OFFSET = 0x0A
HEADER_CHECKSUM_OFFSET = 0x3F
APPLICATION_BASE = 0x08007800
APPLICATION_LIMIT = 0x08040000
HOOK_ADDRESS = 0x0800B802
PATCH_ADDRESS = 0x08036B80
EXPECTED_HOOK_BYTES = bytes.fromhex("10 B5 FF F7")
VID = 0x1C75
PID_61 = 0x028C


class CandidateError(ValueError):
    pass


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def expected_payload_checksum(payload: bytes) -> int:
    """Return the Arturia 8-bit additive checksum stored at header offset 0x0A."""

    return (-sum(payload)) & 0xFF


def refresh_integrity_checksums(header: bytearray, payload: bytes) -> None:
    """Refresh the payload checksum and then the dependent header checksum."""

    if len(header) != HEADER_LEN:
        raise CandidateError("Arturia header must be exactly 64 bytes")
    header[PAYLOAD_CHECKSUM_OFFSET] = expected_payload_checksum(payload)
    header[HEADER_CHECKSUM_OFFSET] = 0
    header[HEADER_CHECKSUM_OFFSET] = (-sum(header)) & 0xFF


def parse_image(
    image: bytes, *, verify_payload_checksum: bool = True
) -> dict[str, int | bool]:
    if len(image) < HEADER_LEN:
        raise CandidateError("firmware image is shorter than the Arturia header")
    if sum(image[:HEADER_LEN]) & 0xFF:
        raise CandidateError("Arturia header additive checksum is invalid")

    payload = image[HEADER_LEN:]
    stored_payload_checksum = image[PAYLOAD_CHECKSUM_OFFSET]
    calculated_payload_checksum = expected_payload_checksum(payload)
    payload_checksum_valid = stored_payload_checksum == calculated_payload_checksum
    if verify_payload_checksum and not payload_checksum_valid:
        raise CandidateError(
            "Arturia payload additive checksum is invalid: "
            f"stored 0x{stored_payload_checksum:02X}, "
            f"expected 0x{calculated_payload_checksum:02X}"
        )

    vid, pid = struct.unpack_from("<HH", image, 0)
    payload_len = struct.unpack_from("<I", image, 6)[0]
    application_base = struct.unpack_from("<I", image, 12)[0]
    application_end = struct.unpack_from("<I", image, 16)[0]
    if payload_len != len(image) - HEADER_LEN:
        raise CandidateError(
            f"declared payload length {payload_len} does not match {len(image) - HEADER_LEN}"
        )
    if application_base != APPLICATION_BASE:
        raise CandidateError(f"unexpected application base 0x{application_base:08X}")
    if application_end >= APPLICATION_LIMIT:
        raise CandidateError(f"declared application end 0x{application_end:08X} is outside slot")
    return {
        "vid": vid,
        "pid": pid,
        "payload_len": payload_len,
        "application_base": application_base,
        "application_end": application_end,
        "payload_checksum": stored_payload_checksum,
        "calculated_payload_checksum": calculated_payload_checksum,
        "payload_checksum_valid": payload_checksum_valid,
    }


def build_candidate(stock: bytes, hook_site: bytes, hook_code: bytes) -> tuple[bytes, dict]:
    stock_info = parse_image(stock)
    if sha256(stock) != STOCK_SHA256:
        raise CandidateError("input is not the exact verified stock 1.2.1 image")
    if stock_info["vid"] != VID or stock_info["pid"] != PID_61:
        raise CandidateError("input is not the 61-key KeyLab Essential mk3 image")
    if len(hook_site) != 4:
        raise CandidateError("hook-site blob must be exactly four bytes")
    if not hook_code:
        raise CandidateError("hook code is empty")

    payload = bytearray(stock[HEADER_LEN:])
    hook_offset = HOOK_ADDRESS - APPLICATION_BASE
    patch_offset = PATCH_ADDRESS - APPLICATION_BASE
    if payload[hook_offset : hook_offset + 4] != EXPECTED_HOOK_BYTES:
        raise CandidateError("stock bytes at the dispatcher hook site do not match")
    if patch_offset < len(payload):
        raise CandidateError("patch origin overlaps the stock payload")
    if PATCH_ADDRESS + len(hook_code) > APPLICATION_LIMIT:
        raise CandidateError("patch code exceeds the application slot")

    payload[hook_offset : hook_offset + 4] = hook_site
    payload.extend(b"\xFF" * (patch_offset - len(payload)))
    payload.extend(hook_code)

    header = bytearray(stock[:HEADER_LEN])
    struct.pack_into("<I", header, 6, len(payload))
    refresh_integrity_checksums(header, payload)
    candidate = bytes(header + payload)
    candidate_info = parse_image(candidate)

    manifest = {
        "status": "REJECTED_ON_HARDWARE_DO_NOT_FLASH",
        "stock_sha256": sha256(stock),
        "candidate_sha256": sha256(candidate),
        "stock_payload_length": stock_info["payload_len"],
        "candidate_payload_length": candidate_info["payload_len"],
        "integrity": {
            "payload_checksum": f"0x{candidate_info['payload_checksum']:02X}",
            "header_checksum_valid": True,
            "payload_checksum_valid": candidate_info["payload_checksum_valid"],
        },
        "hook": {
            "address": f"0x{HOOK_ADDRESS:08X}",
            "stock_bytes": EXPECTED_HOOK_BYTES.hex(" ").upper(),
            "patched_bytes": hook_site.hex(" ").upper(),
        },
        "patch": {
            "address": f"0x{PATCH_ADDRESS:08X}",
            "length": len(hook_code),
            "sha256": sha256(hook_code),
            "end": f"0x{PATCH_ADDRESS + len(hook_code):08X}",
        },
    }
    return candidate, manifest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--stock", type=Path, required=True)
    parser.add_argument("--hook-site", type=Path, required=True)
    parser.add_argument("--hook-code", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()

    candidate, manifest = build_candidate(
        args.stock.read_bytes(),
        args.hook_site.read_bytes(),
        args.hook_code.read_bytes(),
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(candidate)
    args.manifest.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
