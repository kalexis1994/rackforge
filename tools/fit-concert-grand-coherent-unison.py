"""Evaluate coherent per-string tuning against frozen model 07.

Exploratory reused corpus; no claim of blind validation or physical identification.
The source tools directory is explicit because the historical research worktree
may be read-only. Outputs and executable must be in the writable project.
"""
import argparse,importlib.util,json,hashlib
from pathlib import Path
import numpy as np

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--research-tools',type=Path,required=True)
    p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--model',type=Path,required=True)
    p.add_argument('--reference-root',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--broader',action='store_true')
    p.add_argument('--bridge-ablation',action='store_true',help='Diagnostic only: remove dissipative string coupling without changing radiation.')
    p.add_argument('--reactive',action='store_true',help='Evaluate frozen-frequency complex bridge loading; scales below one are diagnostic interpolations.')
    p.add_argument('--candidate-file',type=Path,help='JSON mapping of names to tuning overrides; always includes model 07 baseline.')
    p.add_argument('--notes',default='60,66,69,72',help='Actual reference MIDI notes, comma separated.')
    p.add_argument('--velocities',default='60,84,124',help='Actual reference velocities, comma separated.')
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    spec=importlib.util.spec_from_file_location('settling',a.research_tools/'audit-concert-grand-settling.py')
    m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
    model=json.loads(a.model.read_text());base=model['tuning'];gain=model['training_gain_db']
    refs=m.b.v3.refit.samples(a.reference_root,'bitklavier-stage')
    pairs=[(n,v) for n in map(int,a.notes.split(',')) for v in map(int,a.velocities.split(','))]
    if any(pair not in refs for pair in pairs):raise ValueError('requested notes/velocities must exist in the original samples')
    targets={f'{n}:{v}':m.inspect(refs[n,v],n) for n,v in pairs}
    cases=((1.0,.02),(1.5,.02),(1.0,.08),(1.5,.08),(2.0,.08)) if a.broader else ((.3,0),(.5,0),(.5,.005),(.5,.02),(.3,.02),(.7,.02))
    candidates=[('baseline',base)]+[(f'width{w}-stiffness{b}',{**base,
        'COHERENT_UNISON':1,'UNISON_WIDTH':w,'STRING_STIFFNESS_SPREAD':b})
        for w,b in cases]
    if a.bridge_ablation:
        candidates=[('baseline',base),
            ('independent-no-drain',{**base,'DIAGNOSTIC_STRING_DRAIN':0}),
            ('coherent-no-drain',{**base,'COHERENT_UNISON':1,'UNISON_WIDTH':.5,
                'STRING_STIFFNESS_SPREAD':.02,'DIAGNOSTIC_STRING_DRAIN':0})]
    if a.reactive:
        candidates=[('baseline',base),('coherent', {**base,'COHERENT_UNISON':1,'UNISON_WIDTH':.5,'STRING_STIFFNESS_SPREAD':.02})]
        candidates += [(f'width{w}-reactive{reactive}',{**base,'COHERENT_UNISON':1,
            'UNISON_WIDTH':w,'STRING_STIFFNESS_SPREAD':.02,'REACTIVE_BRIDGE':reactive})
            for w,reactive in ((.5,.25),(.5,.5),(.5,1),(.7,1),(1,1),(1.5,1))]
    if a.candidate_file:
        overrides=json.loads(a.candidate_file.read_text())
        if 'baseline' in overrides:raise ValueError('baseline is reserved')
        candidates=[('baseline',base)]+[(name,{**base,**values}) for name,values in overrides.items()]
    results={}
    for name,tuning in candidates:
        directory=a.output/name
        m.b.v3.render_candidate(a.binary,directory,pairs,model['params'],tuning=tuning,warmup_ms=200)
        rows={}
        for n,v in pairs:
            key=f'{n}:{v}';path=directory/f'model{n:03}v{v}.wav'
            partials=m.inspect(path,n,gain)
            for x,y in zip(partials,targets[key]):x['usable']=y['usable']
            audio,fs=m.b.v3.analysis.read_wav_head(str(path),4)
            audio=audio*10**(gain/20)
            target=m.b.v3.segment(refs[n,v]);short=m.b.v3.segment(path)
            rows[key]=dict(summary=m.summary(partials),partials=partials,
                body_error_db=float(np.mean(abs(m.b.body.body_signature(short,n)-m.b.body.body_signature(target,n)))),
                levels_db=[float(10*np.log10(np.mean(audio[round(lo*fs):round(hi*fs)]**2)))
                           for lo,hi in ((.05,.2),(.5,1.5),(1.5,3.5))])
        if name=='baseline':baseline=rows
        shifts=np.array([np.array(row['levels_db'])-np.array(baseline[k]['levels_db']) for k,row in rows.items()])
        body_delta=float(np.mean([row['body_error_db']-baseline[k]['body_error_db'] for k,row in rows.items()]))
        result=dict(tuning=tuning,rows=rows,late_count=sum(x['summary']['late_count'] for x in rows.values()),
            late_depth_db=sum(x['summary']['late_depth_sum_db'] for x in rows.values()),
            level_mean_delta_db=shifts.mean(axis=0).tolist(),level_worst_drop_db=shifts.min(axis=0).tolist(),
            body_delta_db=body_delta,
            eligible=bool(not a.bridge_ablation and shifts[:,2].mean()>-.5 and shifts[:,2].min()>-2 and abs(shifts[:,0]).max()<.5 and body_delta<.25))
        results[name]=result
        print(name,{k:v for k,v in result.items() if k not in ('rows','tuning')},flush=True)
        (a.output/'results.json').write_text(json.dumps(results,indent=2))
    selected=min([k for k,r in results.items() if k=='baseline' or r['eligible']],key=lambda k:results[k]['late_count'])
    result=dict(selected=selected,results=results,reference_rows=targets,policy=__doc__,
        binary_sha256=hashlib.sha256(a.binary.read_bytes()).hexdigest(),
        model_sha256=hashlib.sha256(a.model.read_bytes()).hexdigest(),
        reference_sha256={str(refs[n,v]):hashlib.sha256(refs[n,v].read_bytes()).hexdigest() for n,v in pairs})
    (a.output/'selection.json').write_text(json.dumps(result,indent=2));print('SELECTED',selected,flush=True)

if __name__=='__main__':main()
