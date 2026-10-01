"""Compare stored piano revisions without fitting or changing their audio.

Separate near-partial energy from sub-fundamental attack energy. Ratios are
independent of export gain; they are diagnostics, not perceptual scores.
Missing historical pairs are recorded rather than reconstructed silently.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import numpy as np
import soundfile as sf

WINDOWS = ((.05, .2), (.2, .5), (.5, 1.5), (1.5, 2.4))


def power_spectrum(x, rate):
    w = np.hanning(len(x))
    size = max(262144, 1 << (len(x)-1).bit_length())
    p = abs(np.fft.rfft(x*w, size))**2 * 2/(size*np.sum(w*w))
    p[0] *= .5
    p[-1] *= .5
    return np.fft.rfftfreq(size, 1/rate), p


def features(path, note, analysis):
    x, rate = sf.read(path, frames=round(6*44100))
    if rate != 44100:
        raise ValueError(f'Unexpected sample rate: {path}: {rate}')
    if x.ndim == 2:
        x = x[:, 0]
    onset = analysis.onset_index(x, rate)
    y = x[onset:]
    if len(y) < round(2.4*rate):
        raise ValueError(f'Insufficient duration: {path}')
    hz, p = power_spectrum(y[round(.05*rate):round(.4*rate)], rate)
    nominal = 440*2**((note-69)/12)
    centers = []
    for h in range(1, 17):
        if (h-.25)*nominal >= 10000:
            break
        near = np.flatnonzero(abs(hz-h*nominal)<.25*nominal)
        f = float(hz[near[np.argmax(p[near])]])
        if f <= 10000:
            centers.append(f)
    radius = max(18, .025*nominal)
    rows = []
    for lo, hi in WINDOWS:
        clip = y[round(lo*rate):round(hi*rate)]
        hz, p = power_spectrum(clip, rate)
        total = float(p.sum())
        harmonics = [float(p[abs(hz-f)<radius].sum()) for f in centers]
        rows.append(dict(window_s=[lo, hi],
            total_power=total, partial_powers=harmonics,
            partials_relative_H1_db=[float(10*np.log10(max(v,1e-24)/max(harmonics[0],1e-24))) for v in harmonics],
            H2_H6_relative_H1_db=float(10*np.log10(max(sum(harmonics[1:6]),1e-24)/max(harmonics[0],1e-24))),
            subfundamental_relative_total_db=float(10*np.log10(max(float(p[hz<.7*nominal].sum()),1e-24)/max(total,1e-24))),
            retained_near_partials_relative_total_db=float(10*np.log10(max(sum(harmonics),1e-24)/max(total,1e-24)))))
    early = rows[0]['total_power']
    for row in rows:
        row['total_relative_initial_db'] = float(10*np.log10(max(row['total_power'],1e-24)/max(early,1e-24)))
        row['H1_relative_initial_db'] = float(10*np.log10(max(row['partial_powers'][0],1e-24)/max(rows[0]['partial_powers'][0],1e-24)))
    return dict(path=str(path.resolve()), sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
        channel='left', onset_s=onset/rate, centers_hz=centers, radius_hz=radius, windows=rows)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--research-tools', type=Path, required=True)
    p.add_argument('--reference-root', type=Path, required=True)
    p.add_argument('--profiles', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    spec = importlib.util.spec_from_file_location('history_refit',a.research_tools/'concert-grand-refit-v2.py')
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    config = json.loads(a.profiles.read_text(encoding='utf-8'))
    profiles = config['profiles']
    pairs = [(n,v) for n in config['notes'] for v in config['velocities']]
    result = dict(policy=__doc__, config=config, results={}, missing={})
    for dataset in ('bitklavier-stage', 'bitklavier-harp'):
        source = m.refit.samples(a.reference_root, dataset)
        profiles[dataset] = {'files':{f'{n}:{v}':str(source[n,v]) for n,v in pairs}}
    for name, entry in profiles.items():
        result['results'][name] = {}
        result['missing'][name] = []
        for n,v in pairs:
            path = Path(entry['files'][f'{n}:{v}']) if 'files' in entry else Path(entry['directory'])/f'model{n:03}v{v}.wav'
            if not path.is_file():
                result['missing'][name].append([n,v])
                continue
            row = features(path,n,m.analysis)
            result['results'][name][f'{n}:{v}'] = row
            if n >= 72 and v == 124:
                print(name,n, 'H2-6/H1', [round(w['H2_H6_relative_H1_db'],1) for w in row['windows']],
                    'H1 decay', [round(w['H1_relative_initial_db'],1) for w in row['windows']],flush=True)
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(result,indent=2),encoding='utf-8')


if __name__ == '__main__':
    main()
