"""Publish causal probe findings, including regressions and fixed-gain auditions."""
import argparse
import csv
import hashlib
import importlib.util
import json
from pathlib import Path
import numpy as np
import soundfile as sf


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--work',type=Path,required=True)
    p.add_argument('--reference',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    spec=importlib.util.spec_from_file_location('e',Path(__file__).with_name('evaluate-concert-grand-full-sound.py'))
    e=importlib.util.module_from_spec(spec);spec.loader.exec_module(e)
    original=json.loads(a.reference.read_text())
    profiles={}
    for marker in sorted(a.work.rglob('measurements.json')):
        profiles[marker.parent.name]=dict(folder=str(marker.parent),rows=json.loads(marker.read_text()))
    summary={};table=[]
    base=profiles['14B']['rows']
    for label,profile in profiles.items():
        rows=profile['rows'];deltas=[];energy=[]
        for key,row in rows.items():
            support=original['rows'][key]['source_support_seconds']
            score=lambda r:np.mean([q['stage_mae_db'][i] for q in r['multi_spectrum'] for i in range(5)
                                    if e.STAGES[i][1]<=support and q['stage_mae_db'][i] is not None])
            value=float(score(row));delta=value-float(score(base[key]));deltas.append(delta)
            table.append(dict(profile=label,note=row['note'],velocity=row['velocity'],
                early_spectral_mae_db=value,change_vs_14b_db=delta,
                initial_level_error_db=row['differences'][2]['level_delta_db'],
                body_level_error_db=row['differences'][4]['level_delta_db']))
            path=Path(profile['folder'])/'stages'/f"energy{row['note']:03}v{row['velocity']}.csv"
            if path.exists():
                audit=np.genfromtxt(path,delimiter=',',names=True)
                energy.append(float((audit['string_energy_j']+audit['hammer_final_energy_j'])/audit['input_energy_j']))
        summary[label]=dict(mean_early_spectral_error_db=float(np.mean([x['early_spectral_mae_db'] for x in table if x['profile']==label])),
            mean_change_vs_baseline_db=float(np.mean(deltas)),worse_pairs=sum(x>0.25 for x in deltas),
            improved_pairs=sum(x<-.25 for x in deltas),max_change_db=float(max(deltas)),
            contact_energy_max_fraction=max(energy) if energy else None,
            interpreted='Lower error is diagnostic, not a perceptual fidelity rating. Same 18 pairs and frozen gain; no fit or independent holdout.')
    with (a.output/'comparacion.csv').open('w',newline='',encoding='utf-8') as f:
        w=csv.DictWriter(f,fieldnames=list(table[0]));w.writeheader();w.writerows(table)
    result=dict(policy='18 exact recorded pairs; 9 roots C3..F#7, velocity60/124. No per-note gain, no EQ, no production selection.',
        common_gain_db=original['gain_db'],profiles=summary,notes='Body decay and radiation transfer are independent failures; turning off all drain is diagnostic only.')
    (a.output/'resumen.json').write_text(json.dumps(result,indent=2))
    candidates=[n for n in profiles if n.startswith('rate-')]
    chosen=min(candidates,key=lambda n:summary[n]['mean_early_spectral_error_db'])
    (a.output/'variante-audicion.json').write_text(json.dumps(dict(name=chosen,experimental_only=True,
        model=json.loads((Path(profiles[chosen]['folder']).parent/'model.json').read_text()),
        overrides=json.loads((Path(profiles[chosen]['folder']).parent/'profiles.json').read_text())[chosen]),indent=2))
    gain=10**(original['gain_db']/20);sets={}
    pairs=[(n,v) for n in (60,72,84,90,96) for v in (60,124)]
    for label in ('14B',chosen,'retarded'):
        folder=Path(profiles[label]['folder']);clips=[]
        for n,v in pairs:clips.append(e.read_model(folder/f'{n}-{v}'/'performance.f32')[:4*e.RATE]*gain)
        sets[label]=clips
    sets['Steinway']=[sf.read(original['rows'][f'{n}:{v}']['source_file'],frames=4*e.RATE,always_2d=True)[0] for n,v in pairs]
    peak=max(float(abs(x).max()) for clips in sets.values() for x in clips)
    trim=min(1,.85/peak);audio={}
    for label,clips in sets.items():
        chunks=[]
        for clip in clips:
            clip=clip.copy()*trim
            clip[-662:]*=np.linspace(1,0,662)[:,None]
            chunks.extend((clip,np.zeros((round(.4*e.RATE),2))))
        x=np.concatenate(chunks);assert np.isfinite(x).all() and abs(x).max()<=.851
        name={'14B':'14B-anterior','retarded':'Radiacion-prueba-descartada','Steinway':'Steinway-original'}.get(label,'Fieltro-experimental')+'.wav'
        path=a.output/name;sf.write(path,x,e.RATE,subtype='PCM_24')
        audio[name]=dict(sha256=hashlib.sha256(path.read_bytes()).hexdigest(),peak=float(abs(x).max()),seconds=len(x)/e.RATE,
            label=label,common_trim_db=float(20*np.log10(trim)))
    (a.output/'audios.json').write_text(json.dumps(dict(order=pairs,held_seconds=4,silence_seconds=.4,
        release='Held fragments, no key release; 15ms end fade.',audios=audio),indent=2))
    for k,v in summary.items():print(k,v,flush=True)
    print('AUDITION',chosen,flush=True)


if __name__=='__main__':main()
