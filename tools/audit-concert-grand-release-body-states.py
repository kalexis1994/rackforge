"""Separate modal amplitude loss, unison cancellation and final stereo decay.

All internal squared norms have arbitrary model units, not joules. The tuning
controls are diagnostic; they do not select a global width for the instrument.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess

import numpy as np


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--trace-probe', type=Path, required=True)
    parser.add_argument('--native-probe', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    spec = importlib.util.spec_from_file_location('body', root / 'tools/measure-concert-grand-release-body.py')
    body = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(body)
    historical_path = root / 'docs/concert-grand-release-body-2026-10-14/baseline.json'
    historical = json.loads(historical_path.read_text(encoding='utf-8'))
    accepted_path = root / 'docs/concert-grand-release-body-2026-10-14/top-unison-comparison.json'
    accepted = json.loads(accepted_path.read_text(encoding='utf-8'))
    assert body.digest(args.native_probe) == accepted['files_sha256'][str(args.native_probe.resolve())]
    assert not args.work.exists() and not args.report.exists()
    args.work.mkdir(parents=True)
    source_paths = [Path(__file__), root / 'plugins/concert-grand/src/lib.rs', root / 'plugins/concert-grand/src/math.rs',
                    root / 'crates/rackforge-plugin-sdk/src/lib.rs', root / 'crates/rackforge-plugin-sdk/src/portable.rs',
                    root / 'tools/measure-concert-grand-release-body.py', args.trace_probe, args.native_probe]
    frozen = {str(p.resolve()): body.digest(p) for p in source_paths}
    cases = [(note, velocity) for note in (48, 57, 58, 59, 60, 61, 62, 63, 72, 84) for velocity in (60, 110)]
    historical_map = {(row['note'], row['velocity']): row for row in historical['cases']}
    results = []
    settings = [('base', None), ('narrow70', .35), ('narrow85', .425), ('wide115', .575)]
    for tag, width in settings:
        grid = cases if width is None else [(60, 60), (60, 110)]
        folder = args.work / tag
        env = {k: v for k, v in os.environ.items() if not k.upper().startswith('CG_')}
        env.update(RUST_MIN_STACK='33554432', CG_BODY_TRACE_DIR=str(folder.resolve()),
                   CG_BODY_TRACE_GRID=','.join(f'{n}:{v}' for n, v in grid))
        tuning = None
        if width is not None:
            tuning = args.work / f'{tag}.txt'
            tuning.write_text(f'UNISON_WIDTH = {width}\n', encoding='utf-8', newline='\n')
            env['CG_BODY_TRACE_TUNING'] = str(tuning.resolve())
        command = [str(args.trace_probe.resolve()), '--exact', 'tests::export_release_body_trace', '--ignored', '--nocapture', '--test-threads=1']
        result = subprocess.run(command, env=env, capture_output=True, text=True, check=True)
        assert '1 passed; 0 failed' in result.stdout
        (args.work / f'{tag}.log').write_text(result.stdout, encoding='utf-8', newline='\n')
        for note, velocity in grid:
            raw = folder / f'{note}-{velocity}.f32'
            csv = raw.with_suffix('.csv')
            audio = np.fromfile(raw, dtype='<f4').reshape(-1, 2).astype(np.float64)
            measured = body.measure(audio, note)
            events = folder / f'{note}-{velocity}.txt'
            events.write_text(f'0 144 {note} {velocity}\n88200 128 {note} 64\n', encoding='utf-8', newline='\n')
            native_raw = folder / f'{note}-{velocity}-independent.f32'
            native_command = [str(args.native_probe.resolve()), str(events.resolve()), str(native_raw.resolve()), '44100', '176400', '256', 'concert']
            if tuning:
                native_command.append(str(tuning.resolve()))
            native = json.loads(subprocess.check_output(native_command, text=True))
            assert body.digest(raw) == body.digest(native_raw), 'instrumentation changed the audio'
            d = np.genfromtxt(csv, delimiter=',', names=True)
            t = (d['frame'] - measured['analysis_onset_frame']) / body.RATE
            z = [d[s] + 1j*d[c] for s, c in [('s0', 'c0'), ('s1', 'c1'), ('s2', 'c2'), ('sh', 'ch'), ('sb', 'cb')]]
            stored_v = sum(abs(value)**2 for value in z[:3])
            sum_v = abs(sum(z[:3]))**2
            coherence = sum_v / np.maximum(3 * stored_v, 1e-30)
            assert np.all((coherence >= 0) & (coherence <= 1 + 1e-12))
            channels = dict(vertical_norm_squared=stored_v, vertical_plus_horizontal_norm_squared=stored_v+abs(z[3])**2,
                            vertical_sum_squared=sum_v, coherence=coherence,
                            projected_partial_squared=abs(sum(z))**2,
                            bridge_weighted_sum_squared=abs(sum(z[:3])+d['horizontal']*z[3])**2)
            internal = {}
            for name, power in channels.items():
                level = 10 * np.log10(np.maximum(power, 1e-30))
                early = (t >= .1) & (t < .2)
                late = (t >= .5) & (t < 1.4)
                internal[name] = dict(fall_db=float(level[late].mean()-level[early].mean()),
                                      slopes=body.slopes(t, level))
            # Exact decomposition in dB: vertical sum = stored norm * 3 * coherence.
            assert abs(internal['vertical_sum_squared']['fall_db'] - internal['vertical_norm_squared']['fall_db']
                       - internal['coherence']['fall_db']) < 1e-9
            row = dict(tag=tag, unison_width=.5 if width is None else width, note=note, velocity=velocity,
                       measured=measured, internal=internal, trace_path=str(csv.resolve()), trace_sha256=body.digest(csv),
                       raw_path=str(raw.resolve()), raw_sha256=body.digest(raw), trace_equals_independent_native=True,
                       native_command=native_command, native=native)
            old = historical_map.get((note, velocity))
            if old:
                row['reference_H1_fall_db'] = old['reference']['partials'][0]['fall_from_100_200ms_to_500_1400ms_db']
                if tag == 'base':
                    assert abs(measured['partials'][0]['fall_from_100_200ms_to_500_1400ms_db'] - old['release']['partials'][0]['fall_from_100_200ms_to_500_1400ms_db']) < 1e-8
            results.append(row)
            if note == 60:
                print(tag, velocity, 'internal norm / projection / audio fall',
                      *[round(value, 3) for value in (internal['vertical_plus_horizontal_norm_squared']['fall_db'],
                        internal['projected_partial_squared']['fall_db'], measured['partials'][0]['fall_from_100_200ms_to_500_1400ms_db'])], flush=True)
    assert frozen == {str(p.resolve()): body.digest(p) for p in source_paths}
    report = dict(scope=__doc__, units='Internal squared modal norms are arbitrary amplitude units, not physical energy in joules',
                  files_sha256=frozen, historical_report_sha256=body.digest(historical_path), accepted_report_sha256=body.digest(accepted_path),
                  self_check=body.self_check(), rate=body.RATE, block=256, warmup_seconds=.2, note_off_seconds=2,
                  source_unchanged=True, cases=results,
                  conclusion='C4 has both modal amplitude decay and cancellation. Narrowing its unison reduces the excess projected decay; no global loss reduction selected.')
    args.report.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8', newline='\n')


if __name__ == '__main__':
    main()
