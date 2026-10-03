"""Audible P3 control of absolute pole reference, with existing gain/statistics.

No SI stretch gain or energy exchange is installed. Frozen earlier controls
must match with the switch off; native raw exports remain at unity gain.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import numpy as np
import soundfile as sf


def module(path,name):
    spec=importlib.util.spec_from_file_location(name,path)
    result=importlib.util.module_from_spec(spec);spec.loader.exec_module(result);return result


def difference(x,y):
    return dict(maximum_sample_error=float(abs(x-y).max()),
        relative_rms_error_db=float(10*np.log10((np.mean((x-y)**2)+1e-30)/(np.mean(y*y)+1e-30))))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ('binary','work','output'):p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--expected-sha256',required=True)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    tools=Path(__file__).resolve().parent;root=tools.parent
    body=module(tools/'identify-concert-grand-body.py','reference_body');fit,attack=body.FIT,body.ATTACK;audit=fit.AUDIT
    assert audit.sha(a.binary)==a.expected_sha256
    prior_path=Path('C:/SHARED/Fisica-pendiente-2026-10-02/P3-geometria-audio/report.json')
    prior=json.loads(prior_path.read_text())
    basework=root/'local/concert-grand-nonlinear/renders'
    profiles={'Referencia-fabrica':('Actual',{'TENSION_REFERENCE':1}),
        'Referencia-por-cuerda':('Geometria-promediada',{'PER_STRING_POLARIZATIONS':1,'NONLINEAR_WIRE_GEOMETRY':2,'TENSION_REFERENCE':1})}
    report=dict(policy=__doc__,binary_sha256=a.expected_sha256,selection=None,factory_changed=False,
        export_gain_db=0,prior_report_sha256=audit.sha(prior_path),profiles={},controls_bit_exact=[],references={})
    clips={}
    for name,(paired,tuning) in profiles.items():
        rows={};attacks={};delta={};paired_delta={};hashes={}
        for note in (42,60,72,84,90):
            for velocity in (60,124):
                key=f'{note}:{velocity}';folder=f'{note}-{velocity}'
                events=f'0 144 {note} {velocity}\n88200 128 {note} 64\n'
                oldraw=basework/paired/folder/'performance.f32';assert audit.sha(oldraw)==prior['profiles'][paired]['raw_sha256'][key]
                old=np.array(audit.load(oldraw),dtype=float)
                # Reuse one check folder, retaining all checked hashes here.
                off={k:v for k,v in tuning.items() if k!='TENSION_REFERENCE'}
                raw=audit.render(a.binary,events,176400,a.work/'off-check',off)
                assert audit.sha(raw)==prior['profiles'][paired]['raw_sha256'][key]
                report['controls_bit_exact'].append(dict(profile=paired,case=key,raw_sha256=audit.sha(raw)))
                raw=audit.render(a.binary,events,176400,a.work/name/folder,tuning)
                x=np.array(audit.load(raw),dtype=float);assert np.isfinite(x).all() and abs(x).max()<1
                clips[(name,key)]=x[:110250].copy();hashes[key]=audit.sha(raw)
                paired_delta[key]=difference(x,old)
                actualraw=basework/'Actual'/folder/'performance.f32'
                assert audit.sha(actualraw)==prior['profiles']['Actual']['raw_sha256'][key]
                actual=np.array(audit.load(actualraw),dtype=float);delta[key]=difference(x,actual)
                meta=prior['references'][key];path=root/'local/concert-grand-identified-body/reference'/f'{note}-{velocity}.wav'
                assert audit.sha(path)==meta['wav_sha256']
                assert audit.sha(Path(meta['metadata']['source']['file']))==meta['metadata']['source']['sha256']
                reference,rate=sf.read(path,always_2d=True);assert rate==44100
                report['references'][key]=meta
                rows[key]=fit.score(fit.measure(reference,note),fit.measure(x,note))
                attacks[key]=attack.compare(attack.measure(reference,note),attack.measure(x,note))
        events=('0 144 60 124\n2205 128 60 64\n4410 144 60 92\n6615 128 60 64\n'
            '8820 176 64 127\n11025 144 60 124\n13230 128 60 64\n22050 176 64 64\n'
            '33075 144 60 60\n35280 128 60 64\n44100 176 64 0\n')
        raw=audit.render(a.binary,events,176400,a.work/name/'reiterations',tuning)
        x=np.array(audit.load(raw),dtype=float);assert np.isfinite(x).all() and abs(x).max()<1
        oldraw=basework/paired/'reiterations/performance.f32'
        assert audit.sha(oldraw)==prior['profiles'][paired]['reiterations']['raw_sha256']
        sf.write(a.output/f'Reiteraciones-{name}.wav',x,44100,subtype='PCM_24')
        report['profiles'][name]=dict(paired_control=paired,tuning=tuning,rows=rows,attacks=attacks,
            mean_score=float(np.mean([r['total'] for r in rows.values()])),
            worst_score=max(r['total'] for r in rows.values()),raw_sha256=hashes,
            delta_to_actual=delta,delta_to_paired_control=paired_delta,
            reiterations=dict(peak=float(abs(x).max()),finite=True,raw_sha256=audit.sha(raw),
                delta_to_paired_control=difference(x,np.array(audit.load(oldraw),dtype=float))))
        for note,label in ((42,'Fsharp2'),(60,'C4'),(72,'C5'),(84,'C6'),(90,'Fsharp6')):
            seq=[]
            for velocity in (60,124):
                part=clips[(name,f'{note}:{velocity}')].copy()
                part[-441:]*=np.linspace(1,0,441)[:,None];seq.extend((part,np.zeros((22050,2))))
            sf.write(a.output/f'{label}-{name}-intensidades.wav',np.concatenate(seq),44100,subtype='PCM_24')
        print(name,report['profiles'][name]['mean_score'],flush=True)
        (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    report['script_sha256']=audit.sha(Path(__file__))
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    (a.output/'LEEME.txt').write_text('P3: referencia absoluta de polos; ganancias empiricas anteriores. No es el nucleo SI.\n'
        'Velocidad 60 a 0 s, 124 a 3 s; tecla 2 s, release 0,5 s, silencio 0,5 s. Misma ganancia.\n'
        'Referencia-fabrica: misma estadistica heredada; Referencia-por-cuerda: geometria media separada.\n'
        'Controles y Steinway en la carpeta anterior P3-geometria-audio. Prototipos no instalados.\n',encoding='utf-8')


if __name__=='__main__':main()
