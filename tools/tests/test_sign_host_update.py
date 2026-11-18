import hashlib
import io
from pathlib import Path
import runpy
import tarfile
import tempfile
import unittest

MODULE = runpy.run_path(str(Path(__file__).resolve().parents[1] / "sign-host-update.py"))
ASSEMBLY = runpy.run_path(str(Path(__file__).resolve().parents[1] / "assemble-release.py"))


class SignedHostUpdateManifest(unittest.TestCase):
    def package(self, root, name, architecture, version="0.1.40"):
        with tarfile.open(root / name, "w:gz") as archive:
            contents = f"version={version}\narchitecture={architecture}\nedition=standard\n".encode()
            entry = tarfile.TarInfo("rackforge/build-info.txt")
            entry.size = len(contents)
            archive.addfile(entry, io.BytesIO(contents))
            public = MODULE["PUBLIC_KEY"].read_bytes()
            entry = tarfile.TarInfo("rackforge/config/update-public-key.txt")
            entry.size = len(public)
            archive.addfile(entry, io.BytesIO(public))

    def test_both_platforms_are_bound_to_their_exact_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.package(root, "RackForge-Linux-x86_64.tar.gz", "linux-x86-64")
            self.package(root, "RackForge-RaspberryPi-arm64.tar.gz", "linux-aarch64")
            manifest = MODULE["manifest"](root, "v0.1.40", "Release notes")
            self.assertEqual(manifest["product"], "org.rackforge.host")
            self.assertEqual(len(manifest["targets"]), 2)
            for target, name in MODULE["TARGETS"].items():
                asset = manifest["targets"][target]
                self.assertEqual(asset["bytes"], (root / name).stat().st_size)
                self.assertEqual(asset["sha256"], hashlib.sha256((root / name).read_bytes()).hexdigest())
                self.assertTrue(asset["url"].endswith("v0.1.40/" + name))

    def test_signing_refuses_a_wrong_architecture_or_version(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.package(root, "RackForge-Linux-x86_64.tar.gz", "linux-aarch64")
            with self.assertRaises(ValueError):
                MODULE["manifest"](root, "0.1.40", "")
            self.package(root, "RackForge-Linux-x86_64.tar.gz", "linux-x86-64", "0.1.39")
            with self.assertRaises(ValueError):
                MODULE["manifest"](root, "0.1.40", "")

    def test_stable_feed_rejects_prereleases_and_invalid_versions(self):
        for version in ("0.1.40-alpha", "../../bad", "01.2.3", "0.1"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                MODULE["manifest"](Path("."), version, "")

    def test_release_staging_rejects_path_components_before_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            expected = Path(directory).resolve() / "rf-release-v0.1.40"
            self.assertEqual(Path(ASSEMBLY["release_work_directory"]("v0.1.40", directory)), expected)
            for tag in ("../escape", "v0.1.40/../../escape", "C:/escape", "v01.2.3"):
                with self.subTest(tag=tag), self.assertRaises(ValueError):
                    ASSEMBLY["release_work_directory"](tag, directory)

    def test_private_update_key_cannot_be_used_from_inside_the_checkout(self):
        with self.assertRaisesRegex(ValueError, "outside the checkout"):
            MODULE["sign"](Path("."), "0.1.40", "", Path(__file__), [])


if __name__ == "__main__":
    unittest.main()
