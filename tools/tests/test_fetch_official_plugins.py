import importlib.util
import tempfile
import unittest
from unittest import mock
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "fetch-official-plugins.py"
SPEC = importlib.util.spec_from_file_location("fetch_official_plugins", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)

PINS = '''OFFICIAL_PLUGINS = (
    {
        "filename": "RF-106.rfplugin",
        "plugin_id": "org.rackforge.rf-106",
        "version": "0.2.19",
        "url": (
            "https://github.com/kalexis1994/rackforge-plugin-rf-106/"
            "releases/download/v0.2.19/RF-106.rfplugin"
        ),
        "sha256": "aaaa",
    },
    {
        "filename": "RF-7.rfplugin",
        "plugin_id": "org.rackforge.rf7",
        "version": "0.5.1",
        "url": "https://github.com/kalexis1994/RF-7/releases/download/v0.5.1/RF-7.rfplugin",
        "sha256": "bbbb",
    },
)
'''


class RewritePinTests(unittest.TestCase):
    def rewrite(self, plugin_id: str, version: str, url: str, digest: str) -> str:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "pins.py"
            source.write_text(PINS, encoding="utf-8")
            MODULE.rewrite_pin(source, {"plugin_id": plugin_id}, version, url, digest)
            return source.read_text(encoding="utf-8")

    def test_a_wrapped_url_moves_with_its_version(self) -> None:
        text = self.rewrite(
            "org.rackforge.rf-106",
            "0.2.20",
            "https://github.com/kalexis1994/rackforge-plugin-rf-106/releases/download/v0.2.20/RF-106.rfplugin",
            "cccc",
        )
        self.assertIn('"releases/download/v0.2.20/RF-106.rfplugin"', text)
        self.assertNotIn("v0.2.19", text)
        self.assertIn('"sha256": "cccc"', text)
        # The other pin is left alone.
        self.assertIn('"sha256": "bbbb"', text)

    def test_a_url_on_one_line_moves_too(self) -> None:
        # RF-7's pin was written on one line; the version and digest moved
        # and the URL stayed on the old release.
        text = self.rewrite(
            "org.rackforge.rf7",
            "0.5.2",
            "https://github.com/kalexis1994/RF-7/releases/download/v0.5.2/RF-7.rfplugin",
            "dddd",
        )
        self.assertIn('"version": "0.5.2"', text)
        self.assertIn('"releases/download/v0.5.2/RF-7.rfplugin"', text)
        self.assertNotIn("v0.5.1", text)
        self.assertIn('"sha256": "dddd"', text)
        self.assertIn('"sha256": "aaaa"', text)


class DefaultPackageRoutingTests(unittest.TestCase):
    def test_default_is_carried_once_outside_the_other_packages(self):
        default = {"filename": "Piano.rfplugin", "plugin_id": "piano", "default": True}
        other = {"filename": "Other.rfplugin", "plugin_id": "other"}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "official"
            extracted = Path(directory) / "plugins"
            with mock.patch.object(MODULE, "OFFICIAL_PLUGINS", (default, other)), \
                 mock.patch.object(MODULE, "fetch") as fetch, \
                 mock.patch.object(MODULE, "extract") as extract, \
                 mock.patch("sys.argv", ["fetch", "--output-directory", str(output),
                                         "--extract-directory", str(extracted)]):
                MODULE.main()
            self.assertEqual(fetch.call_args_list, [mock.call(output.parent, default), mock.call(output, other)])
            self.assertEqual(extract.call_args_list, [mock.call(output.parent, extracted, default), mock.call(output, extracted, other)])

    def test_a_single_plugin_can_be_fetched_for_host_contract_tests(self):
        selected = {"filename": "Selected.rfplugin", "plugin_id": "selected"}
        other = {"filename": "Other.rfplugin", "plugin_id": "other"}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "official"
            with mock.patch.object(MODULE, "OFFICIAL_PLUGINS", (selected, other)), \
                 mock.patch.object(MODULE, "fetch") as fetch, \
                 mock.patch("sys.argv", ["fetch", "--output-directory", str(output), "--plugin-id", "selected"]):
                MODULE.main()
            fetch.assert_called_once_with(output, selected)


if __name__ == "__main__":
    unittest.main()
