"""Fits the Concert Grand per-note calibration table against the YDP targets.
Coordinate descent over 10 anchors x 8 params; alternating anchor parities are
perturbed together (independent regions). Writes tools/piano-cal.txt."""
import json, os, struct, subprocess, sys
import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
AB = os.environ.get("CG_FIT_DIR", os.path.join(ROOT, "target", "fit-renders"))
CAL = os.path.join(ROOT, "tools", "piano-cal.txt")
TARGETS = json.load(open(os.path.join(ROOT, "tools", "piano-targets.json")))
BANDS = [(30, 120), (120, 400), (400, 1200), (1200, 3000), (3000, 8000)]
WINDOWS = [(0.0, 0.08), (0.08, 0.25), (0.3, 0.6), (0.8, 1.2)]
ANCHORS = [21, 30, 39, 48, 57, 66, 75, 84, 96, 108]
NOTES = [21 + 3 * i for i in range(30)]
os.makedirs(AB, exist_ok=True)

def write_cal(table):
    open(CAL, "w").write("\n".join(" ".join(f"{v:.4f}" for v in row) for row in table))

def render():
    env = dict(os.environ, CG_RENDER_DIR=AB, CG_CAL=CAL)
    subprocess.run(
        ["cargo", "+stable-x86_64-pc-windows-msvc", "test", "-p", "rackforge-concert-grand",
         "render_reference", "--release", "--", "--ignored"],
        cwd=ROOT, env=env, capture_output=True, check=True)

def read_wav(path):
    d = open(path, "rb").read(); i = d.index(b"data")
    n = struct.unpack("<I", d[i + 4:i + 8])[0]
    return np.frombuffer(d[i + 8:i + 8 + n], dtype="<i2").astype(np.float64) / 32768.0

def measure(x, rate=44100):
    x = x / (np.abs(x[:rate * 2]).max() + 1e-12)
    rows = []
    for t0, t1 in WINDOWS:
        w = x[int(t0 * rate):int(t1 * rate)]
        S = np.abs(np.fft.rfft(w * np.hanning(len(w)))); F = np.fft.rfftfreq(len(w), 1 / rate)
        rows.append([20 * np.log10(np.sqrt(np.mean(np.abs(S[(F >= lo) & (F < hi)]) ** 2)) + 1e-12)
                     for lo, hi in BANDS])
    w = x[:rate]
    S = np.abs(np.fft.rfft(w * np.hanning(len(w)))); F = np.fft.rfftfreq(len(w), 1 / rate)
    m = (F > 50) & (F < 8000)
    centroid = float(np.sum(S[m] * F[m]) / np.sum(S[m]))
    return rows, centroid

def cost():
    render()
    total = 0.0
    per_note = {}
    for note in NOTES:
        t = TARGETS.get(str(note))
        if t is None:
            continue
        x = read_wav(os.path.join(AB, f"model{note:03}v125.wav"))
        rows, centroid = measure(x)
        c = 0.0
        for wi, row in enumerate(rows):
            # Normalise each window by its own strongest band: for treble
            # notes the 400-1200 band is 20+ dB down and normalising by it
            # turned a third of the compass into noise-driven gradients.
            ref0 = max(t["bands"][wi])
            mod0 = max(row)
            for bi in range(5):
                c += abs((t["bands"][wi][bi] - ref0) - (row[bi] - mod0))
        c += abs(12 * np.log2(centroid / t["centroid"])) * 4.0
        per_note[note] = c
        total += c
    return total, per_note

ANCHOR_NOTES = {a: [n for n in NOTES if abs(n - a) <= 13] for a in ANCHORS}

table = [[1.0] * 8 for _ in range(10)]
if os.path.exists(CAL):
    for r, line in zip(table, open(CAL).read().splitlines()):
        r[:] = [float(v) for v in line.split()]
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
    for param in range(8):
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
