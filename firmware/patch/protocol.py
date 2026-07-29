"""Host-side encoder for the RackForge KeyLab framebuffer protocol.

The module is transport independent and performs no MIDI or USB I/O.
"""

from __future__ import annotations

from collections.abc import Iterable

SYSEX_PREFIX = bytes.fromhex("F0 00 20 6B 7F 42")
SYSEX_SUFFIX = b"\xF7"
MAGIC = bytes((0x7D, ord("A"), ord("P"), 1))
OP_FRAME_CHUNK = 1
OP_PRESENT = 2
FRAMEBUFFER_LEN = 1024
MAX_CHUNK_LEN = 40


def crc16_ccitt_false(data: bytes) -> int:
    crc = 0xFFFF
    for value in data:
        crc ^= value << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) & 0xFFFF if crc & 0x8000 else (crc << 1) & 0xFFFF
    return crc


def _sysex(payload: bytes) -> bytes:
    if any(value > 0x7F for value in payload):
        raise ValueError("RackForge SysEx payload contains a non-MIDI-safe byte")
    return SYSEX_PREFIX + payload + SYSEX_SUFFIX


def frame_chunk(offset: int, raw: bytes) -> bytes:
    if not 0 <= offset < FRAMEBUFFER_LEN:
        raise ValueError("framebuffer offset is outside the 1024-byte buffer")
    if not 1 <= len(raw) <= MAX_CHUNK_LEN:
        raise ValueError(f"chunk length must be in 1..={MAX_CHUNK_LEN}")
    if offset + len(raw) > FRAMEBUFFER_LEN:
        raise ValueError("chunk exceeds the framebuffer")

    payload = bytearray(MAGIC)
    payload.extend(
        (
            OP_FRAME_CHUNK,
            offset & 0x7F,
            (offset >> 7) & 0x7F,
            len(raw),
        )
    )
    for value in raw:
        payload.extend((value & 0x0F, value >> 4))
    checksum = 0
    for value in payload:
        checksum ^= value
    payload.append(checksum & 0x7F)
    return _sysex(bytes(payload))


def present(frame: bytes) -> bytes:
    if len(frame) != FRAMEBUFFER_LEN:
        raise ValueError("a complete framebuffer must contain exactly 1024 bytes")
    crc = crc16_ccitt_false(frame)
    return _sysex(
        MAGIC
        + bytes(
            (
                OP_PRESENT,
                crc & 0x7F,
                (crc >> 7) & 0x7F,
                (crc >> 14) & 0x03,
            )
        )
    )


def upload_messages(frame: bytes) -> Iterable[bytes]:
    if len(frame) != FRAMEBUFFER_LEN:
        raise ValueError("a complete framebuffer must contain exactly 1024 bytes")
    for offset in range(0, FRAMEBUFFER_LEN, MAX_CHUNK_LEN):
        yield frame_chunk(offset, frame[offset : offset + MAX_CHUNK_LEN])
    yield present(frame)


def solid_frame(on: bool) -> bytes:
    return bytes((0xFF if on else 0x00,)) * FRAMEBUFFER_LEN
