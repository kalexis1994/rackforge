import unittest

import artupy_keylab


class ProtocolTests(unittest.TestCase):
    def test_connect_message(self):
        self.assertEqual(
            artupy_keylab.CONNECT,
            bytes.fromhex("F0 00 20 6B 7F 42 02 0F 40 5A 01 F7"),
        )

    def test_daw_and_arturia_preset_messages(self):
        self.assertEqual(
            artupy_keylab.DAW_PRESET,
            bytes.fromhex("F0 00 20 6B 7F 42 21 11 40 02 00 01 F7"),
        )
        self.assertEqual(
            artupy_keylab.ARTURIA_PRESET,
            bytes.fromhex("F0 00 20 6B 7F 42 21 11 40 02 00 00 F7"),
        )

    def test_two_line_message(self):
        self.assertEqual(
            artupy_keylab.screen_two_lines("DOOM", "READY"),
            bytes.fromhex(
                "F0 00 20 6B 7F 42 04 01 60 12 01 "
                "44 4F 4F 4D 00 02 52 45 41 44 59 00 00 F7"
            ),
        )

    def test_rejects_long_text(self):
        with self.assertRaises(ValueError):
            artupy_keylab.screen_two_lines("X" * 19, "OK")

    def test_rejects_non_ascii(self):
        with self.assertRaises(UnicodeEncodeError):
            artupy_keylab.screen_two_lines("CAÑON", "OK")

    def test_port_selection_is_unambiguous(self):
        ports = [
            artupy_keylab.MidiPort(0, "Microsoft GS Wavetable"),
            artupy_keylab.MidiPort(1, "KeyLab Essential mk3"),
        ]
        self.assertEqual(artupy_keylab.choose_port(ports, "1"), ports[1])
        self.assertEqual(artupy_keylab.choose_port(ports, "essential"), ports[1])

    def test_windows_abbreviated_name_is_recognized(self):
        port = artupy_keylab.MidiPort(2, "KL Essential 61 mk3 MCU/HUI")
        self.assertTrue(artupy_keylab.looks_like_keylab(port))


if __name__ == "__main__":
    unittest.main()
