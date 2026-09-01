import importlib.util
import io
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "verify-release-edition.py"
SPEC = importlib.util.spec_from_file_location("verify_release_edition", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class ReleaseEditionVerificationTests(unittest.TestCase):
    def test_minimal_zip_accepts_no_plugins(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "RackForge.apk"
            with zipfile.ZipFile(archive, "w") as output:
                output.writestr("assets/index.html", "RackForge")
            self.assertEqual(MODULE.plugin_names_from_archive(archive), [])
            MODULE.verify("minimal", [], [])

    def test_minimal_rejects_a_plugin(self):
        with self.assertRaisesRegex(ValueError, "contains plugin packages"):
            MODULE.verify("minimal", ["RF-106.rfplugin"], [])

    def test_standard_requires_the_exact_plugin_set(self):
        expected = ["RF-Concert-Grand.rfplugin", "RF-106.rfplugin"]
        MODULE.verify("standard", list(reversed(expected)), expected)
        with self.assertRaisesRegex(ValueError, "missing: RF-106.rfplugin"):
            MODULE.verify("standard", expected[:1], expected)
        with self.assertRaisesRegex(ValueError, "unexpected: Extra.rfplugin"):
            MODULE.verify("standard", expected + ["Extra.rfplugin"], expected)

    def test_tar_reader_returns_plugin_basenames(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "RackForge.tar.gz"
            payload = b"package"
            with tarfile.open(archive, "w:gz") as output:
                info = tarfile.TarInfo("rackforge/bundled-plugins/RF-106.rfplugin")
                info.size = len(payload)
                output.addfile(info, io.BytesIO(payload))
            self.assertEqual(
                MODULE.plugin_names_from_archive(archive), ["RF-106.rfplugin"]
            )

    def test_manifest_ignores_blank_lines_and_bom(self):
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory) / "bundled-plugins.txt"
            manifest.write_text("\ufeffRF-106.rfplugin\n\n", encoding="utf-8")
            self.assertEqual(
                MODULE.plugin_names_from_manifest(manifest), ["RF-106.rfplugin"]
            )



class PinnedOfficialSetTests(unittest.TestCase):
    """The expected Standard set is derived, not retyped.

    The first version of this check named RF-106 by hand in five places, and
    by the time anyone ran it the builds also carried RF-5 — so the check
    would have rejected a correct artifact. The pins are the one list that
    already has to be right.
    """

    def test_the_expected_set_comes_from_the_pins(self):
        pinned = MODULE.pinned_official_filenames()
        self.assertTrue(pinned, "the build pins no official packages at all")
        for name in pinned:
            self.assertTrue(
                name.endswith(".rfplugin"),
                f"{name} is not a plugin package name",
            )

    def test_a_standard_artifact_matching_the_pins_passes(self):
        expected = ["RF-Concert-Grand.rfplugin", *MODULE.pinned_official_filenames()]
        MODULE.verify("standard", list(expected), expected)

    def test_a_missing_pinned_package_is_rejected(self):
        expected = ["RF-Concert-Grand.rfplugin", *MODULE.pinned_official_filenames()]
        with self.assertRaises(ValueError) as raised:
            MODULE.verify("standard", ["RF-Concert-Grand.rfplugin"], expected)
        self.assertIn("missing", str(raised.exception))

    def test_an_unpinned_package_that_slipped_in_is_rejected(self):
        expected = ["RF-Concert-Grand.rfplugin", *MODULE.pinned_official_filenames()]
        with self.assertRaises(ValueError) as raised:
            MODULE.verify("standard", [*expected, "Stowaway.rfplugin"], expected)
        self.assertIn("unexpected", str(raised.exception))

if __name__ == "__main__":
    unittest.main()
