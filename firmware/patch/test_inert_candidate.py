import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

from inert_candidate import MARKER, MARKER_ADDRESS, build_inert_candidate
from patch_firmware import (
    APPLICATION_BASE,
    HEADER_LEN,
    CandidateError,
    parse_image,
)

STOCK = (
    ROOT.parent
    / "recovery"
    / "stock-official-1.2.1"
    / "keylab-essential-61-mk3.bin"
)


class InertCandidateTests(unittest.TestCase):
    def test_changes_only_marker_in_existing_payload(self):
        stock = STOCK.read_bytes()
        candidate, manifest = build_inert_candidate(stock)
        marker_offset = HEADER_LEN + MARKER_ADDRESS - APPLICATION_BASE

        self.assertEqual(len(candidate), len(stock))
        self.assertEqual(candidate[:HEADER_LEN], stock[:HEADER_LEN])
        self.assertEqual(candidate[marker_offset : marker_offset + len(MARKER)], MARKER)
        self.assertEqual(
            stock[marker_offset : marker_offset + len(MARKER)],
            b"\xFF" * len(MARKER),
        )
        self.assertEqual(candidate[:marker_offset], stock[:marker_offset])
        self.assertEqual(
            candidate[marker_offset + len(MARKER) :],
            stock[marker_offset + len(MARKER) :],
        )
        self.assertEqual(manifest["changed_byte_count"], len(MARKER))
        self.assertEqual(
            manifest["status"],
            "OFFLINE_VALIDATED_NOT_HARDWARE_TESTED_DO_NOT_FLASH",
        )
        self.assertFalse(manifest["executable_code_changed"])
        self.assertFalse(manifest["control_flow_changed"])
        self.assertFalse(manifest["payload_checksum_valid"])
        self.assertEqual(manifest["stored_payload_checksum"], "0xD9")
        self.assertEqual(manifest["required_payload_checksum"], "0x1C")
        with self.assertRaisesRegex(CandidateError, "payload additive checksum"):
            parse_image(candidate)

    def test_rejects_unpinned_input(self):
        stock = bytearray(STOCK.read_bytes())
        stock[-1] ^= 1
        with self.assertRaises(CandidateError):
            build_inert_candidate(bytes(stock))


if __name__ == "__main__":
    unittest.main()
