import importlib.util
import unittest
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "soak-android.py"
SPEC = importlib.util.spec_from_file_location("android_soak", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class AndroidSoakTests(unittest.TestCase):
    def test_counter_delta_survives_a_native_stream_counter_reset(self):
        self.assertEqual(MODULE.reset_aware_delta(100, 107), 7)
        self.assertEqual(MODULE.reset_aware_delta(100, 3), 3)

    def test_two_hours_are_required_for_an_unqualified_run(self):
        outcome, failures = MODULE.evaluate(
            {name: 0 for name in MODULE.DEFAULT_LIMITS},
            {"callback_load_percent": 20.0, "thermal_status": 1.0},
            MODULE.QUALIFIED_DURATION_SECONDS - 1,
            MODULE.DEFAULT_LIMITS,
            85.0,
            2,
        )
        self.assertEqual(outcome, "passed_with_duration_waiver")
        self.assertEqual(failures, [])

    def test_exactly_two_clean_hours_are_a_full_pass(self):
        outcome, failures = MODULE.evaluate(
            {name: 0 for name in MODULE.DEFAULT_LIMITS},
            {"callback_load_percent": 20.0, "thermal_status": 1.0},
            MODULE.QUALIFIED_DURATION_SECONDS,
            MODULE.DEFAULT_LIMITS,
            85.0,
            2,
        )
        self.assertEqual(outcome, "passed")
        self.assertEqual(failures, [])

    def test_any_realtime_counter_or_thermal_breach_fails(self):
        totals = {name: 0 for name in MODULE.DEFAULT_LIMITS}
        totals["midi_dropped"] = 1
        outcome, failures = MODULE.evaluate(
            totals,
            {"callback_load_percent": 90.0, "thermal_status": 3.0},
            MODULE.QUALIFIED_DURATION_SECONDS,
            MODULE.DEFAULT_LIMITS,
            85.0,
            2,
        )
        self.assertEqual(outcome, "failed")
        self.assertEqual(len(failures), 3)


if __name__ == "__main__":
    unittest.main()
