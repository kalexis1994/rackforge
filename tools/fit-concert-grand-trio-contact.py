"""Small, reproducible global felt search for the joint-contact research model.

Reuses the historical corpus; holdout is internal, not independent validation.
No note-level EQ or per-note coefficients are adjusted. Audition gains stay fixed.
"""
import argparse, hashlib, importlib.util, json
from pathlib import Path
import numpy as np
import soundfile as sf


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--research-tools',type=Path,required=True)
    p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--model',type=Path,required=True)
    p.add_argument('--reference-root',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--width',type=float,default=.1)
    p.add_argument('--stiffness-spread',type=float,default=.003)
    p.add_argument('--contact-top-hz',type=float,default=8000)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    s=importlib.util.spec_from_file_location('v3',a.research_tools/'concert-grand-refit-v2.py')
    m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
    model=json.loads(a.model.read_text());base=model['tuning']
    joint={**base,'JOINT_HAMMER_CONTACT':1,'COHERENT_UNISON':1,
        'JOINT_CONTACT_DT_S':2e-6,'JOINT_CONTACT_TOP_HZ':a.contact_top_hz,'HAMMER_STRING_HEIGHT_SPREAD_M':80e-6,
        'UNISON_WIDTH':a.width,'UNISON_TREBLE_WIDTH_RATIO':1,'STRING_STIFFNESS_SPREAD':a.stiffness_spread}
    profiles={'C':base,'joint':joint,
        'k16':{**joint,'FELT_TREBLE_DECADES':1.6},
        'k12':{**joint,'FELT_TREBLE_DECADES':1.2},
        'exp08':{**joint,'FELT_EXPONENT_TREBLE':.8},
        'k16-exp08':{**joint,'FELT_TREBLE_DECADES':1.6,'FELT_EXPONENT_TREBLE':.8},
        'k12-exp08':{**joint,'FELT_TREBLE_DECADES':1.2,'FELT_EXPONENT_TREBLE':.8},
        'tau02':{**joint,'STULOV_TAU_S':.0002}}
    notes=(60,66,72,78,84,90,96);velocities=(60,84,124)
    source_pairs=[(n,v) for n in notes for v in velocities]
    pairs=sorted(set(source_pairs)|{(n,v) for n in (65,67,79,91) for v in velocities})
    train=[(n,v) for n,v in source_pairs if n in (60,72,84,96) and v in (60,124)]
    holdout=[pair for pair in source_pairs if pair not in train]
    refs=m.refit.samples(a.reference_root,'bitklavier-stage')
    target={pair:m.segment(refs[pair]) for pair in source_pairs};result={}
    for name,tuning in profiles.items():
        root=a.output/name
        m.render_candidate(a.binary,root,pairs,model['params'],tuning=tuning,warmup_ms=200)
        rows={}
        for n,v in pairs:
            path=root/f'model{n:03}v{v}.wav';x=m.segment(path)
            row=m.pair_score(target[n,v],x) if (n,v) in target else {}
            b,_=sf.read(a.output/f'C/model{n:03}v{v}.wav');y,_=sf.read(path)
            row['levels_change_db']=[float(10*np.log10(np.mean(y[round(lo*44100):round(hi*44100)]**2)/np.mean(b[round(lo*44100):round(hi*44100)]**2)))
                for lo,hi in ((.05,.2),(.5,1.5),(1.5,3.5))]
            row['baseline_output_db']=[float(10*np.log10(np.mean(b[round(lo*44100):round(hi*44100)]**2)))+model['training_gain_db']+model.get('common_trim_db',0)
                for lo,hi in ((.05,.2),(.5,1.5),(1.5,3.5))]
            assert np.isfinite(y).all()
            rows[f'{n}:{v}']=row
        protected=[rows[f'{n}:{v}'] for n,v in pairs if n<=66]
        # Protect against thinning across all guard notes. A 3 dB change in
        # a -100 dBFS tail is not evidence of lost audible body. Positive
        # attack changes are allowed while restoring absent case excitation;
        # keep the accepted middle register within the original narrow guard.
        audible_body=[r['levels_change_db'][i] for r in rows.values() for i in (1,2)
            if r['baseline_output_db'][i]>-80]
        eligible=name=='C' or (max(abs(r['levels_change_db'][0]) for r in protected)<.75
            and min(r['levels_change_db'][0] for r in rows.values())>-.75
            and max(r['levels_change_db'][0] for r in rows.values())<3.5
            and min(audible_body)>-2.)
        result[name]=dict(tuning=tuning,rows=rows,eligible=eligible,
            train=m.aggregate(rows,train),holdout=m.aggregate(rows,holdout))
        print(name,'eligible',eligible,'train',result[name]['train'],'holdout',result[name]['holdout'],flush=True)
        (a.output/'progress.json').write_text(json.dumps(result,indent=2))
    selected=min((name for name,r in result.items() if r['eligible']),key=lambda name:result[name]['train']['total'])
    report=dict(policy=__doc__,selected_by_train_score=selected,results=result,train=train,holdout=holdout,
        eligibility_policy='Middle attack within +/-0.75 dB; all attacks above -0.75 dB and below +3.5 dB; body loss under 2 dB where baseline fixed-gain window exceeds -80 dBFS. Absolute floor is operational, not an audibility claim.',
        binary_sha256=hashlib.sha256(a.binary.read_bytes()).hexdigest(),
        model_sha256=hashlib.sha256(a.model.read_bytes()).hexdigest(),
        guard_only_pairs=[p for p in pairs if p not in source_pairs],
        reference_sha256={str(refs[p]):hashlib.sha256(refs[p].read_bytes()).hexdigest() for p in source_pairs})
    (a.output/'selection.json').write_text(json.dumps(report,indent=2));print('SELECTED',selected,flush=True)


if __name__=='__main__':main()
