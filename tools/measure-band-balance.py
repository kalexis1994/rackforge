"""Signed per-band residual of the model against the YDP reference.

The fit cost in `fit-piano-cal.py` sums absolute errors, so it says how far
the model is from the reference but never which way. When a listener reports
that something is too bright or too thick, that is a question about direction,
and this is the instrument that answers it: the same measurement the fitter
uses, kept signed, and averaged over the compass.

Positive means the model holds MORE energy in that band than the reference,
after both have been normalised the way the fitter normalises them -- within a
time window, against that window's own strongest band. So this reads balance,
not loudness.

    python tools/measure-band-balance.py [render-dir]

With no directory it renders the reference set first.
"""
import importlib.util
import io
import json
import os
import struct
import subprocess
import sys

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
spec = importlib.util.spec_from_file_location(
    "targets", os.path.join(ROOT, "tools", "extract-piano-targets.py"))
EX = importlib.util.module_from_spec(spec)
spec.loader.exec_module(EX)
TARGETS = json.load(
    io.open(os.path.join(ROOT, "tools", "piano-targets.json"), encoding="utf8"))


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


def render(into):
    env = dict(os.environ, CG_RENDER_DIR=into)
    subprocess.run(
        ["cargo", "test", "-p", "rackforge-concert-grand",
         "render_reference", "--release", "--", "--ignored"],
        cwd=ROOT, env=env, capture_output=True, check=True)


def main():
    directory = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        ROOT, "target", "balance-renders")
    if len(sys.argv) <= 1:
        os.makedirs(directory, exist_ok=True)
        render(directory)

    registers = [("bajo 21-47", 21, 47), ("tenor 48-71", 48, 71),
                 ("agudo 72-108", 72, 108)]
    sums = {name: [np.zeros(len(EX.BANDS)), np.zeros(len(EX.BANDS))]
            for name, _, _ in registers}
    for key, target in TARGETS["notes"].items():
        note = int(key)
        path = os.path.join(directory, target["sample"].replace("piano", "model") + ".wav")
        if not os.path.exists(path):
            continue
        x = read_wav(path)
        x = x / (np.abs(x[:EX.RATE * 2]).max() + 1e-12)
        bands = EX.windowed(x, EX.WINDOWS, EX.band_levels)
        for name, low, high in registers:
            if not low <= note <= high:
                continue
            for wi, row in enumerate(bands):
                reference = target["bands"][wi]
                if row is None or reference is None:
                    continue
                # Same normalisation the fitter uses: balance, not level.
                ref0, mod0 = max(reference), max(row)
                for bi in range(len(EX.BANDS)):
                    sums[name][0][bi] += (row[bi] - mod0) - (reference[bi] - ref0)
                    sums[name][1][bi] += 1

    head = "".join(f"{lo}-{hi}".rjust(11) for lo, hi in EX.BANDS)
    print(f"{'registro':<14}{head}")
    for name, _, _ in registers:
        total, n = sums[name]
        if not n.any():
            continue
        row = total / np.maximum(n, 1)
        print(f"{name:<14}" + "".join(f"{v:+11.2f}" for v in row))
    print("\n(modelo menos referencia, en dB; + = el modelo tiene de mas)")


if __name__ == "__main__":
    main()
