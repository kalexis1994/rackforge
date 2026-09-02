"""Per-partial decay against frequency, for the three sources at once.

    python tools/survey-piano-decay.py <model-dir> <pianoteq-dir> <salamander-dir> [velocity]

For every note on the grid and partials n = 1..12, the T60 from the early
slope (0.1-1.0 s after onset) and the late slope (1-3 s), tabulated by the
partial's FREQUENCY in octave bands -- because the model's loss law is a
function of frequency and partial number, and a refit needs to see the
target the same way. Also prints, per source, the T60 against partial
number at three notes (A1, C4, C7) so the wave-number dependence shows.
"""

import importlib.util
import math
import os
import sys

import numpy as np

spec = importlib.util.spec_from_file_location(
    "ra", os.path.join(os.path.dirname(os.path.abspath(__file__)), "reanalyze-piano.py"))
ra = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ra)

BANDS = [(25, 50), (50, 100), (100, 200), (200, 400), (400, 800), (800, 1600), (1600, 3200), (3200, 6400), (6400, 12000)]


def survey(root, source, velocity):
    rows = []  # (note, n, freq, early, late)
    for note in ra.NOTES:
        path = ra.source_path(source, root, note, velocity)
        if not os.path.exists(path):
            continue
        x, rate = ra.read_wav(path)
        if float(np.max(np.abs(x))) < 10 ** (-60 / 20):
            continue
        t0 = ra.onset_index(x, rate) / rate
        f0 = ra.fundamental(x, rate, t0, note)
        if f0 is None:
            continue
        B = ra.inharmonicity(x, rate, t0, f0) or 0.0
        for n in range(1, 13):
            freq = n * f0 * math.sqrt(1 + B * n * n)
            if freq > rate * 0.45:
                break
            t = ra.partial_decay(x, rate, t0, f0, B, n, [("early", 0.1, 1.0), ("late", 1.0, 3.0)])
            rows.append((note, n, freq, t["early"], t["late"]))
    return rows


def finite(values):
    v = [x for x in values if x == x and not math.isinf(x) and x > 0]
    return v


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        sys.exit(2)
    roots = {"model": sys.argv[1], "pianoteq": sys.argv[2], "salamander": sys.argv[3]}
    velocity = int(sys.argv[4]) if len(sys.argv) > 4 else 117
    data = {s: survey(r, s, velocity) for s, r in roots.items()}

    print(f"\n== T60 (s) por banda de FRECUENCIA del parcial, mediana [n], v{velocity} ==")
    print("band(Hz)      " + "".join(f"{s+' early':>16}{s+' late':>16}" for s in roots))
    for lo, hi in BANDS:
        cells = []
        for s in roots:
            e = finite([r[3] for r in data[s] if lo <= r[2] < hi])
            l = finite([r[4] for r in data[s] if lo <= r[2] < hi])
            cells.append(f"{np.median(e):9.1f} [{len(e):2d}]  " if e else f"{'-':>9}       ")
            cells.append(f"{np.median(l):9.1f} [{len(l):2d}]  " if l else f"{'-':>9}       ")
        print(f"{lo:5d}-{hi:<5d}   " + "".join(cells))

    for note in (33, 60, 96):
        print(f"\n== nota {note} ({ra.note_name(note)}): T60 early/late por parcial n=1..12 ==")
        for s in roots:
            rows = [r for r in data[s] if r[0] == note]
            if not rows:
                continue
            print(f"  {s:<10}" + " ".join(f"{r[3]:5.1f}/{r[4]:4.1f}" if r[3] == r[3] else "   -   " for r in rows))


if __name__ == "__main__":
    main()
