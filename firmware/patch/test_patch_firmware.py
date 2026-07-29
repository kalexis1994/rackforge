from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).parent))

from patch_firmware import (  # noqa: E402
    APPLICATION_BASE,
    CandidateError,
    EXPECTED_HOOK_BYTES,
    HEADER_LEN,
    HOOK_ADDRESS,
    PATCH_ADDRESS,
    PAYLOAD_CHECKSUM_OFFSET,
    STOCK_SHA256,
    build_candidate,
    expected_payload_checksum,
    parse_image,
    sha256,
)


REPO_ROOT = Path(__file__).resolve().parents[2]
STOCK = (
    REPO_ROOT
    / "firmware"
    / "recovery"
    / "stock-official-1.2.1"
    / "keylab-essential-61-mk3.bin"
)


class FirmwarePatcherTests(unittest.TestCase):
    def test_verified_stock_image_matches_pinned_hash(self) -> None:
        stock = STOCK.read_bytes()
        self.assertEqual(sha256(stock), STOCK_SHA256)
        info = parse_image(stock)
        self.assertEqual(info["pid"], 0x028C)

    def test_candidate_updates_only_hook_gap_code_and_header(self) -> None:
        stock = STOCK.read_bytes()
        hook_site = bytes.fromhex("00 F0 00 B8")
        hook_code = bytes.fromhex("70 47")
        candidate, manifest = build_candidate(stock, hook_site, hook_code)
        info = parse_image(candidate)

        hook_offset = HEADER_LEN + HOOK_ADDRESS - APPLICATION_BASE
        patch_offset = HEADER_LEN + PATCH_ADDRESS - APPLICATION_BASE
        self.assertEqual(candidate[hook_offset : hook_offset + 4], hook_site)
        self.assertEqual(candidate[patch_offset : patch_offset + 2], hook_code)
        self.assertEqual(info["payload_len"], len(candidate) - HEADER_LEN)
        self.assertTrue(info["payload_checksum_valid"])
        self.assertEqual(
            candidate[PAYLOAD_CHECKSUM_OFFSET],
            expected_payload_checksum(candidate[HEADER_LEN:]),
        )
        self.assertEqual(sum(candidate[:HEADER_LEN]) & 0xFF, 0)
        self.assertEqual(manifest["status"], "REJECTED_ON_HARDWARE_DO_NOT_FLASH")

        stock_payload = stock[HEADER_LEN:]
        candidate_payload = candidate[HEADER_LEN:]
        self.assertEqual(
            candidate_payload[: HOOK_ADDRESS - APPLICATION_BASE],
            stock_payload[: HOOK_ADDRESS - APPLICATION_BASE],
        )
        self.assertEqual(
            candidate_payload[
                HOOK_ADDRESS - APPLICATION_BASE + 4 : len(stock_payload)
            ],
            stock_payload[HOOK_ADDRESS - APPLICATION_BASE + 4 :],
        )

    def test_rejects_an_unrecognized_stock_hook(self) -> None:
        stock = bytearray(STOCK.read_bytes())
        offset = HEADER_LEN + HOOK_ADDRESS - APPLICATION_BASE
        self.assertEqual(stock[offset : offset + 4], EXPECTED_HOOK_BYTES)
        stock[offset] ^= 1
        with self.assertRaises(CandidateError):
            build_candidate(bytes(stock), b"\x00" * 4, b"\x70\x47")

    def test_rejects_payload_with_stale_checksum(self) -> None:
        stock = bytearray(STOCK.read_bytes())
        stock[-1] ^= 1
        with self.assertRaisesRegex(CandidateError, "payload additive checksum"):
            parse_image(bytes(stock))


if __name__ == "__main__":
    unittest.main()
