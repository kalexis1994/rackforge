"""Render a local C4 unison voicing against the accepted treble-only piano.

One continuous instance for the full MIDI. No extra gain, normalization or EQ.
The candidate is for listening; reference metrics do not establish acceptance.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import statistics
import subprocess

import numpy as np
from scipy.io import wavfile


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--work', type=Path, required=True)
    p.add_argument('--audio-dir', type=Path, required=True)
    p.add_argument('--report', type=Path, required=True)
    args = p.parse_args()
    root = Path(__file__).resolve().parents[1]
    spec = importlib.util.spec_from_file_location('body', root / 'tools/measure-concert-grand-release-body.py')
    body = importlib.util.module_from_spec(spec); spec.loader.exec_module(body)
    docs = root / 'docs/concert-grand-release-body-2026-10-14'
    accepted = json.loads((docs / 'top-unison-comparison.json').read_text(encoding='utf-8'))
    midi_base = json.loads((docs / 'campanella-baseline.json').read_text(encoding='utf-8'))
    references = json.loads((docs / 'baseline.json').read_text(encoding='utf-8'))
    reference_map = {(r['note'], r['velocity']): r['reference'] for r in references['cases']}
    baseline = Path(accepted['full_render']['command'][0])
    baseline_raw = Path(accepted['full_render']['command'][2])
    events = Path(midi_base['render_command'][1])
    sdk = Path(midi_base['render_command'][0]).parent / 'librackforge_plugin_sdk.rlib'
    assert body.digest(baseline) == accepted['files_sha256'][str(baseline.resolve())]
    assert body.digest(baseline_raw) == accepted['full_render']['raw_sha256']
    assert body.digest(accepted['full_output']['path']) == accepted['full_output']['sha256']
    assert body.digest(events) == midi_base['files_sha256'][str(events.resolve())]
    assert body.digest(sdk) == midi_base['files_sha256'][str(sdk.resolve())]
    assert not args.audio_dir.exists() and not args.report.exists()
    args.work.mkdir(parents=True, exist_ok=True); args.audio_dir.mkdir(parents=True)
    library = args.work / 'librackforge_concert_grand.rlib'; exe = args.work / 'probe.exe'
    rust = ['rustc', '+1.98.0-x86_64-pc-windows-msvc', '--edition=2024', '-O']
    compile_commands = [
        [*rust, '--crate-name', 'rackforge_concert_grand', '--crate-type', 'rlib', '--extern', f'rackforge_plugin_sdk={sdk}', str(root / 'plugins/concert-grand/src/lib.rs'), '-o', str(library)],
        [*rust, '-L', f'dependency={args.work}', '-L', f'dependency={sdk.parent}', '--extern', f'rackforge_plugin_sdk={sdk}', '--extern', f'rackforge_concert_grand={library}', str(root / 'tools/concert-grand-release-probe.rs'), '-o', str(exe)],
    ]
    for command in compile_commands:
        subprocess.run(command, cwd=root, check=True)
    paths = [Path(__file__), root/'plugins/concert-grand/src/lib.rs', root/'plugins/concert-grand/src/math.rs',
             root/'crates/rackforge-plugin-sdk/src/lib.rs', root/'crates/rackforge-plugin-sdk/src/portable.rs',
             root/'tools/concert-grand-release-probe.rs', root/'tools/measure-concert-grand-release-body.py',
             baseline, baseline_raw, sdk, library, exe, events]
    frozen = {str(path.resolve()): body.digest(path) for path in paths}
    rate = 44100

    def render(binary, event_file, raw, frames):
        command = [str(binary), str(event_file), str(raw), str(rate), str(frames), '256', 'concert']
        native = json.loads(subprocess.check_output(command, cwd=root, text=True))
        audio = np.fromfile(raw, dtype='<f4').reshape(-1, 2)
        assert audio.shape == (frames, 2) and np.isfinite(audio).all() and 0 < abs(audio).max() < 1
        return audio, dict(command=command, native=native, raw_sha256=body.digest(raw))

    def wav(name, audio):
        path = args.audio_dir/name
        wavfile.write(path, rate, audio)
        sr, readback = wavfile.read(path)
        assert sr == rate and np.array_equal(readback, audio)
        return dict(path=str(path.resolve()), sha256=body.digest(path), frames=len(audio), export_gain=1)

    audio, full = render(exe, events, args.work/'campanella-body.f32', midi_base['native']['frames'])
    full_output = wav('Campanella-B-cuerpo-medio.wav', audio)
    print('Full candidate rendered in', full['native']['wall_seconds'], 'seconds', flush=True)
    cases, takes = [], {}
    for note in (48, 57, 58, 59, 60, 61, 62, 63, 72, 84):
        for velocity in (60, 110):
            event_file = args.work/f'{note}-{velocity}.txt'
            event_file.write_text(f'0 144 {note} {velocity}\n88200 128 {note} 64\n', encoding='utf-8', newline='\n')
            a, ar = render(baseline, event_file, args.work/f'{note}-{velocity}-A.f32', 4*rate)
            b, br = render(exe, event_file, args.work/f'{note}-{velocity}-B.f32', 4*rate)
            ma, mb = body.measure(a.astype(float), note), body.measure(b.astype(float), note)
            identical = bool(np.array_equal(a, b))
            assert identical == (note not in range(58, 63)), 'change escaped the intended notes'
            attack = float(20*np.log10(np.linalg.norm(b[:rate//10].astype(float))/np.linalg.norm(a[:rate//10].astype(float))))
            assert abs(attack) < .25, 'attack level changed too much'
            row = dict(note=note, velocity=velocity, baseline=ar, candidate=br, baseline_measured=ma, candidate_measured=mb,
                       identical=identical, attack_100ms_level_delta_db=attack,
                       relative_rms_difference=float(np.linalg.norm(b.astype(float)-a)/np.linalg.norm(a.astype(float))))
            if (note, velocity) in reference_map:
                row['reference'] = reference_map[note, velocity]
            if note == 60:
                key = 'fall_from_100_200ms_to_500_1400ms_db'
                before, after, target = ma['partials'][0][key], mb['partials'][0][key], row['reference']['partials'][0][key]
                assert abs(after-target) < abs(before-target)
                for harmonic in (1, 2):
                    old_fall = ma['partials'][harmonic][key]
                    new_fall = mb['partials'][harmonic][key]
                    ref_fall = row['reference']['partials'][harmonic][key]
                    assert abs(new_fall-ref_fall) <= abs(old_fall-ref_fall)+.25, 'upper-partial decay regressed'
                print('C4', velocity, 'H1 fall A/B/reference:', before, after, target, flush=True)
            cases.append(row); takes[note, velocity] = (a, b)
    silence = np.zeros((rate//4, 2), dtype=np.float32)
    order = [(n, v) for v in (60, 110) for n in (57, 59, 60, 61, 63, 72)]
    isolated = [wav(f'Notas-{label}.wav', np.concatenate([part for key in order for part in (takes[key][side][:3*rate], silence)]))
                for side, label in enumerate(('A-base-aceptada', 'B-cuerpo-medio'))]
    c4_ab = wav('Do-central-A-B.wav', np.concatenate([part for velocity in (60, 110)
                for audio in takes[60, velocity] for part in (audio[:3*rate], silence)]))
    bench_file = baseline.parent/'benchmark.txt'
    timings = {'baseline': [], 'candidate': []}
    for repeat in range(3):
        order_bench = [('baseline', baseline), ('candidate', exe)]
        if repeat % 2: order_bench.reverse()
        for label, binary in order_bench:
            _, result = render(binary, bench_file, args.work/f'bench-{repeat}-{label}.f32', 14*rate)
            timings[label].append(result['native']['wall_seconds'])
    medians = {k: statistics.median(v) for k, v in timings.items()}
    assert frozen == {str(path.resolve()): body.digest(path) for path in paths}
    report = dict(scope=__doc__, candidate_status='Listening candidate, not yet accepted',
                  change={'C4_FUNDAMENTAL_UNISON_NARROWING': .30, 'affected_partial': 'fundamental only', 'affected_notes': [58, 59, 60, 61, 62],
                          'width_factors': [.9, .8, .7, .8, .9], 'extra_oscillators': 0, 'extra_per_sample_operations': 0},
                  files_sha256=frozen, source_diff=subprocess.check_output(['git', 'diff', '--', 'plugins/concert-grand/src/lib.rs'], text=True, cwd=root),
                  source_diff_base_commit=subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True, cwd=root).strip(),
                  diagnosis_sha256=body.digest(docs/'body-state-diagnosis.json'), accepted_baseline_sha256=body.digest(docs/'top-unison-comparison.json'),
                  compile_commands=compile_commands, full_render=full, full_output=full_output,
                  baseline_output=accepted['full_output'], duration_seconds=midi_base['native']['frames']/rate,
                  isolated_outputs=isolated, isolated_order=order, c4_AB_output=c4_ab, cases=cases,
                  timing={'scope': 'Three 14-second native runs each, alternating order, includes raw I/O', 'seconds': timings,
                          'medians': medians, 'ratio': medians['candidate']/medians['baseline'], 'events_sha256': body.digest(bench_file)},
                  validation=['Finite audio below full scale', 'WAV/raw equality', 'Same complete MIDI events, preset and export gain',
                              'All five outside controls identical at both velocities', 'Attack RMS change below 0.25 dB',
                              'C4 H1 fall closer to the historical reference at both velocities', 'Measured sources and binaries unchanged'])
    args.report.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8', newline='\n')
    print(json.dumps({'output': full_output, 'timing': report['timing']}), flush=True)


if __name__ == '__main__':
    main()
