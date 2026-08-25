import importlib.util
import unittest
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "qualify-android-lifecycle.py"
SPEC = importlib.util.spec_from_file_location("android_lifecycle_qualification", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class AndroidLifecycleQualificationTests(unittest.TestCase):
    def healthy_snapshot(self):
        return {
            "audio_running": True,
            "audio_recovery_in_progress": False,
            "selected_audio_output": "USB audio · Scarlett",
            "selected_audio_device_id": 42,
            "open_midi_ports": 2,
            "midi_generation": 8,
            "audio_status": {
                "running": True,
                "stream_health": "healthy",
                "device_id": 42,
                "callback_count": 100,
            },
        }

    def test_usb_recovery_requires_the_real_aaudio_device(self):
        snapshot = self.healthy_snapshot()
        self.assertTrue(
            MODULE.usb_runtime_recovered(snapshot, "USB audio · Scarlett", 2, 7)
        )
        snapshot["audio_status"]["device_id"] = 7
        self.assertFalse(
            MODULE.usb_runtime_recovered(snapshot, "USB audio · Scarlett", 2, 7)
        )

    def test_usb_recovery_requires_midi_reopen_after_generation_change(self):
        snapshot = self.healthy_snapshot()
        snapshot["midi_generation"] = 7
        self.assertFalse(
            MODULE.usb_runtime_recovered(snapshot, "USB audio · Scarlett", 2, 7)
        )
        snapshot["midi_generation"] = 8
        snapshot["open_midi_ports"] = 1
        self.assertFalse(
            MODULE.usb_runtime_recovered(snapshot, "USB audio · Scarlett", 2, 7)
        )

    def test_system_default_does_not_require_a_fixed_device_id(self):
        snapshot = self.healthy_snapshot()
        snapshot["selected_audio_output"] = "System default"
        snapshot["selected_audio_device_id"] = 0
        snapshot["audio_status"]["device_id"] = 17
        self.assertTrue(MODULE.usb_runtime_recovered(snapshot, "System default", 0, 7))

    def test_device_fingerprints_do_not_depend_on_list_order(self):
        snapshot = {
            "usb_devices": [
                {"name": "Controller", "detail": "VID 1234 · PID 0001"},
                {"name": "Interface", "detail": "VID 1234 · PID 0002"},
            ]
        }
        self.assertEqual(
            MODULE.fingerprints(snapshot, "usb_devices"),
            {
                "Controller|VID 1234 · PID 0001",
                "Interface|VID 1234 · PID 0002",
            },
        )


if __name__ == "__main__":
    unittest.main()
