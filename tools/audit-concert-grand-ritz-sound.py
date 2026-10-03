"""P2 native low-band structural pilot, retaining every factory mode >=800 Hz.

Frozen geometry, energy-normalized Ritz modes and matched loading/excitation/
surface projection. Same gain and all other factory settings. No selection or
package change. Upper shapes remain synthetic; this is a hybrid, not an
identified Steinway or a complete soundboard replacement.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess

import numpy as np
import soundfile as sf


def module(path,name):
    spec=importlib.util.spec_from_file_location(name,path)
    m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('binary','structural','work','output'):p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--expected-sha256',required=True)
    p.add_argument('--causal',action='store_true',help='Matched retarded-pressure controls')
    p.add_argument('--probe-only',action='store_true',help='Four strong notes, no long holds')
    p.add_argument('--comparison-profile',default='Panel-costillas-puente')
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    tools=Path(__file__).resolve().parent
    body=module(tools/'identify-concert-grand-body.py','ritz_body');fit,attack=body.FIT,body.ATTACK
    subtle=module(tools/'audit-concert-grand-subtle-unison.py','ritz_subtle');audit=fit.AUDIT
    assert audit.sha(a.binary)==a.expected_sha256
    structure=json.loads((a.structural/'report.json').read_text())
    profiles={'Base':None,**{name:a.structural/f'{name}-native.csv' for name in structure['banks']}}
    assert a.comparison_profile in profiles
    for name,asset in profiles.items():
        if asset:assert audit.sha(asset)==structure['banks'][name]['native_sha256']
    refs={};targets={};attacks={};arrays={}
    ref_dir=tools.parent/'local/concert-grand-identified-body'; provenance=json.loads((ref_dir/'reference-manifest.json').read_text())
    report=dict(policy=__doc__,binary_sha256=a.expected_sha256,structural_report_sha256=audit.sha(a.structural/'report.json'),
                export_gain_db=0,selection=None,factory_changed=False,profiles={},references={},baseline_bit_exact_cases=[])
    velocities=(124,) if a.probe_only else (28,60,92,124)
    pairs=[(n,v) for n in (60,72,84,90) for v in velocities]
    tuning={'CAUSAL_RADIATION_FIR':1,'SHARED_RADIATION_FIR':1} if a.causal else {}
    report['tuning']=tuning
    report['velocities']=velocities
    report['hold_s']=2
    for n,v in pairs:
        key=f'{n}:{v}';path=ref_dir/'reference'/f'{n}-{v}.wav';meta=json.loads(path.with_suffix('.json').read_text())
        assert meta['source']==provenance['sources'][key]
        assert audit.sha(Path(meta['source']['file']))==meta['source']['sha256']
        assert all(meta[k]==provenance[k] for k in ('mapping_sha256','sfz_sha256','dll_sha256'))
        refs[key],rate=sf.read(path,always_2d=True);assert rate==44100
        targets[key]=fit.measure(refs[key],n);attacks[key]=attack.measure(refs[key],n)
        report['references'][key]=dict(metadata=meta,wav_sha256=audit.sha(path))

    def render(asset,events,frames,folder):
        folder.mkdir(parents=True,exist_ok=True)
        event_file=folder/'events.txt';event_file.write_text(events,encoding='utf-8')
        tuning_file=folder/'tuning.txt';tuning_file.write_text(''.join(f'{k}={v}\n' for k,v in tuning.items()),encoding='utf-8')
        signature=dict(binary_sha256=a.expected_sha256,events_sha256=audit.sha(event_file),frames=frames,
            rate=44100,block_frames=256,warmup_ms=200,preset='concert',tuning=tuning,
            structural_asset=None if asset is None else dict(file=str(asset.resolve()),sha256=audit.sha(asset)))
        manifest=folder/'manifest.json';raw=folder/'performance.f32'
        if not (manifest.exists() and json.loads(manifest.read_text())==signature and raw.exists() and raw.stat().st_size==frames*8):
            env={k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
            env.update(CG_EVENT_FILE=str(event_file.resolve()),CG_TOTAL_FRAMES=str(frames),CG_RENDER_DIR=str(folder.resolve()),
                CG_PRESET='concert',CG_RATE='44100',CG_WARMUP_MS='200',RUST_MIN_STACK='33554432')
            env['CG_TUNING']=str(tuning_file.resolve())
            if asset:env['CG_RITZ_BOARD']=str(asset.resolve())
            with (folder/'render.log').open('w',encoding='utf-8') as log:
                subprocess.run([str(a.binary.resolve()),'tests::render_reference_wavs','--exact','--ignored','--nocapture','--test-threads=1'],
                    env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
            assert 'complaints: []' in (folder/'render.log').read_text()
            manifest.write_text(json.dumps(signature,indent=2),encoding='utf-8')
        x=np.array(audit.load(raw),dtype=float)
        assert np.isfinite(x).all() and abs(x).max()<1
        return x,audit.sha(raw)

    for label,asset in profiles.items():
        rows={};temporal={};hashes={};long={}
        for n,v in pairs:
            key=f'{n}:{v}'
            x,digest=render(asset,f'0 144 {n} {v}\n88200 128 {n} 64\n',176400,a.work/label/f'{n}-{v}')
            if label=='Base' and not a.causal:
                old=tools.parent/'local/concert-grand-polarization/renders/Base'/f'{n}-{v}/performance.f32'
                assert np.array_equal(x,audit.load(old)),'Factory control changed'
                report['baseline_bit_exact_cases'].append(key)
            arrays[(label,key)]=x;rows[key]=fit.score(targets[key],fit.measure(x,n));hashes[key]=digest
            temporal[key]=attack.compare(attacks[key],attack.measure(x,n))
            sf.write(a.output/f'{n}-{v}-{label}.wav',x,44100,subtype='PCM_24')
        for n in (() if a.probe_only else (60,72,84,90)):
            x,digest=render(asset,f'0 144 {n} 124\n529200 128 {n} 64\n',617400,a.work/label/f'long-{n}')
            ref=json.loads(Path('C:/SHARED/Unisono-sutil-2026-10-02/report.json').read_text())
            long[str(n)]=subtle.long_measure(x,n,fit,ref['long_reference'][str(n)]['support']);long[str(n)]['raw_sha256']=digest
            sf.write(a.output/f'{n}-124-{label}-sostenida-12s.wav',x,44100,subtype='PCM_24')
        report['profiles'][label]=dict(asset=None if asset is None else dict(file=str(asset),sha256=audit.sha(asset)),
            rows=rows,attacks=temporal,raw_sha256=hashes,long=long)
        print(label,json.dumps(dict(mean_score=float(np.mean([r['total'] for r in rows.values()])),
                                   worst_score=max(r['total'] for r in rows.values()))),flush=True)
        (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    report['timeline']=[]
    for n,name in ((60,'C4'),(72,'C5'),(84,'C6'),(90,'Fsharp6')):
        comparator=[]
        for label in (*profiles,'Steinway'):
            clips=[]
            for v in velocities:
                key=f'{n}:{v}';x=(refs[key] if label=='Steinway' else arrays[(label,key)])[:110250].copy()
                x[-441:]*=np.linspace(1,0,441)[:,None];clips.extend((x,np.zeros((22050,2))))
            sf.write(a.output/f'{name}-{label}-intensidades.wav',np.concatenate(clips),44100,subtype='PCM_24')
        for v in velocities:
            for label in ('Base',a.comparison_profile,'Steinway'):
                key=f'{n}:{v}';x=(refs[key] if label=='Steinway' else arrays[(label,key)])[:110250].copy()
                x[-441:]*=np.linspace(1,0,441)[:,None]
                report['timeline'].append(dict(note=n,velocity=v,label=label,start_s=3*len(comparator)//2))
                comparator.extend((x,np.zeros((22050,2))))
        sf.write(a.output/f'{name}-Base-Tabla-Steinway.wav',np.concatenate(comparator),44100,subtype='PCM_24')
    report['script_sha256']=audit.sha(Path(__file__))
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    (a.output/'LEEME.txt').write_text(
        'P2: modelo estructural bajo 800 Hz y complemento alto de fabrica.\n'
        'Geometria/material/apoyos supuestos, no identificados del Steinway.\n'
        f'Intensidades solas: velocidades {velocities}, tres segundos por entrada.\n'
        f'Observador causal: {a.causal}. Tecla sostenida 2 s.\n'
        f'Comparadores: por intensidad Base / {a.comparison_profile} / Steinway.\n'
        'Misma ganancia; sin EQ, normalizacion o nuevo detune. Prototipo no instalado ni aprobado.\n',encoding='utf-8')


if __name__=='__main__':main()
