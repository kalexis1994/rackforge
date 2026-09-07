"""The reference plays the lab's score: a minimal sampler over Salamander Grand Piano V3.

    python tools/salamander-render.py <score.txt> <out.wav> [--bank <dir>]

The score is the Concert Grand lab's (`onset_ms duration_ms note velocity`,
`onset_ms pedal 0..127`). Each note takes the bank's nearest sampled note
(every three semitones) at the velocity layer the SFZ maps, resampled by the
semitone ratio, at the SFZ's `amp_veltrack=73` gain. A key-up on a damped
key (<= LAST_DAMPER) with the pedal up applies the SFZ's one-second
release; above it, and under the pedal, the sample rings to its end. No
release or pedal noises, no resonance samples: this is the strings alone,
which is what a comparison of the strings alone wants.
"""

import sys
import wave
from pathlib import Path

import numpy as np

BANK = Path(r"C:\Users\kalex\Downloads\SalamanderGrandPianoV3_48khz24bit\48khz24bit")
NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
SAMPLED = [21 + 3 * i for i in range(30)]  # A0, C1, D#1 ... C8
LAYER_TOP = [26, 34, 36, 43, 46, 50, 56, 64, 72, 80, 88, 96, 104, 112, 120, 127]
LAST_DAMPER = 88
RELEASE_S = 1.0
TAIL_S = 3.0


def name_of(note):
    return f"{NAMES[note % 12]}{note // 12 - 1}"


def layer_of(velocity):
    for i, top in enumerate(LAYER_TOP):
        if velocity <= top:
            return i + 1
    return 16


_cache = {}


def load(path):
    if path in _cache:
        return _cache[path]
    with wave.open(str(path)) as w:
        assert w.getsampwidth() == 3 and w.getframerate() == 48000, path
        raw = w.readframes(w.getnframes())
        channels = w.getnchannels()
    a = np.frombuffer(raw, dtype=np.uint8).reshape(-1, 3)
    x = (a[:, 0].astype(np.int32) | (a[:, 1].astype(np.int32) << 8) | (a[:, 2].astype(np.int8).astype(np.int32) << 16))
    x = x.astype(np.float64).reshape(-1, channels) / 8388608.0
    if channels == 1:
        x = np.repeat(x, 2, axis=1)
    _cache[path] = x
    return x


def resample(x, ratio):
    """Playback at `ratio` times the speed: linear interpolation, good enough within a semitone."""
    if abs(ratio - 1.0) < 1e-9:
        return x
    n = int(len(x) / ratio)
    src = np.arange(n) * ratio
    i = np.floor(src).astype(np.int64)
    frac = (src - i)[:, None]
    i = np.minimum(i, len(x) - 2)
    return x[i] * (1 - frac) + x[i + 1] * frac


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    bank = BANK
    if "--bank" in sys.argv:
        bank = Path(sys.argv[sys.argv.index("--bank") + 1])
    score, out = Path(args[0]), Path(args[1])
    sr = 48000
    events = []
    for line in score.read_text(encoding="utf-8").splitlines():
        f = line.split()
        if not f or f[0].startswith("#"):
            continue
        if f[1] in ("pedal", "sostenuto", "soft"):
            if f[1] == "pedal":
                events.append((int(f[0]), "pedal", int(f[2]), 0))
        else:
            events.append((int(f[0]), "note", int(f[2]), int(f[3])))
            events.append((int(f[0]) + int(f[1]), "off", int(f[2]), 0))
    events.sort(key=lambda e: (e[0], e[1] != "pedal"))
    last_ms = max(e[0] for e in events)
    total = int((last_ms / 1000 + TAIL_S) * sr)
    mix = np.zeros((total, 2))
    pedal = False
    held = set()
    ringing = {}  # note -> list of (start_frame, samples) still free to be ended
    every = []  # everything ever started, mixed at the end
    placed = 0

    def end(note, at_ms):
        # a key-up with the pedal up on a damped key: fade what rings over RELEASE_S
        for start, x in ringing.pop(note, []):
            cut = int(at_ms / 1000 * sr) - start
            if cut < 0 or cut >= len(x):
                continue
            fade = np.exp(-np.arange(len(x) - cut) / (RELEASE_S * sr / 6.9))[:, None]
            x[cut:] *= fade
            x[cut + int(RELEASE_S * sr):] = 0.0

    for at_ms, kind, note, velocity in events:
        if kind == "pedal":
            down = velocity > 10
            if pedal and not down:
                for n in list(ringing):
                    if n <= LAST_DAMPER and n not in held:
                        end(n, at_ms)
            pedal = down
            continue
        if kind == "off":
            held.discard(note)
            if note <= LAST_DAMPER and not pedal:
                end(note, at_ms)
            continue
        held.add(note)
        nearest = min(SAMPLED, key=lambda s: abs(s - note))
        layer = layer_of(velocity)
        path = bank / f"{name_of(nearest)}v{layer}.wav"
        x = load(path)
        x = resample(x, 2 ** ((note - nearest) / 12))
        gain = 0.27 + 0.73 * (velocity / 127) ** 2
        start = int(at_ms / 1000 * sr)
        keep = min(len(x), total - start)
        buf = x[:keep].copy() * gain
        ringing.setdefault(note, []).append((start, buf))
        every.append((start, buf))
        placed += 1
    # The fades edited the buffers in place; everything is mixed only now.
    for start, buf in every:
        mix[start:start + len(buf)] += buf
    peak = np.abs(mix).max()
    if peak > 0.99:
        mix *= 0.99 / peak
        print(f"{out.name}: peak-limited by {20 * np.log10(0.99 / peak):+.1f} dB")
    with wave.open(str(out), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes((np.clip(mix, -1, 1) * 32767).astype(np.int16).tobytes())
    print(f"{out}: {placed} notes, {total / sr:.1f} s")


if __name__ == "__main__":
    main()
