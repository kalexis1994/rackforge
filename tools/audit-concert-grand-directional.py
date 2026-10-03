"""P1a directional-loading pilot on a frozen native renderer.

One inherited horizontal coordinate, not six independent string states.
Positive modal masses and identical force/moment projections for loading and
board excitation. Three assumed lever heights; no measured bridge geometry.
No automatic factory selection, export gain, extra detune or note-wise EQ.
"""
import argparse
import csv
import importlib.util
import json
from pathlib import Path

import numpy as np
from scipy.linalg import expm
import soundfile as sf


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for key in ('binary', 'work', 'output', 'matrix-probes'):
        p.add_argument('--'+key, type=Path, required=True)
    p.add_argument('--expected-sha256', required=True)
    a = p.parse_args()
    tools = Path(__file__).resolve().parent
    body = module(tools/'identify-concert-grand-body.py', 'direction_body')
    fit, attack = body.FIT, body.ATTACK
    subtle = module(tools/'audit-concert-grand-subtle-unison.py', 'direction_subtle')
    audit = fit.AUDIT
    assert audit.sha(a.binary) == a.expected_sha256
    a.output.mkdir(parents=True, exist_ok=True)
    # Independent f64 oracle for the exact Strang product implemented in Rust,
    # not exp((-G+iH)dt); the latter includes the known splitting error.
    matrix_checks = []
    for row in csv.DictReader(a.matrix_probes.open()):
        r = {k:float(v) for k,v in row.items()}
        matrix = lambda prefix: np.array([[r[prefix+'00'], r[prefix+'01']],
                                          [r[prefix+'01'], r[prefix+'11']]])
        g, h = matrix('g'), matrix('h')
        half = expm(-g/(2*r['rate']))
        expected = half @ expm(1j*h/r['rate']) @ half
        native = np.eye(2)+matrix('re')+1j*matrix('im')
        error = float(abs(expected-native).max())
        assert error < 1e-6, error
        assert np.linalg.svd(native, compute_uv=False)[0] <= 1+1e-6
        matrix_checks.append(dict(rate=r['rate'], maximum_complex_error=error))
    profiles = {'Base': {}, 'Palanca-10mm': {'DIRECTIONAL_BRIDGE_HEIGHT_M': .01},
                'Palanca-25mm': {'DIRECTIONAL_BRIDGE_HEIGHT_M': .025},
                'Palanca-40mm': {'DIRECTIONAL_BRIDGE_HEIGHT_M': .04}}
    pairs = [(n,v) for n in (60,72,84,90) for v in (28,60,92,124)]
    reference = tools.parent/'local/concert-grand-identified-body'
    provenance = json.loads((reference/'reference-manifest.json').read_text())
    report = dict(policy=__doc__, binary_sha256=a.expected_sha256,
                  independent_matrix_checks=matrix_checks, export_gain_db=0,
                  factory_changed=False, selection=None, profiles={}, references={})
    refs, native_arrays, ref_features, ref_attacks = {}, {}, {}, {}
    baseline_exact_checks = []
    for n,v in pairs:
        key = f'{n}:{v}'
        path = reference/'reference'/f'{n}-{v}.wav'
        meta = json.loads(path.with_suffix('.json').read_text())
        assert meta['source'] == provenance['sources'][key]
        assert all(meta[k] == provenance[k] for k in ('mapping_sha256','sfz_sha256','dll_sha256'))
        assert audit.sha(Path(meta['source']['file'])) == meta['source']['sha256']
        refs[key], rate = sf.read(path, always_2d=True)
        assert rate == 44100
        ref_features[key] = fit.measure(refs[key], n)
        ref_attacks[key] = attack.measure(refs[key], n)
        report['references'][key] = dict(wav_sha256=audit.sha(path), source=meta['source'])
    for label,tuning in profiles.items():
        rows, temporal, source_hashes, long = {}, {}, {}, {}
        for n,v in pairs:
            key = f'{n}:{v}'
            raw = audit.render(a.binary, f'0 144 {n} {v}\n88200 128 {n} 64\n',
                               176400, a.work/label/f'{n}-{v}', tuning)
            x = np.array(audit.load(raw), dtype=float)
            assert np.isfinite(x).all() and abs(x).max() < 1
            if label == 'Base':
                old = tools.parent/'local/concert-grand-subtle-unison/renders/Carga-fisica/Base'/f'{n}-{v}/performance.f32'
                if old.exists():
                    assert np.array_equal(audit.load(old), x), 'Frozen native baseline changed'
                    baseline_exact_checks.append(key)
            native_arrays[(label,key)] = x
            rows[key] = fit.score(ref_features[key], fit.measure(x,n))
            temporal[key] = attack.compare(ref_attacks[key], attack.measure(x,n))
            source_hashes[key] = audit.sha(raw)
            sf.write(a.output/f'{n}-{v}-{label}.wav', x, 44100, subtype='PCM_24')
        for n in (60,72,84,90):
            raw = audit.render(a.binary, f'0 144 {n} 124\n529200 128 {n} 64\n',
                               617400, a.work/label/f'long-{n}', tuning)
            x = np.array(audit.load(raw), dtype=float)
            assert np.isfinite(x).all() and abs(x).max() < 1
            # The same reference-only support and genuine 12s reference hold
            # from the earlier unison audit; no short sample looping.
            old_report = json.loads((Path('C:/SHARED/Unisono-sutil-2026-10-02')/'report.json').read_text())
            support = old_report['long_reference'][str(n)]['support']
            long[str(n)] = subtle.long_measure(x, n, fit, support)
            long[str(n)]['raw_sha256'] = audit.sha(raw)
            sf.write(a.output/f'{n}-124-{label}-sostenida-12s.wav',x,44100,subtype='PCM_24')
        report['profiles'][label] = dict(tuning=tuning, rows=rows, attacks=temporal,
                                       raw_sha256=source_hashes, long=long)
        print(label, json.dumps(dict(mean_score=float(np.mean([r['total'] for r in rows.values()])),
              worst_score=max(r['total'] for r in rows.values()))), flush=True)
        (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    report['baseline_bit_exact_cases'] = baseline_exact_checks
    # Fixed baseline frequencies and reference-only harmonic support; unlike
    # each profile's notch detector, this does not follow a moving native peak.
    # Descriptive colour change, not a calibrated perceptual flanger detector.
    report['relative_colour'] = {}
    reference_long = json.loads((Path('C:/SHARED/Unisono-sutil-2026-10-02')/'report.json').read_text())
    for n in (60,72,84,90):
        key = f'{n}:124'
        f0 = 440*2**((n-69)/12)
        base = native_arrays[('Base',key)]
        hz, psd = fit.EVAL.psd(base[2205:22050])
        support = reference_long['long_reference'][str(n)]['support']
        curves = {label: [] for label in profiles}
        harmonics = []
        for h,allowed in enumerate(support,1):
            if not allowed: continue
            bins = np.flatnonzero(abs(hz-h*f0) < .18*f0)
            frequency = float(hz[bins[np.argmax(psd[bins])]])
            harmonics.append(h)
            for label in profiles:
                t, level = subtle.partial_envelope(native_arrays[(label,key)], frequency, f0, hold=2)
                curves[label].append(level)
        interval = (t >= .25)&(t <= 1.8)
        metrics = {}
        for label in profiles:
            delta = (np.array(curves[label])-np.array(curves['Base']))[:,interval]
            colour = delta-np.median(delta,axis=0,keepdims=True)
            colour -= np.mean(colour,axis=1,keepdims=True)
            ranges = np.percentile(colour,95,axis=1)-np.percentile(colour,5,axis=1)
            metrics[label] = dict(relative_colour_rms_db=float(np.sqrt(np.mean(colour**2))),
                                 median_partial_colour_range_db=float(np.median(ranges)),
                                 max_partial_colour_range_db=float(ranges.max()))
        report['relative_colour'][str(n)] = dict(window_s=[.25,1.8], harmonics=harmonics,
                                              fixed_frequency=True, metrics=metrics)
    timeline=[]
    for n,name in ((60,'C4'),(72,'C5'),(84,'C6'),(90,'Fsharp6')):
        parts=[]
        for v in (28,60,92,124):
            for label in ('Base','Palanca-25mm','Steinway'):
                key=f'{n}:{v}'
                x=(refs[key] if label=='Steinway' else native_arrays[(label,key)])[:110250].copy()
                x[-441:] *= np.linspace(1,0,441)[:,None]
                timeline.append(dict(note=n,velocity=v,label=label,start_s=3*len(parts)//2))
                parts.extend((x,np.zeros((22050,2))))
        sf.write(a.output/f'{name}-A-B-Steinway-intensidades.wav',np.concatenate(parts),44100,subtype='PCM_24')
    report['timeline']=timeline
    report['script_sha256']=audit.sha(Path(__file__))
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    (a.output/'LEEME.txt').write_text(
        'P1a: prototipo de acoplamiento direccional, aun con una horizontal agregada.\n'
        'Cada comparador recorre velocidades 28, 60, 92, 124. En cada velocidad: A base fisica, B palanca supuesta de 25 mm, Steinway. Cada entrada dura 3 s.\n'
        'No es la B anterior rechazada. Las alturas de palanca no son geometria medida del Steinway.\n'
        'Misma ganancia, sin EQ ni normalizacion. No instalado ni aprobado.\n',encoding='utf-8')


if __name__ == '__main__': main()
