"""Compare small fixed string differences with and without native bridge drain.

The no-drain family is a diagnostic control, not an installable calibration.
One tuning and stiffness per string apply across its whole harmonic ladder.
All exports retain unity gain and stereo. Long holds catch delayed modulation;
envelope notches are diagnostic events, not counts of independently audible
defects. This reused reference corpus is not blind validation.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
from pathlib import Path

import numpy as np
from scipy.signal import fftconvolve, find_peaks
import soundfile as sf

RATE = 44100
NOTES = (42, 48, 60, 72, 84, 90)
VELOCITIES = (28, 60, 92, 124)
LONG_NOTES = (60, 72, 84, 90)
INTERVALS = ((.1, .5), (.5, 1.5), (1.5, 3.5), (3.5, 7.5), (7.5, 11.5))


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def partial_envelope(x, frequency, fundamental, hold=12):
    # Mix at the audio rate; a prior audio decimation would alias treble.
    cycles = max(8, int(np.ceil(.04*fundamental)))
    width = int(round(cycles*RATE/fundamental)) | 1
    window = np.hanning(width)
    carrier = np.exp(-2j*np.pi*frequency*np.arange(len(x))/RATE)
    amplitudes = fftconvolve(x*carrier[:, None], window[:, None], mode='same', axes=0)*2/window.sum()
    indices = np.arange(round(.08*RATE), round((hold-.1)*RATE), round(.005*RATE))
    times = indices/RATE
    levels = 10*np.log10(np.maximum(np.mean(abs(amplitudes[indices])**2, axis=1), 1e-30))
    return times, levels


def notches(times, levels, floor):
    indices, props = find_peaks(-levels, prominence=3, distance=round(.12/(times[1]-times[0])))
    return [dict(time_s=float(times[index]), depth_db=float(props['prominences'][i]))
            for i, index in enumerate(indices) if .1 < times[index] < 11.5 and levels[index] > floor]


def long_measure(x, note, fit, support=None):
    f0 = 440*2**((note-69)/12)
    hz, spectrum = fit.EVAL.psd(x[round(.05*RATE):round(.5*RATE)])
    partials = []
    for h in range(1, min(16, int(10000/f0))+1):
        bins = np.flatnonzero(abs(hz-h*f0) < .18*f0)
        frequency = float(hz[bins[np.argmax(spectrum[bins])]])
        times, levels = partial_envelope(x, frequency, f0)
        ceiling = float(levels[(times >= .1)&(times < .5)].max())
        partials.append(dict(harmonic=h, frequency_hz=frequency, early_peak_db=ceiling,
                             times_s=times.tolist(), levels_db=levels.tolist()))
    if support is None:
        strongest = max(row['early_peak_db'] for row in partials)
        support = [row['early_peak_db'] > max(strongest-35, -85) for row in partials]
    for row, allowed in zip(partials, support):
        row['reference_supported'] = allowed
        # Same absolute floor for all native profiles; silence is separately
        # rejected by level/body guards and is not a beat-reduction success.
        row['notches'] = notches(np.array(row['times_s']), np.array(row['levels_db']), -90) if allowed else []
    selected = [row for row in partials if row['reference_supported']]
    return dict(partials=partials, support=support,
                counts=[sum(lo <= event['time_s'] < hi for row in selected for event in row['notches'])
                        for lo, hi in INTERVALS],
                depth_sums_db=[sum(event['depth_db'] for row in selected for event in row['notches']
                                   if lo <= event['time_s'] < hi) for lo, hi in INTERVALS],
                stereo_levels_db=[float(10*np.log10(np.mean(x[round(lo*RATE):round(hi*RATE)]**2)+1e-30))
                                  for lo, hi in ((.05, .2), (.5, 1.5), (1.5, 3.5), (3.5, 7.5), (7.5, 11.5))])


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for key in ('binary', 'reference', 'work', 'output'):
        p.add_argument('--'+key, type=Path, required=True)
    p.add_argument('--expected-sha256', required=True)
    p.add_argument('--variants', type=Path, help='Additional named, fixed global tuning profiles')
    p.add_argument('--extra-only', action='store_true', help='Keep Base and explicitly requested new variants')
    p.add_argument('--notes', default=','.join(map(str, NOTES)))
    p.add_argument('--velocities', default=','.join(map(str, VELOCITIES)))
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=True)
    tools = Path(__file__).resolve().parent
    body = module(tools/'identify-concert-grand-body.py', 'subtle_body')
    fit, attack = body.FIT, body.ATTACK
    assert fit.AUDIT.sha(a.binary) == a.expected_sha256
    variants = {
        'Base': {},
        'A-afinacion-minima': {'UNISON_WIDTH': .04},
        'B-afinacion-sutil': {'UNISON_WIDTH': .10},
        'C-afinacion-y-rigidez': {'UNISON_WIDTH': .10, 'STRING_STIFFNESS_SPREAD': .003},
        'D-afinacion-mayor': {'UNISON_WIDTH': .20},
    }
    if a.extra_only:
        assert a.variants
        variants = {'Base': {}}
    if a.variants:
        extra = json.loads(a.variants.read_text())
        assert not (variants.keys() & extra.keys()), 'Do not replace the frozen controls'
        variants.update(extra)
    profiles = {f'{family}/{label}': {**base, **tuning} for family, base in
                [('Carga-fisica', {}), ('Control-sin-carga', {'DIAGNOSTIC_STRING_DRAIN': 0})]
                for label, tuning in variants.items()}
    notes = tuple(map(int, a.notes.split(',')))
    velocities = tuple(map(int, a.velocities.split(',')))
    assert notes and set(notes).issubset(NOTES) and set(velocities).issubset(VELOCITIES)
    pairs = [(n, v) for n in notes for v in velocities]
    long_notes = tuple(n for n in LONG_NOTES if n in notes)
    assert long_notes
    reference_manifest = json.loads((a.reference/'reference-manifest.json').read_text())
    targets, target_attacks, source_identities = {}, {}, {}
    reference_parts = []
    for n, v in pairs:
        key = f'{n}:{v}'
        path = a.reference/'reference'/f'{n}-{v}.wav'
        meta = json.loads(path.with_suffix('.json').read_text())
        assert meta['source'] == reference_manifest['sources'][key]
        assert all(meta[k] == reference_manifest[k] for k in ('mapping_sha256', 'sfz_sha256', 'dll_sha256'))
        assert fit.AUDIT.sha(Path(meta['source']['file'])) == meta['source']['sha256']
        x, rate = sf.read(path, always_2d=True)
        assert rate == RATE and abs(x).max() < 1
        targets[key], target_attacks[key] = fit.measure(x, n), attack.measure(x, n)
        source_identities[key] = dict(metadata=meta, reference_sha256=fit.AUDIT.sha(path))
        sf.write(a.output/f'{n}-{v}-Steinway.wav', x, RATE, subtype='PCM_24')
        clip = x[:110250].copy()
        clip[-441:] *= np.linspace(1, 0, 441)[:, None]
        reference_parts.extend([clip, np.zeros((22050, 2))])
    sf.write(a.output/'Steinway-intensidades.wav', np.concatenate(reference_parts), RATE, subtype='PCM_24')
    # Independently render original samples through the same documented SFZ
    # mapping for long holds; never extend or loop a short reference WAV.
    fit.EVAL.HOLD, fit.EVAL.SECONDS = 12, 14
    sfz = tools.parent/'local/steinway-midi/render-stage/SteinwayD-StageORTF.sfz'
    dll = tools.parent/'local/steinway-midi/sfizz/library/bin/Release/sfizz.dll'
    assert fit.AUDIT.sha(sfz) == reference_manifest['sfz_sha256']
    assert fit.AUDIT.sha(dll) == reference_manifest['dll_sha256']
    long_refs = {}
    synth = fit.SAMPLER.Engine(dll, sfz)
    try:
        for n in long_notes:
            x = fit.EVAL.render_reference(synth, n, 124)
            assert abs(x).max() < 1
            path = a.output/f'{n}-124-Steinway-sostenida-12s.wav'
            sf.write(path, x, RATE, subtype='PCM_24')
            long_refs[str(n)] = long_measure(x, n, fit)
            long_refs[str(n)]['audio_sha256'] = fit.AUDIT.sha(path)
    finally:
        synth.close()
    report = dict(policy=__doc__, binary_sha256=a.expected_sha256, profiles={},
                  references=source_identities, long_reference=long_refs, export_gain_db=0,
                  sequence=[dict(note=n, velocity=v, start_s=3*i, note_off_s=3*i+2)
                            for i, (n, v) in enumerate(pairs)], intervals_s=INTERVALS,
                  selection=None, factory_changed=False)
    def run(job):
        label, tuning, n, v = job
        raw = fit.AUDIT.render(a.binary, f'0 144 {n} {v}\n88200 128 {n} 64\n', 176400,
                               a.work/label/f'{n}-{v}', tuning)
        x = np.array(fit.AUDIT.load(raw), dtype=float)
        assert np.isfinite(x).all() and abs(x).max() < 1
        key = f'{n}:{v}'
        return key, x, fit.score(targets[key], fit.measure(x, n)), attack.compare(target_attacks[key], attack.measure(x, n)), fit.AUDIT.sha(raw)
    baselines = {}
    with ThreadPoolExecutor(max_workers=2) as pool:
        for label, tuning in profiles.items():
            family, name = label.split('/')
            out = a.output/family
            out.mkdir(parents=True, exist_ok=True)
            rows, attacks, hashes, pieces = {}, {}, {}, []
            for key, x, row, temporal, digest in pool.map(run, [(label, tuning, n, v) for n, v in pairs]):
                rows[key], attacks[key], hashes[key] = row, temporal, digest
                sf.write(out/f'{key.replace(":", "-")}-{name}.wav', x, RATE, subtype='PCM_24')
                clip = x[:110250].copy()
                clip[-441:] *= np.linspace(1, 0, 441)[:, None]
                pieces.extend([clip, np.zeros((22050, 2))])
            sf.write(out/f'{name}-intensidades.wav', np.concatenate(pieces), RATE, subtype='PCM_24')
            long_rows = {}
            for n in long_notes:
                raw = fit.AUDIT.render(a.binary, f'0 144 {n} 124\n529200 128 {n} 64\n', 617400,
                                       a.work/label/f'long-{n}', tuning)
                x = np.array(fit.AUDIT.load(raw), dtype=float)
                assert np.isfinite(x).all() and abs(x).max() < 1
                sf.write(out/f'{n}-124-{name}-sostenida-12s.wav', x, RATE, subtype='PCM_24')
                long_rows[str(n)] = long_measure(x, n, fit, long_refs[str(n)]['support'])
                long_rows[str(n)]['raw_sha256'] = fit.AUDIT.sha(raw)
            stats = fit.aggregate(rows)
            if name == 'Base':
                baselines[family] = dict(rows=rows, attacks=attacks, stats=stats, long=long_rows)
            base = baselines[family]
            guards = None if name == 'Base' else fit.acceptance(stats, rows, base['stats'], base['rows'])
            temporal = attack.guards(attacks, base['attacks'])
            late = sum(sum(row['counts'][2:]) for row in long_rows.values())
            long_drop = min(float(min(np.array(row['stereo_levels_db'])-
                                      np.array(base['long'][key]['stereo_levels_db']))) for key, row in long_rows.items())
            profile = dict(tuning=tuning, diagnostic_decoupling=family=='Control-sin-carga',
                rows=rows, attacks=attacks, metrics=stats, acceptance=guards, temporal=temporal,
                audio_sha256=hashes, long=long_rows, late_notches=late, worst_long_level_delta_db=long_drop)
            report['profiles'][label] = profile
            (a.output/'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
            print(label, json.dumps(dict(total=stats['total'], worst=stats['worst_total'],
                  temporal=temporal['passed'], late_notches=late, long_drop=long_drop)), flush=True)
    print('DONE', a.output, flush=True)


if __name__ == '__main__':
    main()
