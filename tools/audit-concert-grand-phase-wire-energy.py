"""Independent final per-wire energy audit of a string-only coast report."""
import argparse
import json
from pathlib import Path
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_native_strings import fixture
from concert_grand_wave_cache import digest


def wire_energies(f,q,v):
    nt=f['transverse'];nl=f['axial'];width=nl+2*nt
    length,mu,young,density=f['parameters'][:4];ea=young*mu/density;root=np.sqrt(mu*length/2)
    x=(np.arange(f['nodes'])+.5)*length/f['nodes']
    du=np.cos(x[:,None]*np.pi*np.arange(1,nl+1)/length)*(np.pi*np.arange(1,nl+1)/length/root)
    dt=np.cos(x[:,None]*np.pi*np.arange(1,nt+1)/length)*(np.pi*np.arange(1,nt+1)/length/root)
    omega2=np.diag(f['system'].string.K)[f['keep'][:-1]]
    result=[]
    for i in range(3):
        section=slice(i*width,(i+1)*width);a=np.asarray(q)[section];b=np.asarray(v)[section]
        u=du @ a[:nl];vertical=dt @ a[nl:nl+nt];horizontal=dt @ a[nl+nt:]
        eta=.5*(vertical**2+horizontal**2)
        result.append(.5*np.sum(b*b+omega2[section]*a*a)+ea*length*np.mean((u+.5*eta)*eta))
    return np.asarray(result)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--measurement',type=Path,required=True);parser.add_argument('--report',type=Path,required=True)
    args=parser.parse_args()
    if args.report.exists():raise ValueError('fresh audit path required')
    root=Path(__file__).resolve().parents[1];parent=json.loads(args.measurement.read_text())
    for name,sha in parent['source_sha256'].items():
        if digest(root/'tools'/name)!=sha:raise ValueError(f'measured source changed: {name}')
    f=fixture(transverse=96,seed=83,duration=.006);f['launch_speeds'][:]=0.
    record=dict(parent_path=str(args.measurement.resolve()),parent_sha256=digest(args.measurement),
        source_sha256=parent['source_sha256']|{Path(__file__).name:digest(Path(__file__))},
        per_wire_energy_tolerance=1e-9,contact_included=False,global_physics_approved=False,cases=[])
    with threadpool_limits(limits=1):
        initial=wire_energies(f,f['q'],f['v']);record['initial_wire_energy_j']=initial.tolist()
        entries=parent.get('teacher_cases',[])+parent['cases']
        for entry in entries:
            if not entry['completed'] or 'result_path' not in entry:continue
            path=Path(entry['result_path'])
            if digest(path)!=entry['result_sha256']:raise ValueError('measured result changed')
            data=json.loads(path.read_text());end=wire_energies(f,data['q'],data['v'])
            relative=np.abs(end-initial)/np.maximum(np.abs(initial),1e-30)
            record['cases'].append(dict(label=entry['label'],result_path=str(path),result_sha256=digest(path),
                final_wire_energy_j=end.tolist(),wire_energy_relative=relative.tolist(),
                maximum_wire_energy_relative=float(max(relative)),per_wire_1e9_gate_passed=bool(max(relative)<=1e-9)))
    record['sources_unchanged']=all(digest(root/'tools'/name)==sha for name,sha in record['source_sha256'].items())
    args.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')


if __name__=='__main__':main()
