"""Measured SI nonlinear choir/contact component, before moving-body closure.

No modes are removed. A power-of-two cosine grid integrates the same finite
potential exactly. Fixed-bridge accuracy/work and CPU are separate gates.
"""
import argparse
import json
from pathlib import Path
import subprocess
import time
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_native_strings import fixture,write_fixture,replay
from concert_grand_wave_cache import digest


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--local',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--transverse',type=int,nargs='+',default=[12,24,48,96])
    parser.add_argument('--rates',type=int,nargs='+',default=[44100,48000])
    parser.add_argument('--buffers',type=int,default=64)
    parser.add_argument('--replay',action='store_true')
    parser.add_argument('--micro-block',type=int,default=1)
    parser.add_argument('--ramp',action='store_true')
    args=parser.parse_args()
    if args.local.exists() or args.report.exists() or not 8<=args.buffers<=2048 or not 1<=args.micro_block<=64:
        raise ValueError('new native string measurement paths and valid buffer count required')
    root=Path(__file__).resolve().parent.parent
    names=[Path(__file__).name,'concert-grand-native-strings.rs','concert_grand_native_strings.py',
        'concert_grand_shared_strings.py','concert_grand_shared_kernels.py','concert_grand_exponential_contact.py',
        'concert_grand_contact_study.py','concert_grand_coupled_contact.py']
    source={name:digest(root/'tools'/name) for name in names}
    args.local.mkdir(parents=True);args.report.parent.mkdir(parents=True,exist_ok=True)
    exe=args.local/'native-strings.exe'
    subprocess.run(['rustc','--edition=2021','-O','-C','target-cpu=native',str(root/'tools/concert-grand-native-strings.rs'),'-o',str(exe)],check=True)
    begin=time.perf_counter()
    record=dict(stage='native fixed-bridge nonlinear choir measured',source_sha256=source,
        executable_path=str(exe.resolve()),executable_sha256=digest(exe),cases=[],
        fixed_bridge=True,moving_body_attached=False,pressure_included=False,pedals_included=False,
        nonlinear_micro_block=args.micro_block,work_checked_at_nonlinear_block_boundaries=True,ramped_nonlinearity=args.ramp,
        full_piano_realtime=False,global_physics_approved=False,factory_updated=False)
    def save():
        record['seconds']=time.perf_counter()-begin
        args.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    save()
    with threadpool_limits(limits=1):
        for nt in args.transverse:
            for rate in args.rates:
                print('native string component',nt,rate,flush=True)
                f=fixture(transverse=nt,rate=rate,duration=.006)
                path=args.local/f'choir-{nt}-{rate}.bin';write_fixture(path,f)
                case=dict(transverse=nt,axial=2*nt,nodes=f['nodes'],rate_hz=rate,note=60,velocity=84,
                    coefficient_path=str(path.resolve()),coefficient_sha256=digest(path),mechanical_coordinates=len(f['q']))
                try:
                    mode=['verify-ramp',str(args.micro_block)] if args.ramp else (['verify'] if args.micro_block==1 else ['verify-block',str(args.micro_block)])
                    native=json.loads(subprocess.check_output([str(exe),str(path),*mode],text=True,stderr=subprocess.STDOUT))
                    case.update(mechanical_work_relative_max=native['work_relative_max'],felt_heat_j=native['heat_j'],
                        force_peak_n=native['force_peak_n'],maximum_iterations=native['maximum_iterations'],
                        mechanical_work_1e9_gate_passed=native['work_relative_max']<=1e-9,
                        certified_flight_blocks=native['certified_flight_blocks'])
                    if args.micro_block>1:
                        baseline=json.loads(subprocess.check_output([str(exe),str(path),'verify'],text=True,stderr=subprocess.STDOUT))
                        omega=np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
                        expected=np.r_[omega*np.asarray(baseline['q'])[:-1],baseline['v']]
                        actual=np.r_[omega*np.asarray(native['q'])[:-1],native['v']]
                        state=float(np.linalg.norm(actual-expected)/np.linalg.norm(expected))
                        force=float(np.linalg.norm(np.asarray(native['force_trace'])-baseline['force_trace'])/np.linalg.norm(baseline['force_trace']))
                        case.update(blocked_state_relative=state,blocked_force_relative=force,
                            blocked_accuracy_1percent_gate_passed=state<=.01 and force<=.01,
                            nonlinear_outer_iterations=native['maximum_outer_iterations'])
                    if args.replay:
                        reference=replay(f)
                        omega=np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
                        exact=np.r_[omega*reference['q'][:-1],reference['v']]
                        measured=np.r_[omega*np.asarray(native['q'])[:-1],native['v']]
                        state=float(np.linalg.norm(measured-exact)/np.linalg.norm(exact))
                        force=float(np.linalg.norm(np.asarray(native['force_trace'])-reference['force_trace'])/np.linalg.norm(reference['force_trace']))
                        case.update(native_python_state_relative=state,native_python_force_relative=force,
                            native_python_parity_1e8_gate_passed=state<=1e-8 and force<=1e-8,
                            python_mechanical_work_relative=reference['work_relative'])
                    mode=['bench-ramp',str(args.buffers),str(args.micro_block)] if args.ramp else (['bench',str(args.buffers)] if args.micro_block==1 else ['bench-block',str(args.buffers),str(args.micro_block)])
                    bench=json.loads(subprocess.check_output([str(exe),str(path),*mode],text=True,stderr=subprocess.STDOUT))
                    bench['isolated_full_buffer_budget_passed']=bench['max_ms']<=bench['deadline_ms'] and bench['allocations']==0
                    case['benchmark']=bench
                    print('native string result',nt,rate,'P99',bench['p99_ms'],'max',bench['max_ms'],'work',case['mechanical_work_relative_max'],flush=True)
                except subprocess.CalledProcessError as error:
                    case.update(completed=False,failure=error.output)
                    print('native string failure',nt,rate,error.output,flush=True)
                record['cases'].append(case);save()
    record['sources_unchanged']=all(digest(root/'tools'/name)==sha for name,sha in source.items())
    save()


if __name__=='__main__':main()
