"""Separate contact, board loss and proximity on the accepted duplex-free piano.

Original recorded pitches, multiple registers/velocities, fixed factory output
gain. Relative spectral measures diagnose colour; they are not fidelity scores.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import butter, sosfilt


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['binary', 'work', 'output']:
        p.add_argument('--'+name, type=Path, required=True)
    p.add_argument('--profiles', type=Path)
    p.add_argument('--notes', default='36,42,48,54,60,66,72,84')
    a = p.parse_args()
    a.work.mkdir(parents=True, exist_ok=True)
    a.output.mkdir(parents=True, exist_ok=True)
    tools = Path(__file__).resolve().parent
    audit = module(tools/'audit-concert-grand-duplex-fix.py', 'duplex_render')
    evaluator = module(tools/'evaluate-concert-grand-full-sound.py', 'lower_eval')
    sampler = module(tools/'render-bitklavier-steinway-midi.py', 'lower_sampler')
    evaluator.HOLD, evaluator.SECONDS = 2, 4
    assert audit.sha(a.binary) == '3b6ac0541fcd740b2cdab1586158b2e66790a6d367438f7ac91228970bb1d2b7'
    factory = json.loads((tools.parent/'plugins/concert-grand/calibration/factory-0.172.0.json').read_text())
    base = factory['tuning']
    # Existing smooth log-register stiffness law, compensated at and above C4.
    # This probe changes contact stiffness, not a post-render treble shelf.
    delta = np.log10(2)/.448
    profiles = {
        'A-actual': {},
        'B-contacto-graves': {'tuning': {
            'FELT_K_A0':base['FELT_K_A0']*2,
            'FELT_K_DECADES':base['FELT_K_DECADES']-delta,
            'FELT_TREBLE_DECADES':base['FELT_TREBLE_DECADES']-.552*delta}},
        'C-sin-perdida-contacto': {'tuning': {'FELT_RATE_DAMPING':0}},
        'D-caja-menos-resonante': {'params': {'31':7}},
        'E-sin-proximidad': {'tuning': {'PROXIMITY_STRENGTH':0}},
    }
    if a.profiles:
        profiles = json.loads(a.profiles.read_text())
    notes = list(map(int,a.notes.split(',')))
    velocities = [60,110]
    pairs = [(n,v) for n in notes for v in velocities]
    bands = [(25,180),(180,600),(600,1800),(1800,4000),(4000,8000)]
    windows = [(0,.02),(.05,.2),(.2,.5),(.5,1.5)]
    filters = [butter(4,b,fs=44100,btype='bandpass',output='sos') for b in bands]

    def measure(x, note):
        y, onset = evaluator.aligned(x)
        powers = []
        for filt in filters:
            z = sosfilt(filt,y,axis=0)
            powers.append([float(np.mean(z[round(lo*44100):round(hi*44100)]**2)) for lo,hi in windows])
        powers = np.array(powers)
        # Early high-frequency crest is monitored separately from tone colour.
        hf = sosfilt(filters[-1],y,axis=0)
        first = np.mean(hf[:round(.002*44100)]**2)
        later = np.mean(hf[round(.004*44100):round(.02*44100)]**2)
        f0 = 440*2**((note-69)/12)
        hz, spectrum = evaluator.psd(y[round(.05*44100):round(.5*44100)])
        centers = []
        for harmonic in range(1,7):
            if harmonic*f0 >= 15000:
                break
            bins = np.flatnonzero(abs(hz-harmonic*f0) < .18*f0)
            centers.append(float(hz[bins[np.argmax(spectrum[bins])]]))
        partials = []
        for lo,hi in windows:
            hz, power = evaluator.psd(y[round(lo*44100):round(hi*44100)])
            partials.append([float(10*np.log10(power[abs(hz-center)<.055*f0].sum()+1e-30)) for center in centers])
        return dict(bands_db=(10*np.log10(powers+1e-30)).tolist(),
                    brilliance_db=(10*np.log10((powers[2]+powers[3]+1e-30)/(powers[0]+powers[1]+1e-30))).tolist(),
                    partial_centers_hz=centers,partial_bands_db=partials,
                    onset_seconds=onset/44100,peak=float(abs(x).max()),
                    hf_crest_first_2ms_db=float(10*np.log10((first+1e-30)/(later+1e-30))))

    signals = {}; metrics = {}
    def render(job):
        label,n,v = job
        profile = profiles[label]
        folder = a.work/label/f'{n}-{v}'
        events = f'0 144 {n} {v}\n88200 128 {n} 64\n'
        raw = audit.render(a.binary,events,4*44100,folder,profile.get('tuning',{}),params=profile.get('params'))
        x = np.asarray(audit.load(raw),dtype=float)
        assert np.isfinite(x).all() and 0 < abs(x).max() < 1
        return (label,n,v), x

    with ThreadPoolExecutor(max_workers=2) as pool:
        for (label,n,v),x in pool.map(render,[(label,n,v) for label in profiles for n,v in pairs]):
            signals[label,n,v] = x
            metrics.setdefault(label,{})[f'{n}:{v}'] = measure(x,n)
    print('Native profiles complete',flush=True)
    mapping_path = tools.parent/'local/steinway-midi/render-stage/mapping.json'
    mapping = json.loads(mapping_path.read_text())
    source_records = {}
    for n,v in pairs:
        assert n in sampler.ROOTS
        layer = v//8+1
        entry = next(row for row in mapping['layers'] if row['root']==n and row['layer']==layer)
        path = Path(entry['file'])
        digest = audit.sha(path)
        assert digest == mapping['sources_sha256'][str(path.resolve())]
        source_records[f'{n}:{v}'] = dict(file=str(path),sha256=digest,root=n,layer=layer,transposition=0)
    sfz = tools.parent/'local/steinway-midi/render-stage/SteinwayD-StageORTF.sfz'
    dll = tools.parent/'local/steinway-midi/sfizz/library/bin/Release/sfizz.dll'
    synth = sampler.Engine(dll,sfz)
    try:
        for n,v in pairs:
            x = evaluator.render_reference(synth,n,v)
            signals['Steinway',n,v] = x
            metrics.setdefault('Steinway',{})[f'{n}:{v}'] = measure(x,n)
    finally:
        synth.close()

    summary = {}
    for label in profiles:
        lower = [(n,v) for n,v in pairs if n < 60]
        summary[label] = dict(
            median_brilliance_vs_Steinway_db=np.median([
                np.array(metrics[label][f'{n}:{v}']['brilliance_db'])-metrics['Steinway'][f'{n}:{v}']['brilliance_db'] for n,v in lower],axis=0).tolist(),
            median_bands_vs_Steinway_db=np.median([
                np.array(metrics[label][f'{n}:{v}']['bands_db'])-metrics['Steinway'][f'{n}:{v}']['bands_db'] for n,v in lower],axis=0).tolist())
        print(label,json.dumps(summary[label]['median_brilliance_vs_Steinway_db']),flush=True)
    trim = min(1,.85/max(float(abs(x).max()) for x in signals.values()))
    names = {36:'C2',42:'Fs2',48:'C3',54:'Fs3',60:'C4',66:'Fs4',72:'C5',78:'Fs5',84:'C6',90:'Fs6',96:'C7'}
    for n in notes:
        for label in [*profiles,'Steinway']:
            parts=[]
            for v in velocities:
                x = signals[label,n,v][:round(2.5*44100)].copy()*trim
                x[-441:] *= np.linspace(1,0,441)[:,None]
                parts.extend([x,np.zeros((44100//2,2))])
            sf.write(a.output/f'{names[n]}-{label}.wav',np.concatenate(parts),44100,subtype='PCM_24')
    report = dict(binary_sha256=audit.sha(a.binary),profiles=profiles,notes=notes,velocities=velocities,
                  bands_hz=bands,windows_seconds=windows,metrics=metrics,summary=summary,
                  mapping_sha256=audit.sha(mapping_path),sfz_sha256=audit.sha(sfz),dll_sha256=audit.sha(dll),
                  reference_sources=source_records,model_export_gain_db=0,common_export_trim_db=float(20*np.log10(trim)),
                  policy=__doc__+' All profiles keep DUPLEX_LEVEL=0. Native 256 frames, 200ms warmup. Reference is the unchanged SFZ sampler at exact roots. No per-note normalization.')
    (a.output/'audit.json').write_text(json.dumps(report,indent=2,ensure_ascii=False),encoding='utf-8')
    print('Report:',a.output/'audit.json',flush=True)


if __name__ == '__main__':
    main()
