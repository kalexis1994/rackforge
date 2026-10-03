"""Separate native string loss from bridge loading against recorded pressure.

Drain ablations deliberately break the calibrated mechanical/acoustic relation:
they are diagnostic controls, not installable piano candidates. Every render
uses the full native receiver, one fixed output gain, and no per-note EQ.
Reference recordings do not uniquely identify mechanical admittance.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import csv
import importlib.util
import json
from pathlib import Path

import numpy as np
import soundfile as sf


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for key in ('binary', 'expected-sha256', 'reference', 'work', 'output'):
        p.add_argument('--'+key, required=True, type=str if key == 'expected-sha256' else Path)
    p.add_argument('--profiles', type=Path)
    p.add_argument('--loading', type=Path)
    p.add_argument('--all-pairs', action='store_true')
    p.add_argument('--audition', action='store_true', help='C4/C6 controls, four intensities, 2s held and release')
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=True)
    tools = Path(__file__).resolve().parent
    body = module(tools/'identify-concert-grand-body.py', 'decay_body')
    fit, attack = body.FIT, body.ATTACK
    assert fit.AUDIT.sha(a.binary) == a.expected_sha256
    groups = dict(training=body.TRAIN, reserved_pitches=body.PITCH_TEST)
    if a.all_pairs:
        groups['reserved_velocities'] = body.VELOCITY_TEST
    pairs = [pair for group in groups.values() for pair in group]
    variants = dict(Actual=dict(tuning={}, params={}))
    variants.update(json.loads(a.profiles.read_text()) if a.profiles else {
        'Puente-mitad': dict(tuning={'DIAGNOSTIC_STRING_DRAIN': .5}, params={}),
        'Puente-sin-drenaje': dict(tuning={'DIAGNOSTIC_STRING_DRAIN': 0}, params={}),
        'Cuerda-perdidas-menores': dict(tuning={'SLOW_STAGE_RATIO': 6}, params={}),
        'Unisono-sin-perdida-extra': dict(tuning={'UNISON_EXTRA_LOSS': 0}, params={}),
    })
    refs, target, target_attack, identities = {}, {}, {}, {}
    provenance = json.loads((a.reference/'reference-manifest.json').read_text())
    for n, v in pairs:
        key = f'{n}:{v}'
        path = a.reference/'reference'/f'{n}-{v}.wav'
        meta = json.loads(path.with_suffix('.json').read_text())
        assert meta['source'] == provenance['sources'][key]
        assert all(meta[k] == provenance[k] for k in ('mapping_sha256', 'sfz_sha256', 'dll_sha256'))
        # Verify the original recorded source, not only the cached sampler WAV.
        source = Path(meta['source']['file'])
        assert fit.AUDIT.sha(source) == meta['source']['sha256']
        y, rate = sf.read(path, always_2d=True)
        assert rate == body.RATE
        refs[key] = y
        target[key], target_attack[key] = fit.measure(y, n), attack.measure(y, n)
        identities[key] = dict(reference_wav_sha256=fit.AUDIT.sha(path), source=meta['source'])
    report = dict(policy=__doc__, binary_sha256=a.expected_sha256, groups=groups,
                  references=identities, profiles={}, export_gain_db=0, selection=None)
    # Baseline is an identity control, not a candidate expected to improve itself.
    if a.loading:
        loading = {}
        for path in sorted(a.loading.glob('loading*v*.csv')):
            with path.open() as stream:
                rows = [{k: float(v) for k, v in row.items()} for row in csv.DictReader(stream)]
            for row in rows:
                # effective_rate_s divides the collective contraction by norm;
                # it is per-string, despite the old ambiguous column name.
                row['collective_bridge_amplitude_rate_s'] = row['mechanical_norm']*row['intended_rate_s']
                row['intrinsic_t60_s'] = np.log(1000)/row['intrinsic_amplitude_rate_s']
                row['coherent_bridge_t60_s'] = np.log(1000)/row['collective_bridge_amplitude_rate_s']
            loading[path.stem] = dict(sha256=fit.AUDIT.sha(path), partials=rows)
        report['loading'] = loading
    baseline, baseline_attack = {}, {}
    def run(job):
        label, profile, n, v = job
        raw = fit.AUDIT.render(a.binary, f'0 144 {n} {v}\n', round(1.7*body.RATE),
                               a.work/label/f'{n}-{v}', profile['tuning'], params=profile['params'])
        x = np.array(fit.AUDIT.load(raw), dtype=float)
        assert np.isfinite(x).all() and abs(x).max() < 1
        key = f'{n}:{v}'
        feature = fit.measure(x, n)
        row = fit.score(target[key], feature)
        row['H1_fall_db'] = feature['partials_db'][-1][0]-feature['partials_db'][1][0]
        row['reference_H1_fall_db'] = target[key]['partials_db'][-1][0]-target[key]['partials_db'][1][0]
        row['peak'] = float(abs(x).max())
        return key, row, attack.compare(target_attack[key], attack.measure(x, n)), fit.AUDIT.sha(raw)
    with ThreadPoolExecutor(max_workers=2) as pool:
        for label, profile in variants.items():
            results = list(pool.map(run, [(label, profile, n, v) for n, v in pairs]))
            rows = {key: row for key, row, _, _ in results}
            attacks = {key: value for key, _, value, _ in results}
            if label == 'Actual':
                baseline, baseline_attack = rows, attacks
            summaries = {}
            for group, group_pairs in groups.items():
                keys = [f'{n}:{v}' for n, v in group_pairs]
                subset, base = ({k: r[k] for k in keys} for r in (rows, baseline))
                stats, base_stats = fit.aggregate(subset), fit.aggregate(base)
                summaries[group] = dict(metrics=stats, acceptance=None if label == 'Actual' else fit.acceptance(stats, subset, base_stats, base),
                    temporal=attack.guards({k: attacks[k] for k in keys}, {k: baseline_attack[k] for k in keys}))
            diagnostic = 'DIAGNOSTIC_STRING_DRAIN' in profile['tuning']
            report['profiles'][label] = dict(profile=profile, diagnostic_decoupling=diagnostic,
                summaries=summaries, rows=rows, attacks=attacks,
                audio_sha256={k: h for k, _, _, h in results})
            (a.output/'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
            print(label, 'C4', json.dumps(rows['60:60']), flush=True)
            print(label, json.dumps({g: {'total': s['metrics']['total'], 'decay': s['metrics']['decay_db'],
                'temporal_pass': s['temporal']['passed'], 'acceptance': None if s['acceptance'] is None else s['acceptance']['passed']}
                for g, s in summaries.items()}), flush=True)
    if a.audition:
        audition = dict(export_gain_db=0, diagnostic_not_candidate=True, roots=[60, 84],
                        velocities=[28, 60, 92, 124], schedule=[], files={})
        for n in (60, 84):
            name = {60: 'C4', 84: 'C6'}[n]
            pieces = {k: [] for k in ('Actual', 'Prueba-sin-carga', 'Steinway')}
            for i, v in enumerate(audition['velocities']):
                audition['schedule'].append(dict(note=n, velocity=v, start_s=i*3, note_off_s=i*3+2))
                for label, tuning in [('Actual', {}), ('Prueba-sin-carga', {'DIAGNOSTIC_STRING_DRAIN': 0})]:
                    raw = fit.AUDIT.render(a.binary, f'0 144 {n} {v}\n88200 128 {n} 64\n', 176400,
                                           a.work/'audition'/label/f'{n}-{v}', tuning)
                    x = np.array(fit.AUDIT.load(raw), dtype=float)
                    assert np.isfinite(x).all() and abs(x).max() < 1
                    if label == 'Actual':
                        original = a.reference/'audition-capture'/f'{n}-{v}'/'performance.f32'
                        assert np.array_equal(x, fit.AUDIT.load(original)), 'Current control changed'
                    sf.write(a.output/f'{name}-{v}-{label}.wav', x, body.RATE, subtype='PCM_24')
                    audition['files'][f'{name}-{v}-{label}'] = dict(peak=float(abs(x).max()), raw_sha256=fit.AUDIT.sha(raw))
                    clip = x[:110250].copy()
                    clip[-441:] *= np.linspace(1, 0, 441)[:, None]
                    pieces[label].extend([clip, np.zeros((22050, 2))])
                ref = a.reference/'reference'/f'{n}-{v}.wav'
                x, rate = sf.read(ref, always_2d=True)
                assert rate == body.RATE and abs(x).max() < 1
                sf.write(a.output/f'{name}-{v}-Steinway.wav', x, rate, subtype='PCM_24')
                clip = x[:110250].copy()
                clip[-441:] *= np.linspace(1, 0, 441)[:, None]
                pieces['Steinway'].extend([clip, np.zeros((22050, 2))])
                audition['files'][f'{name}-{v}-Steinway'] = dict(reference_sha256=fit.AUDIT.sha(ref), peak=float(abs(x).max()))
            for label, parts in pieces.items():
                sf.write(a.output/f'{name}-{label}-intensidades.wav', np.concatenate(parts), body.RATE, subtype='PCM_24')
        (a.output/'audition.json').write_text(json.dumps(audition, indent=2), encoding='utf-8')
    print('DONE', a.output, flush=True)


if __name__ == '__main__':
    main()
