"""Compare captured bridge-drive envelopes with recorded microphone pressure.

Different signals and units: an acoustic resonator can change their envelopes.
Required gain variation is a diagnosis of temporal mismatch, not a measurement
of real string losses or proof that the hammer or soundboard is at fault.
"""
import argparse
import importlib.util
import json
from pathlib import Path

import numpy as np
import soundfile as sf


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args(); a.output.mkdir(parents=True,exist_ok=True)
    spec=importlib.util.spec_from_file_location('bridge_body',Path(__file__).with_name('identify-concert-grand-body.py'))
    b=importlib.util.module_from_spec(spec); spec.loader.exec_module(b)
    reference=json.loads((a.input/'reference-manifest.json').read_text())
    rows={}
    for path in sorted((a.input/'capture').glob('*/bridge-drive.f32')):
        note,velocity=map(int,path.parent.name.split('-'))
        manifest=json.loads((path.parent/'manifest.json').read_text())
        assert manifest['binary_sha256']==b.SHA and manifest['rate']==b.RATE
        assert path.stat().st_size==manifest['frames']*16*4
        x=np.asarray(np.memmap(path,dtype='<f4',mode='r').reshape(-1,16),dtype=float)
        source=x.sum(axis=1)
        force=b.FIT.measure(np.repeat(source[:,None],2,axis=1),note)
        target_path=a.input/'reference'/f'{note}-{velocity}.wav'
        meta=json.loads(target_path.with_suffix('.json').read_text())
        assert meta['source']==reference['sources'][f'{note}:{velocity}']
        assert all(meta[k]==reference[k] for k in ('mapping_sha256','sfz_sha256','dll_sha256'))
        y,rate=sf.read(target_path,always_2d=True); assert rate==b.RATE
        target=b.FIT.measure(y,note)
        required=np.asarray(target['partials_db'])[1:,0]-np.asarray(force['partials_db'])[1:,0]
        rows[f'{note}:{velocity}']=dict(required_H1_gain_db=required.tolist(),
            late_minus_early_required_gain_db=float(required[-1]-required[0]),
            source_H1_decay_db=float(force['partials_db'][-1][0]-force['partials_db'][1][0]),
            reference_H1_decay_db=float(target['partials_db'][-1][0]-target['partials_db'][1][0]),
            source_H1_H2_impact_db=force['impact']['partials_db'][:2],
            reference_H1_H2_impact_db=target['impact']['partials_db'][:2],
            source_sha256=b.FIT.AUDIT.sha(path),reference_render_sha256=b.FIT.AUDIT.sha(target_path))
    assert len(rows)==105,len(rows)
    report=dict(policy=__doc__,binary_sha256=b.SHA,held_windows=[[.05,.2],[.2,.5],[.5,1.5]],
                rows=rows,median_absolute_gain_variation_db=float(np.median(
                    [abs(v['late_minus_early_required_gain_db']) for v in rows.values()])))
    (a.output/'source-diagnosis.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    for key in ('60:60','60:110','84:124'): print(key,rows[key],flush=True)


if __name__=='__main__': main()
