"""P1b: an isometric horizontal-state lift, then per-wire hammer excitation.

Native research only; inherited normalized energy coordinates, not measured
per-wire masses or directional mobility. One pitch/ladder per wire in mode 2.
Separate state count, excitation, small tuning and directional loading controls.
No factory selection, level normalization, EQ or package change.
"""
import argparse
import importlib.util
import json
from pathlib import Path

import numpy as np
import soundfile as sf

RATE = 44100
PROFILES = {
    'Base': {},
    'Separadas-equivalentes': {'PER_STRING_POLARIZATIONS': 1},
    'Reparto-martillo': {'PER_STRING_POLARIZATIONS': 2},
    'Base-afinacion-004': {'UNISON_WIDTH': .04},
    'Reparto-afinacion-004': {'PER_STRING_POLARIZATIONS': 2, 'UNISON_WIDTH': .04},
    'Reparto-direccional': {'PER_STRING_POLARIZATIONS': 2, 'DIRECTIONAL_BRIDGE_HEIGHT_M': .025},
}


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def difference(candidate, control):
    error = candidate-control
    return dict(maximum_sample_error=float(abs(error).max()),
                relative_rms_error_db=float(10*np.log10((np.mean(error**2)+1e-30)/(np.mean(control**2)+1e-30))))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('binary','work','output'):
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--expected-sha256',required=True)
    a=p.parse_args()
    tools=Path(__file__).resolve().parent
    body=module(tools/'identify-concert-grand-body.py','polar_body')
    fit,attack=body.FIT,body.ATTACK
    subtle=module(tools/'audit-concert-grand-subtle-unison.py','polar_subtle')
    audit=fit.AUDIT
    assert audit.sha(a.binary)==a.expected_sha256
    a.output.mkdir(parents=True,exist_ok=True)
    ref_dir=tools.parent/'local/concert-grand-identified-body'
    provenance=json.loads((ref_dir/'reference-manifest.json').read_text())
    old_long=json.loads(Path('C:/SHARED/Unisono-sutil-2026-10-02/report.json').read_text())
    pairs=[(n,v) for n in (60,72,84,90) for v in (28,60,92,124)]
    refs,targets,target_attacks={}, {}, {}
    report=dict(policy=__doc__,binary_sha256=a.expected_sha256,export_gain_db=0,
                factory_changed=False,selection=None,profiles={},references={},
                equivalence={},baseline_bit_exact_cases=[],controls={})
    arrays={}
    for n,v in pairs:
        key=f'{n}:{v}'
        path=ref_dir/'reference'/f'{n}-{v}.wav'
        meta=json.loads(path.with_suffix('.json').read_text())
        assert meta['source']==provenance['sources'][key]
        assert all(meta[k]==provenance[k] for k in ('mapping_sha256','sfz_sha256','dll_sha256'))
        assert audit.sha(Path(meta['source']['file']))==meta['source']['sha256']
        x,rate=sf.read(path,always_2d=True)
        assert rate==RATE
        refs[key]=x
        targets[key]=fit.measure(x,n)
        target_attacks[key]=attack.measure(x,n)
        report['references'][key]=dict(wav_sha256=audit.sha(path),metadata=meta)
    for label,tuning in PROFILES.items():
        rows,attacks,identities,long={}, {}, {}, {}
        for n,v in pairs:
            key=f'{n}:{v}'
            raw=audit.render(a.binary,f'0 144 {n} {v}\n88200 128 {n} 64\n',
                             176400,a.work/label/f'{n}-{v}',tuning)
            x=np.array(audit.load(raw),dtype=float)
            assert np.isfinite(x).all() and abs(x).max()<1
            arrays[(label,key)]=x
            rows[key]=fit.score(targets[key],fit.measure(x,n))
            attacks[key]=attack.compare(target_attacks[key],attack.measure(x,n))
            identities[key]=audit.sha(raw)
            if label=='Base':
                old=tools.parent/'local/concert-grand-directional/renders/Base'/f'{n}-{v}/performance.f32'
                assert np.array_equal(audit.load(old),x),'Factory control changed'
                report['baseline_bit_exact_cases'].append(key)
            if label=='Separadas-equivalentes':
                report['equivalence'][key]=difference(x,arrays[('Base',key)])
                # Four-second complete output, including release and the
                # inherited nonlinear/auxiliary paths, not just one oscillator.
                assert report['equivalence'][key]['relative_rms_error_db'] < -60
            sf.write(a.output/f'{n}-{v}-{label}.wav',x,RATE,subtype='PCM_24')
        for n in (60,72,84,90):
            raw=audit.render(a.binary,f'0 144 {n} 124\n529200 128 {n} 64\n',
                             617400,a.work/label/f'long-{n}',tuning)
            x=np.array(audit.load(raw),dtype=float)
            assert np.isfinite(x).all() and abs(x).max()<1
            long[str(n)]=subtle.long_measure(x,n,fit,old_long['long_reference'][str(n)]['support'])
            long[str(n)]['raw_sha256']=audit.sha(raw)
            sf.write(a.output/f'{n}-124-{label}-sostenida-12s.wav',x,RATE,subtype='PCM_24')
        report['profiles'][label]=dict(tuning=tuning,rows=rows,attacks=attacks,raw_sha256=identities,long=long)
        print(label,json.dumps(dict(mean_score=float(np.mean([r['total'] for r in rows.values()])),
                                   worst_score=max(r['total'] for r in rows.values()))),flush=True)
        (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    # A short fixed-frequency colour diagnostic, with a reference-only support
    # mask. Separating a fixed level change from moving spectral balance.
    report['relative_colour']={}
    for n in (60,72,84,90):
        key=f'{n}:124'
        f0=440*2**((n-69)/12)
        hz,psd=fit.EVAL.psd(arrays[('Base',key)][2205:22050])
        curves={label:[] for label in PROFILES}
        for h,allowed in enumerate(old_long['long_reference'][str(n)]['support'],1):
            if not allowed:continue
            bins=np.flatnonzero(abs(hz-h*f0)<.18*f0)
            frequency=float(hz[bins[np.argmax(psd[bins])]])
            for label in PROFILES:
                t,level=subtle.partial_envelope(arrays[(label,key)],frequency,f0,hold=2)
                curves[label].append(level)
        interval=(t>=.25)&(t<=1.8)
        metrics={}
        for label in PROFILES:
            delta=(np.array(curves[label])-np.array(curves['Base']))[:,interval]
            colour=delta-np.median(delta,axis=0,keepdims=True)
            colour-=np.mean(colour,axis=1,keepdims=True)
            ranges=np.percentile(colour,95,axis=1)-np.percentile(colour,5,axis=1)
            metrics[label]=dict(relative_colour_rms_db=float(np.sqrt(np.mean(colour**2))),
                                median_partial_colour_range_db=float(np.median(ranges)),
                                max_partial_colour_range_db=float(ranges.max()))
        report['relative_colour'][str(n)]=dict(window_s=[.25,1.8],metrics=metrics)
    # Single/double/triple stringing plus a repeated-note/half-pedal control.
    control_cases={
        'Una-cuerda':('0 144 24 92\n88200 128 24 64\n',176400),
        'Dos-cuerdas':('0 144 30 92\n88200 128 30 64\n',176400),
        'Reiteraciones-pedales':('0 176 64 127\n0 144 60 92\n6000 128 60 64\n'
            '10000 144 60 124\n16000 128 60 64\n22000 144 72 92\n28000 128 72 64\n'
            '32000 176 64 64\n42000 144 60 60\n48000 128 60 64\n60000 176 64 0\n',176400),
    }
    for name,(events,frames) in control_cases.items():
        controls={}
        for label in ('Base','Separadas-equivalentes','Reparto-martillo','Reparto-direccional'):
            raw=audit.render(a.binary,events,frames,a.work/'controls'/name/label,PROFILES[label])
            x=np.array(audit.load(raw),dtype=float)
            assert np.isfinite(x).all() and abs(x).max()<1
            controls[label]=x
            sf.write(a.output/f'{name}-{label}.wav',x,RATE,subtype='PCM_24')
        report['controls'][name]=dict(equivalence=difference(controls['Separadas-equivalentes'],controls['Base']),
                                     peak={k:float(abs(v).max()) for k,v in controls.items()})
        assert report['controls'][name]['equivalence']['relative_rms_error_db'] < -60
    report['timeline']=[]
    for n,name in ((60,'C4'),(72,'C5'),(84,'C6'),(90,'Fsharp6')):
        comparator=[]
        for label in ('Base','Reparto-martillo','Steinway'):
            intensities=[]
            for v in (28,60,92,124):
                key=f'{n}:{v}'
                x=(refs[key] if label=='Steinway' else arrays[(label,key)])[:110250].copy()
                x[-441:]*=np.linspace(1,0,441)[:,None]
                intensities.extend((x,np.zeros((22050,2))))
            sf.write(a.output/f'{name}-{label}-intensidades.wav',np.concatenate(intensities),RATE,subtype='PCM_24')
        for v in (28,60,92,124):
            for label in ('Base','Reparto-martillo','Steinway'):
                key=f'{n}:{v}'
                x=(refs[key] if label=='Steinway' else arrays[(label,key)])[:110250].copy()
                x[-441:]*=np.linspace(1,0,441)[:,None]
                report['timeline'].append(dict(note=n,velocity=v,label=label,start_s=3*len(comparator)//2))
                comparator.extend((x,np.zeros((22050,2))))
        sf.write(a.output/f'{name}-Base-P1b-Steinway.wav',np.concatenate(comparator),RATE,subtype='PCM_24')
    report['script_sha256']=audit.sha(Path(__file__))
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    (a.output/'LEEME.txt').write_text(
        'P1b: tres verticales y tres horizontales en el registro de tres cuerdas.\n'
        'Equivalentes conserva la energia/proyeccion heredada. Reparto-martillo redistribuye la misma energia horizontal con fases y alturas de las verticales.\n'
        'Cada archivo de intensidades: velocidades 28,60,92,124, a 0,3,6,9 s.\n'
        'Comparadores: por intensidad Base / P1b Reparto-martillo / Steinway.\n'
        'Afinacion y palanca son controles separados. Sin ganancia, EQ ni normalizacion adicional. Prototipo, no instalado ni seleccionado.\n',encoding='utf-8')


if __name__=='__main__':main()
