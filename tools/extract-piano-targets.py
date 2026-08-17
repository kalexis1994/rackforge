"""Measures the YDP Grand samples into the target file the fitter scores against.

The reference is the openly licensed YDP Grand Piano SoundFont that RF-Soundfonts
ships. It is not in this repository — it is 118 MB of recorded audio — so this
reads it from a path given on the command line:

    python3 tools/extract-piano-targets.py <ydp-grand-piano.sf2> [out.json]

Extract it from the published package with any zip tool:

    curl -L -o rf.rfplugin https://github.com/kalexis1994/rackforge-plugin-rf-soundfonts/releases/latest/download/RF-Soundfonts.rfplugin
    python3 -c "import zipfile;zipfile.ZipFile('rf.rfplugin').extract('assets/ydp-grand-piano.sf2')"

## Why this replaces the old target file

The previous targets held five bands over four windows. That resolution hid two
defects that had to be found by hand against the recordings:

* **1-4 kHz in the sustain.** The old `(1200, 3000)` and `(3000, 8000)` bands
  straddled it. Measured directly, the model's bass sustain sits 10.4 dB under
  the reference at C2 and 10.5 dB under at C3 — the upper partials that give a
  bass note its pitch and its life, missing, under a fundamental that is 10.9 dB
  too loud. A percussive attack over a dead sustain is what a clavinet is.
* **The attack's broadband knock.** Nothing in a band-energy target separates
  energy sitting on the partials from the noise between them, and the model's
  attack is far cleaner than the reference everywhere: measured as a noise
  floor, 1.0% of C4's attack energy against the recording's 9.6%, 1.5% against
  37.9% at C5, 0.3% against 15.1% at C8. That is the action — the hammer, the
  key, the board struck as a plate — and no arrangement of partial amplitudes
  can stand in for it.

So the bands are octaves, there are six windows instead of four, and every note
carries a noisiness target per window.

Noisiness is measured as a noise floor rather than as off-grid energy. Summing
what falls between the harmonics sounds equivalent and is not: the harmonic grid
covers most of the spectrum under a bass note and almost none of it under C8,
where two partials sit below 10 kHz, so the same recording reads 0% in the bass
and 84% at the top from geometry alone. Taking the median of the power spectrum
in each third-octave rejects the partial peaks whatever their spacing, which
makes the number comparable across the compass.
"""

import json
import os
import struct
import sys

import numpy as np

RATE = 44_100
BANDS = [(30, 60), (60, 120), (120, 250), (250, 500), (500, 1000),
         (1000, 2000), (2000, 4000), (4000, 8000)]
WINDOWS = [(0.0, 0.03), (0.03, 0.08), (0.08, 0.25), (0.3, 0.6), (0.8, 1.2), (1.6, 2.2)]
#: Noisiness gets its own, longer windows. A median needs bins to take the
#: middle of: over 30 ms the resolution is 33 Hz, so a third-octave low in the
#: range holds one or two bins and is skipped, and what survives is scattered
#: enough to read 2% on one note and 77% on its neighbour. Over 80 ms the
#: resolution is 12.5 Hz and the estimate settles.
NOISE_WINDOWS = [(0.0, 0.08), (0.08, 0.3), (0.5, 1.0)]
#: A sample shorter than this much of a window is not measured through it.
COVERAGE = 0.9
#: Only the hardest blow of each note: the model renders its reference at 125.
MINIMUM_VELOCITY = 118


def read_chunks(data, offset, end):
    while offset < end:
        cid = data[offset:offset + 4]
        size = struct.unpack("<I", data[offset + 4:offset + 8])[0]
        body = offset + 8
        yield cid, body, size
        offset = body + size + (size & 1)


def parse(path):
    """The sample pool and the sample headers of a SoundFont 2 file."""
    data = open(path, "rb").read()
    if data[:4] != b"RIFF" or data[8:12] != b"sfbk":
        raise SystemExit(f"{path} is not a SoundFont 2 file")
    pool, headers = None, []
    for cid, body, size in read_chunks(data, 12, len(data)):
        if cid != b"LIST":
            continue
        for inner, at, length in read_chunks(data, body + 4, body + size):
            if inner == b"smpl":
                pool = np.frombuffer(data[at:at + length], dtype="<i2")
            elif inner == b"shdr":
                for off in range(at, at + length - 46 + 1, 46):
                    name = data[off:off + 20].split(b"\0")[0].decode("ascii", "replace")
                    start, end, _ls, _le, rate, pitch, _corr = struct.unpack(
                        "<IIIIIBb", data[off + 20:off + 42])
                    if end > start:
                        headers.append((name, start, end, rate, pitch))
    if pool is None:
        raise SystemExit("the SoundFont holds no sample pool")
    return pool, headers


def velocity_of(name):
    tail = name[-3:]
    return int(tail) if tail.isdigit() else 0


def loudest_per_note(headers):
    """One sample per note: the hardest blow, which is what the model renders."""
    best = {}
    for name, start, end, rate, pitch in headers:
        velocity = velocity_of(name)
        if rate != RATE or velocity < MINIMUM_VELOCITY:
            continue
        if pitch not in best or velocity > velocity_of(best[pitch][0]):
            best[pitch] = (name, start, end, rate, pitch)
    return dict(sorted(best.items()))


def band_levels(segment):
    spectrum = np.abs(np.fft.rfft(segment * np.hanning(len(segment)))) ** 2
    freqs = np.fft.rfftfreq(len(segment), 1 / RATE)
    out = []
    for lo, hi in BANDS:
        inside = (freqs >= lo) & (freqs < hi)
        out.append(float(10 * np.log10(spectrum[inside].sum() + 1e-20)))
    return out


def noisiness(segment, lo=200.0, hi=10_000.0):
    """The noise between the partials, as a share of the band's energy.

    The median of the power spectrum in a third-octave is an estimate of that
    band's floor: half the bins sit below it, and the partial peaks — however
    few or many — are all in the upper half. Integrating the floor and dividing
    by the total gives a noisiness that does not depend on how densely the
    harmonics happen to fill the spectrum, so a C8 and a C2 can be compared.
    """
    spectrum = np.abs(np.fft.rfft(segment * np.hanning(len(segment)))) ** 2
    freqs = np.fft.rfftfreq(len(segment), 1 / RATE)
    inside = (freqs >= lo) & (freqs < hi)
    total = spectrum[inside].sum()
    if total <= 0:
        return None
    floor = 0.0
    edge = lo
    while edge < hi:
        top = min(edge * 2 ** (1 / 3), hi)
        band = (freqs >= edge) & (freqs < top)
        # A third-octave holding fewer than four bins has no usable median.
        if band.sum() >= 4:
            floor += float(np.median(spectrum[band])) * int(band.sum())
        edge = top
    return float(floor / total)


def centroid_of(segment):
    spectrum = np.abs(np.fft.rfft(segment * np.hanning(len(segment))))
    freqs = np.fft.rfftfreq(len(segment), 1 / RATE)
    inside = (freqs > 50) & (freqs < 10_000)
    weight = spectrum[inside].sum()
    return float((spectrum[inside] * freqs[inside]).sum() / weight) if weight > 0 else None


def windowed(x, windows, measurement):
    out = []
    for start, stop in windows:
        first, last = int(start * RATE), int(stop * RATE)
        if len(x) < first + COVERAGE * (last - first):
            out.append(None)
            continue
        out.append(measurement(x[first:min(last, len(x))]))
    return out


def measure(x):
    x = x / (np.abs(x[:RATE * 2]).max() + 1e-12)
    return windowed(x, WINDOWS, band_levels), windowed(x, NOISE_WINDOWS, noisiness)


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__.strip().splitlines()[2])
    sf2 = sys.argv[1]
    out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(
        os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
        "tools", "piano-targets.json")

    pool, headers = parse(sf2)
    chosen = loudest_per_note(headers)
    notes = {}
    for pitch, (name, start, end, _rate, _p) in chosen.items():
        x = np.asarray(pool[start:end], dtype=np.float64) / 32768.0
        bands, noise = measure(x)
        notes[str(pitch)] = {
            "pitch": pitch,
            "vel": velocity_of(name),
            "sample": name,
            "seconds": round(len(x) / RATE, 3),
            "bands": bands,
            "noisiness": noise,
            "centroid": centroid_of(x[:RATE]),
        }
        covered = sum(1 for b in bands if b is not None)
        print(f"  {name:16} note {pitch:3}  {len(x)/RATE:5.2f}s  "
              f"{covered}/{len(WINDOWS)} windows  "
              f"attack noise {100 * (noise[0] or 0):5.1f}%")

    json.dump(
        {
            "schema": 2,
            "source": os.path.basename(sf2),
            "rate": RATE,
            "bands": BANDS,
            "windows": WINDOWS,
            "noise_windows": NOISE_WINDOWS,
            "notes": notes,
        },
        open(out, "w"),
        indent=1,
    )
    print(f"\n{len(notes)} notes written to {out}")


if __name__ == "__main__":
    main()
