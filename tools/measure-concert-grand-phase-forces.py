"""Measure exact quadratic free-carrier convolution, separately from dynamics.

The timed component repeats a fixed state. No contact, cubic force, nonlinear
iteration, conservation ledger or soundboard is included in its CPU budget.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_native_strings import fixture,write_fixture
from concert_grand_phase_forces import quadratic_control,response_metric,relative
from concert_grand_wave_cache import digest


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--measurement',type=Path,required=True)
    parser.add_argument('--local',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--buffers',type=int,default=512)
    parser.add_argument('--control-measurement',type=Path)
    parser.add_argument('--control-ref')
    args=parser.parse_args()
    if args.local.exists() or args.report.exists() or not 8<=args.buffers<=2048:
        raise ValueError('fresh paths and valid buffer count required')
    root=Path(__file__).resolve().parents[1]
    parent=json.loads(args.measurement.read_text(encoding='utf-8'))
    # Authenticate the unchanged physical fixture providers and previous stage.
    for name,sha in parent['source_sha256'].items():
        if digest(root/'tools'/name)!=sha:raise ValueError(f'parent source changed: {name}')
    if digest(Path(parent['executable_path']))!=parent['executable_sha256']:
        raise ValueError('parent executable changed')
    names=[Path(__file__).name,'concert-grand-phase-forces.rs','concert_grand_phase_forces.py',
        'tests/test_concert_phase_forces.py']
    sources=parent['source_sha256']|{name:digest(root/'tools'/name) for name in names}
    control=None
    if bool(args.control_measurement)!=bool(args.control_ref):
        raise ValueError('control measurement and its frozen Git ref must be supplied together')
    if args.control_measurement:
        control=json.loads(args.control_measurement.read_text(encoding='utf-8'))
        for name,sha in control['source_sha256'].items():
            frozen=subprocess.check_output(['git','show',f'{args.control_ref}:tools/{name}'],cwd=root)
            if hashlib.sha256(frozen).hexdigest()!=sha:
                raise ValueError(f'control source does not match its Git snapshot: {name}')
        for meta in control['executables'].values():
            if digest(Path(meta['path']))!=meta['sha256']:raise ValueError('control executable changed')
    args.local.mkdir(parents=True);args.report.parent.mkdir(parents=True,exist_ok=True)
    record=dict(stage='quadratic force convolution on free modal carriers',
        parent_path=str(args.measurement.resolve()),parent_sha256=digest(args.measurement),
        source_sha256=sources,transverse=96,axial=192,mechanical_coordinates=1153,
        rate_hz=128000,frames_per_force_block=16,seed=83,benchmark_fixed_state_repeated=True,
        trajectory='exact free carriers from the serialized q/v; no launches',
        force_operator='gradient of EA integral u_x*(v_x^2+w_x^2)/2 dx',
        cubic_force_included=False,nonlinear_step=False,contact_included=False,
        conservation_ledger_checked=False,full_piano_realtime=False,
        global_physics_approved=False,factory_updated=False,cases=[])
    begin=time.perf_counter()
    if control:
        record.update(control_path=str(args.control_measurement.resolve()),
            control_sha256=digest(args.control_measurement),control_ref=args.control_ref,
            control_sources_authenticated_from_git=True)
    def save():
        record['seconds']=time.perf_counter()-begin
        args.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    save()
    executables={}
    for backend in ['native','portable']:
        exe=args.local/f'phase-{backend}.exe'
        flags=['-C','target-cpu=native'] if backend=='native' else ['-C','target-cpu=generic']
        subprocess.run(['rustc','--edition=2021','-O',*flags,str(root/'tools/concert-grand-phase-forces.rs'),'-o',str(exe)],check=True)
        executables[backend]=dict(path=str(exe.resolve()),sha256=digest(exe),compiler_flags=flags)
    record['executables']=executables;save()
    with threadpool_limits(limits=1):
        for note in [48,60,72]:
            print('quadratic phase component',note,flush=True)
            f=fixture(note=note,transverse=96,rate=128000,seed=83,duration=.006)
            path=args.local/f'choir-{note}.bin';write_fixture(path,f)
            coarse=quadratic_control(f,16,order=1024)
            fine=quadratic_control(f,16,order=2048)
            refine=relative(coarse['exact']['metric'],fine['exact']['metric'])
            case=dict(note=note,coefficient_path=str(path.resolve()),coefficient_sha256=digest(path),
                gauss_time_orders=[1024,2048],spatial_cosine_nodes=f['nodes'],
                gauss_response_refinement_relative=refine,
                affine_endpoint_force_response_relative=relative(fine['affine']['metric'],fine['exact']['metric']),
                largest_free_carrier_cycles_per_block=float(max(fine['omega'])*16/128000/(2*np.pi)),
                reference_response_norm=float(np.linalg.norm(fine['exact']['metric'])),
                reference_response_to_initial_state=float(np.linalg.norm(fine['exact']['metric'])/
                    np.linalg.norm(np.r_[fine['omega']*f['q'][:-1],f['v'][:-1]])),
                backends={})
            for backend,meta in executables.items():
                flags=['--portable'] if backend=='portable' else []
                output=subprocess.check_output([meta['path'],str(path),'16',str(args.buffers),*flags],text=True)
                native=json.loads(output);result_path=args.local/f'{note}-{backend}.json'
                result_path.write_text(output,encoding='utf-8')
                actual=response_metric(native,fine['omega'])
                error=relative(actual,fine['exact']['metric'])
                endpoint_error=relative(np.asarray(native['endpoint_quadratic_gradient']),fine['endpoint_quadratic_gradient'])
                entry=dict(result_path=str(result_path.resolve()),result_sha256=digest(result_path),
                    response_relative=error,endpoint_gradient_relative=endpoint_error,
                    component_accuracy_1e9_gate_passed=error<=1e-9 and refine<=1e-10 and endpoint_error<=1e-10,
                    prepared_terms=native['prepared_terms'],prepared_bytes=native['prepared_bytes'],
                    callback_table_bytes=native['callback_table_bytes'],
                    prepared_index_bits=native['prepared_index_bits'],
                    response_alignment_bytes=native['response_alignment_bytes'],grouped_outputs=native['grouped_outputs'],
                    simd=native['simd'],runtime_flags=flags,
                    preparation_ms=native['preparation_ms'],
                    benchmark={key:native[key] for key in ['buffers','frames','mean_ms','p99_ms','max_ms','allocations']})
                entry['benchmark'].update(frames_per_buffer=256,deadline_ms=2.,
                    isolated_component_budget_passed=native['max_ms']<=2. and native['allocations']==0)
                if control:
                    old_case=next(c for c in control['cases'] if c['note']==note)
                    if digest(Path(old_case['coefficient_path']))!=old_case['coefficient_sha256'] or digest(path)!=old_case['coefficient_sha256']:
                        raise ValueError('control physical input changed')
                    old=old_case['backends'][backend];old_path=Path(old['result_path'])
                    if digest(old_path)!=old['result_sha256']:raise ValueError('control result changed')
                    old_result=json.loads(old_path.read_text(encoding='utf-8'))
                    change=relative(actual,response_metric(old_result,fine['omega']))
                    entry.update(control_response_relative=change,
                        control_response_arrays_equal=native['dq']==old_result['dq'] and native['dv']==old_result['dv'],
                        control_same_term_count=native['prepared_terms']==old['prepared_terms'],
                        control_mean_speedup=old['benchmark']['mean_ms']/native['mean_ms'],
                        control_response_1e13_gate_passed=change<=1e-13)
                    entry['component_accuracy_1e9_gate_passed'] &= change<=1e-13 and entry['control_same_term_count']
                case['backends'][backend]=entry
                print(note,backend,'response',error,'mean/P99/max ms',native['mean_ms'],native['p99_ms'],native['max_ms'],flush=True)
            a=json.loads(Path(case['backends']['native']['result_path']).read_text())
            b=json.loads(Path(case['backends']['portable']['result_path']).read_text())
            case['native_portable_response_relative']=relative(response_metric(a,fine['omega']),response_metric(b,fine['omega']))
            case['native_portable_mean_speedup']=case['backends']['portable']['benchmark']['mean_ms']/case['backends']['native']['benchmark']['mean_ms']
            record['cases'].append(case);save()
    record['sources_unchanged']=all(digest(root/'tools'/name)==sha for name,sha in sources.items())
    record['all_component_accuracy_gates_passed']=all(case['backends'][backend]['component_accuracy_1e9_gate_passed']
        for case in record['cases'] for backend in executables)
    record['parent_unchanged']=digest(args.measurement)==record['parent_sha256']
    if control:record['control_unchanged']=digest(args.control_measurement)==record['control_sha256']
    save()


if __name__=='__main__':main()
