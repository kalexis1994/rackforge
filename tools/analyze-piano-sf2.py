"""Measure the YDP Grand samples: per-partial spectra and decay, to calibrate
the Concert Grand physical model against a real recorded piano."""
import struct
import sys
import numpy as np

SF2 = r"C:\Users\kalex\AppData\Local\Temp\claude\C--Users-kalex-OneDrive-Documents-rackforge\57ee42b5-9774-4130-893f-a6527f4906b6\scratchpad\rf-sf\assets\ydp-grand-piano.sf2"


def read_chunks(data, offset, end):
    while offset < end:
        cid = data[offset:offset + 4]
        size = struct.unpack("<I", data[offset + 4:offset + 8])[0]
        body = offset + 8
        yield cid, body, size
        offset = body + size + (size & 1)


def parse(path):
    data = open(path, "rb").read()
    assert data[:4] == b"RIFF" and data[8:12] == b"sfbk"
    smpl = None
    shdr = []
    for cid, body, size in read_chunks(data, 12, len(data)):
        if cid == b"LIST":
            kind = data[body:body + 4]
            for c2, b2, s2 in read_chunks(data, body + 4, body + size):
                if c2 == b"smpl":
                    smpl = np.frombuffer(data[b2:b2 + s2], dtype="<i2")
                elif c2 == b"shdr":
                    for off in range(b2, b2 + s2 - 46 + 1, 46):
                        name = data[off:off + 20].split(b"\0")[0].decode("ascii", "replace")
                        start, end, ls, le, rate, pitch, corr = struct.unpack(
                            "<IIIIIBb", data[off + 20:off + 42])
                        shdr.append((name, start, end, ls, le, rate, pitch, corr))
    return smpl, shdr


def partial_track(x, rate, f0, nmax=40, frames=None):
    """Amplitude of partials 1..nmax over time via short-window DFT projection."""
    win = int(rate * 0.19)
    hop = int(rate * 0.05)
    if frames is None:
        frames = min(40, (len(x) - win) // hop)
    t = np.arange(win) / rate
    out = np.zeros((frames, nmax))
    for k in range(frames):
        seg = x[k * hop:k * hop + win].astype(np.float64)
        seg = seg * np.hanning(win)
        for n in range(1, nmax + 1):
            f = n * f0
            if f > rate * 0.45:
                break
            # allow inharmonic sharpening: search +-1.2% around n*f0
            best = 0.0
            for df in np.linspace(-0.012, 0.02, 9):
                ff = f * (1 + df)
                z = np.exp(-2j * np.pi * ff * t)
                a = abs(np.dot(seg, z)) / (win / 4)
                best = max(best, a)
            out[k, n - 1] = best
    return out


def analyze(name, x, rate, f0):
    x = x / 32768.0
    tr = partial_track(x, rate, f0)
    a0 = tr[0] + 1e-12
    peak = a0.max()
    strongest = int(np.argmax(a0)) + 1
    print(f"== {name}  f0={f0:.1f}Hz rate={rate}  dur={len(x)/rate:.1f}s")
    print("   initial spectrum dB rel max:",
          " ".join(f"{n+1}:{20*np.log10(a0[n]/peak):.0f}"
                   for n in range(min(24, len(a0)))))
    print(f"   strongest partial: n={strongest}")
    # decay: dB/s for a few partials over first 1.5 s
    seconds = tr.shape[0] * 0.05
    for n in [1, 2, 4, 8, 16]:
        if n <= tr.shape[1] and tr[0, n - 1] / peak > 1e-3:
            serie = 20 * np.log10(tr[:, n - 1] + 1e-12)
            k = min(len(serie) - 1, int(1.5 / 0.05))
            print(f"   partial {n}: {(serie[k]-serie[0])/(k*0.05):.1f} dB/s over first {k*0.05:.1f}s")


def main():
    smpl, shdr = parse(SF2)
    print(f"samples: {len(shdr)}, total pcm {len(smpl)/1e6:.1f}M frames")
    for name, start, end, *_ in shdr[:200]:
        pass
    wanted = sys.argv[1:] if len(sys.argv) > 1 else []
    shown = 0
    for name, start, end, ls, le, rate, pitch, corr in shdr:
        if end <= start or pitch > 127:
            continue
        if wanted and not any(w.lower() in name.lower() for w in wanted):
            continue
        f0 = 440.0 * 2 ** ((pitch - 69) / 12) * 2 ** (corr / 1200)
        x = np.array(smpl[start:end], dtype=np.float64)
        if len(x) < rate * 0.5:
            continue
        analyze(f"{name} (pitch {pitch})", x, rate, f0)
        shown += 1
        if shown >= 8:
            break
    if shown == 0:
        print("names available:")
        for name, start, end, ls, le, rate, pitch, corr in shdr[:80]:
            print(f"  {name}  pitch={pitch} len={(end-start)/max(rate,1):.1f}s")


main()
