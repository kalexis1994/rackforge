import unittest

from protocol import (
    FRAMEBUFFER_LEN,
    MAX_CHUNK_LEN,
    SYSEX_PREFIX,
    SYSEX_SUFFIX,
    crc16_ccitt_false,
    frame_chunk,
    present,
    solid_frame,
    upload_messages,
)


class ProtocolTests(unittest.TestCase):
    def test_crc16_known_vector(self) -> None:
        self.assertEqual(crc16_ccitt_false(b"123456789"), 0x29B1)

    def test_maximum_chunk_is_midi_safe_and_fits_parser(self) -> None:
        message = frame_chunk(984, bytes(range(MAX_CHUNK_LEN)))
        payload_plus_f7 = message[len(SYSEX_PREFIX) :]
        self.assertEqual(message[: len(SYSEX_PREFIX)], SYSEX_PREFIX)
        self.assertEqual(message[-1:], SYSEX_SUFFIX)
        self.assertLessEqual(len(payload_plus_f7), 100)
        self.assertTrue(all(value <= 0x7F for value in message[1:-1]))

    def test_full_upload_covers_frame_and_ends_with_present(self) -> None:
        frame = bytes((index * 37 + 11) & 0xFF for index in range(FRAMEBUFFER_LEN))
        messages = list(upload_messages(frame))
        chunk_count = (FRAMEBUFFER_LEN + MAX_CHUNK_LEN - 1) // MAX_CHUNK_LEN
        self.assertEqual(len(messages), chunk_count + 1)
        self.assertEqual(messages[-1], present(frame))

        reconstructed = bytearray(FRAMEBUFFER_LEN)
        for message in messages[:-1]:
            payload = message[len(SYSEX_PREFIX) : -1]
            offset = payload[5] | payload[6] << 7
            raw_len = payload[7]
            checksum = 0
            for value in payload[:-1]:
                checksum ^= value
            self.assertEqual(checksum & 0x7F, payload[-1])
            for index in range(raw_len):
                reconstructed[offset + index] = payload[8 + index * 2] | (
                    payload[9 + index * 2] << 4
                )
        self.assertEqual(bytes(reconstructed), frame)
        self.assertEqual(crc16_ccitt_false(reconstructed), crc16_ccitt_false(frame))

    def test_rejects_out_of_bounds_chunk(self) -> None:
        with self.assertRaises(ValueError):
            frame_chunk(1020, b"\x00" * 5)


if __name__ == "__main__":
    unittest.main()
