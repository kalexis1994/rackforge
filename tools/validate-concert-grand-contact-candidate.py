"""Whole-keyboard validation of a contact candidate, with frozen reference gain."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
from pathlib import Path
import numpy as np
import soundfile as sf


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--candidate',type=Path,required=True)
    p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--reference',type=Path,required=True)
    p.add_argument('--baseline-work',type=Path,required=True)
    p.add_argument('--work',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();a.work.mkdir(parents=True,exist_ok=True)
    spec=importlib.util.spec_from_file_location('e',Path(__file__).with_name('evaluate-concert-grand-full-sound.py'))
    e=importlib.util.module_from_spec(spec);spec.loader.exec_module(e)
    chosen=json.loads(a.candidate.read_text())
    frozen={**chosen['model'],'tuning':{**chosen['model']['tuning'],**chosen['overrides']}}
    assert e.digest(a.binary)==frozen['binary_sha256']
    ref=json.loads(a.reference.read_text());gain=10**(ref['gain_db']/20)
    pairs=[(n,v) for n in e.ROOTS for v in (28,60,100,124)]
    exploration={(n,v) for n in (48,60,66,72,78,84,90,96,102) for v in (60,124)}
    report=dict(policy='All 30 exact roots, 4 layers:28/60/100/124. One frozen raw-recording gain; no candidate recalibration. Exploration18/validation102 from same library, not an independent piano.',
        model=frozen,gain_db=ref['gain_db'],rows={},summary={})
    with ThreadPoolExecutor(max_workers=2) as pool:
        futures={pair:pool.submit(e.render_model,a.binary,frozen,a.work/f'{pair[0]}-{pair[1]}',*pair) for pair in pairs}
        for i,(n,v) in enumerate(pairs):
            key=f'{n}:{v}';rr=ref['rows'][key]
            native=e.read_model(futures[n,v].result())*gain
            baseline=e.read_model(a.baseline_work/f'{n}-{v}'/'performance.f32')*gain
            original,rate=sf.read(rr['source_file'],frames=16*e.RATE,always_2d=True)
            assert rate==e.RATE
            original=np.pad(original,((0,max(0,len(native)-len(original))),(0,0)))
            features=e.features(native,n,rr['reference']['centers_hz'])
            base_spectrum=e.multi_spectrum(original,baseline,rr['source_support_seconds'])
            spectrum=e.multi_spectrum(original,native,rr['source_support_seconds'])
            score=lambda rows:float(np.mean([q['stage_mae_db'][j] for q in rows for j in range(5) if q['stage_mae_db'][j] is not None]))
            report['rows'][key]=dict(note=n,velocity=v,exploration_pair=(n,v) in exploration,
                reference_source=rr['source_file'],model=features,baseline=rr['model'],reference=rr['reference'],
                spectrum=spectrum,baseline_spectrum=base_spectrum,differences=e.compare(rr['reference'],features,rr['source_support_seconds']),
                early_error_db=score(spectrum),baseline_early_error_db=score(base_spectrum),
                error_change_db=score(spectrum)-score(base_spectrum),raw_sha256=e.digest(futures[n,v].result()))
            if (i+1)%12==0:
                print('VALIDATED',i+1,'/',len(pairs),flush=True)
                (a.work/'progress.json').write_text(json.dumps(dict(done=i+1,total=len(pairs))))
    for label,keep in (('all',lambda r:True),('validation_102',lambda r:not r['exploration_pair']),
                       ('bass',lambda r:r['note']<48),('middle',lambda r:48<=r['note']<78),('treble',lambda r:r['note']>=78)):
        rows=[r for r in report['rows'].values() if keep(r)];d=np.asarray([r['error_change_db'] for r in rows])
        report['summary'][label]=dict(count=len(rows),mean_baseline_error_db=float(np.mean([r['baseline_early_error_db'] for r in rows])),
            mean_candidate_error_db=float(np.mean([r['early_error_db'] for r in rows])),mean_change_db=float(d.mean()),
            improved_over_025_db=int((d<-.25).sum()),worse_over_025_db=int((d>.25).sum()),
            worst_change_db=float(d.max()),best_change_db=float(d.min()))
    a.output.write_text(json.dumps(report,indent=2))
    print(json.dumps(report['summary'],indent=2),flush=True)


if __name__=='__main__':main()
