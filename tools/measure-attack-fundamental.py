#!/usr/bin/env python3
"""The attack, model against the YDP reference.

    CG_RENDER_DIR=target/fit-renders CG_CAL=tools/piano-cal.txt       cargo test -p rackforge-concert-grand render_reference --release -- --ignored
    python tools/measure-attack-fundamental.py . target/fit-renders <label>

A `CG_TUNING` file of `NAME = value` lines sets any knob in the registry
before the render, which is how the levers in `PIANO_MIDRANGE.md` were swept.

Why this exists rather than the fit cost: `fit-piano-cal.py` normalises each
window by its own strongest band, so it scores balance and cannot tell "more
high content" from "less fundamental". The mid register's defect is exactly
that distinction.


  fundamental   how far the fundamental's band sits below the window's
                strongest band, model minus reference, over the three attack
                windows (0-250 ms) of the anchors 51..81. Negative means the
                model's fundamental is the weaker one -- the defect.
  brillo        the 2-8 kHz bands relative to the strongest band, same
                windows, model minus reference. Positive means the model's
                attack is the brighter one.

Plus the fit's own cost split by register, so nothing is traded away unseen.
"""
import importlib.util, os, sys
import numpy as np

ROOT = sys.argv[1]
RENDERS = sys.argv[2]
LABEL = sys.argv[3] if len(sys.argv) > 3 else "baseline"
sys.path.insert(0, os.path.join(ROOT, "tools"))
spec = importlib.util.spec_from_file_location("fit", os.path.join(ROOT, "tools", "fit-piano-cal.py"))
fit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fit)
T = fit.TARGETS["notes"]
B = fit.EX.BANDS


def band_of(hz):
    for i, (a, b) in enumerate(B):
        if a <= hz < b:
            return i
    return len(B) - 1


fundamental, brillo, total, per = [], [], 0.0, {}
for note in fit.NOTES:
    path = os.path.join(RENDERS, f"model{note:03}v125.wav")
    if not os.path.exists(path) or str(note) not in T:
        continue
    bands, noise, cent = fit.measure(fit.read_wav(path))
    tgt = T[str(note)]
    per[note] = fit.note_cost(bands, noise, cent, tgt)
    total += per[note]
    if not (51 <= note <= 81):
        continue
    ref = tgt["bands"]
    fb = band_of(440.0 * 2 ** ((note - 69) / 12))
    for w in (0, 1, 2):
        if bands[w] is None or ref[w] is None:
            continue
        r0, m0 = max(ref[w]), max(bands[w])
        fundamental.append((bands[w][fb] - m0) - (ref[w][fb] - r0))
        for k in (6, 7):
            brillo.append((bands[w][k] - m0) - (ref[w][k] - r0))

bass = sum(v for n, v in per.items() if n < 51)
mid = sum(v for n, v in per.items() if 51 <= n <= 81)
top = sum(v for n, v in per.items() if n > 81)
print(
    f"{LABEL:<26} fundamental {np.mean(fundamental):+6.1f} dB  brillo {np.mean(brillo):+6.1f} dB  "
    f"cost {total:7.1f}  bajo {bass:6.1f}  medio {mid:6.1f}  agudo {top:6.1f}",
    flush=True,
)
