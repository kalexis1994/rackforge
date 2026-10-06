"""The Pi's dedicated installer and bench, run where CI runs.

The installer's own self-tests drive it against scratch trees with a
pretend systemctl; they need bash and a Linux userland.
"""

import shutil
import subprocess
import unittest
from pathlib import Path

PI = Path(__file__).resolve().parents[2] / "platforms" / "raspberry-pi"


@unittest.skipUnless(shutil.which("bash") and shutil.which("getent"), "needs bash and getent")
class DedicatedInstaller(unittest.TestCase):
    def run_selftest(self, name):
        result = subprocess.run(
            ["bash", str(PI / "dedicated" / "tests" / name)],
            capture_output=True,
            text=True,
            timeout=300,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("all checks passed", result.stdout)

    def test_the_resource_engine_keeps_its_promises(self):
        self.run_selftest("selftest.sh")

    def test_every_module_applies_and_reverts_exactly(self):
        self.run_selftest("cli-selftest.sh")


class Bench(unittest.TestCase):
    def test_the_bench_compiles(self):
        path = PI / "bench" / "rackforge-pi-bench"
        compile(path.read_text(encoding="utf-8"), str(path), "exec")


if __name__ == "__main__":
    unittest.main()
