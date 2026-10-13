"""Measure independent numerical energy roots, reusing authenticated teachers."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import time
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_native_strings import fixture,write_fixture
from concert_grand_phase_forces import relative
from concert_grand_wave_cache import digest


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference-report',type=Path,required=True);parser.add_argument('--reference-ref',required=True)
    parser.add_argument('--local',type=Path,required=True);parser.add_argument('--report',type=Path,required=True)
    args=parser.parse_args()
    if args.local.exists() or args.report.exists():raise ValueError('fresh paths required')
    root=Path(__file__).resolve().parents[1];old=json.loads(args.reference_report.read_text())
    for name,sha in old['source_sha256'].items():
        frozen=subprocess.check_output(['git','show',args.reference_ref+':tools/'+name],cwd=root)
        if hashlib.sha256(frozen).hexdigest()!=sha:raise ValueError(f'frozen source mismatch: {name}')
    frozen_report=subprocess.check_output(['git','show',args.reference_ref+':'+args.reference_report.resolve().relative_to(root).as_posix()],cwd=root)
    if frozen_report!=args.reference_report.read_bytes():raise ValueError('reference report changed')
    for entry in old['teacher_cases']:
        if digest(Path(entry['result_path']))!=entry['result_sha256'] or digest(Path(entry['coefficient_path']))!=entry['coefficient_sha256']:
            raise ValueError('reference data changed')
    names=list(old['source_sha256'])+[Path(__file__).name,'audit-concert-grand-phase-wire-energy.py']
    source={name:digest(root/'tools'/name) for name in names}
    spec=importlib.util.spec_from_file_location('wire_audit',root/'tools/audit-concert-grand-phase-wire-energy.py')
    audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)
    args.local.mkdir(parents=True);args.report.parent.mkdir(parents=True,exist_ok=True)
    exe=args.local/'trajectory.exe'
    subprocess.run(['rustc','--edition=2021','-O','-C','target-cpu=native','--cfg','concert_phase_library',
        str(root/'tools/concert-grand-phase-trajectory.rs'),'-o',str(exe)],check=True)
    record=dict(stage='independent wire energy roots in phase trajectory',source_sha256=source,
        reference_report_path=str(args.reference_report.resolve()),reference_report_sha256=digest(args.reference_report),
        reference_ref=args.reference_ref,reference_sources_authenticated_from_git=True,reference_reintegrated=False,
        executable_path=str(exe.resolve()),executable_sha256=digest(exe),teacher_cases=old['teacher_cases'],
        reference_state_refinement_relative=old['reference_state_refinement_relative'],reference_pair_passed=old['reference_pair_passed'],
        transverse=96,axial=192,note=60,seed=83,duration_s=.006,contact_included=False,
        pressure_included=False,pedals_included=False,full_piano_realtime=False,global_physics_approved=False,
        factory_updated=False,numerical_energy_constraint=True,physical_dissipation=False,heat_j=0.,cases=[])
    started=time.perf_counter()
    def save():
        record['seconds']=time.perf_counter()-started
        args.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    def metric(data,omega):return np.r_[omega*np.asarray(data['q'])[:-1],data['v']]
    def run(label,path,mode,f,omega,teacher=None):
        command=[str(exe),str(path),*map(str,mode)];begin=time.perf_counter()
        process=subprocess.run(command,capture_output=True,text=True)
        entry=dict(label=label,command=command,elapsed_s=time.perf_counter()-begin,
            coefficient_path=str(path.resolve()),coefficient_sha256=digest(path))
        if process.returncode:
            entry.update(completed=False,returncode=process.returncode,failure=process.stderr);return entry,None
        data=json.loads(process.stdout);output=args.local/(label+'.json');output.write_text(process.stdout,encoding='utf-8')
        entry.update(result_path=str(output.resolve()),result_sha256=digest(output),completed=data['completed'])
        for key in ['failure','work_relative_max','raw_local_work_relative_max','numerical_lambda_abs_max',
            'midpoint_residual_relative_max','maximum_iterations','allocations','known_prepared_bytes',
            'per_wire_energy_constraint','forced_axial_waveform','numerical_energy_constraint','seconds']:
            entry[key]=data[key]
        start=audit.wire_energies(f,f['q'],f['v']);end=audit.wire_energies(f,data['q'],data['v'])
        change=np.abs(end-start)/np.maximum(np.abs(start),1e-30)
        entry.update(initial_wire_energy_j=start.tolist(),final_wire_energy_j=end.tolist(),
            wire_energy_relative=change.tolist(),maximum_wire_energy_relative=float(max(change)),
            per_wire_1e9_gate_passed=bool(entry['completed'] and max(change)<=1e-9),
            total_work_1e9_gate_passed=entry['completed'] and data['work_relative_max']<=1e-9)
        if teacher is not None and entry['completed']:
            error=relative(metric(data,omega),metric(teacher,omega))
            entry.update(state_relative=error,state_1percent_gate_passed=error<=.01,
                scoped_coast_state_and_energy_passed=record['reference_pair_passed'] and error<=.01
                    and entry['per_wire_1e9_gate_passed'] and entry['total_work_1e9_gate_passed'])
        return entry,data
    save()
    with threadpool_limits(limits=1):
        teacher=json.loads(Path(old['teacher_cases'][-1]['result_path']).read_text())
        f=fixture(transverse=96,rate=2048000,seed=83,duration=.006);f['launch_speeds'][:]=0.
        omega=np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
        path=args.local/'coast-input.bin';write_fixture(path,f)
        frozen=next(c for c in old['cases'] if c['label']=='forced2048-N1')
        if digest(path)!=frozen['coefficient_sha256']:raise ValueError('physical input changed')
        for label,flags in [('independent-wire-roots',[]),('shared-wire-control',['--shared-energy-root'])]:
            entry,data=run(label,path,['verify',1,8,*flags],f,omega,teacher)
            if flags and data is not None:
                previous=json.loads(Path(frozen['result_path']).read_text())
                entry['frozen_shared_arrays_and_work_equal']=all(data[key]==previous[key] for key in ['q','v','work_relative_max','numerical_lambda_abs_max'])
                if not entry['frozen_shared_arrays_and_work_equal']:raise ValueError('shared control changed')
            record['cases'].append(entry);save()
            print(label,'state',entry.get('state_relative'),'wire work',entry['maximum_wire_energy_relative'],flush=True)
        entry,data=run('wire-root-benchmark',path,['bench',1,8,8],f,omega)
        if data is not None:
            entry['benchmark']={key:data[key] for key in ['buffers','mean_ms','p99_ms','max_ms','allocations']}
            entry['benchmark'].update(rate_hz=2048000,microframes_per_buffer=256,deadline_ms=.125,
                physical_duration_s=.001,isolated_component_budget_passed=entry['completed'] and data['buffers']==8
                    and data['max_ms']<=.125 and data['allocations']==0)
        record['benchmark']=entry
    record['sources_unchanged']=all(digest(root/'tools'/name)==sha for name,sha in source.items())
    record['reference_report_unchanged']=digest(args.reference_report)==record['reference_report_sha256']
    save()


if __name__=='__main__':main()
