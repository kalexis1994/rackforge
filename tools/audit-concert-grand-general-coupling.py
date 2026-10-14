"""Audit common mechanical-port normalization without per-note retuning.

Screen every key at two velocities, compare the previously known references,
check long holds, measure native cost, then render two complete MIDI pieces.
Parallel screening timings are not performance measurements.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
from pathlib import Path
import statistics
import subprocess

import numpy as np
from scipy.io import wavfile
from threadpoolctl import threadpool_limits


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec); spec.loader.exec_module(result)
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--work', type=Path, required=True)
    p.add_argument('--audio-dir', type=Path, required=True)
    p.add_argument('--midi-dir', type=Path, required=True)
    p.add_argument('--report', type=Path, required=True)
    a = p.parse_args()
    root = Path(__file__).resolve().parents[1]
    body = module('body', root/'tools/measure-concert-grand-release-body.py')
    midi = module('midi', root/'tools/render-concert-grand-release-midi.py')
    docs = root/'docs/concert-grand-release-body-2026-10-14'
    accepted_path = docs/'top-unison-comparison.json'
    accepted = json.loads(accepted_path.read_text(encoding='utf-8'))
    refs_path = docs/'baseline.json'
    refs = json.loads(refs_path.read_text(encoding='utf-8'))
    refmap = {(r['note'], r['velocity']): r['reference'] for r in refs['cases']}
    baseline = Path(accepted['full_render']['command'][0])
    sdk = Path(next(path for path in accepted['files_sha256'] if path.endswith('librackforge_plugin_sdk.rlib')))
    assert body.digest(baseline) == accepted['files_sha256'][str(baseline)]
    assert body.digest(sdk) == accepted['files_sha256'][str(sdk)]
    assert not a.work.exists() and not a.audio_dir.exists() and not a.report.exists()
    a.work.mkdir(parents=True); a.audio_dir.mkdir(parents=True)
    build = a.work/'build'; build.mkdir()
    library = build/'librackforge_concert_grand.rlib'; exe = build/'probe.exe'
    rust = ['rustc', '+1.98.0-x86_64-pc-windows-msvc', '--edition=2024', '-O']
    commands = [
        [*rust, '--crate-name', 'rackforge_concert_grand', '--crate-type', 'rlib', '--extern', f'rackforge_plugin_sdk={sdk}', str(root/'plugins/concert-grand/src/lib.rs'), '-o', str(library)],
        [*rust, '-L', f'dependency={build}', '-L', f'dependency={sdk.parent}', '--extern', f'rackforge_plugin_sdk={sdk}', '--extern', f'rackforge_concert_grand={library}', str(root/'tools/concert-grand-release-probe.rs'), '-o', str(exe)],
    ]
    for command in commands: subprocess.run(command, cwd=root, check=True)
    sources = [Path(__file__), root/'plugins/concert-grand/src/lib.rs', root/'plugins/concert-grand/src/math.rs',
               root/'crates/rackforge-plugin-sdk/src/lib.rs', root/'crates/rackforge-plugin-sdk/src/portable.rs',
               root/'tools/concert-grand-release-probe.rs', root/'tools/measure-concert-grand-release-body.py',
               root/'tools/render-concert-grand-release-midi.py', baseline, sdk, library, exe]
    frozen = {str(path.resolve()): body.digest(path) for path in sources}
    rate = 44100

    def render(binary, events, raw, frames):
        command = [str(binary), str(events), str(raw), str(rate), str(frames), '256', 'concert']
        native = json.loads(subprocess.check_output(command, text=True, cwd=root))
        x = np.fromfile(raw, dtype='<f4').reshape(-1, 2)
        assert x.shape == (frames, 2) and np.isfinite(x).all() and 0 < abs(x).max() < 1
        return x, dict(command=command, native=native, raw_sha256=body.digest(raw))

    def write_wav(name, x):
        path = a.audio_dir/name
        wavfile.write(path, rate, x)
        sr, y = wavfile.read(path)
        assert sr == rate and np.array_equal(x, y)
        return dict(path=str(path), sha256=body.digest(path), frames=len(x), rate=rate, export_gain=1)

    key = 'fall_from_100_200ms_to_500_1400ms_db'
    screen = a.work/'screen'; screen.mkdir()
    def case(pair):
        note, velocity = pair
        event = screen/f'{note}-{velocity}.txt'
        event.write_text(f'0 144 {note} {velocity}\n88200 128 {note} 64\n', encoding='utf-8', newline='\n')
        paths = [screen/f'{note}-{velocity}-{label}.f32' for label in ('A', 'B')]
        aa, _ = render(baseline, event, paths[0], 4*rate)
        bb, _ = render(exe, event, paths[1], 4*rate)
        ma, mb = body.measure(aa.astype(float), note), body.measure(bb.astype(float), note)
        row = dict(note=note, velocity=velocity,
                   H_fall_A=[r[key] for r in ma['partials']], H_fall_B=[r[key] for r in mb['partials']],
                   stage_A=[r['stereo_rms_db'] for r in ma['stages']], stage_B=[r['stereo_rms_db'] for r in mb['stages']],
                   peak_A=ma['peak'], peak_B=mb['peak'], raw_paths=[str(path) for path in paths],
                   raw_sha256=[body.digest(path) for path in paths])
        row['stage_delta_db'] = (np.array(row['stage_B'])-row['stage_A']).tolist()
        if pair in refmap:
            row['reference_H_fall'] = [r[key] for r in refmap[pair]['partials']]
        return row

    with threadpool_limits(limits=1):
        with ThreadPoolExecutor(max_workers=4) as pool:
            cases = list(pool.map(case, [(note, velocity) for note in range(21, 109) for velocity in (60, 110)]))
    reference_cases = [r for r in cases if 'reference_H_fall' in r]
    mean_error = {label: np.mean([np.abs(np.array(r[f'H_fall_{label}'])-r['reference_H_fall']) for r in reference_cases], axis=0).tolist() for label in ('A', 'B')}
    stage_delta = np.array([row['stage_delta_db'] for row in cases])
    summary = dict(keys=88, velocities=[60, 110], pairs=len(cases), known_reference_pairs=len(reference_cases),
                   reference_H1_H2_H3_mean_absolute_fall_error_db=mean_error,
                   stages_seconds=[[0,.1],[.15,.8],[.9,1.4],[1.5,1.9],[2.3,2.8]],
                   stage_delta_median_db=np.median(stage_delta, axis=0).tolist(),
                   stage_delta_min_db=stage_delta.min(axis=0).tolist(), stage_delta_max_db=stage_delta.max(axis=0).tolist(),
                   large_attack_changes=[r['note:velocity'] if 'note:velocity' in r else [r['note'],r['velocity'],r['stage_delta_db'][0]] for r in cases if abs(r['stage_delta_db'][0])>1],
                   note='Changes in all 88 keys are screened. Only the twelve known reference pairs have a reference comparison; not blind validation.')
    print('88-key screen:', json.dumps(summary), flush=True)

    # Long holds cannot hide a late beat or a ringing tail outside four seconds.
    long_rows = []
    for note in (21,36,48,57,60,63,72,84,96,108):
        event = a.work/f'long-{note}.txt'
        event.write_text(f'0 144 {note} 110\n{12*rate} 128 {note} 64\n', encoding='utf-8', newline='\n')
        row = dict(note=note, velocity=110, windows_seconds=[[3.5,4.5],[7.5,8.5],[11,11.5],[13,14]])
        for label, binary in [('A',baseline),('B',exe)]:
            x, result = render(binary, event, a.work/f'long-{note}-{label}.f32', 14*rate)
            row[label] = dict(render=result, rms_db=[float(10*np.log10(max(float(np.mean(x[round(lo*rate):round(hi*rate)].astype(float)**2)),1e-30))) for lo,hi in row['windows_seconds']])
        long_rows.append(row)

    # Timings are deliberately sequential, after parallel screening completes.
    bench_file = baseline.parent/'benchmark.txt'
    timings = {'A': [], 'B': []}
    for repeat in range(3):
        order = [('A',baseline),('B',exe)]
        if repeat % 2: order.reverse()
        for label, binary in order:
            _, result = render(binary, bench_file, a.work/f'bench-{repeat}-{label}.f32', 14*rate)
            timings[label].append(result['native']['wall_seconds'])
    medians = {label: statistics.median(values) for label,values in timings.items()}
    print('Native timing:', medians, 'ratio',medians['B']/medians['A'], flush=True)
    isolated_order = [(note,velocity) for velocity in (60,110) for note in (36,48,57,60,63,72,84,96)]
    gap = np.zeros((rate//4,2),dtype=np.float32)
    isolated = []
    for side,label in enumerate(('A-base-sin-ajuste-local','B-acoplamiento-general')):
        parts = []
        for pair in isolated_order:
            row = next(r for r in cases if (r['note'],r['velocity'])==pair)
            parts.extend((np.fromfile(row['raw_paths'][side],dtype='<f4').reshape(-1,2)[:3*rate],gap))
        isolated.append(write_wav(f'Notas-{label}.wav',np.concatenate(parts)))

    pieces = []
    for name, filename, tail in [('Campanella','liszt-la-campanella.mid',5),('Nocturno','chopin-nocturno-op9-2.mid',8)]:
        path = a.midi_dir/filename
        sources.append(path); frozen[str(path.resolve())] = body.digest(path)
        source, duration = midi.midi_events(path)
        events, skipped = [], []
        for seconds,message in source:
            if message[0]&240 == 192:
                assert seconds == 0 and message == [192,0]
                skipped.append(message); continue
            assert len(message)==3 and message[0]&15==0
            value = seconds*rate
            frame = (2*value.numerator+value.denominator)//(2*value.denominator)
            events.append((frame,*message))
        value = duration*rate
        frames = (2*value.numerator+value.denominator)//(2*value.denominator)+tail*rate
        event_path = a.work/f'{name}-events.txt'
        event_path.write_text(''.join(' '.join(map(str,e))+'\n' for e in events),encoding='utf-8',newline='\n')
        piece = dict(name=name,midi=str(path),midi_sha256=body.digest(path),midi_duration_seconds=float(duration),
                     duration_seconds=frames/rate,events=len(events),events_sha256=body.digest(event_path),skipped_initial_program=skipped,
                     tail_seconds=tail,synthetic_events=[],final_pedal_state_preserved=True)
        if name=='Campanella':
            output = accepted['full_output']
            assert body.digest(output['path'])==output['sha256']
            assert body.digest(event_path)==accepted['files_sha256'][str(Path(accepted['full_render']['command'][1]))]
            piece['A_output'] = output
        else:
            x,result=render(baseline,event_path,a.work/f'{name}-A.f32',frames)
            piece['A_render']=result;piece['A_output']=write_wav(f'{name}-A-base-sin-ajuste-local.wav',x)
        x,result=render(exe,event_path,a.work/f'{name}-B.f32',frames)
        assert float(np.sqrt(np.mean(x[-rate//10:].astype(float)**2)))<1e-5
        piece['B_render']=result;piece['B_output']=write_wav(f'{name}-B-acoplamiento-general.wav',x)
        pieces.append(piece)
        print(name,'B rendered in',result['native']['wall_seconds'],'seconds',flush=True)

    assert frozen=={str(path.resolve()):body.digest(path) for path in sources}
    report=dict(scope=__doc__,status='Common physical-model correction; listening comparison pending',
                baseline='Accepted treble residual, local C4 tuning removed (0b7cf7f5)',
                physical_change='Normalize the rank-one mechanical port by its own active-string norm; mask absent strings; retain quiet coupled coordinates',
                source_diff_base_commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),
                source_diff=subprocess.check_output(['git','diff','--','plugins/concert-grand/src/lib.rs'],cwd=root,text=True),
                files_sha256=frozen,compile_commands=commands,summary=summary,cases=cases,long_holds=long_rows,
                timing=dict(scope='14-second native script; three sequential runs each, alternating order, includes raw I/O',seconds=timings,medians=medians,ratio=medians['B']/medians['A']),
                isolated_order=isolated_order,isolated_outputs=isolated,pieces=pieces,
                prior_reference_report_sha256=body.digest(refs_path),accepted_baseline_report_sha256=body.digest(accepted_path),
                self_check=body.self_check(),limitations='Internal norm contraction is not SI energy or proof of perceptual improvement. No manual note-specific retuning. Known references cover twelve cases, not the entire keyboard.')
    a.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n',encoding='utf-8',newline='\n')


if __name__=='__main__':main()
