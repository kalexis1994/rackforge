"""Three-way re-analysis of the piano: our model, Pianoteq, and real recordings.

    python tools/reanalyze-piano.py <model-dir> <pianoteq-dir> <salamander-48khz24bit-dir> [out.json]

Every measurement is made the same way on every source, on the same note
grid (A0 upward in minor thirds) at the same three blows (velocities 36, 60,
117, which are Salamander's layers v3, v8, v15). Levels inside a source are
relative to that source's own first partial or its own peak, never to the
other sources: the one normalisation trap this project has paid for twice.
Frequencies come from each file's own rate, so 44.1 k and 48 k renders sit
side by side untouched.

What is measured per note and blow:

- the partial ladder, n = 1..24, in an EARLY window (60-200 ms after onset)
  and a BODY window (500-800 ms), each level relative to n1 in the same
  window -- density and brightness of the harmonic series, which the band
  fit is blind to;
- the inharmonicity coefficient B, fitted from partials 2..8;
- per-partial decay: T60 of n1, n2, n4 from the early slope (0.1-1.0 s) and
  the late slope (1-3 s);
- the RMS envelope: peak level in dBFS (touch dynamic range across blows),
  time to peak, and level relative to peak at 0.5, 1, 2 and 4 s;
- the power-weighted spectral centroid at the attack and in the body, and
  band ratios 2-4 kHz and 4-8 kHz against 100-1000 Hz.

Pianoteq's trial version mutes some keys; a source file whose peak is under
-60 dBFS is treated as absent, not as a quiet piano.
"""

import json
import math
import os
import sys
import wave

import numpy as np

VELOCITIES = (36, 60, 117)
SALAMANDER_LAYER = {36: 3, 60: 8, 117: 15}
NOTES = list(range(21, 109, 3))
REGISTERS = [("bajo 21-47", 21, 47), ("tenor 48-71", 48, 71), ("agudo 72-108", 72, 108)]
NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
LADDER_N = 24


def note_name(note):
    return f"{NAMES[note % 12]}{note // 12 - 1}"


def read_wav(path):
    with wave.open(path, "rb") as handle:
        channels = handle.getnchannels()
        width = handle.getsampwidth()
        rate = handle.getframerate()
        frames = handle.readframes(handle.getnframes())
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


def db(x):
    return 20.0 * math.log10(max(float(x), 1e-12))


def rms_envelope(x, rate, hop_s=0.005, win_s=0.010):
    hop = max(1, int(rate * hop_s))
    win = max(hop, int(rate * win_s))
    count = max(1, (len(x) - win) // hop)
    env = np.empty(count)
    for i in range(count):
        seg = x[i * hop:i * hop + win]
        env[i] = math.sqrt(float(np.mean(seg * seg)) + 1e-24)
    return env, hop / rate


def onset_index(x, rate):
    env, dt = rms_envelope(x, rate, hop_s=0.001, win_s=0.002)
    peak = env.max()
    threshold = peak * 10 ** (-40 / 20)
    idx = int(np.argmax(env > threshold))
    return int(idx * dt * rate)


def spectrum(x, rate, t_from, t_to, pad_power=18):
    a, b = int(t_from * rate), int(t_to * rate)
    seg = x[a:b]
    if len(seg) < 64:
        return None, None
    seg = seg * np.hanning(len(seg))
    n = 1 << max(pad_power, int(math.ceil(math.log2(len(seg)))))
    mag = np.abs(np.fft.rfft(seg, n)) / len(seg)
    freqs = np.fft.rfftfreq(n, 1.0 / rate)
    return freqs, mag


def peak_near(freqs, mag, centre, tolerance):
    lo = np.searchsorted(freqs, centre - tolerance)
    hi = np.searchsorted(freqs, centre + tolerance)
    if hi - lo < 3:
        return None, None
    k = lo + int(np.argmax(mag[lo:hi]))
    # parabolic interpolation on log magnitude
    if 0 < k < len(mag) - 1 and mag[k] > 0:
        l, c, r = (math.log(max(mag[k - 1], 1e-20)), math.log(max(mag[k], 1e-20)),
                   math.log(max(mag[k + 1], 1e-20)))
        denom = l - 2 * c + r
        delta = 0.5 * (l - r) / denom if abs(denom) > 1e-12 else 0.0
        delta = max(-1.0, min(1.0, delta))
        return freqs[k] + delta * (freqs[1] - freqs[0]), mag[k]
    return freqs[k], mag[k]


def fundamental(x, rate, t0, note):
    expected = 440.0 * 2 ** ((note - 69) / 12)
    freqs, mag = spectrum(x, rate, t0 + 0.05, t0 + 0.45, pad_power=19)
    if freqs is None:
        return None
    f, _ = peak_near(freqs, mag, expected, expected * 0.045)
    return f


def inharmonicity(x, rate, t0, f0):
    freqs, mag = spectrum(x, rate, t0 + 0.05, t0 + 0.45, pad_power=19)
    if freqs is None:
        return None
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
            return None
        ns, ys = np.array(ns, dtype=float), np.array(ys)
        B = max(0.0, float(np.sum(ns * ys) / np.sum(ns * ns)))
    return B


def ladder(x, rate, t0, f0, B, t_from, t_to):
    freqs, mag = spectrum(x, rate, t0 + t_from, t0 + t_to)
    if freqs is None:
        return [float("nan")] * LADDER_N
    levels = []
    for n in range(1, LADDER_N + 1):
        centre = n * f0 * math.sqrt(1 + B * n * n)
        if centre > rate * 0.48:
            levels.append(float("nan"))
            continue
        _, m = peak_near(freqs, mag, centre, max(3.0, centre * 0.015))
        levels.append(db(m) if m is not None else float("nan"))
    base = levels[0]
    return [v - base if not math.isnan(v) else v for v in levels]


def partial_decay(x, rate, t0, f0, B, n, windows):
    centre = n * f0 * math.sqrt(1 + B * n * n)
    win = 4096 if rate > 40000 else 2048
    hop = win // 4
    frames = []
    times = []
    hann = np.hanning(win)
    k = int(round(centre * win / rate))
    if k < 2 or k >= win // 2 - 2:
        return {name: float("nan") for name, _, _ in windows}
    for start in range(0, len(x) - win, hop):
        seg = x[start:start + win] * hann
        spec = np.abs(np.fft.rfft(seg))
        lo, hi = max(0, k - 3), k + 4
        frames.append(db(spec[lo:hi].max()))
        times.append((start + win / 2) / rate - t0)
    times = np.array(times)
    frames = np.array(frames)
    out = {}
    for name, a, b in windows:
        mask = (times >= a) & (times <= b) & (frames > -90)
        if mask.sum() < 4:
            out[name] = float("nan")
            continue
        slope, _ = np.polyfit(times[mask], frames[mask], 1)
        out[name] = 60.0 / -slope if slope < -0.5 else float("inf")
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


def band_ratio(x, rate, t_from, t_to, band, reference=(100.0, 1000.0)):
    freqs, mag = spectrum(x, rate, t_from, t_to, pad_power=16)
    if freqs is None:
        return float("nan")
    p = mag ** 2

    def energy(lo, hi):
        m = (freqs >= lo) & (freqs < hi)
        return float(p[m].sum())

    return 10.0 * math.log10(max(energy(*band), 1e-24) / max(energy(*reference), 1e-24))


def analyse(path, note):
    x, rate = read_wav(path)
    peak = float(np.max(np.abs(x))) if len(x) else 0.0
    if peak < 10 ** (-60 / 20):
        return None
    t0 = onset_index(x, rate) / rate
    f0 = fundamental(x, rate, t0, note)
    if f0 is None:
        return None
    B = inharmonicity(x, rate, t0, f0)
    if B is None:
        B = 0.0
    env, dt = rms_envelope(x, rate)
    env_db = 20 * np.log10(np.maximum(env, 1e-12))
    peak_i = int(np.argmax(env_db))
    peak_db = float(env_db[peak_i])
    milestones = {}
    for t in (0.5, 1.0, 2.0, 4.0):
        i = int((t0 + t) / dt)
        milestones[str(t)] = float(env_db[i] - peak_db) if i < len(env_db) else float("nan")
    return {
        "rate": rate,
        "onset_s": t0,
        "f0": f0,
        "B": B,
        "peak_dbfs": db(peak),
        "time_to_peak_ms": (peak_i * dt - t0) * 1000.0,
        "envelope_rel_peak_db": milestones,
        "ladder_early": ladder(x, rate, t0, f0, B, 0.06, 0.20),
        "ladder_body": ladder(x, rate, t0, f0, B, 0.50, 0.80),
        "t60": {str(n): partial_decay(x, rate, t0, f0, B, n,
                                      [("early", 0.1, 1.0), ("late", 1.0, 3.0)])
                for n in (1, 2, 4)},
        "centroid_attack_hz": centroid(x, rate, t0 + 0.02, t0 + 0.12),
        "centroid_body_hz": centroid(x, rate, t0 + 0.5, t0 + 0.7),
        "band_2_4k_attack_db": band_ratio(x, rate, t0 + 0.02, t0 + 0.15, (2000.0, 4000.0)),
        "band_4_8k_attack_db": band_ratio(x, rate, t0 + 0.02, t0 + 0.15, (4000.0, 8000.0)),
        "band_2_4k_body_db": band_ratio(x, rate, t0 + 0.5, t0 + 0.8, (2000.0, 4000.0)),
    }


def source_path(source, root, note, velocity):
    if source == "model":
        return os.path.join(root, f"model{note:03d}v{velocity}.wav")
    if source == "pianoteq":
        return os.path.join(root, f"pt{note:03d}v{velocity}.wav")
    return os.path.join(root, f"{note_name(note)}v{SALAMANDER_LAYER[velocity]}.wav")


def nanmean(values):
    values = [v for v in values if v is not None and not (isinstance(v, float) and (math.isnan(v) or math.isinf(v)))]
    return float(np.mean(values)) if values else float("nan")


def fmt(v, width=7, digits=1):
    if v is None or (isinstance(v, float) and (math.isnan(v))):
        return "-".rjust(width)
    if isinstance(v, float) and math.isinf(v):
        return "inf".rjust(width)
    return f"{v:{width}.{digits}f}"


def report(results, sources):
    def notes_in(lo, hi):
        return [n for n in NOTES if lo <= n <= hi]

    def val(source, note, velocity, getter):
        entry = results.get(source, {}).get(str(note), {}).get(str(velocity))
        if not entry:
            return None
        try:
            return getter(entry)
        except (KeyError, IndexError, TypeError):
            return None

    def register_mean(source, lo, hi, velocity, getter):
        return nanmean([val(source, n, velocity, getter) for n in notes_in(lo, hi)])

    print("\n== Escalera de parciales, ventana temprana (60-200 ms), dB rel n1, v117 ==")
    ns = [2, 3, 4, 5, 6, 8, 10, 12, 16, 20]
    for name, lo, hi in REGISTERS:
        print(f"-- {name} --  " + "".join(f"n{n}".rjust(7) for n in ns))
        for source in sources:
            row = [register_mean(source, lo, hi, 117, lambda e, n=n: e["ladder_early"][n - 1]) for n in ns]
            print(f"  {source:<9}" + "".join(fmt(v) for v in row))

    print("\n== Escalera de parciales, ventana cuerpo (500-800 ms), dB rel n1, v117 ==")
    for name, lo, hi in REGISTERS:
        print(f"-- {name} --  " + "".join(f"n{n}".rjust(7) for n in ns))
        for source in sources:
            row = [register_mean(source, lo, hi, 117, lambda e, n=n: e["ladder_body"][n - 1]) for n in ns]
            print(f"  {source:<9}" + "".join(fmt(v) for v in row))

    print("\n== Escalera temprana a v36 (pp), dB rel n1 ==")
    for name, lo, hi in REGISTERS:
        print(f"-- {name} --  " + "".join(f"n{n}".rjust(7) for n in ns))
        for source in sources:
            row = [register_mean(source, lo, hi, 36, lambda e, n=n: e["ladder_early"][n - 1]) for n in ns]
            print(f"  {source:<9}" + "".join(fmt(v) for v in row))

    print("\n== Centroide espectral (Hz): ataque 20-120 ms / cuerpo 500-700 ms, por velocidad ==")
    for name, lo, hi in REGISTERS:
        print(f"-- {name} --        " + "".join(f"v{v} att".rjust(10) + f"v{v} body".rjust(10) for v in VELOCITIES))
        for source in sources:
            cells = []
            for v in VELOCITIES:
                cells.append(fmt(register_mean(source, lo, hi, v, lambda e: e["centroid_attack_hz"]), 10, 0))
                cells.append(fmt(register_mean(source, lo, hi, v, lambda e: e["centroid_body_hz"]), 10, 0))
            print(f"  {source:<9}" + "".join(cells))

    print("\n== Bandas en el ataque (20-150 ms), dB rel 100-1000 Hz: 2-4k / 4-8k, por velocidad ==")
    for name, lo, hi in REGISTERS:
        print(f"-- {name} --        " + "".join(f"v{v} 2-4k".rjust(10) + f"v{v} 4-8k".rjust(10) for v in VELOCITIES))
        for source in sources:
            cells = []
            for v in VELOCITIES:
                cells.append(fmt(register_mean(source, lo, hi, v, lambda e: e["band_2_4k_attack_db"]), 10))
                cells.append(fmt(register_mean(source, lo, hi, v, lambda e: e["band_4_8k_attack_db"]), 10))
            print(f"  {source:<9}" + "".join(cells))

    print("\n== Nivel pico (dBFS) por velocidad, y rango dinamico v117-v36 ==")
    for name, lo, hi in REGISTERS:
        print(f"-- {name} --        " + "".join(f"v{v}".rjust(8) for v in VELOCITIES) + "   range")
        for source in sources:
            peaks = [register_mean(source, lo, hi, v, lambda e: e["peak_dbfs"]) for v in VELOCITIES]
            rng = peaks[2] - peaks[0] if not any(math.isnan(p) for p in peaks) else float("nan")
            print(f"  {source:<9}" + "".join(fmt(p, 8) for p in peaks) + fmt(rng, 8))

    print("\n== Tiempo al pico (ms) por velocidad ==")
    for name, lo, hi in REGISTERS:
        print(f"-- {name} --        " + "".join(f"v{v}".rjust(8) for v in VELOCITIES))
        for source in sources:
            print(f"  {source:<9}" + "".join(fmt(register_mean(source, lo, hi, v, lambda e: e["time_to_peak_ms"]), 8) for v in VELOCITIES))

    print("\n== T60 (s) por parcial, pendiente temprana 0.1-1.0 s / tardia 1-3 s, v117 ==")
    for name, lo, hi in REGISTERS:
        print(f"-- {name} --        " + "".join(f"n{n} early".rjust(10) + f"n{n} late".rjust(10) for n in (1, 2, 4)))
        for source in sources:
            cells = []
            for n in (1, 2, 4):
                cells.append(fmt(register_mean(source, lo, hi, 117, lambda e, n=n: e["t60"][str(n)]["early"]), 10))
                cells.append(fmt(register_mean(source, lo, hi, 117, lambda e, n=n: e["t60"][str(n)]["late"]), 10))
            print(f"  {source:<9}" + "".join(cells))

    print("\n== Envolvente RMS rel pico (dB) a 0.5 / 1 / 2 / 4 s, v117 ==")
    for name, lo, hi in REGISTERS:
        print(f"-- {name} --        " + "".join(f"{t}s".rjust(8) for t in ("0.5", "1.0", "2.0", "4.0")))
        for source in sources:
            print(f"  {source:<9}" + "".join(fmt(register_mean(source, lo, hi, 117, lambda e, t=t: e["envelope_rel_peak_db"][t]), 8) for t in ("0.5", "1.0", "2.0", "4.0")))

    print("\n== Inarmonicidad log10(B) por nota (v117) ==")
    print("  note   " + "".join(f"{s:>10}" for s in sources))
    for n in NOTES:
        cells = []
        for s in sources:
            b = val(s, n, 117, lambda e: e["B"])
            cells.append(fmt(math.log10(b) if b and b > 0 else float("nan"), 10, 2))
        print(f"  {n:3d} {note_name(n):<4}" + "".join(cells))


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        sys.exit(2)
    roots = {"model": sys.argv[1], "pianoteq": sys.argv[2], "salamander": sys.argv[3]}
    out = sys.argv[4] if len(sys.argv) > 4 else None
    results = {}
    missing = []
    for source, root in roots.items():
        results[source] = {}
        for note in NOTES:
            for velocity in VELOCITIES:
                path = source_path(source, root, note, velocity)
                if not os.path.exists(path):
                    missing.append((source, note, velocity, "no file"))
                    continue
                entry = analyse(path, note)
                if entry is None:
                    missing.append((source, note, velocity, "silent"))
                    continue
                results[source].setdefault(str(note), {})[str(velocity)] = entry
    if missing:
        print(f"{len(missing)} entradas ausentes o mudas (Pianoteq Trial enmudece teclas):")
        for m in missing[:12]:
            print("  ", m)
    report(results, list(roots))
    if out:
        with open(out, "w", encoding="utf-8") as handle:
            json.dump(results, handle, indent=1)
        print(f"\nJSON: {out}")


if __name__ == "__main__":
    main()
