"""Three-way comparison: the YDP samples, our model, and a reference renderer.

Renders single notes through a licensed Pianoteq installation's command-line
exporter and measures them exactly as the YDP samples and our own renders are
measured. Nothing here reads or disassembles anything: it drives the product as
its own manual documents (`--midi foo.mid --wav foo.wav`) and looks at the
audio that comes out, which is what the licence is for.

Why it exists. The fit cost scores band levels inside time windows and is blind
to spectral density -- a band holding the right energy in 24 components scores
the same as one holding it in 82. The bass difference the model has been chased
over for forty versions lives in exactly that blind spot, so it needs a second
opinion from outside: a model that is known to convince, measured the same way.

    python tools/compare-reference-render.py <render-dir-of-model-wavs>

Requires a licensed Pianoteq install; set PIANOTEQ to its executable if it is
not at the default Windows path. Skips the reference column if absent.
"""
import os
import struct
import subprocess
import sys
import tempfile

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "tools"))

DEFAULT_PIANOTEQ = r"C:\Program Files\Modartt\Pianoteq 9\Pianoteq 9.exe"
RATE = 44_100
#: The notes the complaint is about, with the velocities the YDP samples use.
NOTES = [(21, 123), (30, 124), (36, 120)]
HOLD_SECONDS = 4.0


def midi_file(path, note, velocity, hold=HOLD_SECONDS):
    """A one-note format-0 SMF: strike, hold, release, let it ring."""
    ticks = 480  # per quarter note
    tempo = 500_000  # microseconds per quarter, so one tick is ~1.04 ms
    hold_ticks = int(hold * 1_000_000 / tempo * ticks)
    tail_ticks = ticks * 2

    def varlen(value):
        out = [value & 0x7F]
        value >>= 7
        while value:
            out.append((value & 0x7F) | 0x80)
            value >>= 7
        return bytes(reversed(out))

    events = b""
    events += varlen(0) + b"\xff\x51\x03" + tempo.to_bytes(3, "big")
    events += varlen(0) + bytes([0x90, note, velocity])
    events += varlen(hold_ticks) + bytes([0x80, note, 0])
    events += varlen(tail_ticks) + b"\xff\x2f\x00"

    header = b"MThd" + struct.pack(">IHHH", 6, 0, 1, ticks)
    track = b"MTrk" + struct.pack(">I", len(events)) + events
    with open(path, "wb") as handle:
        handle.write(header + track)


def render(executable, note, velocity, out_path, preset=None):
    with tempfile.TemporaryDirectory() as work:
        mid = os.path.join(work, "note.mid")
        midi_file(mid, note, velocity)
        command = [executable, "--headless", "--quiet",
                   "--midi", mid, "--wav", out_path,
                   "--rate", str(RATE), "--bit-depth", "16", "--mono"]
        if preset:
            command[1:1] = ["--preset", preset]
        result = subprocess.run(command, capture_output=True, text=True)
        if result.returncode != 0 or not os.path.exists(out_path):
            raise RuntimeError(
                f"render failed ({result.returncode}): "
                f"{(result.stderr or result.stdout)[:400]}")


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


def clusters(x, rate, lo=2000.0, hi=4000.0, seconds=1.5):
    """Peaks in the band and how tightly they bunch.

    The unison shows up here: three strings a few cents apart put two or three
    peaks a few Hz apart where one partial would put one. A median gap far
    below the ladder's spacing is that signature.
    """
    a = int(0.10 * rate)
    b = min(a + int(seconds * rate), len(x))
    segment = x[a:b]
    if len(segment) < rate:
        return None
    window = segment * np.hanning(len(segment))
    magnitude = np.abs(np.fft.rfft(window))
    freqs = np.fft.rfftfreq(len(window), 1.0 / rate)
    db = 20 * np.log10(magnitude + 1e-12)
    db -= db.max()
    peaks = [
        freqs[i]
        for i in np.where((freqs >= lo) & (freqs < hi))[0]
        if 0 < i < len(db) - 1 and db[i] > db[i - 1] and db[i] > db[i + 1] and db[i] > -50
    ]
    if len(peaks) < 4:
        return len(peaks), 0.0, 0
    gaps = np.diff(peaks)
    median = float(np.median(gaps))
    tight = int((gaps < 0.35 * median).sum()) if median else 0
    return len(peaks), median, tight


def relief(x, rate, lo=2000.0, hi=4000.0, t0=0.3, t1=0.9):
    """How far the peaks stand over the floor: tone against mush."""
    a, b = int(t0 * rate), min(int(t1 * rate), len(x))
    segment = x[a:b]
    if len(segment) < 4096:
        return None
    window = segment * np.hanning(len(segment))
    magnitude = np.abs(np.fft.rfft(window))
    freqs = np.fft.rfftfreq(len(window), 1.0 / rate)
    band = magnitude[(freqs >= lo) & (freqs < hi)]
    if len(band) < 50:
        return None
    db = 20 * np.log10(band + 1e-12)
    return float(np.percentile(db, 95) - np.median(db))


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    model_dir = sys.argv[1]
    executable = os.environ.get("PIANOTEQ", DEFAULT_PIANOTEQ)

    import importlib.util
    spec = importlib.util.spec_from_file_location(
        "targets", os.path.join(ROOT, "tools", "extract-piano-targets.py"))
    extractor = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(extractor)

    sf2 = os.environ.get("CG_SF2")
    samples = {}
    if sf2 and os.path.exists(sf2):
        pool, headers = extractor.parse(sf2)
        for note, entry in extractor.loudest_per_note(headers).items():
            _name, start, end, _rate, _pitch = entry
            samples[note] = np.asarray(pool[start:end], dtype=np.float64) / 32768.0

    have_reference = os.path.exists(executable)
    if not have_reference:
        print(f"(no reference renderer at {executable}; showing two columns)")

    out = os.path.join(tempfile.gettempdir(), "rackforge-reference")
    os.makedirs(out, exist_ok=True)

    print(f"{'note':>5} {'source':>12} {'peaks':>7} {'median gap':>12} {'tight':>7} {'relief':>8}")
    print("-" * 56)
    for note, velocity in NOTES:
        rows = []
        if note in samples:
            rows.append(("YDP sample", samples[note]))
        model = os.path.join(model_dir, f"model{note:03}v{velocity}.wav")
        if os.path.exists(model):
            rows.append(("our model", read_wav(model)))
        if have_reference:
            path = os.path.join(out, f"reference{note:03}.wav")
            try:
                if not os.path.exists(path):
                    render(executable, note, velocity, path)
                rows.append(("reference", read_wav(path)))
            except Exception as error:  # the product may refuse offline export
                print(f"      reference render failed: {error}")
        for label, audio in rows:
            found = clusters(audio, RATE)
            proud = relief(audio, RATE)
            if found is None:
                continue
            count, median, tight = found
            print(f"{note:>5} {label:>12} {count:>7} {median:>11.1f}H {tight:>7} "
                  f"{(proud if proud is not None else float('nan')):>7.1f}d")
        print()


if __name__ == "__main__":
    main()
