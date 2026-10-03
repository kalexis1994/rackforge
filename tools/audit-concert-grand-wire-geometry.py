"""P3 attribution: false wire products, control-rate aliasing and axial source.

Existing phasor engine and synthetic factory board, without P2 or new tuning.
Empirical amplitude/slope units remain; these are not the SI conservative core.
Same gain, validated reference provenance and a frozen factory control.
"""
import argparse
import importlib.util
import json
from pathlib import Path

import numpy as np
import soundfile as sf


PROFILES={
    'Actual':{},
    'Control-polarizaciones':{'PER_STRING_POLARIZATIONS':1},
    'Geometria-instantanea':{'PER_STRING_POLARIZATIONS':1,'NONLINEAR_WIRE_GEOMETRY':1},
    'Geometria-promediada':{'PER_STRING_POLARIZATIONS':1,'NONLINEAR_WIRE_GEOMETRY':2},
    'Fuente-por-cuerda':{'PER_STRING_POLARIZATIONS':1,'NONLINEAR_WIRE_GEOMETRY':3},
}


def module(path,name):
    spec=importlib.util.spec_from_file_location(name,path)
    result=importlib.util.module_from_spec(spec);spec.loader.exec_module(result);return result


def difference(a,b):
    return dict(maximum_sample_error=float(abs(a-b).max()),
                relative_rms_error_db=float(10*np.log10((np.mean((a-b)**2)+1e-30)/(np.mean(b*b)+1e-30))))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('binary','work','output'):p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--expected-sha256',required=True)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    tools=Path(__file__).resolve().parent
    body=module(tools/'identify-concert-grand-body.py','wire_body');fit,attack=body.FIT,body.ATTACK;audit=fit.AUDIT
    assert audit.sha(a.binary)==a.expected_sha256
    notes=(42,60,72,84,90);velocities=(60,124)
    reference_dir=tools.parent/'local/concert-grand-identified-body'
    provenance=json.loads((reference_dir/'reference-manifest.json').read_text())
    prior=json.loads(Path('C:/SHARED/Fisica-pendiente-2026-10-02/P2-tabla-audio/report.json').read_text())
    refs={};targets={};target_attacks={};audio={}
    report=dict(policy=__doc__,binary_sha256=a.expected_sha256,selection=None,factory_changed=False,
                export_gain_db=0,notes=notes,velocities=velocities,profiles={},references={},factory_bit_exact_cases=[])
    for n in notes:
        for v in velocities:
            key=f'{n}:{v}';path=reference_dir/'reference'/f'{n}-{v}.wav'
            meta=json.loads(path.with_suffix('.json').read_text())
            assert meta['source']==provenance['sources'][key]
            assert all(meta[k]==provenance[k] for k in ('mapping_sha256','sfz_sha256','dll_sha256'))
            assert audit.sha(Path(meta['source']['file']))==meta['source']['sha256']
            refs[key],rate=sf.read(path,always_2d=True);assert rate==44100
            targets[key]=fit.measure(refs[key],n);target_attacks[key]=attack.measure(refs[key],n)
            report['references'][key]=dict(metadata=meta,wav_sha256=audit.sha(path))
    for name,tuning in PROFILES.items():
        rows={};attacks={};hashes={};delta={}
        for n in notes:
            for v in velocities:
                key=f'{n}:{v}'
                raw=audit.render(a.binary,f'0 144 {n} {v}\n88200 128 {n} 64\n',176400,a.work/name/f'{n}-{v}',tuning)
                x=np.array(audit.load(raw),dtype=float)
                assert np.isfinite(x).all() and abs(x).max()<1
                audio[(name,key)]=x;hashes[key]=audit.sha(raw)
                if name=='Actual' and n!=42:
                    assert hashes[key]==prior['profiles']['Base']['raw_sha256'][key]
                    report['factory_bit_exact_cases'].append(key)
                if name!='Actual':delta[key]=difference(x,audio[('Actual',key)])
                rows[key]=fit.score(targets[key],fit.measure(x,n))
                attacks[key]=attack.compare(target_attacks[key],attack.measure(x,n))
                sf.write(a.output/f'{n}-{v}-{name}.wav',x,44100,subtype='PCM_24')
        # Includes repeated blows, sustain, half pedal and full damper return.
        events=('0 144 60 124\n2205 128 60 64\n4410 144 60 92\n6615 128 60 64\n'
                '8820 176 64 127\n11025 144 60 124\n13230 128 60 64\n22050 176 64 64\n'
                '33075 144 60 60\n35280 128 60 64\n44100 176 64 0\n')
        raw=audit.render(a.binary,events,176400,a.work/name/'reiterations',tuning)
        x=np.array(audit.load(raw),dtype=float)
        assert np.isfinite(x).all() and abs(x).max()<1
        sf.write(a.output/f'Reiteraciones-{name}.wav',x,44100,subtype='PCM_24')
        repeat=dict(raw_sha256=audit.sha(raw),peak=float(abs(x).max()),finite=True)
        report['profiles'][name]=dict(tuning=tuning,rows=rows,attacks=attacks,raw_sha256=hashes,
                                     delta_to_actual=delta,reiterations=repeat)
        print(name,dict(mean_score=float(np.mean([r['total'] for r in rows.values()])),
                        worst_score=max(r['total'] for r in rows.values())),flush=True)
        (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    for note,label in ((42,'Fsharp2'),(60,'C4'),(72,'C5'),(84,'C6'),(90,'Fsharp6')):
        for name in (*PROFILES,'Steinway'):
            clips=[]
            for v in velocities:
                x=(refs[f'{note}:{v}'] if name=='Steinway' else audio[(name,f'{note}:{v}')])[:110250].copy()
                x[-441:]*=np.linspace(1,0,441)[:,None]
                clips.extend((x,np.zeros((22050,2))))
            sf.write(a.output/f'{label}-{name}-intensidades.wav',np.concatenate(clips),44100,subtype='PCM_24')
    report['script_sha256']=audit.sha(Path(__file__))
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    (a.output/'LEEME.txt').write_text(
        'P3: controles de geometria en el motor actual. No es el nucleo conservativo SI.\n'
        'Intensidades: velocidad 60 a 0 s y 124 a 3 s; tecla 2 s y 0,5 s de release.\n'
        'Actual=fabrica. Control-polarizaciones=separacion equivalente de H.\n'
        'Geometria-instantanea=estiramiento por cuerda cada 32 muestras.\n'
        'Geometria-promediada=media de fase para estudiar alias de esa actualizacion.\n'
        'Fuente-por-cuerda=media de fase y excitacion longitudinal sin cruces entre cuerdas/direcciones.\n'
        'Misma ganancia, sin nueva afinacion, EQ o tabla P2. Prototipos no instalados.\n',encoding='utf-8')


if __name__=='__main__':main()
