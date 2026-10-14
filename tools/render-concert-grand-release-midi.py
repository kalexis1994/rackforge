"""Render the complete source MIDI through the unchanged published piano.

Uses one continuous production instance, original tempo/velocity/controllers,
44.1 kHz stereo float WAV, unity export gain, and a five-second release tail.
No sample cache, per-note mixing, normalization, or extra effects are used.
"""
import argparse
from collections import Counter
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import time

import numpy as np
from scipy.io import wavfile


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def midi_events(path):
    """Read format 0/1 SMF, retaining stable track order at coincident ticks."""
    data = path.read_bytes()
    assert data[:4] == b'MThd'
    header_len = int.from_bytes(data[4:8], 'big')
    fmt, tracks, division = struct.unpack('>HHH', data[8:14])
    assert fmt in (0, 1) and 0 < division < 0x8000
    pos = 8 + header_len
    merged = []
    last_tick = 0
    for track in range(tracks):
        assert data[pos:pos+4] == b'MTrk'
        length = int.from_bytes(data[pos+4:pos+8], 'big')
        stream = data[pos+8:pos+8+length]
        assert len(stream) == length
        pos += 8 + length
        cursor, tick, running, order = 0, 0, None, 0

        def vlq():
            nonlocal cursor
            value = 0
            for _ in range(4):
                byte = stream[cursor]
                cursor += 1
                value = (value << 7) | (byte & 127)
                if byte < 128:
                    return value
            raise ValueError('Invalid MIDI VLQ')

        while cursor < length:
            tick += vlq()
            status = stream[cursor]
            if status >= 128:
                cursor += 1
                if status < 0xf0:
                    running = status
            else:
                assert running is not None
                status = running
            if status == 0xff:
                kind = stream[cursor]
                cursor += 1
                size = vlq()
                payload = stream[cursor:cursor+size]
                cursor += size
                if kind == 0x51:
                    assert size == 3
                    merged.append((tick, track, order, 'tempo', int.from_bytes(payload, 'big')))
                if kind == 0x2f:
                    assert size == 0 and cursor == length
            elif status in (0xf0, 0xf7):
                raise ValueError('SysEx requires explicit handling')
            else:
                assert 0x80 <= status < 0xf0
                size = 1 if status & 0xf0 in (0xc0, 0xd0) else 2
                payload = stream[cursor:cursor+size]
                assert len(payload) == size and all(b < 128 for b in payload)
                cursor += size
                merged.append((tick, track, order, 'midi', [status, *payload]))
            order += 1
            last_tick = max(last_tick, tick)
    assert pos == len(data)
    merged.sort(key=lambda e: e[:3])
    tempo, previous, seconds = 500000, 0, Fraction(0)
    result = []
    for tick, track, order, kind, value in merged:
        seconds += Fraction((tick - previous) * tempo, division * 1000000)
        previous = tick
        if kind == 'tempo':
            assert value > 0
            tempo = value
        else:
            result.append((seconds, value))
    duration = seconds + Fraction((last_tick - previous) * tempo, division * 1000000)
    return result, duration


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--midi', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--audio-dir', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    assert sha(args.midi) == 'af07dbf66a65bbb761ca0ef261e3657267350f62bb963d6f298d2ea46684122a'
    assert not args.work.exists() and not args.audio_dir.exists() and not args.report.exists()
    production = ['plugins/concert-grand', 'crates/rackforge-plugin-sdk']
    assert not subprocess.check_output(['git', 'diff', 'v0.1.32', '--', *production], cwd=root)
    source_events, duration = midi_events(args.midi)
    rate, block, tail = 44100, 256, 5

    def frame(t):
        value = t * rate
        return (value.numerator * 2 + value.denominator) // (2 * value.denominator)

    events, skipped_programs, held, controllers, counts = [], [], set(), {}, Counter()
    for seconds, message in source_events:
        status, key = message[:2]
        kind, channel = status & 0xf0, status & 15
        assert channel == 0
        if kind == 0xc0:
            assert key == 0
            skipped_programs.append({'seconds': float(seconds), 'program': key,
                                     'reason': 'Piano program 0 selected by concert preset'})
            continue
        assert len(message) == 3 and kind in (0x80, 0x90, 0xb0)
        value = message[2]
        if kind == 0xb0:
            controllers[key] = value
            counts[f'cc{key}'] += 1
        elif kind == 0x90 and value:
            held.add((channel, key))
            counts['strikes'] += 1
        else:
            held.discard((channel, key))
            counts['note_off'] += 1
        events.append((frame(seconds), *message))
    assert counts['strikes'] == 4249 and counts['note_off'] == 4249
    assert set(controllers) <= {64, 66, 67}
    end = frame(duration)
    synthetic = [(end, 0x80 | channel, key, 0) for channel, key in sorted(held)]
    synthetic += [(end, 0xb0, cc, 0) for cc in (64, 66, 67) if controllers.get(cc, 0)]
    events += synthetic
    assert all(a[0] <= b[0] for a, b in zip(events, events[1:]))
    total = end + tail * rate
    args.work.mkdir(parents=True)
    args.audio_dir.mkdir(parents=True)
    event_file = args.work / 'events.txt'
    event_file.write_text(''.join(' '.join(map(str, e)) + '\n' for e in events), encoding='utf-8', newline='\n')
    rust = ['rustc', '+1.98.0-x86_64-pc-windows-msvc', '--edition=2024', '-O']
    sdk = args.work / 'librackforge_plugin_sdk.rlib'
    piano = args.work / 'librackforge_concert_grand.rlib'
    exe = args.work / 'probe.exe'
    commands = [
        [*rust, '--crate-name', 'rackforge_plugin_sdk', '--crate-type', 'rlib', str(root / 'crates/rackforge-plugin-sdk/src/lib.rs'), '-o', str(sdk)],
        [*rust, '--crate-name', 'rackforge_concert_grand', '--crate-type', 'rlib', '--extern', f'rackforge_plugin_sdk={sdk}', str(root / 'plugins/concert-grand/src/lib.rs'), '-o', str(piano)],
        [*rust, '-L', f'dependency={args.work}', '--extern', f'rackforge_plugin_sdk={sdk}', '--extern', f'rackforge_concert_grand={piano}', str(root / 'tools/concert-grand-release-probe.rs'), '-o', str(exe)],
    ]
    for command in commands:
        subprocess.run(command, check=True, cwd=root)
    files = [root / 'tools/concert-grand-release-probe.rs', Path(__file__), root / 'plugins/concert-grand/src/lib.rs',
             root / 'crates/rackforge-plugin-sdk/src/lib.rs', sdk, piano, exe, args.midi, event_file]
    frozen = {str(p.resolve()): sha(p) for p in files}
    raw = args.work / 'Campanella-release-0.171.35.f32'
    command = [str(exe), str(event_file), str(raw), str(rate), str(total), str(block), 'concert']
    start = time.perf_counter()
    native = json.loads(subprocess.check_output(command, text=True, cwd=root))
    elapsed = time.perf_counter() - start
    audio = np.fromfile(raw, dtype='<f4').reshape(-1, 2)
    assert audio.shape == (total, 2) and np.isfinite(audio).all()
    peak = float(np.abs(audio).max())
    assert 0 < peak < 1
    wav = args.audio_dir / 'Campanella-release-0.171.35.wav'
    wavfile.write(wav, rate, audio)
    check_rate, check_audio = wavfile.read(wav)
    assert check_rate == rate and np.array_equal(check_audio, audio)
    assert frozen == {str(p.resolve()): sha(p) for p in files}
    assert not subprocess.check_output(['git', 'diff', 'v0.1.32', '--', *production], cwd=root)
    report = dict(role='Published release listening baseline; no new DSP improvement applied',
                  release='RackForge v0.1.32 / Concert Grand 0.171.35',
                  git_head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip(),
                  midi_duration_seconds=float(duration), audio_duration_seconds=total / rate,
                  rate=rate, channels=2, block=block, tail_seconds=tail, export_gain=1,
                  counts=dict(counts), source_events=len(source_events), rendered_events=len(events),
                  program_selection=skipped_programs, synthetic_end_events=synthetic,
                  native=native, render_process_seconds=elapsed, peak=peak,
                  tail_last_100ms_rms=float(np.sqrt(np.mean(audio[-rate//10:].astype(np.float64)**2))),
                  last_samples=audio[-1].tolist(), output=str(wav.resolve()), output_sha256=sha(wav),
                  raw_sha256=sha(raw), files_sha256=frozen, compile_commands=commands, render_command=command,
                  validation=['4249 strikes and 4249 note releases', 'Original tempo, order and continuous controllers',
                              'Single continuous production instance; no note cache', 'Production source matches v0.1.32',
                              'Finite stereo output, peak below full scale, exact WAV/raw sample equality'])
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8', newline='\n')
    print(json.dumps({k: report[k] for k in ('audio_duration_seconds', 'render_process_seconds', 'peak', 'tail_last_100ms_rms', 'output')}))


if __name__ == '__main__':
    main()
