"""Across-register bass/source ablations; fixed gain, no per-note matching."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
from pathlib import Path
import numpy as np
import soundfile as sf
from scipy.signal import butter,sosfiltfilt

def load(path,name):
    spec=importlib.util.spec_from_file_location(name,path)
    m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m

def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('model','binary','work','output'):
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--profiles',type=Path)
    p.add_argument('--notes',default='36,39,42,45,48,54,60')
    a=p.parse_args();a.work.mkdir(parents=True,exist_ok=True);a.output.mkdir(parents=True,exist_ok=True)
    e=load(Path(__file__).with_name('evaluate-concert-grand-full-sound.py'),'e_bass')
    s=load(Path(__file__).with_name('render-bitklavier-steinway-midi.py'),'s_bass')
    e.HOLD,e.SECONDS=3,4
    model=json.loads(a.model.read_text(encoding='utf-8'))
    frozen={**model['model'],'tuning':{**model['model']['tuning'],**model['overrides']}}
    assert e.digest(a.binary)==frozen['binary_sha256']
    profiles={'Actual':{},'Sin-pulso-contacto':{'tuning':{'CONTACT_BOARD_GAIN':0}},
              'Sin-bloom':{'params':{'15':0}}}
    for ratio in (4,16):
        d=np.log10(ratio)/.448
        profiles[f'Fieltro-graves-x{ratio}']={'tuning':{
            'FELT_K_A0':frozen['tuning']['FELT_K_A0']*ratio,
            'FELT_K_DECADES':frozen['tuning']['FELT_K_DECADES']-d,
            'FELT_TREBLE_DECADES':frozen['tuning']['FELT_TREBLE_DECADES']-.552*d}}
    if a.profiles:profiles=json.loads(a.profiles.read_text(encoding='utf-8'))
    pairs=[(int(n),v) for n in a.notes.split(',') for v in (60,124)]
    pairs+= [(42,40),(42,84)] if 42 in [n for n,v in pairs] else []
    bands=((25,180),(180,600),(600,1800),(1800,8000))
    filters=[butter(4,b,fs=e.RATE,btype='bandpass',output='sos') for b in bands]
    windows=((0,.02),(.02,.05),(.05,.2),(.2,.5),(.5,1.5))
    def measure(x):
        y,at=e.aligned(x)
        rows=[]
        for filt in filters:
            z=sosfiltfilt(filt,y,axis=0)
            rows.append([float(10*np.log10(np.mean(z[round(lo*e.RATE):round(hi*e.RATE)]**2)+1e-30)) for lo,hi in windows])
        env=np.mean(y[:e.RATE].reshape(100,441,2)**2,axis=(1,2))
        return dict(bands_db=rows,peak_10ms_time=float(np.argmax(env)*.01),
                    onset_seconds=at/e.RATE,peak=float(abs(y).max()))
    signals={};metrics={}
    def render(item):
        label,n,v=item;cfg=profiles[label]
        f={**frozen,'tuning':{**frozen['tuning'],**cfg.get('tuning',{})},
           'params':{**frozen['params'],**cfg.get('params',{})}}
        x=e.read_model(e.render_model(a.binary,f,a.work/label/f'{n}-{v}',n,v))*10**(26.401176937389373/20)
        return (label,n,v),x
    with ThreadPoolExecutor(max_workers=2) as pool:
        for key,x in pool.map(render,[(label,n,v) for label in profiles for n,v in pairs]):
            signals[key]=x;metrics.setdefault(key[0],{})[f'{key[1]}:{key[2]}']=measure(x)
        print('Rendered all native profiles',flush=True)
    synth=s.Engine(Path('local/steinway-midi/sfizz/library/bin/Release/sfizz.dll'),Path('local/steinway-midi/render-stage/SteinwayD-StageORTF.sfz'))
    try:
        for n,v in pairs:
            assert n in s.ROOTS,'Only exact recorded pitches in this audit'
            x=e.render_reference(synth,n,v);signals['Steinway',n,v]=x
            metrics.setdefault('Steinway',{})[f'{n}:{v}']=measure(x)
    finally:synth.close()
    trim=min(1,.85/max(float(abs(x).max()) for x in signals.values()))
    if 42 in [n for n,v in pairs]:
        for label in (*profiles,'Steinway'):
            parts=[]
            for v in (40,60,84,124):
                x=signals[label,42,v][:round(2.5*e.RATE)].copy()*trim
                x[-441:]*=np.linspace(1,0,441)[:,None]
                parts.extend((x,np.zeros((round(.5*e.RATE),2))))
            sf.write(a.output/f'Fs2-{label}.wav',np.concatenate(parts),e.RATE,subtype='PCM_24')
    report=dict(candidate_sha256=e.digest(a.model),binary_sha256=e.digest(a.binary),
                profiles=profiles,pairs=pairs,bands_hz=bands,windows_seconds=windows,
                native_gain_db=26.401176937389373,common_export_trim_db=float(20*np.log10(trim)),metrics=metrics,
                policy=__doc__+' Source ablations diagnose routes; no claim of physical identification.')
    (a.output/'audit.json').write_text(json.dumps(report,indent=2,ensure_ascii=False),encoding='utf-8')
    for label,rows in metrics.items():
        diffs=np.array([np.array(rows[f'{n}:{v}']['bands_db'])[:,2]-np.array(metrics['Steinway'][f'{n}:{v}']['bands_db'])[:,2] for n,v in pairs if n<=48])
        print(label,'median bass-band deltas (50–200ms)',np.round(np.median(diffs,axis=0),2).tolist(),flush=True)

if __name__=='__main__':main()
