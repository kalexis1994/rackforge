"""A Standard MIDI File as the Concert Grand lab's score: one event per line.

    python tools/midi-to-score.py <file.mid> [out.txt] [--channel N] [--transpose N]

Lines are `onset_ms duration_ms note velocity` for notes and
`onset_ms pedal|sostenuto|soft 0..127` for CC 64 / 66 / 67, in milliseconds
from the first event, with the file's tempo map honoured. Every track and
channel is merged (drums excluded), so a two-hand piano file comes out as
one instrument, which is what a piano is. No dependencies: the parser is
the forty lines a format 0/1 file needs.
"""

import struct
import sys


def read_var(data, i):
    value = 0
    while True:
        byte = data[i]
        i += 1
        value = (value << 7) | (byte & 0x7F)
        if byte < 0x80:
            return value, i


def parse(path):
    data = open(path, "rb").read()
    assert data[:4] == b"MThd", "not a MIDI file"
    fmt, tracks, division = struct.unpack(">HHH", data[8:14])
    assert division & 0x8000 == 0, "SMPTE timing is not handled"
    i = 14
    tempo_changes = []  # (tick, us_per_quarter)
    events = []  # (tick, order, kind, a, b, channel)
    order = 0
    for _ in range(tracks):
        assert data[i:i + 4] == b"MTrk"
        length = struct.unpack(">I", data[i + 4:i + 8])[0]
        j, end = i + 8, i + 8 + length
        tick = 0
        status = 0
        while j < end:
            delta, j = read_var(data, j)
            tick += delta
            byte = data[j]
            if byte == 0xFF:
                meta = data[j + 1]
                size, k = read_var(data, j + 2)
                if meta == 0x51:
                    tempo_changes.append((tick, int.from_bytes(data[k:k + 3], "big")))
                j = k + size
                continue
            if byte in (0xF0, 0xF7):
                size, k = read_var(data, j + 1)
                j = k + size
                continue
            if byte >= 0x80:
                status = byte
                j += 1
            kind = status & 0xF0
            channel = status & 0x0F
            if kind in (0xC0, 0xD0):
                j += 1
                continue
            a, b = data[j], data[j + 1]
            j += 2
            if channel == 9:
                continue
            if kind == 0x90 and b > 0:
                events.append((tick, order, "on", a, b, channel))
            elif kind == 0x80 or (kind == 0x90 and b == 0):
                events.append((tick, order, "off", a, b, channel))
            elif kind == 0xB0 and a in (64, 66, 67):
                events.append((tick, order, "cc", a, b, channel))
            order += 1
        i = end
    return division, sorted(set(tempo_changes)), sorted(events)


def tick_to_ms(tick, division, tempos):
    """Milliseconds at `tick`, walking the tempo map (500000 us/quarter until the first change)."""
    us = 0.0
    last_tick, tempo = 0, 500000
    for change_tick, change_tempo in tempos:
        if change_tick >= tick:
            break
        us += (change_tick - last_tick) * tempo / division
        last_tick, tempo = change_tick, change_tempo
    us += (tick - last_tick) * tempo / division
    return us / 1000.0


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    flags = sys.argv[1:]
    if not args:
        print(__doc__)
        sys.exit(2)
    transpose = int(flags[flags.index("--transpose") + 1]) if "--transpose" in flags else 0
    only = int(flags[flags.index("--channel") + 1]) if "--channel" in flags else None
    division, tempos, events = parse(args[0])
    if not tempos:
        tempos = [(0, 500000)]
    open_notes = {}
    lines = []
    first = None
    for tick, _, kind, a, b, channel in events:
        if only is not None and channel != only:
            continue
        ms = tick_to_ms(tick, division, tempos)
        if first is None:
            first = ms
        ms -= first
        if kind == "on":
            note = a + transpose
            if (channel, a) in open_notes:  # re-struck before its off: close the old one here
                start, vel = open_notes.pop((channel, a))
                lines.append((start, 0, f"{int(start)} {max(1, int(ms - start))} {note} {vel}"))
            open_notes[(channel, a)] = (ms, b)
        elif kind == "off":
            if (channel, a) in open_notes:
                start, vel = open_notes.pop((channel, a))
                lines.append((start, 0, f"{int(start)} {max(1, int(ms - start))} {a + transpose} {vel}"))
        else:
            name = {64: "pedal", 66: "sostenuto", 67: "soft"}[a]
            lines.append((ms, 1, f"{int(ms)} {name} {b}"))
    for (channel, a), (start, vel) in open_notes.items():
        lines.append((start, 0, f"{int(start)} 2000 {a + transpose} {vel}"))
    lines.sort()
    out = args[1] if len(args) > 1 else args[0].rsplit(".", 1)[0] + ".txt"
    with open(out, "w", encoding="utf-8") as handle:
        handle.write(f"# {args[0]}: {len(events)} events, division {division}, {len(tempos)} tempo changes\n")
        handle.write("\n".join(line for _, _, line in lines) + "\n")
    notes = sum(1 for _, k, _ in lines if k == 0)
    pedals = len(lines) - notes
    length = max((float(l.split()[0]) for _, _, l in lines), default=0) / 1000
    print(f"{out}: {notes} notes, {pedals} pedal events, {length:.1f} s")


if __name__ == "__main__":
    main()
