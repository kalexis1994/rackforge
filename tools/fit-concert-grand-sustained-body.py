"""Fit shared, passive soundboard parameters against sustained partial energy.

All candidates use the same collocated mobility for loading and radiation.
No per-note gain, EQ, loss correction, or diagnostic drain ablation is fitted.
Historical samples are reused; validation is internal, not a blind listening test.
"""
import argparse, hashlib, importlib.util, json
from pathlib import Path
import numpy as np


def load(name,path):
    s=importlib.util.spec_from_file_location(name,path)
    m=importlib.util.module_from_spec(s);s.loader.exec_module(m);return m


def delta(a,b):
    return float(10*np.log10(max(a,1e-22)/max(b,1e-22)))


def score(target, model):
    decay=[];balance=[];body=[]
    # H1-H6 matter individually: a low-frequency noise burst cannot satisfy
    # a missing tonal fundamental. Reference mask stays fixed across models.
    initial=target['windows'][0]['partial_powers']
    ceiling=max(initial)
    for i,(a,b) in enumerate(zip(target['windows'][:3],model['windows'][:3])):
        if i:
            body.append(abs(b['total_relative_initial_db']-a['total_relative_initial_db']))
        for k in range(min(6,len(a['partial_powers']),len(b['partial_powers']))):
            if initial[k]<ceiling*1e-3 or a['partial_powers'][k]<ceiling*1e-5:
                continue
            if i:
                da=delta(a['partial_powers'][k],initial[k])
                db=delta(b['partial_powers'][k],model['windows'][0]['partial_powers'][k])
                decay.append(min(abs(db-da),24))
            if k:
                balance.append(min(abs(b['partials_relative_H1_db'][k]-a['partials_relative_H1_db'][k]),24))
    # Some soft upper notes contain only one retained partial. There is no
    # harmonic-balance evidence there, so that component contributes zero
    # for every candidate instead of producing NaN or inventing a target.
    d=float(np.mean(decay)) if decay else 0.
    b=float(np.mean(balance)) if balance else 0.
    e=float(np.mean(body)) if body else 0.
    return dict(decay_db=d,balance_db=b,body_db=e,total=.5*d+.35*b+.15*e,
        observed_decay_terms=len(decay),observed_balance_terms=len(balance))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--research-tools',type=Path,required=True)
    p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--model',type=Path,required=True)
    p.add_argument('--reference-root',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--profiles',type=Path,required=True)
    p.add_argument('--notes',default='60,66,72,78,84,90,96')
    p.add_argument('--velocities',default='60,124')
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    v3=load('sustain_v3',a.research_tools/'concert-grand-refit-v2.py')
    audit=load('sustain_audit',Path(__file__).with_name('audit-concert-grand-timbre-history.py'))
    model=json.loads(a.model.read_text(encoding='utf-8'))
    profiles=json.loads(a.profiles.read_text(encoding='utf-8'))
    pairs=[(int(n),int(v)) for n in a.notes.split(',') for v in a.velocities.split(',')]
    sources=v3.refit.samples(a.reference_root,'bitklavier-stage')
    target={p:audit.features(sources[p],p[0],v3.analysis) for p in pairs}
    result={}
    for name,overrides in profiles.items():
        if 'tuning' in overrides or 'params' in overrides:
            tuning={**model['tuning'],**overrides.get('tuning',{})}
            params={**model['params'],**overrides.get('params',{})}
        else:
            tuning={**model['tuning'],**overrides};params=model['params']
        root=a.output/name
        v3.render_candidate(a.binary,root,pairs,params,tuning=tuning,warmup_ms=200)
        rows={};gain=[]
        for n,v in pairs:
            f=audit.features(root/f'model{n:03}v{v}.wav',n,v3.analysis)
            t=target[n,v];row=score(t,f)
            row['H1_decay_db']=[w['H1_relative_initial_db'] for w in f['windows']]
            row['target_H1_decay_db']=[w['H1_relative_initial_db'] for w in t['windows']]
            row['features']=f;rows[f'{n}:{v}']=row
            gain.append(delta(t['windows'][0]['total_power'],f['windows'][0]['total_power']))
        # Retain mean and worst-note values; a median cannot hide a bad octave.
        result[name]=dict(tuning=tuning,params=params,rows=rows,training_gain_db=float(np.median(gain)),
            aggregate={key:float(np.mean([r[key] for r in rows.values()])) for key in ('decay_db','balance_db','body_db','total')},
            worst_total=float(max(r['total'] for r in rows.values())))
        print(name,result[name]['aggregate'],'worst',round(result[name]['worst_total'],3),flush=True)
        (a.output/'progress.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
    report=dict(policy=__doc__,profiles=result,pairs=pairs,targets={f'{n}:{v}':f for (n,v),f in target.items()},
        binary_sha256=hashlib.sha256(a.binary.read_bytes()).hexdigest(),
        base_model_sha256=hashlib.sha256(a.model.read_bytes()).hexdigest())
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print('RANK',sorted(result,key=lambda name:result[name]['aggregate']['total']),flush=True)


if __name__=='__main__':main()
