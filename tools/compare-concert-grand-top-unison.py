"""Compare the tiny treble unison residual against the frozen release render."""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import time

import numpy as np
from scipy.io import wavfile


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--audio-dir', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    baseline_path = root / 'docs/concert-grand-release-body-2026-10-14/campanella-baseline.json'
    base = json.loads(baseline_path.read_text(encoding='utf-8'))
    baseline = Path(base['render_command'][0])
    sdk = baseline.parent / 'librackforge_plugin_sdk.rlib'
    events = Path(base['render_command'][1])
    baseline_raw = Path(base['render_command'][2])
    for path in (baseline, sdk, events):
        assert sha(path) == base['files_sha256'][str(path.resolve())]
    assert sha(baseline_raw) == base['raw_sha256']
    assert sha(base['output']) == base['output_sha256']
    assert not subprocess.check_output(['git', 'diff', 'v0.1.32', '--', 'crates/rackforge-plugin-sdk'], cwd=root)
    assert not args.audio_dir.exists() and not args.report.exists()
    args.audio_dir.mkdir(parents=True)
    args.work.mkdir(parents=True, exist_ok=True)
    candidate = args.work / 'probe.exe'
    library = args.work / 'librackforge_concert_grand.rlib'
    rust = ['rustc', '+1.98.0-x86_64-pc-windows-msvc', '--edition=2024', '-O']
    compile_commands = [
        [*rust, '--crate-name', 'rackforge_concert_grand', '--crate-type', 'rlib', '--extern', f'rackforge_plugin_sdk={sdk}', str(root / 'plugins/concert-grand/src/lib.rs'), '-o', str(library)],
        [*rust, '-L', f'dependency={args.work}', '-L', f'dependency={baseline.parent}', '--extern', f'rackforge_plugin_sdk={sdk}', '--extern', f'rackforge_concert_grand={library}', str(root / 'tools/concert-grand-release-probe.rs'), '-o', str(candidate)],
    ]
    for command in compile_commands:
        subprocess.run(command, check=True, cwd=root)
    frozen_paths = [Path(__file__), root / 'tools/concert-grand-release-probe.rs', root / 'plugins/concert-grand/src/lib.rs',
                    root / 'plugins/concert-grand/src/math.rs',
                    root / 'crates/rackforge-plugin-sdk/src/lib.rs', candidate, library, sdk, events, baseline]
    frozen = {str(p.resolve()): sha(p) for p in frozen_paths}
    rate = base['rate']
    block = base['block']

    def render(exe, event_file, raw, frames):
        command = [str(exe), str(event_file), str(raw), str(rate), str(frames), str(block), 'concert']
        begin = time.perf_counter()
        native = json.loads(subprocess.check_output(command, text=True, cwd=root))
        wall = time.perf_counter() - begin
        samples = np.fromfile(raw, dtype='<f4').reshape(-1, 2)
        assert samples.shape == (frames, 2) and np.isfinite(samples).all()
        assert 0 < np.abs(samples).max() < 1
        return samples, dict(command=command, native=native, process_seconds=wall, raw_sha256=sha(raw))

    def write_wav(path, samples):
        wavfile.write(path, rate, samples)
        check_rate, check_samples = wavfile.read(path)
        assert check_rate == rate and np.array_equal(check_samples, samples)
        return dict(path=str(path), sha256=sha(path), frames=len(samples), rate=rate, gain=1)

    full, full_report = render(candidate, events, args.work / 'campanella-candidate.f32', base['native']['frames'])
    full_wav = write_wav(args.audio_dir / 'Campanella-B-unisonos-agudos.wav', full)
    original = np.fromfile(baseline_raw, dtype='<f4').reshape(-1, 2)
    full_delta = float(np.linalg.norm(full.astype(np.float64) - original) / np.linalg.norm(original.astype(np.float64)))
    print(f'Full candidate: {full_report["process_seconds"]:.2f} seconds; relative RMS delta {full_delta:.6g}', flush=True)

    notes = [60, 84, 90, 91, 96, 100, 103, 108]
    note_reports, montage_a, montage_b = [], [], []
    silence = np.zeros((rate // 4, 2), dtype=np.float32)
    for note in notes:
        event_file = args.work / f'note-{note}.txt'
        event_file.write_text(f'0 144 {note} 96\n{rate*2} 128 {note} 0\n', encoding='utf-8', newline='\n')
        a, ar = render(baseline, event_file, args.work / f'{note}-A.f32', rate * 3)
        b, br = render(candidate, event_file, args.work / f'{note}-B.f32', rate * 3)
        equal = bool(np.array_equal(a, b))
        assert equal if note <= 90 else not equal
        windows = []
        for start, end in ((.05, .15), (.15, .3), (.3, .5), (.5, 1), (1, 2)):
            aa = a[round(start*rate):round(end*rate)].astype(np.float64)
            bb = b[round(start*rate):round(end*rate)].astype(np.float64)
            windows.append(dict(start=start, end=end, level_delta_db=float(20 * np.log10(np.linalg.norm(bb) / np.linalg.norm(aa)))))
        note_reports.append(dict(note=note, velocity=96, release=ar, candidate=br, identical=equal, windows=windows,
                                 relative_rms_delta=float(np.linalg.norm(b.astype(np.float64)-a) / np.linalg.norm(a.astype(np.float64)))))
        montage_a.extend((a, silence))
        montage_b.extend((b, silence))
    isolates = [write_wav(args.audio_dir / 'Notas-A-release.wav', np.concatenate(montage_a)),
                write_wav(args.audio_dir / 'Notas-B-unisonos-agudos.wav', np.concatenate(montage_b))]

    # Alternating order limits warmup/order bias. This is offline wall time,
    # including raw output I/O, not a WASM or real-time deadline measurement.
    bench_events = []
    for i in range(24):
        note = [60, 84, 91, 96, 100, 103, 108, 96][i % 8]
        bench_events.extend(((i*rate//2, 144, note, 96), (i*rate//2+rate//3, 128, note, 0)))
    bench_file = args.work / 'benchmark.txt'
    bench_file.write_text(''.join(' '.join(map(str, e))+'\n' for e in sorted(bench_events)), encoding='utf-8', newline='\n')
    timings = {'release': [], 'candidate': []}
    for repeat in range(3):
        order = [('release', baseline), ('candidate', candidate)]
        if repeat % 2:
            order.reverse()
        for label, exe in order:
            _, result = render(exe, bench_file, args.work / f'benchmark-{repeat}-{label}.f32', rate*14)
            timings[label].append(result['native']['wall_seconds'])
    medians = {key: statistics.median(value) for key, value in timings.items()}
    assert frozen == {str(p.resolve()): sha(p) for p in frozen_paths}
    report = dict(scope=__doc__, candidate_status='Conservative listening candidate; subjective benefit pending',
                  change={'TOP_UNISON_CENTS': .02, 'extra_oscillators': 0, 'extra_per_sample_operations': 0,
                          'note': 'Residual fades in with the existing top-octave taper; same fixed note/partial variation and unison controls'},
                  baseline_report=str(baseline_path), baseline_report_sha256=sha(baseline_path),
                  files_sha256=frozen, compile_commands=compile_commands,
                  source_diff=subprocess.check_output(['git', 'diff', 'v0.1.32', '--', 'plugins/concert-grand/src/lib.rs'], cwd=root, text=True),
                  audio_duration_seconds=len(full)/rate, full_render=full_report, full_output=full_wav,
                  full_relative_rms_delta=full_delta, isolated_note_order=notes, isolated_outputs=isolates, notes=note_reports,
                  timing={'scope': '14-second native offline script, three runs each, alternating order; includes raw I/O',
                          'seconds': timings, 'median_seconds': medians, 'median_ratio': medians['candidate']/medians['release']},
                  validation=['Finite output; peak below full scale', 'Exact WAV/raw equality', 'Same complete MIDI events, preset and export gain as baseline',
                              'C4, C6 and F#6 match release bit for bit in isolated runs', 'Source and executable hashes unchanged during comparison'])
    args.report.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8', newline='\n')
    print(json.dumps({'full_output': full_wav, 'timing': report['timing']}), flush=True)


if __name__ == '__main__':
    main()
