import re
import tomllib
import unittest
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[2]
SEED = REPOSITORY / "config" / "audio.toml"
INSTALLERS = (
    REPOSITORY / "platforms" / "raspberry-pi" / "scripts" / "install-appliance.sh",
    REPOSITORY / "platforms" / "linux-x86_64" / "install.sh",
)
TOKENS = {"@RACKFORGE_ROOT@", "@RACKFORGE_DEFAULT_PACKAGE@"}


def rendered() -> dict:
    """The seed as an installer leaves it on a machine."""
    text = SEED.read_text(encoding="utf-8")
    text = text.replace("@RACKFORGE_ROOT@", "/home/player/rackforge")
    text = text.replace(
        "@RACKFORGE_DEFAULT_PACKAGE@",
        "/home/player/rackforge/plugin-store/packages/org.rackforge.concert-grand/0.171.34",
    )
    return tomllib.loads(text)


class AudioSeedTests(unittest.TestCase):
    """The file every fresh installation starts its audio engine from.

    The Web host copies this seed to `config/audio.toml` the first time an
    instrument is activated, replacing `package` and dropping `preset` and
    nothing else. So every other line here reaches Core exactly as written,
    and for three releases what reached Core could not start:

      * relative `package` and `data_root` -- "must be absolute paths";
      * a `[resources]` override for a plugin that is not the active one --
        "plugin does not declare resource";
      * a USB interface pinned with `fallback = "none"`, so any machine
        without that exact device had no audio at all.

    None of it showed at install time. It showed as an engine that never
    started and an interface that sat on "Waiting for Core".
    """

    def test_the_only_tokens_are_ones_the_installers_replace(self):
        found = set(re.findall(r"@[A-Z_]+@", SEED.read_text(encoding="utf-8")))
        self.assertTrue(found, "the seed carries no tokens; paths cannot be absolute")
        self.assertEqual(
            found - TOKENS,
            set(),
            "a token no installer substitutes would survive onto the machine",
        )

    def test_core_requires_absolute_paths(self):
        config = rendered()
        for field in ("package", "data_root"):
            self.assertTrue(
                config[field].startswith("/"),
                f"{field} is relative; Core refuses the configuration outright",
            )

    def test_it_overrides_no_resources(self):
        # Core bails on an override the active plugin does not declare, and
        # the active plugin here is whichever instrument was activated.
        self.assertNotIn(
            "resources",
            rendered(),
            "a resource override belongs to one plugin and this seed serves any",
        )

    def test_it_names_no_make_and_no_model(self):
        # The appliance is imaged before anyone knows which interface it will
        # meet. A vendor id here is a machine that works on one desk.
        device = rendered()["audio"]["output"]["device"]
        self.assertEqual(
            device,
            {"mode": "automatic"},
            "the seed pins a specific device; every other machine has no audio",
        )

    def test_nothing_in_the_seed_names_a_vendor(self):
        text = SEED.read_text(encoding="utf-8")
        body = "\n".join(
            line for line in text.splitlines() if not line.lstrip().startswith("#")
        )
        for hardcoded in ("vendor_id", "product_id", "serial"):
            self.assertNotIn(
                hardcoded,
                body,
                f"{hardcoded} ties the shipped configuration to one interface",
            )

    def test_it_asks_for_the_only_format_the_engine_renders(self):
        self.assertEqual(rendered()["audio"]["output"]["sample_format"], "s32_le")

    def test_both_installers_render_it_rather_than_copy_it(self):
        for installer in INSTALLERS:
            text = installer.read_text(encoding="utf-8")
            self.assertIn(
                "rackforge_render_audio_seed",
                text,
                f"{installer.name} does not render the seed",
            )
            self.assertNotRegex(
                text,
                r"install -m 0644[^\n]*\n?[^\n]*config/audio\.toml\"",
                f"{installer.name} copies the seed verbatim; its tokens would survive",
            )


if __name__ == "__main__":
    unittest.main()
