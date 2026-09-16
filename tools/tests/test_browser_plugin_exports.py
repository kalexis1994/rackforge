import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def export_names(path: Path) -> list[str]:
    source = path.read_text(encoding="utf-8")
    match = re.search(r"const EXPORTS = \[(.*?)\];", source, re.DOTALL)
    if match is None:
        raise AssertionError(f"{path} has no browser plugin export table")
    return re.findall(r'"(rackforge_[a-z0-9_]+)"', match.group(1))


class BrowserPluginExportContract(unittest.TestCase):
    def test_browser_and_probe_use_the_same_export_indexes(self) -> None:
        browser = export_names(ROOT / "web/src/browser/pluginHost.ts")
        probe = export_names(ROOT / "tools/check-browser-host.mjs")
        self.assertEqual(
            browser,
            probe,
            "the browser probe must address plugin exports by the same indexes as the page",
        )


if __name__ == "__main__":
    unittest.main()
