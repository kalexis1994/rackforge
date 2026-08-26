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
        expected = ["RackForge-Concert-Grand.rfplugin", "RF-106.rfplugin"]
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


if __name__ == "__main__":
    unittest.main()
