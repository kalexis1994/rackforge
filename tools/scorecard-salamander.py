"""The Concert Grand against ONE reference, on a fixed grid, with one number per register.

    python tools/scorecard-salamander.py run   <tag> [options]     render + score
    python tools/scorecard-salamander.py render <tag> [options]     render the grid only
    python tools/scorecard-salamander.py score  <tag> [--against <other-tag>]

Options for render/run:
    --velocities 36,60,90,117   blows to render (Salamander layers v3, v8, v12, v15)
    --params "index=value,..."  parameter overrides handed to the engine (CG_PARAMS)
    --tuning <file>             a lab tuning file applied before rendering (CG_TUNING)
    --preset <id>               a factory preset loaded first (CG_PRESET, phrases only)
    --no-phrases                skip the listening set
    --jobs N                    concurrent renders (default 4)

## Why one reference

The model has been fitted to a Disklavier at fortissimo, then compared with a
Yamaha C5 (Salamander) and a modelled Steinway D (Pianoteq). Every
correction pulled it toward a different instrument. This tool commits to ONE
reference -- the Salamander Grand Piano V3, the only one on this machine with
a velocity axis -- so that every change is judged against the same piano at
the same blows, on the same notes, by the same code, and the movement between
two builds can be read off as a single table.

## What is measured

The same grid as the reference: 30 notes in minor thirds from A0 (21, 24, ...
108) at four blows. Every level is RELATIVE INSIDE ITS OWN SOURCE (to its own
first partial, its own peak, its own 100-1000 Hz band): the model is never
compared to the reference by absolute level, because the reference's gain
staging belongs to the sample library, not to the piano.

Per note and blow:
- the partial ladder n = 1..24 relative to n1, early (60-200 ms) and body
  (500-800 ms);
- spectral centroid, attack (20-120 ms) and body (500-700 ms);
- band ratios 2-4 kHz and 4-8 kHz against 100-1000 Hz, attack and body;
- attack-over-sustain: 0.5-4 kHz level in the attack (20-150 ms) minus the
  same band in the body (500-800 ms) -- how much of the upper mids is the
  blow and how much is the tone (the model used to knock where the real
  note swells);
- relief and density in 2-4 kHz during 0.1-0.6 s: how far the partials stand
  out of the floor and how many do;
- per-partial T60 for n = 1..16, early slope (0.1-1 s) and late (1-3 s),
  bucketed by the partial's frequency so the loss law can be read as the
  function of frequency it is;
- the RMS envelope relative to its peak at 0.5 / 1 / 2 / 4 s;
- the inharmonicity coefficient B.

Per register (bass 21-47, tenor 48-71, treble 72-108) the deltas model minus
reference are averaged and folded into a DISTANCE in points: 1 point is 1 dB
for a level, 6 points is one octave of centroid or one doubling of a T60.
The distance is not a fit cost to optimise blindly -- its purpose is to make a
change's effect legible, component by component, so the biggest gap is on
top and a regression somewhere else cannot hide behind an improvement.

## Pairing with the ear

`run` and `render` also produce a listening set in the same directory
(bass phrase, tenor phrase, treble run, ten-note chord, one short C4), from
the same build and the same overrides, so what the table describes and what
the ear judges are the same render. Listen BEFORE reading the table.

Outputs live under target/scorecard/<tag>/ (wavs) and
target/scorecard/<tag>.json (measurements). The reference's own analysis is
cached in target/scorecard/salamander.json.
"""

import json
import math
import os
import shutil
import subprocess
import sys
import time
import wave

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_ROOT = os.path.join(ROOT, "target", "scorecard")
REFERENCE = r"C:\Users\kalex\Downloads\SalamanderGrandPianoV3_48khz24bit\48khz24bit"
TOOLCHAIN = "+1.98.0-x86_64-pc-windows-msvc"
ANALYSIS_VERSION = 4

NOTES = list(range(21, 109, 3))
DEFAULT_VELOCITIES = (36, 60, 90, 117)
# Salamander's sfz maps velocity ranges to layers; these are the layers the
# grid's blows fall in.
SALAMANDER_LAYER = {36: 3, 60: 8, 90: 12, 117: 15}
REGISTERS = [("bajo", 21, 47), ("tenor", 48, 71), ("agudo", 72, 108)]
NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
LADDER_N = 24
DECAY_N = 16
T60_BANDS = [(25, 100), (100, 200), (200, 400), (400, 800), (800, 1600), (1600, 3200), (3200, 6400)]
SECONDS = 5.2
RATE = 48000

PHRASES = {
    "bass": "0 2500 36 105\n0 2500 43 105\n2800 2500 30 110\n2800 2500 42 100\n5600 3000 33 100\n5600 3000 40 100\n5600 3000 45 100\n",
    "tenor": "0 1200 55 80\n600 1200 59 70\n1200 1200 62 90\n1800 2000 67 100\n2600 2500 60 60\n2600 2500 64 60\n2600 2500 67 60\n",
    "treble": "0 300 84 100\n250 300 86 90\n500 300 88 95\n750 300 89 100\n1000 300 91 105\n1250 300 93 110\n1500 300 95 115\n1750 2500 96 117\n",
    "chord10": "0 2500 55 117\n0 2500 59 117\n0 2500 62 117\n0 2500 64 117\n0 2500 67 117\n0 2500 71 117\n0 2500 74 117\n0 2500 76 117\n0 2500 79 117\n0 2500 83 117\n",
    "c4short": "0 400 60 100\n",
    "c4pp": "0 2500 60 30\n",
}


def note_name(note):
    return f"{NAMES[note % 12]}{note // 12 - 1}"


def db(x):
    return 20.0 * math.log10(max(float(x), 1e-12))


# ---------------------------------------------------------------- reading


def read_wav_head(path, seconds):
    """The first `seconds` of a WAV as mono float64, channel 0, plus rate."""
    with wave.open(path, "rb") as handle:
        channels = handle.getnchannels()
        width = handle.getsampwidth()
        rate = handle.getframerate()
        frames = handle.readframes(min(handle.getnframes(), int(seconds * rate)))
    if width == 2:
        data = np.frombuffer(frames, dtype="<i2").astype(np.float64) / 32768.0
    elif width == 3:
        raw = np.frombuffer(frames, dtype=np.uint8).reshape(-1, 3)
        data = (raw[:, 0].astype(np.int32) | (raw[:, 1].astype(np.int32) << 8)
                | (raw[:, 2].astype(np.int8).astype(np.int32) << 16)).astype(np.float64) / 8388608.0
    elif width == 4:
        data = np.frombuffer(frames, dtype="<i4").astype(np.float64) / 2147483648.0
    else:
        raise ValueError(f"unsupported sample width {width} in {path}")
    if channels > 1:
        data = data.reshape(-1, channels)[:, 0]
    return data, rate


# ---------------------------------------------------------------- analysis


def rms_envelope(x, rate, hop_s, win_s):
    hop = max(1, int(rate * hop_s))
    win = max(hop, int(rate * win_s))
    count = max(1, (len(x) - win) // hop)
    sq = x * x
    cumulative = np.concatenate([[0.0], np.cumsum(sq)])
    starts = np.arange(count) * hop
    env = np.sqrt((cumulative[starts + win] - cumulative[starts]) / win + 1e-24)
    return env, hop / rate


def onset_index(x, rate):
    env, dt = rms_envelope(x, rate, 0.001, 0.002)
    threshold = env.max() * 10 ** (-40 / 20)
    return int(int(np.argmax(env > threshold)) * dt * rate)


def spectrum(x, rate, t_from, t_to, pad_power=18):
    a, b = int(t_from * rate), int(t_to * rate)
    seg = x[a:b]
    if len(seg) < 64:
        return None, None
    seg = seg * np.hanning(len(seg))
    n = 1 << max(pad_power, int(math.ceil(math.log2(len(seg)))))
    mag = np.abs(np.fft.rfft(seg, n)) / len(seg)
    return np.fft.rfftfreq(n, 1.0 / rate), mag


def peak_near(freqs, mag, centre, tolerance):
    lo = np.searchsorted(freqs, centre - tolerance)
    hi = np.searchsorted(freqs, centre + tolerance)
    if hi - lo < 3:
        return None, None
    k = lo + int(np.argmax(mag[lo:hi]))
    if 0 < k < len(mag) - 1 and mag[k] > 0:
        l, c, r = (math.log(max(mag[k - 1], 1e-20)), math.log(max(mag[k], 1e-20)),
                   math.log(max(mag[k + 1], 1e-20)))
        denom = l - 2 * c + r
        delta = 0.5 * (l - r) / denom if abs(denom) > 1e-12 else 0.0
        delta = max(-1.0, min(1.0, delta))
        return freqs[k] + delta * (freqs[1] - freqs[0]), mag[k]
    return freqs[k], mag[k]


def fundamental(freqs, mag, note):
    expected = 440.0 * 2 ** ((note - 69) / 12)
    f, _ = peak_near(freqs, mag, expected, expected * 0.045)
    return f


def inharmonicity(freqs, mag, f0):
    B = 0.0
    for _ in range(3):
        ns, ys = [], []
        for n in range(2, 9):
            centre = n * f0 * math.sqrt(1 + B * n * n)
            f, _ = peak_near(freqs, mag, centre, max(2.0, centre * 0.02))
            if f is None or f <= 0:
                continue
            ns.append(n * n)
            ys.append((f / (n * f0)) ** 2 - 1.0)
        if len(ns) < 3:
            return 0.0
        ns, ys = np.array(ns, dtype=float), np.array(ys)
        B = max(0.0, float(np.sum(ns * ys) / np.sum(ns * ns)))
    return B


def partial_frequency(f0, B, n):
    return n * f0 * math.sqrt(1 + B * n * n)


def ladder(x, rate, t0, f0, B, t_from, t_to):
    freqs, mag = spectrum(x, rate, t0 + t_from, t0 + t_to)
    if freqs is None:
        return [float("nan")] * LADDER_N
    levels = []
    for n in range(1, LADDER_N + 1):
        centre = partial_frequency(f0, B, n)
        if centre > rate * 0.48:
            levels.append(float("nan"))
            continue
        _, m = peak_near(freqs, mag, centre, max(3.0, centre * 0.015))
        levels.append(db(m) if m is not None else float("nan"))
    base = levels[0]
    return [v - base if not math.isnan(v) else v for v in levels]


def rel_strongest(levels):
    """The same ladder re-based on the strongest of its first six partials:
    the first partial of a bass note is whatever the board lets through of
    it, and a ladder hung from it swings with the board, not the string."""
    head = finite(levels[:6])
    if not head:
        return levels
    top = max(head)
    return [v - top if not math.isnan(v) else v for v in levels]


def stft_db(x, rate):
    win = 4096
    hop = win // 4
    hann = np.hanning(win)
    count = max(0, (len(x) - win) // hop)
    frames = np.empty((count, win // 2 + 1))
    for i in range(count):
        frames[i] = np.abs(np.fft.rfft(x[i * hop:i * hop + win] * hann))
    times = (np.arange(count) * hop + win / 2) / rate
    return 20 * np.log10(np.maximum(frames, 1e-12)), times, win


def partial_decays(spec_db, times, win, rate, t0, f0, B):
    """T60 per partial from the early and late slopes, from one shared STFT."""
    out = {}
    for n in range(1, DECAY_N + 1):
        centre = partial_frequency(f0, B, n)
        k = int(round(centre * win / rate))
        if k < 4 or k >= win // 2 - 4 or centre > rate * 0.45:
            break
        track = spec_db[:, k - 3:k + 4].max(axis=1)
        rel = times - t0
        row = {"hz": centre}
        for name, a, b in (("early", 0.1, 1.0), ("late", 1.0, 3.0)):
            mask = (rel >= a) & (rel <= b) & (track > -90)
            if mask.sum() < 4:
                row[name] = float("nan")
                continue
            slope, _ = np.polyfit(rel[mask], track[mask], 1)
            row[name] = 60.0 / -slope if slope < -0.5 else float("nan")
        out[str(n)] = row
    return out


def centroid(x, rate, t_from, t_to, lo=50.0, hi=10000.0):
    freqs, mag = spectrum(x, rate, t_from, t_to, pad_power=16)
    if freqs is None:
        return float("nan")
    m = (freqs >= lo) & (freqs <= hi)
    p = mag[m] ** 2
    if p.sum() <= 0:
        return float("nan")
    return float(np.sum(freqs[m] * p) / p.sum())


def band_energy_db(freqs, mag, lo, hi):
    m = (freqs >= lo) & (freqs < hi)
    return 10.0 * math.log10(max(float((mag[m] ** 2).sum()), 1e-24))


def relief_and_density(x, rate, t0, lo=2000.0, hi=4000.0):
    """How far the partials stand out of the floor in 2-4 kHz, and how many."""
    freqs, mag = spectrum(x, rate, t0 + 0.1, t0 + 0.6, pad_power=17)
    if freqs is None:
        return float("nan"), float("nan")
    m = (freqs >= lo) & (freqs < hi)
    band = 20 * np.log10(np.maximum(mag[m], 1e-12))
    if len(band) < 16:
        return float("nan"), float("nan")
    whole = 20 * np.log10(np.maximum(mag[(freqs >= 20) & (freqs < 10000)], 1e-12))
    top = float(whole.max())
    # Local maxima at least 6 dB over both neighbours 8 bins away (a partial,
    # not a ripple of the window).
    step = 8
    core = band[step:-step]
    peaks = (core > band[:-2 * step] + 6) & (core > band[2 * step:] + 6)
    peak_levels = core[peaks]
    floor = float(np.median(band))
    density = int(np.sum(peak_levels > top - 45))
    relief = float(np.mean(np.sort(peak_levels)[-10:]) - floor) if len(peak_levels) else float("nan")
    return relief, density


def analyse(path, note):
    x, rate = read_wav_head(path, SECONDS)
    if not len(x) or float(np.max(np.abs(x))) < 10 ** (-60 / 20):
        return None
    t0 = onset_index(x, rate) / rate
    freqs, mag = spectrum(x, rate, t0 + 0.05, t0 + 0.45, pad_power=19)
    if freqs is None:
        return None
    f0 = fundamental(freqs, mag, note)
    if f0 is None:
        return None
    B = inharmonicity(freqs, mag, f0)
    env, dt = rms_envelope(x, rate, 0.005, 0.010)
    env_db = 20 * np.log10(np.maximum(env, 1e-12))
    peak_i = int(np.argmax(env_db))
    peak_db = float(env_db[peak_i])
    milestones = {}
    for t in (0.5, 1.0, 2.0, 4.0):
        i = int((t0 + t) / dt)
        milestones[str(t)] = float(env_db[i] - peak_db) if i < len(env_db) else float("nan")
    fa, ma = spectrum(x, rate, t0 + 0.02, t0 + 0.15, pad_power=16)
    fb, mb = spectrum(x, rate, t0 + 0.5, t0 + 0.8, pad_power=16)
    # Bands are fractions of the note's WHOLE energy (50 Hz - 10 kHz), not of
    # 100-1000 Hz: for a treble note that band holds no tone, only the knock
    # and the room, and a ratio against it measures the floor, not the colour.
    ref_a = band_energy_db(fa, ma, 50, 10000)
    ref_b = band_energy_db(fb, mb, 50, 10000)
    spec_db, times, win = stft_db(x, rate)
    relief, density = relief_and_density(x, rate, t0)
    return {
        "f0": f0,
        "B": B,
        "peak_dbfs": db(np.max(np.abs(x))),
        "time_to_peak_ms": (peak_i * dt - t0) * 1000.0,
        "envelope": milestones,
        "ladder_early": ladder(x, rate, t0, f0, B, 0.06, 0.20),
        "ladder_body": ladder(x, rate, t0, f0, B, 0.50, 0.80),
        "centroid_attack": centroid(x, rate, t0 + 0.02, t0 + 0.12),
        "centroid_body": centroid(x, rate, t0 + 0.5, t0 + 0.7),
        "band_2_4k_attack": band_energy_db(fa, ma, 2000, 4000) - ref_a,
        "band_4_8k_attack": band_energy_db(fa, ma, 4000, 8000) - ref_a,
        "band_2_4k_body": band_energy_db(fb, mb, 2000, 4000) - ref_b,
        "band_4_8k_body": band_energy_db(fb, mb, 4000, 8000) - ref_b,
        "attack_over_sustain": band_energy_db(fa, ma, 500, 4000) - band_energy_db(fb, mb, 500, 4000),
        # What sits under the tone: everything below 0.7 f0 (the knock, the
        # board, the room), relative to the whole. A gap the treble exposes.
        "floor_below_f0_attack": band_energy_db(fa, ma, 30, max(31.0, 0.7 * f0)) - ref_a,
        "floor_below_f0_body": band_energy_db(fb, mb, 30, max(31.0, 0.7 * f0)) - ref_b,
        "relief_2_4k": relief,
        "density_2_4k": density,
        "t60": partial_decays(spec_db, times, win, rate, t0, f0, B),
    }


# ---------------------------------------------------------------- sources


def model_path(directory, note, velocity):
    return os.path.join(directory, f"model{note:03d}v{velocity}.wav")


def reference_path(note, velocity):
    return os.path.join(REFERENCE, f"{note_name(note)}v{SALAMANDER_LAYER[velocity]}.wav")


def analyse_source(paths, label, cache=None):
    """{note: {velocity: analysis}} for a dict {(note, velocity): path}."""
    results = {}
    started = time.time()
    done = 0
    for (note, velocity), path in sorted(paths.items()):
        key = f"{note}:{velocity}"
        if cache is not None and key in cache:
            entry = cache[key]
        elif os.path.exists(path):
            entry = analyse(path, note)
            if cache is not None:
                cache[key] = entry
        else:
            entry = None
        if entry is not None:
            results.setdefault(str(note), {})[str(velocity)] = entry
        done += 1
    print(f"  {label}: {done} archivos analizados en {time.time() - started:.0f} s")
    return results


# ---------------------------------------------------------------- rendering


def test_binary():
    """The release test executable of the plugin crate, built if needed."""
    result = subprocess.run(
        ["cargo", TOOLCHAIN, "test", "--release", "-p", "rackforge-concert-grand",
         "--lib", "--no-run", "--message-format=json"],
        cwd=ROOT, capture_output=True, text=True, check=True)
    for line in result.stdout.splitlines():
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get("reason") == "compiler-artifact" and message.get("executable") \
                and message.get("target", {}).get("name") == "rackforge_concert_grand":
            return message["executable"]
    raise SystemExit("no encuentro el binario de tests de rackforge-concert-grand")


def render_environment(options, directory):
    env = dict(os.environ, CG_RENDER_DIR=directory, CG_RATE=str(RATE))
    if options.get("params"):
        env["CG_PARAMS"] = options["params"]
    if options.get("tuning"):
        env["CG_TUNING"] = os.path.abspath(options["tuning"])
    if options.get("preset"):
        env["CG_PRESET"] = options["preset"]
    return env


def launch(binary, env, log):
    return subprocess.Popen(
        [binary, "tests::render_reference_wavs", "--ignored", "--exact", "--nocapture"],
        cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)


def render(tag, options):
    directory = os.path.join(OUT_ROOT, tag)
    os.makedirs(directory, exist_ok=True)
    binary = test_binary()
    velocities = options["velocities"]
    jobs = max(1, options.get("jobs", 4))
    print(f"render: {len(NOTES)} notas x {len(velocities)} velocidades a {RATE} Hz en {directory}")
    started = time.time()
    queue = []
    for velocity in velocities:
        env = render_environment(options, directory)
        env["CG_CHROMATIC"] = "1"
        env["CG_VELOCITY"] = str(velocity)
        queue.append((f"v{velocity}", env))
    if not options.get("no_phrases"):
        for name, score in PHRASES.items():
            sub = os.path.join(directory, "phrases", name)
            os.makedirs(sub, exist_ok=True)
            with open(os.path.join(sub, "score.txt"), "w", encoding="utf-8") as handle:
                handle.write(score)
            env = render_environment(options, sub)
            env["CG_SCORE"] = os.path.join(sub, "score.txt")
            queue.append((f"phrase {name}", env))
    running = []
    logs = []
    while queue or running:
        while queue and len(running) < jobs:
            label, env = queue.pop(0)
            log = open(os.path.join(directory, f"render-{label.replace(' ', '-')}.log"), "w")
            logs.append(log)
            running.append((label, launch(binary, env, log)))
        for label, process in list(running):
            code = process.poll()
            if code is None:
                continue
            running.remove((label, process))
            if code != 0:
                raise SystemExit(f"render {label} fallo con codigo {code}; ver los .log en {directory}")
            print(f"  {label} listo ({time.time() - started:.0f} s)")
        time.sleep(0.5)
    for log in logs:
        log.close()
    for name in PHRASES:
        source = os.path.join(directory, "phrases", name, "score.wav")
        if os.path.exists(source):
            shutil.move(source, os.path.join(directory, f"phrase-{name}.wav"))
    with open(os.path.join(directory, "render.json"), "w", encoding="utf-8") as handle:
        json.dump({"tag": tag, "options": options, "rate": RATE, "rendered": time.ctime()}, handle, indent=1)
    print(f"render: {time.time() - started:.0f} s")


# ---------------------------------------------------------------- scoring


def finite(values):
    return [v for v in values if v is not None and isinstance(v, (int, float))
            and not (isinstance(v, float) and (math.isnan(v) or math.isinf(v)))]


def mean(values):
    v = finite(values)
    return float(np.mean(v)) if v else float("nan")


def median(values):
    v = finite(values)
    return float(np.median(v)) if v else float("nan")


def std(values):
    v = finite(values)
    return float(np.std(v)) if len(v) > 1 else float("nan")


def entry(results, note, velocity):
    return results.get(str(note), {}).get(str(velocity))


def notes_in(lo, hi):
    return [n for n in NOTES if lo <= n <= hi]


def summarise(results, velocities):
    """Register-level summaries of one source, all relative inside the source."""
    summary = {}
    for name, lo, hi in REGISTERS:
        reg = {}
        for velocity in velocities:
            entries = [e for e in (entry(results, n, velocity) for n in notes_in(lo, hi)) if e]
            v = {}
            v["ladder_early"] = [mean([e["ladder_early"][i] for e in entries]) for i in range(LADDER_N)]
            v["ladder_body"] = [mean([e["ladder_body"][i] for e in entries]) for i in range(LADDER_N)]
            v["ladder_early_top"] = [mean([rel_strongest(e["ladder_early"])[i] for e in entries]) for i in range(LADDER_N)]
            v["ladder_body_top"] = [mean([rel_strongest(e["ladder_body"])[i] for e in entries]) for i in range(LADDER_N)]
            for key in ("centroid_attack", "centroid_body", "band_2_4k_attack", "band_4_8k_attack",
                        "band_2_4k_body", "band_4_8k_body", "attack_over_sustain",
                        "floor_below_f0_attack", "floor_below_f0_body",
                        "relief_2_4k", "density_2_4k", "time_to_peak_ms"):
                v[key] = mean([e[key] for e in entries])
            v["envelope"] = {t: mean([e["envelope"][t] for e in entries]) for t in ("0.5", "1.0", "2.0", "4.0")}
            rows = [(p["hz"], p["early"], p["late"]) for e in entries for p in e["t60"].values()]
            v["t60_bands"] = {}
            for blo, bhi in T60_BANDS:
                inb = [r for r in rows if blo <= r[0] < bhi]
                v["t60_bands"][f"{blo}-{bhi}"] = {
                    "early": median([r[1] for r in inb]),
                    "late": median([r[2] for r in inb]),
                    "count": len(inb),
                }
            v["peak_std"] = std([e["peak_dbfs"] for e in entries])
            v["notes"] = len(entries)
            reg[str(velocity)] = v
        summary[name] = reg
    return summary


def log2_ratio(a, b):
    if a is None or b is None or not (a > 0) or not (b > 0):
        return float("nan")
    return math.log2(a / b)


def components(model, reference, velocities):
    """Per register: the named gaps, model against reference, in points."""
    out = {}
    loud = str(max(velocities))
    soft = str(min(velocities))
    for name, _, _ in REGISTERS:
        m, r = model[name], reference[name]
        comps = []

        def add(label, value, kind="dB"):
            if value is None or (isinstance(value, float) and (math.isnan(value) or math.isinf(value))):
                return
            comps.append({"name": label, "delta": value, "kind": kind})

        for velocity in (loud, soft):
            tag = "ff" if velocity == loud else "pp"
            for window in ("early", "body"):
                mv, rv = m[velocity][f"ladder_{window}_top"], r[velocity][f"ladder_{window}_top"]
                diffs = finite([mv[i] - rv[i] for i in range(0, 12)])
                if diffs:
                    add(f"escalera {window} {tag} n1-12 rel la mas fuerte (media firmada)", float(np.mean(diffs)))
                    add(f"escalera {window} {tag} n1-12 rel la mas fuerte (|delta| media)", float(np.mean(np.abs(diffs))), "abs")
            add(f"centroide ataque {tag}", 6 * log2_ratio(m[velocity]["centroid_attack"], r[velocity]["centroid_attack"]), "oct")
            add(f"centroide cuerpo {tag}", 6 * log2_ratio(m[velocity]["centroid_body"], r[velocity]["centroid_body"]), "oct")
            for key, label in (("band_2_4k_attack", "2-4k ataque"), ("band_4_8k_attack", "4-8k ataque"),
                               ("band_2_4k_body", "2-4k cuerpo"), ("band_4_8k_body", "4-8k cuerpo"),
                               ("attack_over_sustain", "ataque sobre sustain 0.5-4k"),
                               ("floor_below_f0_attack", "suelo bajo f0 ataque"),
                               ("floor_below_f0_body", "suelo bajo f0 cuerpo")):
                add(f"{label} {tag}", m[velocity][key] - r[velocity][key])
        add("relieve 2-4k ff", m[loud]["relief_2_4k"] - r[loud]["relief_2_4k"])
        add("densidad 2-4k ff (picos)", float(m[loud]["density_2_4k"] - r[loud]["density_2_4k"]), "count")
        for t in ("0.5", "1.0", "2.0", "4.0"):
            add(f"envolvente {t} s ff", m[loud]["envelope"][t] - r[loud]["envelope"][t])
        for band, mb in m[loud]["t60_bands"].items():
            rb = r[loud]["t60_bands"][band]
            if mb["count"] >= 3 and rb["count"] >= 3:
                add(f"T60 temprano {band} Hz ff", 6 * log2_ratio(mb["early"], rb["early"]), "oct")
                add(f"T60 tardio {band} Hz ff", 6 * log2_ratio(mb["late"], rb["late"]), "oct")
        # Dynamics: how much the blow changes things, model against reference.
        add("dinamica: centroide ataque ff-pp", 6 * (log2_ratio(m[loud]["centroid_attack"], m[soft]["centroid_attack"])
                                                    - log2_ratio(r[loud]["centroid_attack"], r[soft]["centroid_attack"])), "oct")
        add("dinamica: 2-4k ataque ff-pp", (m[loud]["band_2_4k_attack"] - m[soft]["band_2_4k_attack"])
            - (r[loud]["band_2_4k_attack"] - r[soft]["band_2_4k_attack"]))
        add("dinamica: 4-8k ataque ff-pp", (m[loud]["band_4_8k_attack"] - m[soft]["band_4_8k_attack"])
            - (r[loud]["band_4_8k_attack"] - r[soft]["band_4_8k_attack"]))
        for n in (4, 8):
            add(f"dinamica: escalera temprana n{n} ff-pp",
                (m[loud]["ladder_early_top"][n - 1] - m[soft]["ladder_early_top"][n - 1])
                - (r[loud]["ladder_early_top"][n - 1] - r[soft]["ladder_early_top"][n - 1]))
        add("parejo nota a nota: std del pico ff (aviso)", m[loud]["peak_std"] - r[loud]["peak_std"], "advisory")
        scored = [abs(c["delta"]) for c in comps if c["kind"] in ("dB", "oct", "abs")]
        out[name] = {"components": comps, "distance": float(np.mean(scored)) if scored else float("nan")}
    return out


def fmt(v, width=7, digits=1):
    if v is None or (isinstance(v, float) and math.isnan(v)):
        return "-".rjust(width)
    if isinstance(v, float) and math.isinf(v):
        return "inf".rjust(width)
    return f"{v:{width}.{digits}f}"


def print_report(tag, model, reference, comps, velocities, against=None):
    loud, soft = str(max(velocities)), str(min(velocities))
    print(f"\n==== SCORECARD {tag} contra Salamander (Yamaha C5), 30 notas x {len(velocities)} velocidades ====")
    print("Puntos: 1 = 1 dB; 6 = una octava de centroide o un doblez de T60. Delta = modelo menos referencia.")
    for name, _, _ in REGISTERS:
        d = comps[name]["distance"]
        line = f"\n-- {name.upper()}: distancia {d:.2f} puntos"
        if against:
            line += f" (antes {against[name]['distance']:.2f}, {against[name]['distance'] - d:+.2f})"
        print(line)
        rows = sorted(comps[name]["components"], key=lambda c: -abs(c["delta"]))
        before = {c["name"]: c["delta"] for c in against[name]["components"]} if against else {}
        for c in rows:
            mark = ""
            if c["name"] in before:
                moved = abs(before[c["name"]]) - abs(c["delta"])
                if abs(moved) >= 0.5:
                    mark = f"   {'mejora' if moved > 0 else 'EMPEORA'} {moved:+.1f} (antes {before[c['name']]:+.1f})"
            unit = {"dB": "dB", "oct": "pt", "abs": "dB", "count": "picos", "advisory": "dB"}[c["kind"]]
            print(f"   {c['delta']:+7.1f} {unit:<5} {c['name']}{mark}")

    ns = [1, 2, 3, 4, 5, 6, 8, 10, 12, 16]
    for window in ("early", "body"):
        print(f"\n== Escalera {window} rel la mas fuerte de n1-6 (dB), ff / pp ==" + "".join(f"n{n}".rjust(7) for n in ns))
        for name, _, _ in REGISTERS:
            for velocity, tag_v in ((loud, "ff"), (soft, "pp")):
                print(f"  {name:<6}{tag_v} modelo " + "".join(fmt(model[name][velocity][f'ladder_{window}_top'][n - 1]) for n in ns))
                print(f"  {name:<6}{tag_v} salam. " + "".join(fmt(reference[name][velocity][f'ladder_{window}_top'][n - 1]) for n in ns))

    print("\n== Bandas rel energia total 50-10k (dB), ff: 2-4k att/cuerpo, 4-8k att/cuerpo, suelo bajo f0 att/cuerpo: modelo | Salamander ==")
    for name, _, _ in REGISTERS:
        mv, rv = model[name][loud], reference[name][loud]
        keys = ("band_2_4k_attack", "band_2_4k_body", "band_4_8k_attack", "band_4_8k_body", "floor_below_f0_attack", "floor_below_f0_body")
        print(f"  {name:<6} " + " ".join(fmt(mv[k], 6) for k in keys) + " | " + " ".join(fmt(rv[k], 6) for k in keys))

    print("\n== T60 por banda de frecuencia (s), temprano/tardio, ff: modelo | Salamander ==")
    print("  banda        " + "".join(f"{name:>16}" for name, _, _ in REGISTERS))
    for band in [f"{lo}-{hi}" for lo, hi in T60_BANDS]:
        line = f"  {band:<12}"
        for name, _, _ in REGISTERS:
            mb, rb = model[name][loud]["t60_bands"][band], reference[name][loud]["t60_bands"][band]
            line += f" {fmt(mb['early'], 4)}/{fmt(mb['late'], 4)}|{fmt(rb['early'], 4)}/{fmt(rb['late'], 4)}"
        print(line)

    print("\n== Centroide (Hz) ataque/cuerpo por velocidad: modelo | Salamander ==")
    for name, _, _ in REGISTERS:
        line = f"  {name:<6}"
        for velocity in velocities:
            mv, rv = model[name][str(velocity)], reference[name][str(velocity)]
            line += f"  v{velocity}: {fmt(mv['centroid_attack'], 5, 0)}/{fmt(mv['centroid_body'], 5, 0)}|{fmt(rv['centroid_attack'], 5, 0)}/{fmt(rv['centroid_body'], 5, 0)}"
        print(line)

    print("\n== Envolvente rel pico (dB) a 0.5/1/2/4 s, ff: modelo | Salamander ==")
    for name, _, _ in REGISTERS:
        mv, rv = model[name][loud]["envelope"], reference[name][loud]["envelope"]
        print(f"  {name:<6} " + "/".join(fmt(mv[t], 6) for t in mv) + " | " + "/".join(fmt(rv[t], 6) for t in rv))

    print("\n== Ataque sobre sustain 0.5-4 kHz (dB; negativo = el ataque queda bajo el cuerpo), por velocidad ==")
    for name, _, _ in REGISTERS:
        line = f"  {name:<6}"
        for velocity in velocities:
            line += f"  v{velocity}: {fmt(model[name][str(velocity)]['attack_over_sustain'], 6)}|{fmt(reference[name][str(velocity)]['attack_over_sustain'], 6)}"
        print(line)


def score(tag, against_tag=None):
    directory = os.path.join(OUT_ROOT, tag)
    with open(os.path.join(directory, "render.json"), encoding="utf-8") as handle:
        render_info = json.load(handle)
    velocities = render_info["options"]["velocities"]
    cache_path = os.path.join(OUT_ROOT, "salamander.json")
    cache = {}
    if os.path.exists(cache_path):
        with open(cache_path, encoding="utf-8") as handle:
            stored = json.load(handle)
        if stored.get("version") == ANALYSIS_VERSION:
            cache = stored["entries"]
    print("analisis:")
    reference = analyse_source({(n, v): reference_path(n, v) for n in NOTES for v in velocities}, "Salamander", cache)
    with open(cache_path, "w", encoding="utf-8") as handle:
        json.dump({"version": ANALYSIS_VERSION, "entries": cache}, handle)
    model = analyse_source({(n, v): model_path(directory, n, v) for n in NOTES for v in velocities}, tag)
    model_summary = summarise(model, velocities)
    reference_summary = summarise(reference, velocities)
    comps = components(model_summary, reference_summary, velocities)
    against = None
    if against_tag:
        with open(os.path.join(OUT_ROOT, f"{against_tag}.json"), encoding="utf-8") as handle:
            against = json.load(handle)["components"]
    print_report(tag, model_summary, reference_summary, comps, velocities, against)
    with open(os.path.join(OUT_ROOT, f"{tag}.json"), "w", encoding="utf-8") as handle:
        json.dump({"tag": tag, "version": ANALYSIS_VERSION, "render": render_info,
                   "components": comps, "model": model_summary, "reference": reference_summary,
                   "notes": model}, handle, indent=1)
    print(f"\nJSON: target/scorecard/{tag}.json")
    print(f"Escucha: target/scorecard/{tag}/phrase-*.wav (misma build, mismos overrides que la tabla)")


def parse(argv):
    if len(argv) < 3:
        print(__doc__)
        sys.exit(2)
    command, tag = argv[1], argv[2]
    options = {"velocities": list(DEFAULT_VELOCITIES), "jobs": 4}
    against = None
    i = 3
    while i < len(argv):
        arg = argv[i]
        if arg == "--velocities":
            options["velocities"] = [int(v) for v in argv[i + 1].split(",")]
            i += 2
        elif arg == "--params":
            options["params"] = argv[i + 1]
            i += 2
        elif arg == "--tuning":
            options["tuning"] = argv[i + 1]
            i += 2
        elif arg == "--preset":
            options["preset"] = argv[i + 1]
            i += 2
        elif arg == "--jobs":
            options["jobs"] = int(argv[i + 1])
            i += 2
        elif arg == "--no-phrases":
            options["no_phrases"] = True
            i += 1
        elif arg == "--against":
            against = argv[i + 1]
            i += 2
        else:
            raise SystemExit(f"opcion desconocida {arg}")
    for velocity in options["velocities"]:
        if velocity not in SALAMANDER_LAYER:
            raise SystemExit(f"velocidad {velocity} sin capa Salamander asignada; usa {sorted(SALAMANDER_LAYER)}")
    return command, tag, options, against


def main():
    command, tag, options, against = parse(sys.argv)
    os.makedirs(OUT_ROOT, exist_ok=True)
    if command in ("render", "run"):
        render(tag, options)
    if command in ("score", "run"):
        score(tag, against)
    if command not in ("render", "run", "score"):
        raise SystemExit(f"comando desconocido {command}")


if __name__ == "__main__":
    main()
