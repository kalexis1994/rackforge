"""Audit axial elimination against frozen collocation and fine SI references."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_native_strings import fixture, write_fixture
from concert_grand_phase_forces import relative


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference-report',type=Path,required=True)
    parser.add_argument('--reference-ref',required=True)
    parser.add_argument('--local',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    args=parser.parse_args();root=Path(__file__).resolve().parents[1]
    if args.local.exists() or args.report.exists():raise ValueError('fresh paths required')
    path=args.reference_report.resolve().relative_to(root).as_posix()
    if subprocess.check_output(['git','show',args.reference_ref+':'+path],cwd=root)!=args.reference_report.read_bytes():
        raise ValueError('reference report changed')
    old=json.loads(args.reference_report.read_text())
    for name,sha in old['source_sha256'].items():
        frozen_source=subprocess.check_output(['git','show',args.reference_ref+':tools/'+name],cwd=root)
        if hashlib.sha256(frozen_source).hexdigest()!=sha:raise ValueError('frozen source mismatch: '+name)
    ancestor=old;chain=[]
    while 'teacher_cases' not in ancestor:
        link=Path(ancestor['reference_report_path'])
        if digest(link)!=ancestor['reference_report_sha256']:raise ValueError('ancestor report changed')
        chain.append(dict(path=str(link),sha256=digest(link)))
        ancestor=json.loads(link.read_text())
    for c in ancestor['teacher_cases']:
        for prefix in ['result','coefficient']:
            if digest(c[prefix+'_path'])!=c[prefix+'_sha256']:raise ValueError('fine reference changed')
    teacher=json.loads(Path(ancestor['teacher_cases'][-1]['result_path']).read_text())
    controls=[next(c for c in old['cases'] if c['label']==f'plain-{rate}') for rate in [128000,256000,512000]]
    for c in controls:
        for prefix in ['result','coefficient']:
            if digest(c[prefix+'_path'])!=c[prefix+'_sha256']:raise ValueError('control data changed')
    frozen=old['plain_executable_path']
    if digest(frozen)!=old['plain_executable_sha256']:raise ValueError('control executable changed')
    names=set(old['source_sha256'])|{Path(__file__).name}
    sources={name:digest(root/'tools'/name) for name in sorted(names)}
    args.local.mkdir(parents=True)
    plain=args.local/'plain.exe';profiled=args.local/'profile.exe'
    for exe,flags in [(plain,[]),(profiled,['--cfg','concert_phase_profile'])]:
        subprocess.run(['rustc','--edition=2021','-O','-C','target-cpu=native','--cfg','concert_phase_library',
                        *flags,str(root/'tools/concert-grand-phase-trajectory.rs'),'-o',str(exe)],check=True)
    spec=importlib.util.spec_from_file_location('wire_audit',root/'tools/audit-concert-grand-phase-wire-energy.py')
    audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)
    fixtures={};omegas={};inputs={};initials={}
    for c in controls:
        rate=c['rate_hz'];f=fixture(transverse=96,rate=rate,seed=83,duration=.006);f['launch_speeds'][:]=0.
        coefficient=args.local/f'input-{rate}.bin';write_fixture(coefficient,f)
        if digest(coefficient)!=c['coefficient_sha256']:raise ValueError('physical input changed')
        fixtures[rate]=f;inputs[rate]=coefficient
        omegas[rate]=np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
        initials[rate]=audit.wire_energies(f,f['q'],f['v'])
    record=dict(stage='exact axial nodal elimination inside collocation sweeps',source_sha256=sources,
        reference_ref=args.reference_ref,reference_report_path=str(args.reference_report.resolve()),
        reference_report_sha256=digest(args.reference_report),reference_sources_authenticated_from_git=True,
        reference_reintegrated=False,reference_chain=chain,teacher_cases=ancestor['teacher_cases'],
        reference_pair_passed=ancestor['reference_pair_passed'],
        reference_state_refinement_relative=ancestor['reference_state_refinement_relative'],
        plain_executable_path=str(plain.resolve()),plain_executable_sha256=digest(plain),
        profile_executable_path=str(profiled.resolve()),profile_executable_sha256=digest(profiled),
        frozen_executable_path=frozen,frozen_executable_sha256=digest(frozen),
        note=60,seed=83,transverse=96,axial=192,fidelity_checked_duration_s=.006,benchmark_duration_s=.008,
        longer_duration_fidelity_approved=False,full_piano_realtime=False,global_physics_approved=False,
        contact_included=False,pressure_included=False,pedals_included=False,physical_dissipation=False,
        factory_updated=False,cases=[],benchmarks=[])
    bench_controls={};plain_cases={}
    exact_keys=['q','v','work_relative_max','raw_local_work_relative_max','numerical_lambda_abs_max',
                'maximum_iterations','midpoint_residual_relative_max','initial_wire_energy_j','final_wire_energy_j']

    def run(label,exe,control,buffers=0,flags=()):
        rate=control['rate_hz'];order=control['order'];f=fixtures[rate]
        output=args.local/(label+'.json')
        if output.exists():raise ValueError('duplicate result path: '+label)
        mode=['bench','1',str(order),str(buffers),f'--buffer-microframes={rate//8000}'] if buffers else ['verify','1',str(order)]
        command=[str(exe),str(inputs[rate]),*mode,'--collocation',*flags]
        process=subprocess.run(command,capture_output=True,text=True,check=True)
        output.write_text(process.stdout,encoding='utf-8');data=json.loads(process.stdout)
        if buffers:
            if rate not in bench_controls:bench_controls[rate]=data
            base=bench_controls[rate]
        else:base=json.loads(Path(control['result_path']).read_text())
        def metric(d):return np.r_[omegas[rate]*np.asarray(d['q'])[:-1],d['v'][:-1]]
        change=relative(metric(data),metric(base))
        final=audit.wire_energies(f,data['q'],data['v'])
        wire_change=np.abs(final-initials[rate])/np.maximum(np.abs(initials[rate]),1e-30)
        entry={k:v for k,v in data.items() if k not in ['q','v']}
        entry.update(label=label,rate_hz=rate,order=order,command=command,executable_sha256=digest(exe),
            coefficient_path=str(inputs[rate].resolve()),coefficient_sha256=digest(inputs[rate]),
            result_path=str(output.resolve()),result_sha256=digest(output),
            frozen_control_state_relative=change,control_1e9_gate_passed=change<=1e-9,
            frozen_control_arrays_and_work_equal=all(data[k]==base[k] for k in exact_keys),
            independent_wire_energy_relative=wire_change.tolist(),maximum_wire_energy_relative=float(max(wire_change)),
            per_wire_1e9_gate_passed=bool(max(wire_change)<=1e-9),total_work_1e9_gate_passed=data['work_relative_max']<=1e-9,
            deadline_ms=.125,isolated_component_budget_passed=buffers>0 and data['max_ms']<=.125 and data['allocations']==0)
        if not buffers:
            error=relative(metric(data),metric(teacher))
            entry.update(state_relative=error,state_1percent_gate_passed=error<=.01,
                scoped_coast_state_and_energy_passed=bool(data['completed'] and error<=.01 and ancestor['reference_pair_passed']
                    and max(wire_change)<=1e-9 and data['work_relative_max']<=1e-9))
            if label.startswith('plain-'):plain_cases[rate]=data
            if '--audit-full-collocation-residual' in flags:
                entry['audited_arrays_and_metrics_equal']=all(data[k]==plain_cases[rate][k] for k in exact_keys)
                if not entry['audited_arrays_and_metrics_equal'] or data['audited_residual_relative_max']>1e-11:
                    raise ValueError('independent residual audit changed state or failed')
        record['benchmarks' if buffers else 'cases'].append(entry)
        record['sources_unchanged']=all(digest(root/'tools'/name)==sha for name,sha in sources.items())
        args.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')
        print(label,'mean',data['mean_ms'],'iterations',data['maximum_iterations'],'state change',change,
              'wire work',float(max(wire_change)),flush=True)
        if not data['completed'] or data['allocations']!=0 or change>1e-9 or max(wire_change)>1e-9 or data['work_relative_max']>1e-9:
            raise ValueError('state, energy or allocation gate failed')

    for c in controls:
        rate=c['rate_hz']
        run(f'plain-{rate}',plain,c)
        run(f'profile-{rate}',profiled,c)
        run(f'audit-{rate}',plain,c,flags=['--audit-full-collocation-residual'])
        run(f'simultaneous-{rate}',plain,c,flags=['--simultaneous-collocation'])
    for c in reversed(controls):
        rate=c['rate_hz']
        run(f'frozen-{rate}',frozen,c,64)
        run(f'simultaneous-benchmark-{rate}',plain,c,64,['--simultaneous-collocation'])
        run(f'eliminated-{rate}',plain,c,64)
        run(f'eliminated-repeat-{rate}',plain,c,64)
        run(f'frozen-repeat-{rate}',frozen,c,64)
        run(f'profile-simultaneous-{rate}',profiled,c,64,['--simultaneous-collocation'])
        run(f'profile-eliminated-{rate}',profiled,c,64)


if __name__=='__main__':
    with threadpool_limits(limits=1):main()
