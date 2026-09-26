#!/usr/bin/env python3
"""Two builds' renders, alternated in one file, for the ear to judge.

    python3 tools/stitch-ab.py A.f32 B.f32 out.wav [--name-a hoy --name-b nuevo]

Both inputs are mono 32-bit float at 48 kHz -- what the plugin's render
tests write. The output is 16-bit mono WAV: silence, A, B, A, B.

## Why a tool and not another test

An A/B inside the plugin can only compare two things the same build can do:
a knob at two settings, a delay at zero and at 128. A change in STRUCTURE
has no knob -- the old behaviour is gone from the code -- so the two takes
come from two checkouts, and something outside both has to put them next to
each other.

## Why the silence at the front

A wireless headset takes about a second to wake and eats whatever is
playing while it does. Two seconds of silence before the first note means
the first attack is heard rather than swallowed, and the comparison is not
decided by which take happened to be first.
"""

import argparse
import struct
from pathlib import Path

RATE = 48_000
LEAD_IN_S = 2.0
GAP_S = 1.2


def read_f32(path: Path) -> list[float]:
    raw = path.read_bytes()
    if len(raw) % 4:
        raise SystemExit(f"{path}: {len(raw)} bytes is not whole 32-bit floats")
    return list(struct.unpack(f"<{len(raw) // 4}f", raw))


def write_wav(path: Path, samples: list[float]) -> None:
    data = bytearray()
    for sample in samples:
        clipped = max(-32_768, min(32_767, int(sample * 32_767.0)))
        data += struct.pack("<h", clipped)
    header = b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt "
    header += struct.pack("<IHHIIHH", 16, 1, 1, RATE, RATE * 2, 2, 16)
    header += b"data" + struct.pack("<I", len(data))
    path.write_bytes(header + bytes(data))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("a")
    parser.add_argument("b")
    parser.add_argument("out")
    parser.add_argument("--name-a", default="A")
    parser.add_argument("--name-b", default="B")
    arguments = parser.parse_args()

    take_a = read_f32(Path(arguments.a))
    take_b = read_f32(Path(arguments.b))
    gap = [0.0] * int(GAP_S * RATE)
    track = [0.0] * int(LEAD_IN_S * RATE)
    for take in (take_a, take_b, take_a, take_b):
        track += take + gap

    out = Path(arguments.out)
    write_wav(out, track)
    peak_a = max((abs(s) for s in take_a), default=0.0)
    peak_b = max((abs(s) for s in take_b), default=0.0)
    print(f"escrito {out} ({len(track) / RATE:.1f} s)")
    print(f"  {LEAD_IN_S:.0f} s de silencio, luego "
          f"A({arguments.name_a}) B({arguments.name_b}) A B, "
          f"{GAP_S:.1f} s entre tomas")
    print(f"  picos: A {peak_a:.4f}, B {peak_b:.4f}")


if __name__ == "__main__":
    main()
