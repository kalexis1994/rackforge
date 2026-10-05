"""Compare two completed identical-geometry arithmetic preflights."""
import argparse,json,hashlib
from pathlib import Path
import numpy as np


def digest(path):
    with Path(path).open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for n in ['before','after','report']:p.add_argument('--'+n,type=Path,required=True)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    old=json.loads(a.before.read_text());new=json.loads(a.after.read_text())
    for name in ['cells','notes','frequencies_hz','inputs_sha256']:
        if old[name]!=new[name]:raise ValueError('same geometry, forces and frequencies required')
    paths=[Path(r['transfer_path']) for r in [old,new]]
    for path,r in zip(paths,[old,new]):
        if digest(path)!=r['transfer_sha256']:raise ValueError('changed transfer input')
    with np.load(paths[0]) as before,np.load(paths[1]) as after:
        errors={}
        for name in ['mobility_full','pressure_full','mobility_projected','pressure_projected']:
            b=before[name];d=after[name]-b
            errors[name]=float(np.max(np.linalg.norm(d,axis=1)/np.maximum(np.linalg.norm(b,axis=1),1e-40)))
    report=dict(parent_reports_sha256={str(x):digest(x) for x in [a.before,a.after]},
        source_sha256=digest(Path(__file__)),transfer_relative_by_input=errors,criterion=1e-10,
        passed=max(errors.values())<1e-10,before_seconds=old['seconds'],after_seconds=new['seconds'],
        observed_ratio=old['seconds']/new['seconds'],before_peak_bytes=old['peak_working_set_bytes'],after_peak_bytes=new['peak_working_set_bytes'],
        scope='One standalone before/after run at identical three-point preflight. Not repeated timing statistics, runtime audio or different cell resolutions.')
    a.report.parent.mkdir(parents=True,exist_ok=True);a.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    print(json.dumps(report),flush=True)

if __name__=='__main__':main()
