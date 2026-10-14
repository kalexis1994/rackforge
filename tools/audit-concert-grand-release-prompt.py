"""Attribute middle-register decay to the published empirical prompt table.

The table-off run is diagnostic. It is not selected or installed as a voicing.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import time
import numpy as np


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--baseline',type=Path,required=True);p.add_argument('--work',type=Path,required=True)
    p.add_argument('--report',type=Path,required=True);a=p.parse_args()
    if a.work.exists() or a.report.exists():raise ValueError('fresh paths required')
    root=Path(__file__).resolve().parents[1]
    spec=importlib.util.spec_from_file_location('release_body',root/'tools/measure-concert-grand-release-body.py')
    body=importlib.util.module_from_spec(spec);spec.loader.exec_module(body)
    d=json.loads(a.baseline.read_text(encoding='utf-8'))
    assert all(body.digest(root/s)==h for s,h in d['source_sha256'].items())
    probe=Path(d['cases'][0]['command'][0]);assert body.digest(probe)==d['probe_sha256']
    a.work.mkdir(parents=True);tuning=a.work/'table-off.txt'
    tuning.write_text('PROMPT_MEASURED_POWER = 0\n',encoding='utf-8',newline='\n')
    report=dict(scope=__doc__,baseline_path=str(a.baseline.resolve()),baseline_sha256=body.digest(a.baseline),
        source_sha256=body.digest(__file__),tuning_sha256=body.digest(tuning),coefficient_change={'PROMPT_MEASURED_POWER':[1.,0.]},
        candidate_selected=False,production_DSP_changed=False,output_gain=1.,per_note_normalization=False,cases=[])
    begin=time.perf_counter()
    for row in d['cases']:
        note,velocity=row['note'],row['velocity'];raw=a.work/f'{note}-{velocity}.f32'
        command=row['command'].copy();command[2]=str(raw.resolve());command.append(str(tuning.resolve()))
        native=json.loads(subprocess.check_output(command,text=True))
        x=np.fromfile(raw,dtype='<f4').reshape(-1,2).astype(float)
        original=Path(row['release_raw_path']);assert body.digest(original)==row['release_raw_sha256']
        y=np.fromfile(original,dtype='<f4').reshape(-1,2).astype(float)
        assert x.shape==y.shape and np.isfinite(x).all() and abs(x).max()<1.
        result=body.measure(x,note);key='fall_from_100_200ms_to_500_1400ms_db'
        before=row['release']['partials'][0][key];after=result['partials'][0][key];target=row['reference']['partials'][0][key]
        size=round(.1*body.RATE)
        entry=dict(note=note,velocity=velocity,command=command,native=native,raw_path=str(raw.resolve()),raw_sha256=body.digest(raw),
            measured=result,H1_fall_baseline_db=before,H1_fall_table_off_db=after,H1_fall_reference_db=target,
            absolute_fall_error_baseline_db=abs(before-target),absolute_fall_error_table_off_db=abs(after-target),
            first_100ms_wave_relative_rms=float(np.linalg.norm(x[:size]-y[:size])/np.linalg.norm(y[:size])))
        report['cases'].append(entry)
        print(f'{note}:{velocity}',*[round(v,2) for v in (before,after,target)],flush=True)
    report['total_seconds']=time.perf_counter()-begin
    assert all(body.digest(root/s)==h for s,h in d['source_sha256'].items())
    a.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n',encoding='utf-8',newline='\n')


if __name__=='__main__':main()
