"""Fits the Concert Grand per-note calibration table against the YDP targets.

WARNING before you trust a number out of this file: the cost is blind to
spectral DENSITY. It scores band levels inside time windows, so a band holding
the right energy in 24 components scores the same as one holding it in 82 --
and the instrument has 82 where the model has 24 (PIANO_MODEL.md). It also
reads unison beating as decay error, so it actively walks the unison detune
back toward zero, which is what made the bass sound like a plucked string for
forty versions. Never let it touch the unison width.
Coordinate descent over 10 anchors x 9 params; anchors far enough apart to
share no interpolation region are perturbed together. Writes
tools/piano-cal.txt.

The table is seeded from the one compiled into the model, not from 1.0s and
not from a stale tools/piano-cal.txt: the compiled table is the current
instrument, and a run that starts anywhere else throws away every calibration
already fitted.

More sweeps are not better. The cost cannot see a transient traded against a
sustain — the shape term normalises each window by its own strongest band and
the centroid term is level-blind — so a long run buys centroid accuracy by
sharpening the attack and thinning the tone. Six sweeps took the cost from
7981 to 4994 and the mean centroid error to 0.20 octaves, but pinned `chiff`
at its 4.0 ceiling on three anchors and `decay` at its 0.25 floor on two, and
the result measured 1.1 dB more crest factor and 3.2 dB less sustained energy
at 0.3-0.6 s than the three-sweep table that was kept. Parameters against
their bounds mean the fit is compensating for something the model cannot
express; stop and fix the model instead. Always validate a run on measures the
cost does not contain — absolute band levels and crest factor — before
writing its table into lib.rs."""
import importlib.util
import io
import json, os, re, struct, subprocess, sys
import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
AB = os.environ.get("CG_FIT_DIR", os.path.join(ROOT, "target", "fit-renders"))
CAL = os.path.join(ROOT, "tools", "piano-cal.txt")

# The measurements come from the extractor rather than being restated here, so
# the model can never be scored against a target measured a different way.
_spec = importlib.util.spec_from_file_location(
    "targets", os.path.join(ROOT, "tools", "extract-piano-targets.py"))
EX = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(EX)

TARGETS = json.load(
    io.open(os.path.join(ROOT, "tools", "piano-targets.json"), encoding="utf8"))
if TARGETS.get("schema") != 2:
    raise SystemExit(
        "tools/piano-targets.json is the old five-band file. Rebuild it:\n"
        "  python3 tools/extract-piano-targets.py <ydp-grand-piano.sf2>")
NOTES = sorted(int(key) for key in TARGETS["notes"])
PARAMS = 9  # felt, floor, thump, chiff, decay, clang, phantoms, level, treble life
LEVEL = 7   # pinned: the cost is gain-blind, so anything it "finds" here is drift
ANCHORS = [21, 30, 39, 48, 57, 66, 75, 84, 96, 108]

# Each term is an average error in its own unit — dB, or semitones for the
# centroid — so the weights below compare like with like and can be read.
WEIGHT_SHAPE = 1.0     # spectral balance within a window
WEIGHT_DECAY = 1.0     # how far each band falls from the attack to each window
WEIGHT_CENTROID = 1.0  # brightness against pitch, in semitones
WEIGHT_NOISE = 0.6     # the action's knock; the target scatters between takes
os.makedirs(AB, exist_ok=True)

def write_cal(table):
    io.open(CAL, "w", encoding="utf8").write(
        "\n".join(" ".join(f"{v:.4f}" for v in row) for row in table))

def seed_table():
    """The table compiled into the model, so a run continues the calibration
    instead of restarting it. Falls back to tools/piano-cal.txt, then to 1.0s."""
    source = os.path.join(ROOT, "plugins", "concert-grand", "src", "lib.rs")
    rows = re.findall(r"^\s*\[((?:[\d.]+, ){%d}[\d.]+)\],\s*$" % (PARAMS - 1),
                      io.open(source, encoding="utf8").read(), re.M)
    if len(rows) == len(ANCHORS):
        return [[float(v) for v in row.split(", ")] for row in rows]
    table = [[1.0] * PARAMS for _ in ANCHORS]
    if os.path.exists(CAL):
        for r, line in zip(table, io.open(CAL, encoding="utf8").read().splitlines()):
            values = [float(v) for v in line.split()]
            r[:len(values)] = values[:PARAMS]
    return table

def render():
    env = dict(os.environ, CG_RENDER_DIR=AB, CG_CAL=CAL)
    subprocess.run(
        ["cargo", "test", "-p", "rackforge-concert-grand",
         "render_reference", "--release", "--", "--ignored"],
        cwd=ROOT, env=env, capture_output=True, check=True)

def read_wav(path):
    """Reads a mono WAV at either 16 or 24 bits.

    The model's renders moved to 24 bits so that soft dynamics can be measured
    at all -- at 16, a quietly played note's 2-4 kHz band sits on the
    quantisation floor. Reference samples are still 16-bit and read the same.
    """
    data = open(path, "rb").read()
    fmt = data.index(b"fmt ")
    bits = struct.unpack("<H", data[fmt + 22:fmt + 24])[0]
    i = data.index(b"data")
    n = struct.unpack("<I", data[i + 4:i + 8])[0]
    raw = data[i + 8:i + 8 + n]
    if bits == 24:
        byte = np.frombuffer(raw, np.uint8).reshape(-1, 3).astype(np.int32)
        value = byte[:, 0] | (byte[:, 1] << 8) | (
            byte[:, 2].astype(np.uint8).view(np.int8).astype(np.int32) << 16)
        return value.astype(np.float64) / 8388608.0
    return np.frombuffer(raw, dtype="<i2").astype(np.float64) / 32768.0


def measure(x):
    """Exactly what the extractor measured the reference with."""
    x = x / (np.abs(x[:EX.RATE * 2]).max() + 1e-12)
    return (
        EX.windowed(x, EX.WINDOWS, EX.band_levels),
        EX.windowed(x, EX.NOISE_WINDOWS, EX.noisiness),
        EX.centroid_of(x[:EX.RATE]),
    )


def note_cost(bands, noise, centroid, target):
    """Four averages, each in dB or semitones, so the weights are readable."""
    shape, shape_n = 0.0, 0
    decay, decay_n = 0.0, 0
    reference = target["bands"]
    for wi, row in enumerate(bands):
        if row is None or reference[wi] is None:
            continue
        # Within a window, compare the balance and not the level: normalise
        # both by their own strongest band. Normalising by a fixed band turned
        # a third of the compass into noise-driven gradients, because for a
        # treble note most bands are 20 dB down.
        ref0, mod0 = max(reference[wi]), max(row)
        for bi in range(len(EX.BANDS)):
            shape += abs((reference[wi][bi] - ref0) - (row[bi] - mod0))
            shape_n += 1
        # Between windows, compare the fall from the attack. Nothing else here
        # sees a transient traded against a sustain, and without it a run buys
        # centroid accuracy by sharpening the attack and thinning the tone.
        if wi > 0 and bands[0] is not None and reference[0] is not None:
            for bi in range(len(EX.BANDS)):
                decay += abs((row[bi] - bands[0][bi])
                             - (reference[wi][bi] - reference[0][bi]))
                decay_n += 1

    noise_error, noise_n = 0.0, 0
    for wi, value in enumerate(noise):
        aim = target["noisiness"][wi]
        if value is None or aim is None or aim <= 0 or value <= 0:
            continue
        noise_error += abs(10 * np.log10(value / aim))
        noise_n += 1

    semitones = 0.0
    if centroid and target.get("centroid"):
        semitones = abs(12 * np.log2(centroid / target["centroid"]))

    return (
        WEIGHT_SHAPE * (shape / shape_n if shape_n else 0.0)
        + WEIGHT_DECAY * (decay / decay_n if decay_n else 0.0)
        + WEIGHT_CENTROID * semitones
        + WEIGHT_NOISE * (noise_error / noise_n if noise_n else 0.0)
    )


def cost():
    render()
    total = 0.0
    per_note = {}
    for note in NOTES:
        target = TARGETS["notes"].get(str(note))
        path = os.path.join(AB, f"model{note:03}v125.wav")
        if target is None or not os.path.exists(path):
            continue
        c = note_cost(*measure(read_wav(path)), target)
        per_note[note] = c
        total += c
    return total, per_note


ANCHOR_NOTES = {a: [n for n in NOTES if abs(n - a) <= 13] for a in ANCHORS}

table = seed_table()
write_cal(table)
best, best_notes = cost()
print(f"start cost {best:.1f}", flush=True)

# True per-anchor descent. Anchors 27+ semitones apart do not share any
# interpolation region, so a whole group can be perturbed in one render and
# each anchor judged on its OWN neighbourhood cost. The previous fitter moved
# anchors in parity groups and could only ever produce two global constants.
GROUPS = [[0, 4, 8], [1, 5, 9], [2, 6], [3, 7]]
STEP = 0.3
for sweep in range(int(sys.argv[1]) if len(sys.argv) > 1 else 3):
    for param in [p for p in range(PARAMS) if p != LEVEL]:
        for group in GROUPS:
            for direction in (1 + STEP, 1 / (1 + STEP)):
                trial = [row[:] for row in table]
                for a in group:
                    trial[a][param] = min(4.0, max(0.25, trial[a][param] * direction))
                write_cal(trial)
                _, notes = cost()
                keep = [a for a in group
                        if sum(notes.get(n, 0) for n in ANCHOR_NOTES[ANCHORS[a]])
                        < sum(best_notes.get(n, 0) for n in ANCHOR_NOTES[ANCHORS[a]]) - 0.02]
                if keep:
                    for a in keep:
                        table[a][param] = trial[a][param]
                    write_cal(table)
                    best, best_notes = cost()
                else:
                    write_cal(table)
        print(f"sweep {sweep} param {param}: cost {best:.1f}", flush=True)
    STEP *= 0.6
write_cal(table)
print("FINAL", best, flush=True)
for row in table:
    print(" ".join(f"{v:.3f}" for v in row), flush=True)
