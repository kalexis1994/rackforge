import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

from integrity_probe import (
    MARKER,
    MARKER_ADDRESS,
    STATUS,
    build_integrity_probe,
)
from patch_firmware import (
    APPLICATION_BASE,
    HEADER_LEN,
    HEADER_CHECKSUM_OFFSET,
    PAYLOAD_CHECKSUM_OFFSET,
    CandidateError,
    parse_image,
)

STOCK = (
    ROOT.parent
    / "recovery"
    / "stock-official-1.2.1"
    / "keylab-essential-61-mk3.bin"
)


class IntegrityProbeTests(unittest.TestCase):
    def test_changes_only_marker_and_two_checksums(self):
        stock = STOCK.read_bytes()
        candidate, manifest = build_integrity_probe(stock)
        marker_offset = HEADER_LEN + MARKER_ADDRESS - APPLICATION_BASE
        info = parse_image(candidate)

        changed = {
            index
            for index, (before, after) in enumerate(zip(stock, candidate))
            if before != after
        }
        expected = {
            PAYLOAD_CHECKSUM_OFFSET,
            HEADER_CHECKSUM_OFFSET,
            *range(marker_offset, marker_offset + len(MARKER)),
        }

        self.assertEqual(changed, expected)
        self.assertEqual(len(candidate), len(stock))
        self.assertEqual(candidate[marker_offset : marker_offset + len(MARKER)], MARKER)
        self.assertTrue(info["payload_checksum_valid"])
        self.assertEqual(sum(candidate[:HEADER_LEN]) & 0xFF, 0)
        self.assertEqual(manifest["status"], STATUS)
        self.assertEqual(manifest["changed_byte_count"], len(MARKER) + 2)
        self.assertFalse(manifest["executable_code_changed"])
        self.assertFalse(manifest["control_flow_changed"])

    def test_rejects_unpinned_input(self):
        stock = bytearray(STOCK.read_bytes())
        stock[-1] ^= 1
        with self.assertRaises(CandidateError):
            build_integrity_probe(bytes(stock))


if __name__ == "__main__":
    unittest.main()
