"""Localize perceived impact body with harmonics and subfundamental energy.

Both-channel power avoids stereo cancellation. Window spectra do not identify
mechanical sources by themselves. The unchanged exported WAVs are measured;
filters, level matching and inferred material coefficients are not applied.
"""
import argparse
import importlib.util
import json
from pathlib import Path

import numpy as np
import soundfile as sf

WINDOWS = ((0, .02), (.02, .05), (.05, .1), (.1, .2), (.2, .5), (.5, 1.5))
BANDS = ((25, 180), (180, 600), (600, 900), (900, 1400), (1400, 4000), (4000, 8000))
LABELS = ('actual', 'movilidad-regional', 'Steinway')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--notes', default='72,84,90')
    p.add_argument('--velocities', default='28,60,92,124')
    p.add_argument('--labels', default=','.join(LABELS))
    p.add_argument('--stage-dir', type=Path)
    a = p.parse_args(); a.output.mkdir(parents=True, exist_ok=True)
    spec = importlib.util.spec_from_file_location('impact_eval', Path(__file__).with_name('evaluate-concert-grand-full-sound.py'))
    ev = importlib.util.module_from_spec(spec); spec.loader.exec_module(ev)
    audit_spec = importlib.util.spec_from_file_location('impact_sha', Path(__file__).with_name('audit-concert-grand-duplex-fix.py'))
    audit = importlib.util.module_from_spec(audit_spec); audit_spec.loader.exec_module(audit)
    metrics = {}
    labels = a.labels.split(',')
    for note in map(int, a.notes.split(',')):
        f0 = 440*2**((note-69)/12)
        for velocity in map(int, a.velocities.split(',')):
            key = f'{note}:{velocity}'; metrics[key] = {}
            for label in labels:
                path = a.input/f'{note}-{velocity}-{label}.wav'
                x, rate = sf.read(path, always_2d=True)
                assert rate == 44100 and x.shape[1] == 2 and len(x) >= 2*rate
                assert np.isfinite(x).all() and 0 < abs(x).max() < 1
                y, at = ev.aligned(x)
                hz, power = ev.psd(y[round(.05*rate):round(.5*rate)])
                centers = []
                for h in range(1,13):
                    if h*f0 >= 15000:
                        break
                    bins = np.flatnonzero(abs(hz-h*f0) < .18*f0)
                    centers.append(float(hz[bins[np.argmax(power[bins])]]))
                rows = []
                for lo, hi in WINDOWS:
                    hz, power = ev.psd(y[round(lo*rate):round(hi*rate)])
                    partials = np.array([power[abs(hz-c)<.055*f0].sum() for c in centers])
                    sub = power[(hz >= 25) & (hz < .7*f0)].sum()
                    tonal = partials[:3].sum()
                    upper = partials[3:].sum()
                    total = power[(hz >= 25) & (hz < 15000)].sum()
                    db = lambda q: float(10*np.log10(q+1e-30))
                    rows.append(dict(window_s=[lo, hi], total_db=db(total),
                        bands_db=[db(power[(hz >= l)&(hz < h)].sum()) for l,h in BANDS],
                        partials_db=[db(q) for q in partials],
                        H1_H3_db=db(tonal), H4_H12_db=db(upper),
                        lower_upper_partial_ratio_db=db(tonal)-db(upper),
                        subfundamental_db=db(sub),
                        subfundamental_relative_total_db=db(sub)-db(total)))
                metrics[key][label] = dict(file=str(path.resolve()), sha256=audit.sha(path),
                    onset_seconds=at/rate, onset_threshold_seconds={str(t):ev.onset(x,t)/rate for t in (-50,-40,-30)},
                    centers_hz=centers, peak=float(abs(x).max()), windows=rows)
    report = dict(policy=__doc__, windows_s=WINDOWS, bands_hz=BANDS,
                  source_sequence_note=dict(start_seconds=57,note=84,velocity=124),
                  metrics=metrics, export_gain_db=0)
    if a.stage_dir:
        path = a.stage_dir/'stages084v124.csv'
        data = np.genfromtxt(path, delimiter=',', names=True)
        centers = metrics['84:124']['actual']['centers_hz'][:3]
        f0 = 440*2**((84-69)/12)
        stages = {}
        for name in ['bridge_sum','board_drive_sum','board_direct','board_right_direct','preoutput_left']:
            x = np.repeat(data[name][:,None],2,axis=1)
            values = []
            for lo,hi in WINDOWS[:4]:
                hz, power = ev.psd(x[round(lo*44100):round(hi*44100)])
                partials = [float(10*np.log10(power[abs(hz-c)<.055*f0].sum()+1e-30)) for c in centers]
                values.append(dict(window_s=[lo,hi],H2_H1_db=partials[1]-partials[0],H3_H1_db=partials[2]-partials[0]))
            stages[name] = values
        report['stage_probe'] = dict(file=str(path.resolve()),sha256=audit.sha(path),
            note=84,velocity=124,profile='actual',windows=stages,
            policy='Stage signals have different units; only partial ratios within each signal are compared. A transfer difference also contains propagation, modal response and loading effects.')
    (a.output/'mediciones.json').write_text(json.dumps(report,indent=2,ensure_ascii=False),encoding='utf-8')
    # The selected onset occupies 57..60 in all three 72-second sequences.
    # Extract that actual interval rather than silently re-rendering the strike.
    extracts = {}
    for label in labels:
        filename = {'actual':'Actual','movilidad-regional':'Experimento-movilidad','Steinway':'Steinway'}.get(label)
        composite = a.input.parent/(filename+'-intensidades.wav') if filename else None
        if composite is not None and composite.exists():
            path = composite
            x,rate=sf.read(path,start=57*44100,frames=3*44100,always_2d=True)
            start = 57
        else:
            path = a.input/f'84-124-{label}.wav'
            x,rate=sf.read(path,frames=3*44100,always_2d=True)
            start = 0
        assert rate == 44100 and len(x)==3*44100
        sf.write(a.output/f'C6-v124-{label}.wav',x,rate,subtype='PCM_24')
        extracts[label] = dict(file=str(path.resolve()),sha256=audit.sha(path),start_seconds=start)
    report['extracts'] = extracts
    (a.output/'mediciones.json').write_text(json.dumps(report,indent=2,ensure_ascii=False),encoding='utf-8')
    row = metrics['84:124']
    for i,(lo,hi) in enumerate(WINDOWS):
        print(f'{lo*1000:.0f}..{hi*1000:.0f} ms', flush=True)
        for label in labels:
            r=row[label]['windows'][i]
            print(label, json.dumps({k:round(r[k],2) for k in ['H1_H3_db','H4_H12_db','lower_upper_partial_ratio_db','subfundamental_relative_total_db']}),flush=True)


if __name__=='__main__':
    main()
