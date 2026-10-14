"""Counterfactual repeated F5 contacts in the Nocturne; no DSP edits.

An omitted Note On is an intervention, not a valid performance replacement.
Its first 12 ms precede the next Note On, avoiding later strike-serial changes
in the comparison of attack increments. Downstream feedback remains included.
"""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import subprocess

import numpy as np
from scipy.io import wavfile


def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    root=Path(__file__).resolve().parents[1]
    docs=root/'docs/concert-grand-release-body-2026-10-14'
    work=root/'local/concert-grand-nocturne-attacks-2026-10-14'
    stage=work/'restrike';stage.mkdir(exist_ok=False)
    audio=Path('C:/SHARED/Piano-Ticks-Nocturno-2026-10-14')
    prior_path=docs/'nocturne-attacks.json';prior=json.loads(prior_path.read_text(encoding='utf-8'))
    for p,h in prior['files_sha256'].items():assert sha(p)==h,p
    probe=Path(prior['prefixes']['original']['command'][0]);rate=44100
    original=work/'original-0-53s.txt'
    rows=[list(map(int,line.split())) for line in original.read_text(encoding='utf-8').splitlines()]
    targets=[n for n in prior['notes'] if n['note']==77 and 49<n['onset_seconds']<51]
    assert len(targets)==2
    paths=[]
    for i,n in enumerate(targets):
        frame=round(n['onset_seconds']*rate)
        event=[frame,144,77,n['velocity']];assert rows.count(event)==1
        # No later strike is changed within the causal 12 ms window.
        later=[r[0] for r in rows if r[0]>frame and r[1]==144 and r[3]>0]
        assert min(later)-frame > .012*rate
        p=stage/f'omitted-{i}.txt'
        p.write_text(''.join(' '.join(map(str,r))+'\n' for r in rows if r!=event),encoding='utf-8',newline='\n')
        paths.append(p)
    variants=dict(original='',contacto_modelo='MERGE_RAMP_CONTACT = 1\n',contacto_4ms='MERGE_RAMP_S = 0.004\n')
    tuning={}
    for name,text in variants.items():
        p=stage/f'{name}.txt';p.write_text(text,encoding='utf-8',newline='\n');tuning[name]=p
    frozen={str(p):sha(p) for p in [Path(__file__),prior_path,original,probe,root/'plugins/concert-grand/src/lib.rs',*paths,*tuning.values()]}
    def render(job):
        name,event_path=job
        raw=stage/f'{name}-{event_path.stem}.f32'
        command=[str(probe),str(event_path),str(raw),str(rate),str(53*rate),'256','concert',str(tuning[name])]
        native=json.loads(subprocess.check_output(command,cwd=root,text=True))
        x=np.fromfile(raw,dtype='<f4').reshape(-1,2)
        assert len(x)==53*rate and np.isfinite(x).all() and abs(x).max()<1
        return (name,event_path.stem),(x,dict(command=command,native=native,raw_sha256=sha(raw)))
    jobs=[(name,p) for name in variants for p in ([original] if name!='original' else [])+paths]
    with ThreadPoolExecutor(max_workers=3) as pool:results=dict(pool.map(render,jobs))
    results[('original',original.stem)]=(np.fromfile(work/'original.f32',dtype='<f4').reshape(-1,2),prior['prefixes']['original'])
    comparisons=[]
    for i,n in enumerate(targets):
        at=round(n['onset_seconds']*rate);frames=round(.012*rate);one=dict(midi=n,variants={})
        for name in variants:
            full=results[(name,original.stem)][0]
            omitted=results[(name,paths[i].stem)][0]
            assert np.array_equal(full[:at],omitted[:at]), 'Intervention must preserve all preceding audio'
            delta=full[at:at+frames].astype(float)-omitted[at:at+frames]
            one['variants'][name]=dict(first_12ms_difference_rms_db=float(10*np.log10(max(float(np.mean(delta**2)),1e-30))),
                first_12ms_difference_peak=float(abs(delta).max()),
                largest_adjacent_difference=float(abs(np.diff(delta,axis=0)).max()))
        comparisons.append(one)
    outputs={}
    for name in variants:
        full=results[(name,original.stem)][0]
        p=audio/f'Melodia-49.55-50.55-{name}.wav';assert not p.exists()
        x=full[round(49.55*rate):round(50.55*rate)]
        # Three identical repeats, same gain, 350 ms silence between excerpts.
        gap=np.zeros((round(.35*rate),2),dtype=np.float32)
        x=np.concatenate([x,gap,x,gap,x]);wavfile.write(p,rate,x)
        sr,y=wavfile.read(p);assert sr==rate and np.array_equal(x,y)
        outputs[name]=dict(path=str(p),sha256=sha(p),source_seconds=[49.55,50.55],repeats=3,gap_seconds=.35,export_gain=1)
    assert frozen=={str(p):sha(p) for p in map(Path,frozen)}
    report=dict(scope=__doc__,files_sha256=frozen,variants=variants,
        renders={f'{name}:{stem}':r for (name,stem),(_,r) in results.items()},comparisons=comparisons,outputs=outputs,
        status='Diagnostic comparisons, no default or production source change',
        limitations='Adjacent-sample differences contain ordinary sinusoidal slope, not only clicks. Contact variants affect all prior repeats. Audibility and full-keyboard quality are not established by these measurements.')
    (docs/'nocturne-restrike.json').write_text(json.dumps(report,indent=2,allow_nan=False)+'\n',encoding='utf-8',newline='\n')
    print(json.dumps(comparisons,indent=2),flush=True)


if __name__=='__main__':main()
