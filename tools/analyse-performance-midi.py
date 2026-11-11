#!/usr/bin/env python3
"""What a real performance actually asks of a piano, in voices and in attacks.

    python3 tools/analyse-performance-midi.py FILE.midi [FILE.midi ...]

RackForge's polyphony ramp presses N notes and holds them, which is a
worst case nobody plays: twelve keys struck inside one 2.7 ms block is not
a chord, it is a cluster. A real performance spreads a chord over tens of
milliseconds, and its sustain pedal leaves far more notes RINGING than the
hands ever hold at once.

Those are two different costs and this separates them, because they load
the engine in different places:

* **Voices sounding** is the steady cost -- every ringing string is
  rendered every block whether it was struck now or four seconds ago. The
  sustain pedal is what drives this, and it is the number the ramp was
  trying to measure.
* **Attacks per block** is the transient cost, and it is the one that
  misses deadlines: a note-on builds a partial ladder, and on a Raspberry
  Pi 4 that is 42-47 % of it in the strike simulation alone. What matters
  is not how many notes a chord has but how many land in the SAME block.

Pure standard-library: it runs on the appliance, where installing a MIDI
package to answer a question about the appliance is the wrong trade.
"""

import struct
import sys
from pathlib import Path

# 128 frames at 48 kHz, the appliance's period.
BLOCK_SECONDS = 128 / 48_000


def read_variable(data: bytes, at: int) -> tuple[int, int]:
    """A MIDI variable-length quantity: seven bits per byte, high bit continues."""
    value = 0
    while True:
        byte = data[at]
        at += 1
        value = (value << 7) | (byte & 0x7F)
        if not byte & 0x80:
            return value, at


def read_events(path: Path) -> tuple[list[tuple[int, int, int, int]], int]:
    """Every channel event as (tick, status, data1, data2), and ticks per beat.

    Tracks are merged by tick, which is what a player sees. Tempo changes
    come back as status 0xFF so the caller can turn ticks into seconds.
    """
    data = path.read_bytes()
    if data[:4] != b"MThd":
        raise SystemExit(f"{path}: not a MIDI file")
    _, _, tracks, division = struct.unpack(">IHHH", data[4:14])
    if division & 0x8000:
        raise SystemExit(f"{path}: SMPTE timing is not handled")
    at = 14
    events: list[tuple[int, int, int, int]] = []
    for _ in range(tracks):
        if data[at : at + 4] != b"MTrk":
            raise SystemExit(f"{path}: expected a track at byte {at}")
        length = struct.unpack(">I", data[at + 4 : at + 8])[0]
        end = at + 8 + length
        at += 8
        tick = 0
        status = 0
        while at < end:
            delta, at = read_variable(data, at)
            tick += delta
            byte = data[at]
            if byte & 0x80:
                status = byte
                at += 1
            # else: running status, `status` carries over and `at` stays put.
            if status == 0xFF:
                kind = data[at]
                at += 1
                length_meta, at = read_variable(data, at)
                payload = data[at : at + length_meta]
                at += length_meta
                if kind == 0x51:  # tempo, microseconds per beat
                    micros = int.from_bytes(payload, "big")
                    events.append((tick, 0xFF, micros, 0))
            elif status in (0xF0, 0xF7):
                length_sysex, at = read_variable(data, at)
                at += length_sysex
            else:
                high = status & 0xF0
                if high in (0xC0, 0xD0):
                    events.append((tick, high, data[at], 0))
                    at += 1
                else:
                    events.append((tick, high, data[at], data[at + 1]))
                    at += 2
        at = end
    events.sort(key=lambda event: event[0])
    return events, division


def timeline(events, division):
    """(seconds, status, data1, data2), honouring tempo changes."""
    micros_per_beat = 500_000
    seconds = 0.0
    tick = 0
    out = []
    for event_tick, status, one, two in events:
        seconds += (event_tick - tick) / division * micros_per_beat / 1e6
        tick = event_tick
        if status == 0xFF:
            micros_per_beat = one
            continue
        out.append((seconds, status, one, two))
    return out


def analyse(path: Path) -> None:
    events, division = read_events(path)
    played = timeline(events, division)
    if not played:
        print(f"{path.name}: sin eventos")
        return

    # A note stops being rendered when its key is up AND the pedal is up.
    # That is what makes a pedalled passage expensive: the hands have moved
    # on and the strings are still ringing.
    held: set[int] = set()
    pedalled: set[int] = set()
    pedal_down = False
    sounding_peak = 0
    sounding_at = 0.0
    attacks: list[float] = []
    notes = 0

    for seconds, status, one, two in played:
        if status == 0x90 and two > 0:
            held.add(one)
            pedalled.discard(one)
            attacks.append(seconds)
            notes += 1
        elif status == 0x80 or (status == 0x90 and two == 0):
            if one in held:
                held.discard(one)
                if pedal_down:
                    pedalled.add(one)
        elif status == 0xB0 and one == 64:
            was = pedal_down
            pedal_down = two >= 64
            if was and not pedal_down:
                pedalled.clear()
        sounding = len(held) + len(pedalled)
        if sounding > sounding_peak:
            sounding_peak, sounding_at = sounding, seconds

    # How many attacks land inside one render block, and inside a hand's
    # worth of milliseconds. The first is what a note-on block has to
    # absorb; the second is what a listener calls a chord.
    def busiest(window: float) -> tuple[int, float]:
        best, best_at, start = 0, 0.0, 0
        for end in range(len(attacks)):
            while attacks[end] - attacks[start] > window:
                start += 1
            if end - start + 1 > best:
                best, best_at = end - start + 1, attacks[start]
        return best, best_at

    per_block, block_at = busiest(BLOCK_SECONDS)
    per_20ms, _ = busiest(0.020)
    per_50ms, _ = busiest(0.050)
    duration = played[-1][0]

    print(f"\n{path.name}")
    print(f"  {duration / 60:.1f} min, {notes} notas, {notes / duration:.1f} por segundo")
    print(f"  voces sonando, pico:        {sounding_peak:>3}  (a los {sounding_at / 60:.1f} min)")
    print(f"  ataques en un bloque de 2.7 ms: {per_block:>3}  (a los {block_at / 60:.1f} min)")
    print(f"  ataques en 20 ms:               {per_20ms:>3}")
    print(f"  ataques en 50 ms:               {per_50ms:>3}")


def main() -> None:
    if len(sys.argv) < 2:
        raise SystemExit(__doc__.strip().splitlines()[2].strip())
    for name in sys.argv[1:]:
        analyse(Path(name))


if __name__ == "__main__":
    main()
