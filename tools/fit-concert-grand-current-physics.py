"""Fit the audible, frozen physical piano with shared parameters and holdouts.

The native engine renders every candidate. No note equalizer or diagnostic
decoupling is fitted. Reference roots are played without transposition. Internal
holdouts reuse this recording corpus; they are not independent acoustic truth.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import butter, sosfilt
from scipy.stats import qmc

RATE = 44100
WINDOWS = ((0, .02), (.05, .2), (.2, .5), (.5, 1.5))
BANDS = ((25, 180), (180, 600), (600, 1800), (1800, 4000), (4000, 8000))
FROZEN_SHA = '3b6ac0541fcd740b2cdab1586158b2e66790a6d367438f7ac91228970bb1d2b7'


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


TOOLS = Path(__file__).resolve().parent
AUDIT = module(TOOLS/'audit-concert-grand-duplex-fix.py', 'current_render')
EVAL = module(TOOLS/'evaluate-concert-grand-full-sound.py', 'current_eval')
SAMPLER = module(TOOLS/'render-bitklavier-steinway-midi.py', 'current_sampler')
FILTERS = [butter(4, b, fs=RATE, btype='bandpass', output='sos') for b in BANDS]
EVAL.HOLD, EVAL.SECONDS = 2, 4


def measure(x, note):
    assert np.isfinite(x).all() and np.max(np.abs(x)) > 0
    y, at = EVAL.aligned(x)
    bands = []
    for filt in FILTERS:
        z = sosfilt(filt, y, axis=0)
        bands.append([float(10*np.log10(np.mean(z[round(lo*RATE):round(hi*RATE)]**2)+1e-30))
                      for lo, hi in WINDOWS])
    f0 = 440*2**((note-69)/12)
    hz, spectrum = EVAL.psd(y[round(.05*RATE):round(.5*RATE)])
    centers = []
    for h in range(1, 7):
        if h*f0 >= 15000:
            break
        bins = np.flatnonzero(abs(hz-h*f0) < .18*f0)
        centers.append(float(hz[bins[np.argmax(spectrum[bins])]]))
    partials, totals = [], []
    for lo, hi in WINDOWS:
        clip = y[round(lo*RATE):round(hi*RATE)]
        hz, p = EVAL.psd(clip)
        # The diagnostic radius remains identical for all model candidates.
        partials.append([float(10*np.log10(p[abs(hz-f)<.055*f0].sum()+1e-30)) for f in centers])
        totals.append(float(10*np.log10(np.mean(clip**2)+1e-30)))
    hf = sosfilt(FILTERS[-1], y, axis=0)
    crest = 10*np.log10((np.mean(hf[:round(.002*RATE)]**2)+1e-30)/
                       (np.mean(hf[round(.004*RATE):round(.02*RATE)]**2)+1e-30))
    # Impact body needs its own window: the previous score left a gap between
    # the first 20ms and the sustained 50..200ms partial measurements. Resolve
    # at least eight fundamental cycles in the bass rather than interpreting
    # a two-cycle spectrum as independent harmonic evidence.
    impact_lo = .02
    impact_hi = impact_lo+max(.08, 8/f0)
    hz, p = EVAL.psd(y[round(impact_lo*RATE):round(impact_hi*RATE)])
    impact = dict(window_s=[impact_lo, impact_hi],
                  partials_db=[float(10*np.log10(p[abs(hz-c)<.055*f0].sum()+1e-30)) for c in centers[:3]])
    return dict(partials_db=partials, bands_db=bands, total_db=totals, impact=impact,
                centers_hz=centers, onset_seconds=at/RATE,
                peak=float(np.max(np.abs(x))), hf_crest_db=float(crest))


def score(target, prediction, version=2):
    t, p = np.asarray(target['partials_db']), np.asarray(prediction['partials_db'])
    # Fixed reference-only reliability mask. Ignore weak partials and their
    # noise-reduced tails, never a partial missing only from the model.
    mask = (t[1:] > t[1].max()-50) & (t[1] > t[1].max()-30)[None, :]
    assert mask[:, 0].all(), 'Reference fundamental lacks measurement support'
    h1 = abs(p[1:, 0]-t[1:, 0])
    decay = abs((p[2:]-p[1])-(t[2:]-t[1]))[mask[1:]]
    balance = abs((p[1:, 1:]-p[1:, :1])-(t[1:, 1:]-t[1:, :1]))[mask[:, 1:]]
    a, b = np.asarray(target['bands_db']), np.asarray(prediction['bands_db'])
    bandmask = a[:, :2] > a[:, :2].max(axis=0)-45
    band = abs(b[:, :2]-a[:, :2])[bandmask]
    values = dict(h1_db=float(h1.mean()), h1_late_db=float(h1[-1]),
                  decay_db=float(decay.mean()) if decay.size else 0.,
                  balance_db=float(balance.mean()) if balance.size else 0.,
                  attack_body_bands_db=float(band.mean()),
                  h1_signed_db=(p[1:, 0]-t[1:, 0]).tolist(),
                  observed_partial_terms=int(mask.sum()))
    legacy = (.35*values['h1_db']+.25*values['decay_db']+
              .25*values['balance_db']+.15*values['attack_body_bands_db'])
    if 'impact' in target and 'impact' in prediction:
        ti, pi = [np.asarray(f['impact']['partials_db']) for f in (target,prediction)]
        keep = ti[1:] > ti.max()-30
        ratios = abs((pi[1:]-pi[0])-(ti[1:]-ti[0]))[keep]
        impact_error = .25*abs(pi[0]-ti[0])+.75*(float(ratios.mean()) if ratios.size else 0.)
        values.update(impact_body_db=float(impact_error), observed_impact_balance_terms=int(keep.sum()),
                      impact_H2_H1_error_db=float((pi[1]-pi[0])-(ti[1]-ti[0])),legacy_total=legacy)
    elif version == 2:
        raise ValueError('Version 2 requires freshly measured impact-body features')
    values['total'] = .75*legacy+.25*values['impact_body_db'] if version == 2 else legacy
    return values


def aggregate(rows, baseline=None):
    keys = ['h1_db', 'h1_late_db', 'decay_db', 'balance_db', 'attack_body_bands_db', 'total']
    if all('impact_body_db' in r for r in rows.values()):
        keys.append('impact_body_db')
    out = {k: float(np.mean([r[k] for r in rows.values()])) for k in keys}
    out.update({f'worst_{k}': float(max(r[k] for r in rows.values())) for k in keys})
    registers = {}
    for label, lo, hi in [('grave', 0, 59), ('medio', 60, 77), ('agudo', 78, 127)]:
        subset = [r['total'] for key, r in rows.items() if lo <= int(key.split(':')[0]) <= hi]
        if subset:
            registers[label] = float(np.mean(subset))
    out['register_total'] = registers
    totals = [r['total'] for r in rows.values()]
    objective = out['total']+.6*np.percentile(totals, 95)+.4*max(totals)+.4*max(registers.values())
    if baseline is not None:
        regressions = [max(0., r['total']-baseline[key]['total']-1.) for key, r in rows.items()]
        objective += 2*np.mean(regressions)+max(regressions)
    out['objective'] = float(objective)
    return out


def acceptance(stats, rows, baseline_stats, baseline_rows):
    """Engineering guards, not a perceptual pass or acoustic identification."""
    regression = max(rows[key]['total']-baseline_rows[key]['total'] for key in rows)
    checks = {
        'mean_improves': stats['total'] < baseline_stats['total'],
        'worst_pair_improves': stats['worst_total'] < baseline_stats['worst_total'],
        'every_register_improves': all(stats['register_total'][r] < value
                                     for r, value in baseline_stats['register_total'].items()),
        'worst_late_H1_improves': stats['worst_h1_late_db'] < baseline_stats['worst_h1_late_db'],
        'worst_decay_within_0_5db': stats['worst_decay_db'] <= baseline_stats['worst_decay_db']+.5,
        'worst_balance_within_0_5db': stats['worst_balance_db'] <= baseline_stats['worst_balance_db']+.5,
        'worst_attack_body_within_0_5db': stats['worst_attack_body_bands_db'] <= baseline_stats['worst_attack_body_bands_db']+.5,
        'no_pair_regresses_over_2db': regression <= 2.,
    }
    if 'worst_impact_body_db' in stats:
        checks['worst_impact_body_within_0_5db'] = stats['worst_impact_body_db'] <= baseline_stats['worst_impact_body_db']+.5
    return dict(passed=all(checks.values()), checks=checks, maximum_pair_regression_db=regression,
                policy='These guards screen measured tradeoffs. Listening remains required. Internal holdouts share the recording corpus.')


def profiles(count, center=None):
    # Density moves the complete statistical bank; mobility remains collocated
    # in radiation and string loading. Every loss remains positive.
    points = qmc.Sobol(5, scramble=True, seed=1973).random_base2(int(np.log2(count)))
    result = {}
    for i, (density, loss, mobility, extra, pressure) in enumerate(points):
        if center is None:
            d = .82+.43*density
            l = 3.+6.*loss
            m = 10**(np.log10(.00012)+mobility*np.log10(.0007/.00012))
            e = .5+3.*extra
            db = -3.+6.*pressure
        else:
            d = center['params']['32']+(.07*(2*density-1))
            l = np.clip(center['params']['31']+2.*(2*loss-1), 2.5, 12.)
            m = center['tuning']['PHYSICAL_BRIDGE_MOBILITY']*2**(2*mobility-1)
            e = max(.1, center['tuning']['UNISON_EXTRA_LOSS']+1.*(2*extra-1))
            old_scale = center['tuning']['PHYSICAL_PRESSURE_SCALE']
            old_m = center['tuning']['PHYSICAL_BRIDGE_MOBILITY']
            db = 20*np.log10(old_scale*old_m/.0015)+2*(2*pressure-1)
        result[f'joint-{i:03}'] = dict(params={'31':float(l), '32':float(d)}, tuning={
            'PHYSICAL_BRIDGE_MOBILITY':float(m), 'PHYSICAL_PRESSURE_SCALE':float(np.clip(.0015/m*10**(db/20), .125, 16.)),
            'UNISON_EXTRA_LOSS':float(e)})
    return result


def contact_profiles(count, centers):
    factory = json.loads((TOOLS.parent/'plugins/concert-grand/calibration/factory-0.172.0.json').read_text())['tuning']
    points = qmc.Sobol(6, scramble=True, seed=1974).random_base2(int(np.log2(count)))
    result = {}
    for name, center in centers.items():
        result[name] = center
        for i, (global_k, bass_k, treble_k, damping, memory, pressure) in enumerate(points):
            bass = 1.+1.2*bass_k
            delta = np.log10(bass)/.448
            tuning = {**center['tuning'],
                      'FELT_K_A0':float(factory['FELT_K_A0']*(.85+.45*global_k)*bass),
                      'FELT_K_DECADES':float(factory['FELT_K_DECADES']-delta),
                      'FELT_TREBLE_DECADES':float(factory['FELT_TREBLE_DECADES']-.552*delta-.3+.65*treble_k),
                      'FELT_RATE_DAMPING':float(.02+.2*damping),
                      'STULOV_TAU_S':float(10**(np.log10(.00005)+memory*np.log10(4.))),
                      'PHYSICAL_PRESSURE_SCALE':float(center['tuning']['PHYSICAL_PRESSURE_SCALE']*10**((-1.+2*pressure)/20))}
            result[f'{name}-contact-{i:03}'] = dict(tuning=tuning, params=center['params'])
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for key in ('binary', 'work', 'output'):
        p.add_argument('--'+key, type=Path, required=True)
    p.add_argument('--profiles', type=Path)
    p.add_argument('--center', type=Path)
    p.add_argument('--contact-centers', type=Path)
    p.add_argument('--expected-sha256', default=FROZEN_SHA,
                   help='Explicit immutable renderer identity for a new model experiment')
    p.add_argument('--score-version', type=int, choices=(1,2), default=2,
                   help='Version 2 adds measured impact body; 1 preserves historical total weights')
    p.add_argument('--count', type=int, default=32, choices=(16, 32, 64, 128))
    p.add_argument('--notes', default='36,42,48,54,60,66,72,78,84,90,96')
    p.add_argument('--velocities', default='60,110')
    p.add_argument('--export', action='store_true')
    p.add_argument('--measure-seconds', type=float, choices=(1.7,2.,4.), default=4.,
                   help='Sustained-only short capture for fitting; exported auditions always use 4s')
    a = p.parse_args()
    a.work.mkdir(parents=True, exist_ok=True); a.output.mkdir(parents=True, exist_ok=True)
    render_seconds = 4. if a.export else a.measure_seconds
    binary_sha = AUDIT.sha(a.binary)
    assert binary_sha == a.expected_sha256, 'Renderer differs from the explicitly frozen identity'
    notes, velocities = [list(map(int, s.split(','))) for s in (a.notes, a.velocities)]
    pairs = [(n, v) for n in notes for v in velocities]
    base = {'tuning':{}, 'params':{}}
    variants = {'actual':base}
    if a.profiles:
        variants.update(json.loads(a.profiles.read_text()))
    elif a.contact_centers:
        variants.update(contact_profiles(a.count, json.loads(a.contact_centers.read_text())))
    else:
        center = json.loads(a.center.read_text()) if a.center else None
        if center is not None:
            variants['center'] = center
        variants.update(profiles(a.count, center))
    mapping_path = TOOLS.parent/'local/steinway-midi/render-stage/mapping.json'
    mapping = json.loads(mapping_path.read_text())
    sfz = mapping_path.parent/'SteinwayD-StageORTF.sfz'
    dll = TOOLS.parent/'local/steinway-midi/sfizz/library/bin/Release/sfizz.dll'
    sources = {}
    for n, v in pairs:
        assert n in SAMPLER.ROOTS
        entry = next(row for row in mapping['layers'] if row['root']==n and row['layer']==v//8+1)
        path = Path(entry['file'])
        digest = AUDIT.sha(path)
        assert digest == mapping['sources_sha256'][str(path.resolve())]
        assert entry['frames']/RATE > 1.6, 'Reference tail too short for these windows'
        sources[f'{n}:{v}'] = dict(file=str(path), sha256=digest, root=n, layer=v//8+1,
                                 transposition=0, source_frames=entry['frames'])
    ref_cache = a.work/'reference.json'
    signature = dict(sources=sources, mapping_sha256=AUDIT.sha(mapping_path),
                     sfz_sha256=AUDIT.sha(sfz), dll_sha256=AUDIT.sha(dll), windows=WINDOWS,feature_version=2)
    targets = {}
    if ref_cache.exists():
        cache = json.loads(ref_cache.read_text())
        if cache['signature'] == json.loads(json.dumps(signature)):
            targets = cache['targets']
    if not targets or a.export:
        synth = SAMPLER.Engine(dll, sfz)
        try:
            for n, v in pairs:
                x = EVAL.render_reference(synth, n, v)
                targets[f'{n}:{v}'] = measure(x, n)
                if a.export:
                    assert abs(x).max() < 1, 'Reference export would clip'
                    sf.write(a.output/f'{n}-{v}-Steinway.wav', x, RATE, subtype='PCM_24')
        finally:
            synth.close()
        ref_cache.write_text(json.dumps(dict(signature=signature, targets=targets), indent=2), encoding='utf-8')
    report = dict(policy=__doc__, binary_sha256=binary_sha, reference=signature,
                  native_render_seconds=render_seconds,
                  score_version=a.score_version,
                  pairs=pairs, targets=targets, windows_seconds=WINDOWS, bands_hz=BANDS,
                  export_gain_db=0, profiles={})
    def run(job):
        label, profile, n, v = job
        folder = a.work/label/f'{n}-{v}'
        events = f'0 144 {n} {v}\n'
        if render_seconds > 2.: events += f'88200 128 {n} 64\n'
        raw = AUDIT.render(a.binary, events, round(render_seconds*RATE),
                           folder, profile['tuning'], params=profile['params'])
        x = np.array(AUDIT.load(raw), dtype=np.float64)
        assert np.isfinite(x).all() and abs(x).max() < 1, 'Clipping or invalid candidate'
        f = measure(x, n)
        if a.export:
            sf.write(a.output/f'{n}-{v}-{label}.wav', x, RATE, subtype='PCM_24')
        return f'{n}:{v}', f
    with ThreadPoolExecutor(max_workers=2) as pool:
        baseline = None
        for label, profile in variants.items():
            cache_path = a.work/label/'features.json'
            feature_signature = dict(profile=profile, binary_sha256=binary_sha, pairs=pairs,
                                     native_render_seconds=render_seconds,
                                     feature_version=2)
            features = None
            if cache_path.exists() and not a.export:
                cache = json.loads(cache_path.read_text())
                if cache['signature'] == json.loads(json.dumps(feature_signature)):
                    features = cache['features']
            if features is None:
                features = dict(pool.map(run, [(label, profile, n, v) for n, v in pairs]))
                cache_path.write_text(json.dumps(dict(signature=feature_signature, features=features), indent=2), encoding='utf-8')
            rows = {key:score(targets[key], f,a.score_version) for key, f in features.items()}
            if baseline is None:
                baseline = rows
            stats = aggregate(rows, baseline)
            gates = acceptance(stats, rows, aggregate(baseline), baseline) if label != 'actual' else None
            report['profiles'][label] = dict(profile=profile, metrics=stats, rows=rows, features=features, acceptance=gates)
            (a.output/'progress.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
            print(label, json.dumps({k:round(stats[k], 3) for k in ('objective','total','worst_total','h1_late_db','worst_h1_late_db')}), flush=True)
    ranked = sorted(report['profiles'], key=lambda k:report['profiles'][k]['metrics']['objective'])
    report['ranking'] = ranked
    report['eligible'] = [key for key in ranked if key != 'actual' and report['profiles'][key]['acceptance']['passed']]
    (a.output/'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    selected = report['eligible'][0] if report['eligible'] else 'actual'
    (a.output/'best-profile.json').write_text(json.dumps(report['profiles'][selected]['profile'], indent=2), encoding='utf-8')
    if a.export:
        schedule = []
        for label in [*variants, 'Steinway']:
            parts = []
            for n, v in pairs:
                x, rate = sf.read(a.output/f'{n}-{v}-{label}.wav', always_2d=True)
                assert rate == RATE
                clip = x[:round(2.5*RATE)].copy()
                clip[-441:] *= np.linspace(1, 0, 441)[:, None]
                parts.extend([clip, np.zeros((RATE//2, 2))])
                if label == 'actual':
                    schedule.append(dict(start_seconds=3*(len(schedule)), note=n, velocity=v,
                                         note_off_seconds=2, root=n, transposition=0))
            sf.write(a.output/f'{label}-intensidades.wav', np.concatenate(parts), RATE, subtype='PCM_24')
        (a.output/'audition.json').write_text(json.dumps(dict(sequence=schedule, export_gain_db=0,
              policy='Unity gain for every render. No note normalization. 2s held, 0.5s release, 0.5s silence.'), indent=2), encoding='utf-8')
    print('RANK', ranked[:8], flush=True)


if __name__ == '__main__':
    main()
