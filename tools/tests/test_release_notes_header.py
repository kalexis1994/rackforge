import importlib.util
import io
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "assemble-release.py"
SPEC = importlib.util.spec_from_file_location("assemble_release", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


def package(name: str, version: str) -> bytes:
    """A .rfplugin carrying the one field the check reads."""
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as archive:
        archive.writestr(
            "rackforge-plugin.toml",
            f'schema_version = 3\nid = "org.rackforge.{name.lower()}"\nversion = "{version}"\n',
        )
    return buffer.getvalue()


def standard_archive(directory: Path, packages: dict[str, str], extra: str = "") -> Path:
    """A Standard release archive carrying the given packages and versions."""
    archive = directory / "RackForge-Linux-x86_64.tar.gz"
    with tarfile.open(archive, "w:gz") as tar:
        for name, version in packages.items():
            payload = package(name.removesuffix(".rfplugin"), version)
            info = tarfile.TarInfo(f"rackforge/bundled-plugins/{name}")
            info.size = len(payload)
            tar.addfile(info, io.BytesIO(payload))
        if extra:
            info = tarfile.TarInfo(extra)
            info.size = 0
            tar.addfile(info, io.BytesIO(b""))
    return archive


def pin(filename: str, version: str, url: str) -> dict:
    return {
        "filename": filename,
        "plugin_id": f"org.rackforge.{filename.removesuffix('.rfplugin').lower()}",
        "version": version,
        "url": url,
    }


PINS = (
    pin(
        "RF-106.rfplugin",
        "0.2.19",
        "https://github.com/kalexis1994/rackforge-plugin-rf-106/"
        "releases/download/v0.2.19/RF-106.rfplugin",
    ),
    pin(
        "RF-Tines.rfplugin",
        "0.2.1",
        "https://github.com/kalexis1994/RF-Tines/releases/download/v0.2.1/RF-Tines.rfplugin",
    ),
)
MATCHING = {
    "RF-Concert-Grand.rfplugin": "0.171.34",
    "RF-106.rfplugin": "0.2.19",
    "RF-Tines.rfplugin": "0.2.1",
}


class PackagedPluginTests(unittest.TestCase):
    def test_versions_are_read_from_each_package(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = standard_archive(
                Path(directory), MATCHING, extra="rackforge/target/release/rackforge-core"
            )
            self.assertEqual(MODULE.packaged_plugins(str(archive)), MATCHING)

    def test_a_package_without_a_version_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "RackForge-Linux-x86_64.tar.gz"
            payload = io.BytesIO()
            with zipfile.ZipFile(payload, "w") as broken:
                broken.writestr("rackforge-plugin.toml", 'id = "org.rackforge.rf-106"\n')
            body = payload.getvalue()
            with tarfile.open(archive, "w:gz") as tar:
                info = tarfile.TarInfo("rackforge/bundled-plugins/RF-106.rfplugin")
                info.size = len(body)
                tar.addfile(info, io.BytesIO(body))
            with self.assertRaisesRegex(SystemExit, "declares no version"):
                MODULE.packaged_plugins(str(archive))


class PinCheckTests(unittest.TestCase):
    """The notes may not claim a version the release does not carry.

    Each of these is a way the opening could have gone wrong quietly: a pin
    moved without a rebuild, a package dropped from the bundle, one that
    slipped in unpinned. The header is believed by whoever reads it, so the
    assembly stops instead.
    """

    def test_an_archive_matching_the_pins_passes(self):
        with tempfile.TemporaryDirectory() as directory:
            standard_archive(Path(directory), MATCHING)
            MODULE.check_pins_against_artifact(PINS, directory)

    def test_a_version_the_archive_does_not_carry_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            standard_archive(Path(directory), {**MATCHING, "RF-Tines.rfplugin": "0.2.0"})
            with self.assertRaises(SystemExit) as raised:
                MODULE.check_pins_against_artifact(PINS, directory)
            self.assertEqual(
                "RF-Tines.rfplugin is 0.2.0 in the archive and 0.2.1 in the pins",
                str(raised.exception),
            )

    def test_a_pinned_package_missing_from_the_archive_is_refused(self):
        packages = {k: v for k, v in MATCHING.items() if k != "RF-106.rfplugin"}
        with tempfile.TemporaryDirectory() as directory:
            standard_archive(Path(directory), packages)
            with self.assertRaisesRegex(SystemExit, "missing RF-106.rfplugin"):
                MODULE.check_pins_against_artifact(PINS, directory)

    def test_an_unpinned_package_in_the_archive_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            standard_archive(Path(directory), {**MATCHING, "Stowaway.rfplugin": "1.0.0"})
            with self.assertRaisesRegex(SystemExit, "unpinned packages: Stowaway.rfplugin"):
                MODULE.check_pins_against_artifact(PINS, directory)

    def test_a_standard_archive_without_concert_grand_is_refused(self):
        packages = {k: v for k, v in MATCHING.items() if k != "RF-Concert-Grand.rfplugin"}
        with tempfile.TemporaryDirectory() as directory:
            standard_archive(Path(directory), packages)
            with self.assertRaisesRegex(SystemExit, "no Concert Grand"):
                MODULE.check_pins_against_artifact(PINS, directory)

    def test_a_missing_archive_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(SystemExit, "needs the Standard Linux archive"):
                MODULE.check_pins_against_artifact(PINS, directory)


class NotesHeaderTests(unittest.TestCase):
    def test_the_header_names_the_tag_the_commit_and_every_pin(self):
        header = MODULE.notes_header(PINS, "v9.9.9", "abc1234")
        self.assertIn(
            "RackForge v9.9.9 Preview, built and verified by GitHub Actions "
            "from commit `abc1234`.",
            header,
        )
        self.assertIn(
            "[RF-106 0.2.19](https://github.com/kalexis1994/rackforge-plugin-rf-106)", header
        )
        self.assertIn("and [RF-Tines 0.2.1](https://github.com/kalexis1994/RF-Tines).", header)
        self.assertIn("Concert Grand from this repository", header)

    def test_the_header_carries_the_real_pins(self):
        """Nothing may drop out of the list between the pins and the notes."""
        pins = MODULE.official_pins()
        header = MODULE.notes_header(pins, "v9.9.9", "abc1234")
        for plugin in pins:
            name = str(plugin["filename"]).removesuffix(".rfplugin")
            self.assertIn(f"[{name} {plugin['version']}]", header)

    def test_a_url_that_is_not_a_release_download_is_refused(self):
        with self.assertRaisesRegex(SystemExit, "cannot read a repository"):
            MODULE.upstream_repository("https://example.invalid/RF-Tines.rfplugin")


if __name__ == "__main__":
    unittest.main()
