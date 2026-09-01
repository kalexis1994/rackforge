"""How much of a note is its attack, and whether that recedes with the blow.

    python tools/measure-attack-vs-velocity.py [render-dir]

With no render directory it renders the compass itself, at four velocities.

## What this measures, and why the existing tools do not

`measure-dynamic-slope.py` asks how a note's TIMBRE travels from soft to loud,
inside one window well after the strike. `measure-attack-floor.py` asks how
loud the broadband bed under the partials is, at one velocity. Neither asks
the question a player asks with their hands: as I play softer, does the hammer
get out of the way?

So this takes the ratio of the strike itself to the note it leaves behind --
0-30 ms against 80-250 ms -- and watches that ratio as the blow softens. On a
real piano it falls: a soft blow means a slow hammer, a long contact time, and
felt that never leaves its soft regime, so the strike loses more than the tone
does. A model whose ratio is FLATTER than the reference has a hammer that
stays present when it should recede, which is heard as every note being struck
equally hard however gently it is played.

Reported broadband and again over 1-4 kHz, because that band is where the
hammer's own noise lives and where the ear places "attack".

## The two traps this walks around

Windows are aligned to the ONSET, not to the start of the file, and by the
same detector on both sides. Salamander's recordings carry 9-12 ms of room
before the strike; the model's renders begin at it. Measuring both from sample
zero lays the reference's attack window across silence it does not have and
reads a difference that is only bookkeeping.

The quantity is a RATIO INTERNAL to each recording, so it survives the fact
that Salamander applies its own `amp_veltrack` gain at playback and that the
library and the model are staged differently. Nothing here compares an
absolute level across the two instruments.
"""

import importlib.util
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


DY = _load("dynamics", "extract-piano-dynamics.py")

#: The strike, and the note it leaves behind.
ATTACK = (0.000, 0.030)
BODY = (0.080, 0.250)
#: Where the hammer's own noise lives and where the ear places "attack".
HAMMER_BAND = (1000.0, 4000.0)
#: Velocities spanning pianissimo to fortissimo.
VELOCITIES = [25, 50, 80, 115]
REGISTERS = [("bass 21-47", 21, 47), ("tenor 48-71", 48, 71), ("treble 72-108", 72, 108)]


def onset(x, rate):
    """The strike, as the first crossing of a tenth of the early peak.

    Deliberately crude and deliberately the SAME on both sides: what matters
    is not that it finds the exact sample the hammer touched, but that it
    makes the same choice for the reference and for the render.
    """
    head = x[: int(0.100 * rate)]
    if head.size == 0:
        return 0
    peak = np.abs(head).max()
    if peak <= 0:
        return 0
    above = np.flatnonzero(np.abs(head) >= 0.10 * peak)
    return int(above[0]) if above.size else 0


def _level(segment, rate, band=None):
    """Level in dB over a segment, broadband or inside one band."""
    if segment.size < 64:
        return None
    if band is None:
        power = float(np.mean(segment.astype(np.float64) ** 2))
    else:
        spectrum = np.abs(np.fft.rfft(segment * np.hanning(segment.size))) ** 2
        freqs = np.fft.rfftfreq(segment.size, 1.0 / rate)
        inside = (freqs >= band[0]) & (freqs < band[1])
        if inside.sum() < 3:
            return None
        power = float(np.mean(spectrum[inside]))
    return 10.0 * np.log10(power + 1e-30)


def attack_ratio(x, rate):
    """Strike over body, broadband and in the hammer band, in dB."""
    start = onset(x, rate)
    out = []
    for band in (None, HAMMER_BAND):
        levels = []
        for lo, hi in (ATTACK, BODY):
            segment = x[start + int(lo * rate): start + int(hi * rate)]
            levels.append(_level(segment, rate, band))
        out.append(None if None in levels else levels[0] - levels[1])
    return out


def reference():
    """Salamander, per pitch and per velocity: the same two ratios."""
    root = os.environ.get(
        "SALAMANDER",
        os.path.join(os.path.expanduser("~"), "Downloads",
                     "SalamanderGrandPianoV3_48khz24bit"))
    sfz = next((os.path.join(root, name) for name in sorted(os.listdir(root))
                if name.endswith(".sfz") and "Retuned" not in name), None)
    if sfz is None:
        raise SystemExit(f"no .sfz in {root} (set SALAMANDER to point at it)")
    wanted = {}
    for region in DY.regions(open(sfz, encoding="utf-8", errors="replace").read()):
        for velocity in VELOCITIES:
            if region["lovel"] <= velocity <= region["hivel"]:
                wanted[(region["pitch"], velocity)] = region["file"]
    out = {}
    for (pitch, velocity), name in sorted(wanted.items()):
        path = os.path.join(root, name)
        if not os.path.exists(path):
            continue
        x, rate, _ = DY.read_wav_start(path, 2.5)
        out[(pitch, velocity)] = attack_ratio(x, rate)
    return out


def render(into, velocity):
    os.makedirs(into, exist_ok=True)
    environment = dict(
        os.environ, CG_CHROMATIC="1", CG_VELOCITY=str(velocity), CG_RENDER_DIR=into)
    subprocess.run(
        ["cargo", "+1.98.0-x86_64-pc-windows-msvc", "test", "--release",
         "-p", "rackforge-concert-grand", "render_reference", "--", "--ignored"],
        cwd=ROOT, env=environment, capture_output=True, check=True)


def model(directory):
    out = {}
    for name in os.listdir(directory):
        match = re.fullmatch(r"model(\d+)v(\d+)\.wav", name)
        if not match:
            continue
        x, rate, _ = DY.read_wav_start(os.path.join(directory, name), 2.5)
        out[(int(match.group(1)), int(match.group(2)))] = attack_ratio(x, rate)
    return out


def main():
    directory = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "target", "attack")
    if len(sys.argv) <= 1:
        for velocity in VELOCITIES:
            render(directory, velocity)

    real, ours = reference(), model(directory)
    print("Strike (0-30 ms) over body (80-250 ms), in dB. Higher = more hammer.")
    print("The column that matters is the LAST: how much the attack recedes as")
    print("the blow softens. A model that recedes less keeps its hammer after")
    print("the player has already let go of it.\n")

    for column, title in enumerate(
            ("broadband", f"{HAMMER_BAND[0]:.0f}-{HAMMER_BAND[1]:.0f} Hz")):
        print(f"== {title} ==")
        head = "".join(f"v{v}".rjust(9) for v in VELOCITIES)
        print(f"{'':<18}{head}     soft->loud")
        for label, low, high in REGISTERS:
            for who, table in (("real", real), ("model", ours)):
                row = []
                for velocity in VELOCITIES:
                    values = [table[(p, velocity)][column]
                              for p in range(low, high + 1)
                              if (p, velocity) in table
                              and table[(p, velocity)][column] is not None]
                    row.append(float(np.mean(values)) if values else None)
                if any(v is None for v in row):
                    continue
                print(f"{label if who == 'real' else '':<12}{who:<6}"
                      + "".join(f"{v:9.2f}" for v in row)
                      + f"{row[-1] - row[0]:+10.2f}")
            print()


if __name__ == "__main__":
    main()
