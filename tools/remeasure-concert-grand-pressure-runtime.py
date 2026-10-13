"""Retime frozen body/pressure bundles, checking portable/SIMD/Python parity.

No fit, new coefficients, changed audio gain, or validation selection.
"""
import argparse
import json
from pathlib import Path
import subprocess
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_realtime_pressure import PressureRuntime
from concert_grand_wave_cache import digest


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['parent-report','body-report','local','report']:
        parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args()
    if args.report.exists() or args.local.exists():
        raise ValueError('preserve previous measurements')
    parent,body=[json.loads(p.read_text()) for p in [args.parent_report,args.body_report]]
    if parent['body_report_sha256']!=digest(args.body_report) or digest(body['model_path'])!=body['model_sha256']:
        raise ValueError('frozen body changed')
    args.local.mkdir(parents=True)
    root=Path(__file__).resolve().parent.parent
    exe=args.local/'pressure-body.exe'
    subprocess.run(['rustc','--edition=2021','-O','-C','target-cpu=native',str(root/'tools/concert-grand-pressure-body.rs'),'-o',str(exe)],check=True)
    rows=[]
    with np.load(body['model_path']) as b,threadpool_limits(limits=1):
        for row in parent['rates']:
            bundle=row['coefficient_path']
            observer=row['fitted_observer']
            if digest(bundle)!=row['coefficient_sha256'] or digest(observer['model_path'])!=observer['model_sha256']:
                raise ValueError('frozen pressure artifact changed')
            with np.load(observer['model_path']) as o:
                runtime=PressureRuntime(b['omega'],b['damping'],b['ports'],b['mapping'],o['delays'],np.zeros((*o['delays'].shape,len(b['omega']))),row['rate_hz'])
                runtime.set_state_readout(o['state_readout'],o['force_readout'])
            outputs=[]; work=checksum=0.
            for frame in range(1200):
                force=np.array([((frame*17+port*13)%97-48)/48 if frame<400 else 0. for port in range(len(b['mapping']))])
                v,p=runtime.process(force); outputs.append(p)
                work+=runtime.step.dt*force @ v; checksum+=np.arange(1,len(v)+1) @ v
            verifications=[];benches=[]
            for kernel,extra in [('simd',[]),('scalar',['scalar'])]:
                native=json.loads(subprocess.check_output([str(exe),bundle,'verify',*extra],text=True))
                error=float(np.linalg.norm(np.asarray(outputs)-native['pressure'])/np.linalg.norm(outputs))
                if not np.isfinite(error) or error>1e-8:
                    raise ValueError('native microphone replay mismatch')
                np.testing.assert_allclose(native['real'],runtime.state.real,rtol=1e-8,atol=1e-12)
                np.testing.assert_allclose(native['imag'],runtime.state.imag,rtol=1e-8,atol=1e-12)
                np.testing.assert_allclose([native['work_j'],native['checksum']],[work,checksum],rtol=1e-8,atol=1e-12)
                if native['held_port_identity_max_relative']>1e-8:
                    raise ValueError('held port closure identity failed')
                verifications.append(dict(kernel=kernel,pressure_relative=error,state_ports_and_work_passed=True,
                    held_port_identity_max_relative=native['held_port_identity_max_relative']))
                bench=json.loads(subprocess.check_output([str(exe),bundle,'bench',*extra],text=True))
                bench.update(kernel=kernel,p99_deadline_fraction=bench['p99_ms']/bench['deadline_ms'],
                    body_and_pressure_20percent_budget_passed=bench['p99_ms']<=.2*bench['deadline_ms'] and bench['max_ms']<=bench['deadline_ms'] and bench['allocations']==0)
                benches.append(bench)
            # A second SIMD measurement follows the portable run; both remain.
            bench=json.loads(subprocess.check_output([str(exe),bundle,'bench'],text=True))
            bench.update(kernel='simd-repeat',p99_deadline_fraction=bench['p99_ms']/bench['deadline_ms'],
                body_and_pressure_20percent_budget_passed=bench['p99_ms']<=.2*bench['deadline_ms'] and bench['max_ms']<=bench['deadline_ms'] and bench['allocations']==0)
            benches.append(bench)
            rows.append(dict(rate_hz=row['rate_hz'],coefficient_sha256=row['coefficient_sha256'],verification=verifications,benchmarks=benches))
            print(row['rate_hz'],[(x['kernel'],x['p99_ms'],x['max_ms']) for x in benches],flush=True)
    result=dict(stage='frozen body and pressure portable SIMD replay and timing',
        parent_report_sha256=digest(args.parent_report),body_report_sha256=digest(args.body_report),
        source_sha256={name:digest(root/'tools'/name) for name in [Path(__file__).name,'concert-grand-pressure-body.rs','concert_grand_realtime_pressure.py']},
        native_executable_sha256=digest(exe),rates=rows,
        coefficients_unchanged=True,pressure_included=True,strings_included=False,
        full_piano_realtime=False,global_physics_approved=False,factory_updated=False,
        scope='Host-specific body/pressure benchmark, 88 force/velocity ports, 256 frames, 2048 buffers. No host callback, live strings/contact/pedals or whole-piano timing.')
    args.report.write_text(json.dumps(result,indent=2,allow_nan=False)+'\n',encoding='utf-8')


if __name__=='__main__':
    main()
