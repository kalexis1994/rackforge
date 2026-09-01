"""Measures a velocity-layered piano into targets for the model's DYNAMICS.

    python tools/extract-piano-dynamics.py <SalamanderGrandPianoV3_48khz24bit> [out.json]

## Why this exists

Every note in `tools/piano-targets.json` is at velocity 120-127:
`loudest_per_note` in `extract-piano-targets.py` keeps only the hardest blow and
`MINIMUM_VELOCITY = 118` throws the rest away. The model has therefore never
been calibrated anywhere but fortissimo, and how it brightens between pianissimo
and forte -- which is most of playing -- is measured against nothing at all.

The Salamander Grand Piano carries 30 notes at 16 velocity layers each, which is
the axis the YDP set does not have.

## What may and may not be taken from it

Salamander is a Yamaha C5. The YDP is a Disklavier Pro, and the whole
calibration table is fitted to it, already within about 1 dB through the mids at
fortissimo. Refitting against a different instrument would trade a good fit for
a different one, so only the RELATIVE change is taken from here: how a note's
band balance moves as the blow gets harder. That shape is far more alike between
two grands than their absolute timbres are.

Two things follow, and both are why this file measures shape and not level:

* `measure()` normalises each sample by its own peak, so what is recorded is
  balance, not loudness. That is deliberate.
* The library's own `<group> amp_veltrack=73` means the player's velocity adds
  gain at playback time, on top of the recorded sample. So the raw peaks do not
  say how much louder a fortissimo is than a pianissimo -- that number belongs
  to the library's gain staging, not to the piano -- and it is not extracted.
  A flat gain cannot change band balance, so it does not disturb what is.

## How it is measured

Exactly as the YDP targets are, by importing that extractor and calling its own
functions, so the two files can be compared band for band. The one thing set
differently is its `RATE`, because these recordings are 48 kHz. That is safe:
the bands are bounds in hertz and the windows are spans in seconds, and an FFT
bin's width is one over the window's duration whatever the sample rate, so the
resolution is identical.

Only the first few seconds of each recording are read. The last window ends at
2.2 s and the peak is taken over the first 2 s, so the rest is 480 samples'
worth of disk traffic for nothing.
"""

import importlib.util
import json
import os
import re
import struct
import sys

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
spec = importlib.util.spec_from_file_location(
    "targets", os.path.join(ROOT, "tools", "extract-piano-targets.py"))
EX = importlib.util.module_from_spec(spec)
spec.loader.exec_module(EX)

#: Enough for the last window (to 2.2 s) and the peak (first 2 s).
SECONDS_NEEDED = 2.5


def read_wav_start(path, seconds):
    """The first `seconds` of a WAV, mixed to mono, plus its sample rate.

    Reads the header and only as many frames as are asked for: these
    recordings run about sixteen seconds each and there are 480 of them.
    """
    with open(path, "rb") as handle:
        header = handle.read(4096)
        fmt = header.index(b"fmt ")
        channels = struct.unpack("<H", header[fmt + 10:fmt + 12])[0]
        rate = struct.unpack("<I", header[fmt + 12:fmt + 16])[0]
        bits = struct.unpack("<H", header[fmt + 22:fmt + 24])[0]
        data = header.index(b"data")
        length = struct.unpack("<I", header[data + 4:data + 8])[0]
        width = bits // 8
        frame = width * channels
        wanted = min(length, int(seconds * rate) * frame)
        handle.seek(data + 8)
        raw = handle.read(wanted)

    usable = len(raw) - (len(raw) % frame)
    raw = raw[:usable]
    if bits == 24:
        byte = np.frombuffer(raw, np.uint8).reshape(-1, 3).astype(np.int32)
        flat = byte[:, 0] | (byte[:, 1] << 8) | (
            byte[:, 2].astype(np.uint8).view(np.int8).astype(np.int32) << 16)
        flat = flat.astype(np.float64) / 8388608.0
    elif bits == 16:
        flat = np.frombuffer(raw, dtype="<i2").astype(np.float64) / 32768.0
    else:
        raise SystemExit(f"{path}: {bits}-bit WAV is not handled")
    return flat.reshape(-1, channels).mean(axis=1), rate, length // frame / rate


#: `sample=48khz24bit\A0v1.wav lokey=21 ... lovel=1 hivel=26 pitch_keycenter=21`
REGION = re.compile(
    r"sample=(?P<dir>[^\s\\/]+)[\\/](?P<file>(?P<note>[A-G]#?-?\d+)v(?P<layer>\d+)\.wav)"
    r"(?P<rest>[^\n]*)")


SEMITONE = {"C": 0, "C#": 1, "D": 2, "D#": 3, "E": 4, "F": 5,
            "F#": 6, "G": 7, "G#": 8, "A": 9, "A#": 10, "B": 11}


def pitch_of_name(name):
    """`A0` -> 21, `C4` -> 60, `D#3` -> 51."""
    step = re.fullmatch(r"([A-G]#?)(-?\d+)", name)
    return (int(step.group(2)) + 1) * 12 + SEMITONE[step.group(1)]


def regions(sfz_text):
    """Every single-note region: its file, its velocity span and its pitch.

    Regions whose sample is not `<note>v<layer>.wav` are skipped, which drops
    the sympathetic (`harm*`) and key-release (`rel*`) groups. Those are worth
    measuring one day -- the model's attack noise floor and its release are
    both open -- but they are not what a dynamics target is about, and their
    groups carry their own `volume=` offsets.

    The pitch comes from the file's own name rather than from
    `pitch_keycenter`, because the key is allowed to be left out: SFZ defaults
    it to 60, and this library duly omits it on all sixteen layers of C4 --
    which is middle C, the one note a piano calibration can least afford to
    drop. Where the key IS written it is checked against the name, so a
    disagreement is caught rather than quietly preferred.
    """
    for match in REGION.finditer(sfz_text):
        rest = match.group("rest")
        pitch = pitch_of_name(match.group("note"))
        written = re.search(r"pitch_keycenter=(\d+)", rest)
        if written and int(written.group(1)) != pitch:
            raise SystemExit(
                f"{match.group('file')}: named {match.group('note')} ({pitch})"
                f" but keyed to {written.group(1)}")
        lovel = re.search(r"lovel=(\d+)", rest)
        hivel = re.search(r"hivel=(\d+)", rest)
        yield {
            "file": os.path.join(match.group("dir"), match.group("file")),
            "sample": match.group("file"),
            "pitch": pitch,
            "layer": int(match.group("layer")),
            "lovel": int(lovel.group(1)) if lovel else 1,
            # The topmost layer is written without a hivel: it takes the rest.
            "hivel": int(hivel.group(1)) if hivel else 127,
        }


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__.strip().splitlines()[2].strip())
    root = sys.argv[1]
    out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(
        ROOT, "tools", "piano-dynamics.json")

    sfz = next((os.path.join(root, name) for name in sorted(os.listdir(root))
                if name.endswith(".sfz") and "Retuned" not in name), None)
    if sfz is None:
        raise SystemExit(f"no .sfz in {root}")
    found = sorted(regions(open(sfz, encoding="utf-8", errors="replace").read()),
                   key=lambda r: (r["pitch"], r["layer"]))
    if not found:
        raise SystemExit(f"{sfz} declares no single-note regions")

    notes, rate_seen = {}, None
    for index, region in enumerate(found, 1):
        path = os.path.join(root, region["file"])
        if not os.path.exists(path):
            print(f"  missing: {region['file']}", file=sys.stderr)
            continue
        x, rate, seconds = read_wav_start(path, SECONDS_NEEDED)
        if rate_seen is None:
            rate_seen = rate
            # The measurement reads its rate from the module it lives in.
            EX.RATE = rate
        elif rate != rate_seen:
            raise SystemExit(f"{region['sample']} is {rate} Hz, not {rate_seen}")
        bands, noise = EX.measure(x)
        notes.setdefault(str(region["pitch"]), {
            "pitch": region["pitch"],
            "layers": [],
        })["layers"].append({
            "layer": region["layer"],
            "lovel": region["lovel"],
            "hivel": region["hivel"],
            "sample": region["sample"],
            "seconds": round(seconds, 3),
            "bands": bands,
            "noisiness": noise,
            "centroid": EX.centroid_of(x[:int(rate)]),
        })
        if index % 60 == 0:
            print(f"  {index}/{len(found)}", file=sys.stderr)

    document = {
        "schema": 1,
        "source": os.path.basename(os.path.normpath(root)),
        "instrument": "Yamaha C5, Salamander Grand Piano V3, CC-BY 3.0",
        "rate": rate_seen,
        "measures": "band balance only; each sample normalised by its own peak",
        "bands": EX.BANDS,
        "windows": EX.WINDOWS,
        "noise_windows": EX.NOISE_WINDOWS,
        "notes": dict(sorted(notes.items(), key=lambda kv: int(kv[0]))),
    }
    with open(out, "w", encoding="utf-8") as handle:
        json.dump(document, handle, indent=1)
    layers = sum(len(note["layers"]) for note in notes.values())
    print(f"{out}: {len(notes)} notes, {layers} velocity layers, {rate_seen} Hz")


if __name__ == "__main__":
    main()
