import datetime
import importlib.util
import tempfile
import unittest
import zipfile
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "build-plugin-catalog.py"
SPEC = importlib.util.spec_from_file_location("build_plugin_catalog", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


def package(
    path: Path,
    plugin_id: str = "org.example.synth",
    version: str = "1.0.0",
    description: str | None = "A test instrument",
    portable: bool = True,
    spdx: str | None = "GPL-3.0-only",
) -> None:
    manifest = (
        f'schema_version = 1\nid = "{plugin_id}"\nname = "Synth"\n'
        f'version = "{version}"\nkind = "instrument"\n'
    )
    if description:
        manifest += f'description = "{description}"\n'
    if portable:
        manifest += '\n[component]\nabi = "wasm-v1"\npath = "component.wasm"\n'
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("rackforge-plugin.toml", manifest)
        archive.writestr(
            "metadata/runtime.json",
            f'{{"schema_version":1,"id":"{plugin_id}","version":"{version}"}}',
        )
        if spdx:
            archive.writestr("LICENSE", f"SPDX-License-Identifier: {spdx}\nText\n")


class ReleaseHost:
    """GitHub releases and their assets, served from a temporary directory."""

    def __init__(self, directory: Path):
        self.directory = directory
        self.releases: list[dict] = []
        self.downloads: list[str] = []

    def publish(self, tag: str, asset: str = "Synth.rfplugin", **package_options) -> None:
        path = self.directory / f"{tag}-{asset}"
        package(path, **package_options)
        self.releases.insert(
            0,
            {
                "tag_name": tag,
                "draft": False,
                "prerelease": False,
                "published_at": f"2026-10-0{len(self.releases) + 1}T00:00:00Z",
                "assets": [
                    {"name": asset, "browser_download_url": f"https://host.invalid/{path.name}"}
                ],
            },
        )

    def releases_of(self, repository: str) -> list[dict]:
        return self.releases if repository == "example/synth" else []

    def fetch(self, url: str, destination: Path) -> None:
        self.downloads.append(url)
        destination.write_bytes((self.directory / url.rsplit("/", 1)[1]).read_bytes())


PLUGIN = {"id": "org.example.synth", "repository": "example/synth", "asset": "Synth.rfplugin"}


class CatalogTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.host = ReleaseHost(Path(self.temporary.name))

    def tearDown(self):
        self.temporary.cleanup()

    def entry(self, plugin=PLUGIN, keep=2):
        return MODULE.plugin_entry(plugin, keep, self.host.releases_of, self.host.fetch)

    def test_versions_order_by_semver_not_text(self):
        ordered = sorted(["0.10.0", "0.9.0", "1.0.0", "1.0.0-beta.2", "1.0.0-beta.10"],
                         key=MODULE.version_key)
        self.assertEqual(ordered, ["0.9.0", "0.10.0", "1.0.0-beta.2", "1.0.0-beta.10", "1.0.0"])
        with self.assertRaises(MODULE.CatalogError):
            MODULE.version_key("1.0")

    def test_lists_the_newest_stable_releases_with_their_bytes(self):
        for tag in ("v0.9.0", "v0.10.0", "v0.11.0"):
            self.host.publish(tag, version=tag[1:])
        self.host.releases[0]["prerelease"] = True  # v0.11.0
        draft = dict(self.host.releases[1], draft=True, tag_name="v0.12.0")
        self.host.releases.insert(0, draft)
        self.host.releases.insert(0, {"draft": False, "prerelease": False, "assets": []})

        entry = self.entry()
        self.assertEqual([item["version"] for item in entry["releases"]], ["0.10.0", "0.9.0"])
        self.assertEqual(entry["name"], "Synth")
        self.assertEqual(entry["summary"], "A test instrument")
        self.assertEqual(entry["license"], "GPL-3.0-only")
        self.assertEqual(entry["homepage"], "https://github.com/example/synth")
        artifact = entry["releases"][0]["artifacts"][0]
        self.assertEqual(artifact["platform"], "wasm-v1")
        self.assertEqual(artifact["url"], "https://host.invalid/v0.10.0-Synth.rfplugin")
        local = Path(self.temporary.name) / "v0.10.0-Synth.rfplugin"
        self.assertEqual(artifact["size"], local.stat().st_size)
        self.assertEqual(artifact["sha256"], MODULE.sha256(local))
        # Only what is listed is downloaded: never the prerelease, nor more
        # releases than the catalog keeps.
        self.assertEqual(len(self.host.downloads), 2)

    def test_the_version_is_the_packages_not_the_tag(self):
        # Concert Grand rides RackForge's own releases, tagged with RackForge's
        # version; two releases may carry the same plugin version.
        self.host.publish("v0.1.32", version="0.171.35")
        self.host.publish("v0.1.33", version="0.171.35")
        self.host.publish("v0.1.34", version="0.172.0")
        entry = self.entry()
        self.assertEqual([item["version"] for item in entry["releases"]], ["0.172.0", "0.171.35"])

    def test_configuration_overrides_what_the_package_says(self):
        self.host.publish("v1.0.0", description=None, spdx=None)
        with self.assertRaisesRegex(MODULE.CatalogError, "summary"):
            self.entry()
        with self.assertRaisesRegex(MODULE.CatalogError, "license"):
            self.entry(dict(PLUGIN, summary="Configured summary"))
        entry = self.entry(dict(PLUGIN, summary="Configured summary", license="MIT"))
        self.assertEqual((entry["summary"], entry["license"]), ("Configured summary", "MIT"))

    def test_a_package_must_be_the_plugin_and_portable(self):
        self.host.publish("v1.0.0", plugin_id="org.example.other")
        with self.assertRaisesRegex(MODULE.CatalogError, "not org.example.synth"):
            self.entry()
        self.host.releases.clear()
        self.host.publish("v1.0.0", portable=False)
        with self.assertRaisesRegex(MODULE.CatalogError, "portable component"):
            self.entry()

    def test_a_failing_plugin_is_left_out_and_reported(self):
        self.host.publish("v1.0.0")
        config = {
            "schema_version": 1,
            "repository_id": "org.example.catalog",
            "name": "Example",
            "releases": 3,
            "plugins": [PLUGIN, {"id": "org.example.gone", "repository": "example/gone",
                                 "asset": "Gone.rfplugin"}],
        }
        catalog, failures = MODULE.build_catalog(
            config,
            self.host.releases_of,
            self.host.fetch,
            now=datetime.datetime(2026, 10, 14, 12, 0, tzinfo=datetime.timezone.utc),
        )
        self.assertEqual([plugin["id"] for plugin in catalog["plugins"]], ["org.example.synth"])
        self.assertEqual(len(failures), 1)
        self.assertIn("org.example.gone", failures[0])
        self.assertEqual(catalog["generated_at"], "2026-10-14T12:00:00Z")
        self.assertEqual(catalog["repository_id"], "org.example.catalog")

    def test_the_shipped_configuration_reads(self):
        config = MODULE.read_config(MODULE.ROOT / "config" / "plugin-catalog.toml")
        self.assertEqual(config["repository_id"], "org.rackforge.official")
        self.assertTrue(all(plugin["asset"].endswith(".rfplugin") for plugin in config["plugins"]))


if __name__ == "__main__":
    unittest.main()
