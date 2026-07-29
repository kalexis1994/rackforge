from io import BytesIO
from pathlib import Path
import sys
import unittest
import zipfile

sys.path.insert(0, str(Path(__file__).parent))

from package_candidate import (  # noqa: E402
    EXPECTED_ENTRIES,
    DISPLAY_HOOK_SHA256,
    DISPLAY_HOOK_STATUS,
    INTEGRITY_PROBE_SHA256,
    INTEGRITY_PROBE_STATUS,
    OFFICIAL_PACKAGE_SHA256,
    PackageError,
    RAW_SYSEX_HOOK_STATUS,
    build_package,
)
from patch_firmware import build_candidate, sha256  # noqa: E402
from integrity_probe import build_integrity_probe  # noqa: E402


REPO_ROOT = Path(__file__).resolve().parents[2]
RECOVERY = REPO_ROOT / "firmware" / "recovery" / "stock-official-1.2.1"
OFFICIAL_PACKAGE = RECOVERY / "keylab-essential-61-mk3_Firmware_Update_1.2.1.kle3"
STOCK = RECOVERY / "keylab-essential-61-mk3.bin"


class PackageCandidateTests(unittest.TestCase):
    def test_replaces_only_the_61_key_image(self) -> None:
        official = OFFICIAL_PACKAGE.read_bytes()
        self.assertEqual(sha256(official), OFFICIAL_PACKAGE_SHA256)
        candidate, _ = build_candidate(
            STOCK.read_bytes(), bytes.fromhex("00 F0 00 B8"), b"\x70\x47"
        )
        package, manifest = build_package(official, candidate)

        with zipfile.ZipFile(BytesIO(official), "r") as source:
            with zipfile.ZipFile(BytesIO(package), "r") as output:
                self.assertEqual(set(output.namelist()), EXPECTED_ENTRIES)
                target = manifest["replaced_entry"]
                self.assertEqual(output.read(target), candidate)
                for name in EXPECTED_ENTRIES - {target}:
                    self.assertEqual(output.read(name), source.read(name))

    def test_rejects_unknown_status(self) -> None:
        with self.assertRaises(PackageError):
            build_package(
                OFFICIAL_PACKAGE.read_bytes(),
                STOCK.read_bytes(),
                "UNSAFE_UNREVIEWED_STATUS",
            )

    def test_renamed_probe_cannot_claim_historical_hardware_status(self) -> None:
        candidate, _ = build_integrity_probe(STOCK.read_bytes())
        self.assertNotEqual(sha256(candidate), INTEGRITY_PROBE_SHA256)
        with self.assertRaisesRegex(PackageError, "pinned hash"):
            build_package(
                OFFICIAL_PACKAGE.read_bytes(),
                candidate,
                INTEGRITY_PROBE_STATUS,
            )

    def test_rejects_other_candidate_as_archived_integrity_probe(self) -> None:
        candidate, _ = build_candidate(
            STOCK.read_bytes(), bytes.fromhex("00 F0 00 B8"), b"\x70\x47"
        )
        with self.assertRaisesRegex(PackageError, "pinned hash"):
            build_package(
                OFFICIAL_PACKAGE.read_bytes(),
                candidate,
                INTEGRITY_PROBE_STATUS,
            )

    def test_packages_only_the_pinned_one_time_display_hook(self) -> None:
        candidate, _ = build_candidate(
            STOCK.read_bytes(),
            bytes.fromhex("0A F0 30 BC"),
            bytes.fromhex(
                "0B 4B 1B 68 98 47 00 BF"
            ),
        )
        self.assertNotEqual(sha256(candidate), DISPLAY_HOOK_SHA256)
        with self.assertRaisesRegex(PackageError, "pinned hash"):
            build_package(
                OFFICIAL_PACKAGE.read_bytes(),
                candidate,
                DISPLAY_HOOK_STATUS,
            )

    def test_rejects_an_unpinned_raw_sysex_hook(self) -> None:
        candidate, _ = build_candidate(
            STOCK.read_bytes(), bytes.fromhex("00 F0 00 B8"), b"\x70\x47"
        )
        with self.assertRaisesRegex(PackageError, "pinned hash"):
            build_package(
                OFFICIAL_PACKAGE.read_bytes(),
                candidate,
                RAW_SYSEX_HOOK_STATUS,
            )


if __name__ == "__main__":
    unittest.main()
