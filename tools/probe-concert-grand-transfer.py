"""Causal stage probes, preserving the frozen 14B parameters and common gain.

Diagnostic ablations are not proposed physical fits. Stage signals have different
units; compare spectral shapes, never raw magnitudes across those units.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess

import numpy as np
import soundfile as sf


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary', type=Path, required=True)
    p.add_argument('--model', type=Path, required=True)
    p.add_argument('--work', type=Path, required=True)
    p.add_argument('--reference', type=Path, required=True)
    p.add_argument('--profiles', type=Path)
    a = p.parse_args()
    a.work.mkdir(parents=True, exist_ok=True)
    model = json.loads(a.model.read_text())
    binary_sha = hashlib.sha256(a.binary.read_bytes()).hexdigest()
    assert binary_sha == model['binary_sha256']
    spec = importlib.util.spec_from_file_location('full_evaluation', Path(__file__).with_name('evaluate-concert-grand-full-sound.py'))
    e = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(e)
    original = json.loads(a.reference.read_text())
    pairs = [(n, v) for n in (48,60,66,72,78,84,90,96,102) for v in (60,124)]
    profiles = json.loads(a.profiles.read_text()) if a.profiles else {
        '14B': {}, 'contact-all': {'SIM_MIN_MODES':1,'SIM_TOP_HZ':19000},
        'contact-gate-only': {'SIM_MIN_MODES':1},
        'no-drain': {'DIAGNOSTIC_STRING_DRAIN':0},
    }

    def run(item):
        name, overrides = item
        folder = a.work/name
        folder.mkdir(exist_ok=True)
        (folder/'grid').mkdir(exist_ok=True)
        tuning = {**model['tuning'], **overrides}
        frozen = {**model, 'tuning':tuning}
        (folder/'tuning.txt').write_text('\n'.join(f'{k} = {v}' for k,v in sorted(tuning.items()))+'\n')
        signature = {'binary':binary_sha, 'params':model['params'],'tuning':tuning,'pairs':pairs}
        marker = folder/'stage-manifest.json'
        if not (marker.exists() and json.loads(marker.read_text()) == json.loads(json.dumps(signature))):
            env = {k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
            env.update(CG_GRID=','.join(f'{n}:{v}' for n,v in pairs), CG_STAGE_DIR=str((folder/'stages').resolve()),
                CG_RENDER_DIR=str((folder/'grid').resolve()), CG_RATE='44100', CG_WARMUP_MS='200',
                CG_PARAMS=','.join(f'{k}={v}' for k,v in model['params'].items()), CG_PRESET='concert',
                CG_TUNING=str((folder/'tuning.txt').resolve()), CG_LEFT='1', RUST_MIN_STACK='33554432')
            with (folder/'stage.log').open('w') as log:
                subprocess.run([str(a.binary.resolve()),'--exact','tests::render_reference_wavs','--ignored','--nocapture'],
                    env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
            assert 'complaints: []' in (folder/'stage.log').read_text()
            marker.write_text(json.dumps(signature,indent=2))
        result = {}
        for n,v in pairs:
            key = f'{n}:{v}'
            raw = e.render_model(a.binary,frozen,folder/f'{n}-{v}',n,v)
            x = e.read_model(raw)*10**(original['gain_db']/20)
            ref = original['rows'][key]
            features = e.features(x,n,ref['reference']['centers_hz'])
            stage = np.genfromtxt(folder/'stages'/f'stages{n:03}v{v}.csv',delimiter=',',names=True)
            traces = {}
            for field in stage.dtype.names:
                z = stage[field][round(.05*e.RATE):round(.2*e.RATE)]
                hz,power = e.psd(np.column_stack((z,z)))
                traces[field] = dict(total_db=float(e.db(e.power(z))),
                    bands_db=[float(e.db(power[(hz>=lo)&(hz<hi)].sum())) for lo,hi in e.BANDS],
                    partials_db=[float(e.db(power[abs(hz-f)<max(8,.025*440*2**((n-69)/12))].sum())) for f in features['centers_hz']])
            loading = np.genfromtxt(folder/'stages'/f'loading{n:03}v{v}.csv',delimiter=',',names=True)
            strike_path = folder/'stages'/f'strike{n:03}v{v}.csv'
            modes = max(0,len(strike_path.read_text().splitlines())-1)
            reference_audio = sf.read(ref['source_file'],always_2d=True)[0][:16*e.RATE]
            reference_audio = np.pad(reference_audio,((0,max(0,len(x)-len(reference_audio))),(0,0)))
            result[key] = dict(note=n,velocity=v,contact_modes=modes,model=features,
                differences=e.compare(ref['reference'],features,ref['source_support_seconds']),
                multi_spectrum=e.multi_spectrum(reference_audio,x,ref['source_support_seconds']),
                traces_50_200ms=traces,loading={f:np.atleast_1d(loading[f]).tolist() for f in loading.dtype.names})
        (folder/'measurements.json').write_text(json.dumps(result,indent=2))
        print('DONE',name,flush=True)
        return name,result
    with ThreadPoolExecutor(max_workers=2) as pool:
        results = dict(pool.map(run,profiles.items()))
    report = dict(model=model,binary_sha256=binary_sha,common_gain_db=original['gain_db'],
        profile_overrides=profiles,stages_units='Different internal units; spectral shapes only.',rows=results)
    (a.work/'audit.json').write_text(json.dumps(report,indent=2))
    for name,rows in results.items():
        for key in ('60:124','72:124','84:124','90:124','96:124'):
            r=rows[key]; d=r['differences']; f=r['model']['stages']
            print(name,key,'modes',r['contact_modes'],'level',round(d[2]['level_delta_db'],2),
                  'bands',np.round(d[2]['band_delta_db'],1).tolist(),'spectral',
                  [round(q['stage_mae_db'][2],2) for q in r['multi_spectrum']],flush=True)


if __name__=='__main__':
    main()
