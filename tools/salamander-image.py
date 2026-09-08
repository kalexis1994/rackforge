"""The reference's stereo image, note by note: what the spaced pair hears from each key.

    python tools/salamander-image.py [--layer 12] [--bank <dir>]

For every sampled note (A0, C1, D#1 ... C8) at one velocity layer, in the
first 50 ms after onset and in 50-300 ms: the level difference between the
channels (right minus left, dB), the time difference by cross-correlation
(positive when the right channel LEADS, microseconds, from the band above
300 Hz where a lag means something), and the arrival difference of the
attack itself (the first crossing of -30 dB under each channel's peak).
Then the lateral position each note would need for a pair at `SPACING`
metres and `HEIGHT` above the strings to produce that lag -- the geometry
the model's board would have to reproduce.
"""

import math
import os
import sys
import wave
from pathlib import Path

import numpy as np

BANK = Path(r"C:\Users\kalex\Downloads\SalamanderGrandPianoV3_48khz24bit\48khz24bit")
NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
SAMPLED = [21 + 3 * i for i in range(30)]
C = 343.0
SPACING = 0.17  # metres between the capsules, the model's MIC_SPACING_M
HEIGHT = 0.12  # metres over the strings, where the reference's pair sits


def name_of(note):
    return f"{NAMES[note % 12]}{note // 12 - 1}"


def load(path):
    with wave.open(str(path)) as w:
        raw = w.readframes(w.getnframes())
        channels, sr = w.getnchannels(), w.getframerate()
    a = np.frombuffer(raw, dtype=np.uint8).reshape(-1, 3)
    x = (a[:, 0].astype(np.int32) | (a[:, 1].astype(np.int32) << 8) | (a[:, 2].astype(np.int8).astype(np.int32) << 16))
    return x.astype(np.float64).reshape(-1, channels) / 8388608.0, sr


def db(v):
    return 20 * math.log10(max(v, 1e-12))


def onset(x, sr):
    e = np.convolve(x * x, np.ones(48) / 48, "valid")
    return int(np.argmax(e > e.max() * 1e-3))


def highpass(x, sr, hz):
    X = np.fft.rfft(x, axis=0)
    f = np.fft.rfftfreq(len(x), 1 / sr)
    X[f < hz] = 0
    return np.fft.irfft(X, len(x), axis=0)


def lag_us(l, r, sr, max_us=600.0):
    """Lag of r relative to l by GCC-PHAT (the phase transform whitens the
    tone, so a 2 kHz note's period does not alias the lag); positive = r
    leads. Bounded by what a close pair can produce."""
    n = len(l)
    L, R = np.fft.rfft(l, 2 * n), np.fft.rfft(r, 2 * n)
    G = L * np.conj(R)
    G /= np.maximum(np.abs(G), 1e-12)
    cc = np.fft.irfft(G, 2 * n)
    cc = np.concatenate((cc[-n:], cc[:n]))  # lags -n..n-1
    m = int(max_us * 1e-6 * sr)
    centre = n
    window = cc[centre - m: centre + m + 1]
    k = int(np.argmax(window)) - m
    # parabolic interpolation
    if 0 < k + m < len(window) - 1:
        y0, y1, y2 = window[k + m - 1], window[k + m], window[k + m + 1]
        d = 0.5 * (y0 - y2) / (y0 - 2 * y1 + y2) if (y0 - 2 * y1 + y2) != 0 else 0.0
    else:
        d = 0.0
    # cc index k>0 means l is delayed relative to r? With G = L*conj(R), the peak at lag k means l(t) ~ r(t-k): r leads by k.
    return (k + d) / sr * 1e6


def envelope_lag_us(l, r, sr, max_us=600.0):
    """Lag of the attack's envelope (1-6 kHz band, rectified and smoothed over 0.2 ms); positive = r leads."""
    def env(x):
        X = np.fft.rfft(x); f = np.fft.rfftfreq(len(x), 1 / sr); X[(f < 1000) | (f > 6000)] = 0
        y = np.abs(np.fft.irfft(X, len(x))); n = max(1, int(0.0002 * sr))
        return np.convolve(y, np.ones(n) / n, "same")
    return lag_us(env(l), env(r), sr, max_us)


def arrival_us(x, sr):
    peak = np.abs(x).max()
    return int(np.argmax(np.abs(x) > peak * 10 ** (-30 / 20))) / sr * 1e6


def lateral_for_lag(itd_s):
    """The lateral offset (metres, positive toward the right capsule) that gives this lag
    for a source at HEIGHT under a pair SPACING apart."""
    best, best_x = 1e30, 0.0
    for x in np.linspace(-1.2, 1.2, 4801):
        d_l = math.hypot(x + SPACING / 2, HEIGHT)
        d_r = math.hypot(x - SPACING / 2, HEIGHT)
        lag = (d_l - d_r) / C
        if abs(lag - itd_s) < best:
            best, best_x = abs(lag - itd_s), x
    return best_x


def main():
    layer = int(sys.argv[sys.argv.index("--layer") + 1]) if "--layer" in sys.argv else 12
    bank = Path(sys.argv[sys.argv.index("--bank") + 1]) if "--bank" in sys.argv else BANK
    print(f"Salamander v{layer}: the pair's image per note (right minus left)")
    print(f" note      | 0-50 ms: ILD dB  ITD(phat) us  attack-envelope us | 50-300 ms: ILD dB  ITD us | lateral m from the attack (pair {SPACING} m, {HEIGHT} m up)")
    rows = []
    for note in SAMPLED:
        path = bank / f"{name_of(note)}v{layer}.wav"
        if not path.is_file():
            continue
        x, sr = load(path)
        at = onset(x.mean(axis=1), sr)
        out = [f"{note:3d} {name_of(note):4s}"]
        itd_first = None
        for a, b in ((0.0, 0.05), (0.05, 0.3)):
            w = x[at + int(a * sr): at + int(b * sr)]
            ild = db(math.sqrt(np.mean(w[:, 1] ** 2))) - db(math.sqrt(np.mean(w[:, 0] ** 2)))
            hp = highpass(w, sr, 300.0)
            itd = lag_us(hp[:, 0], hp[:, 1], sr)
            if itd_first is None:
                first = x[at: at + int(0.012 * sr)]
                attack = envelope_lag_us(first[:, 0], first[:, 1], sr)
                itd_first = attack
                out.append(f"{ild:+6.1f}  {itd:+7.0f}  {attack:+8.0f}")
            else:
                out.append(f"{ild:+6.1f}  {itd:+7.0f}")
        lateral = lateral_for_lag(itd_first * 1e-6)
        out.append(f"{lateral:+6.2f}")
        rows.append((note, itd_first, lateral))
        print(" | ".join(out))
    if rows:
        notes = np.array([r[0] for r in rows]); lat = np.array([r[2] for r in rows])
        p = np.polyfit(notes, lat, 1)
        print(f"\nlateral position vs note, linear fit: {p[1]:+.3f} m + {p[0] * 12:+.4f} m per octave; spread A0..C8 {np.polyval(p, 21):+.2f} .. {np.polyval(p, 108):+.2f} m")


if __name__ == "__main__":
    main()
