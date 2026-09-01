"""How much the model brightens between two blows, against a real piano.

    python tools/measure-dynamic-slope.py [render-dir] [soft] [loud]

With no render directory it renders the compass itself, at both velocities.

The model is calibrated only at fortissimo -- every note in
`tools/piano-targets.json` is at velocity 120-127 -- so nothing has ever
checked how its timbre travels between a soft blow and a hard one. That
journey is what `tools/piano-dynamics.json` measures on a real instrument,
and this compares ours to it.

What is compared is the CHANGE, not the balance: each sample, on both sides,
is normalised by its own peak, and then the loud reading has the soft one
subtracted from it. So a piano that is simply darker than ours scores zero
here as long as it brightens by the same amount, which is the point --
Salamander is a different instrument and only the shape of its dynamics is
borrowed. See `extract-piano-dynamics.py` for why that is the only honest
thing to take from it.

The cost is the mean absolute error in dB over the bands above 250 Hz. Below
that a note's balance barely moves with velocity on either instrument, so
including those bands mostly averages the interesting error away.
"""

import importlib.util
import json
import os
import re
import subprocess
import sys

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _load(name, path):
    spec = importlib.util.spec_from_file_location(name, os.path.join(ROOT, "tools", path))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


EX = _load("targets", "extract-piano-targets.py")
DY = _load("dynamics", "extract-piano-dynamics.py")

#: The window the comparison reads: the note's body, once the strike's own
#: noise has passed and before the sustain has thinned it.
WINDOW = 2
#: Below this the balance hardly moves with velocity on either instrument.
FIRST_BAND = 3
REGISTERS = [("bass 21-47", 21, 47), ("tenor 48-71", 48, 71), ("treble 72-108", 72, 108)]


def render(into, velocity):
    os.makedirs(into, exist_ok=True)
    environment = dict(
        os.environ, CG_CHROMATIC="1", CG_VELOCITY=str(velocity), CG_RENDER_DIR=into)
    subprocess.run(
        ["cargo", "+1.98.0-x86_64-pc-windows-msvc", "test", "--release",
         "-p", "rackforge-concert-grand", "render_reference", "--", "--ignored"],
        cwd=ROOT, env=environment, capture_output=True, check=True)


def model_bands(directory):
    """Every rendered note: its band levels in the window, and its peak.

    The peak is carried along because the two halves of dynamics pull against
    each other. Everything else here is measured after normalising each render
    by its own peak, which is what makes the timbre comparable -- but the knob
    that most changes how much a note brightens is also the one that sets how
    much LOUDER it gets, and quietening the brightening by narrowing the action
    would buy it with dynamic range the instrument does not have to spare. So
    the loudness range is reported beside the cost rather than left to be
    discovered later.
    """
    bands, peaks = {}, {}
    for name in os.listdir(directory):
        match = re.fullmatch(r"model(\d+)v(\d+)\.wav", name)
        if not match:
            continue
        samples, rate, _ = DY.read_wav_start(os.path.join(directory, name), 2.5)
        EX.RATE = rate
        key = (int(match.group(1)), int(match.group(2)))
        peaks[key] = float(np.abs(samples).max())
        measured = EX.measure(samples)[0][WINDOW]
        if measured is not None:
            bands[key] = np.array(measured)
    return bands, peaks


def reference_slope(reference, soft_layer, loud_layer):
    """The real piano's change, per note."""
    out = {}
    for key, note in reference["notes"].items():
        layers = {layer["layer"]: layer for layer in note["layers"]}
        soft, loud = layers.get(soft_layer), layers.get(loud_layer)
        if not soft or not loud:
            continue
        if soft["bands"][WINDOW] is None or loud["bands"][WINDOW] is None:
            continue
        out[int(key)] = np.array(loud["bands"][WINDOW]) - np.array(soft["bands"][WINDOW])
    return out


def layer_for(reference, velocity):
    """Which of the sixteen layers a MIDI velocity would actually play."""
    any_note = next(iter(reference["notes"].values()))
    for layer in any_note["layers"]:
        if layer["lovel"] <= velocity <= layer["hivel"]:
            return layer["layer"]
    raise SystemExit(f"velocity {velocity} falls outside every layer")


def main():
    directory = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "target", "dyn")
    soft = int(sys.argv[2]) if len(sys.argv) > 2 else 35
    loud = int(sys.argv[3]) if len(sys.argv) > 3 else 116
    if len(sys.argv) <= 1:
        render(directory, soft)
        render(directory, loud)

    reference = json.load(
        open(os.path.join(ROOT, "tools", "piano-dynamics.json"), encoding="utf-8"))
    real = reference_slope(reference, layer_for(reference, soft), layer_for(reference, loud))
    model, peaks = model_bands(directory)

    bands = reference["bands"]
    head = "".join(f"{lo}-{hi}".rjust(10) for lo, hi in bands)
    print(f"brightening from velocity {soft} to {loud}, window "
          f"{reference['windows'][WINDOW]} s")
    print("(+ = that band weighs more when struck hard)\n")
    print(f"{'':<14}{head}")
    errors = []
    for name, low, high in REGISTERS:
        theirs, ours = [], []
        for pitch, slope in real.items():
            if not low <= pitch <= high:
                continue
            if (pitch, soft) in model and (pitch, loud) in model:
                theirs.append(slope)
                ours.append(model[(pitch, loud)] - model[(pitch, soft)])
        if not theirs:
            continue
        theirs, ours = np.mean(theirs, axis=0), np.mean(ours, axis=0)
        errors.append(np.abs(ours - theirs)[FIRST_BAND:])
        print(f"{name:<14}" + "".join(f"{v:+10.2f}" for v in theirs) + "   real")
        print(f"{'':<14}" + "".join(f"{v:+10.2f}" for v in ours) + "   model")
        print(f"{'':<14}" + "".join(f"{v:+10.2f}" for v in ours - theirs) + "   error\n")
    spread = [20 * np.log10(peaks[(p, loud)] / peaks[(p, soft)])
              for p in {q for q, _ in peaks}
              if (p, soft) in peaks and (p, loud) in peaks and peaks[(p, soft)] > 0]
    if spread:
        print(f"loudness range velocity {soft} to {loud}: "
              f"{np.mean(spread):.1f} dB (a real piano spans 30 to 40 dB "
              f"across its whole range)")
    if errors:
        print(f"COST {np.mean(np.concatenate(errors)):.2f} dB "
              f"(mean absolute error above {bands[FIRST_BAND][0]} Hz)")


if __name__ == "__main__":
    main()
