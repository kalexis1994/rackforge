"""Bounded C4 modal-reduction feasibility study; mechanical probes are not audio pressure."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import time
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_native_strings import write_fixture, fixture
from concert_grand_modal_viability import project, pulse_fixture, omega, state_errors, common_signal, signal_errors


def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference-report',type=Path,required=True)
    parser.add_argument('--reference-ref',required=True)
    parser.add_argument('--local',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--resume-report',type=Path)
    parser.add_argument('--resume-producer',type=Path)
    args=parser.parse_args();root=Path(__file__).resolve().parents[1]
    if args.report.exists() or (args.local.exists() and not args.resume_report):raise ValueError('fresh paths or explicit resume required')
    if bool(args.resume_report)!=bool(args.resume_producer):raise ValueError('resume requires report and original producer')
    relative=args.reference_report.resolve().relative_to(root).as_posix()
    if subprocess.check_output(['git','show',args.reference_ref+':'+relative],cwd=root)!=args.reference_report.read_bytes():
        raise ValueError('frozen report changed')
    old=json.loads(args.reference_report.read_text(encoding='utf-8'))
    for name,sha in old['source_sha256'].items():
        if hashlib.sha256(subprocess.check_output(['git','show',args.reference_ref+':tools/'+name],cwd=root)).hexdigest()!=sha:
            raise ValueError('frozen source changed: '+name)
    names=set(old['source_sha256'])|{'concert-grand-observation.rs','concert_grand_modal_viability.py',
          'tests/test_concert_modal_viability.py',Path(__file__).name}
    sources={name:digest(root/'tools'/name) for name in sorted(names)}
    args.local.mkdir(parents=True,exist_ok=bool(args.resume_report))
    exe=args.local/'trajectory.exe'
    cache={};checkpoint=None
    if args.resume_report:
        checkpoint=json.loads(args.resume_report.read_text(encoding='utf-8'))
        for name,sha in checkpoint['source_sha256'].items():
            actual=digest(args.resume_producer) if name==Path(__file__).name else digest(root/'tools'/name)
            if actual!=sha:raise ValueError('resume source mismatch: '+name)
        if digest(exe)!=checkpoint['executable_sha256']:raise ValueError('resume executable changed')
        cache={c['label']:c for c in checkpoint['runs']}
    else:
        subprocess.run(['rustc','--edition=2021','-O','-C','target-cpu=native','--cfg','concert_phase_library',
                        str(root/'tools/concert-grand-phase-trajectory.rs'),'-o',str(exe)],check=True)
    spec=importlib.util.spec_from_file_location('wire_audit',root/'tools/audit-concert-grand-phase-wire-energy.py')
    audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)
    record=dict(stage='modal truncation and internal clock feasibility',source_sha256=sources,
        reference_ref=args.reference_ref,reference_report_path=str(args.reference_report.resolve()),
        reference_report_sha256=digest(args.reference_report),executable_path=str(exe.resolve()),executable_sha256=digest(exe),
        duration_s=.096,note=60,physical_dissipation=False,contact_simulated=False,soundboard_included=False,
        acoustic_pressure=False,perceptual_quality_approved=False,full_piano_realtime=False,factory_updated=False,
        mechanical_screening_tolerance=.05,numerical_refinement_tolerance=.001,deadline_ms=.125,
        polyphony_measured=False,runs=[],experiments=[])
    if checkpoint:
        record['resumed_checkpoint']=dict(path=str(args.resume_report.resolve()),sha256=digest(args.resume_report),
            producer_source=args.resume_producer.read_text(encoding='utf-8'),producer_sha256=digest(args.resume_producer),
            original_source_sha256=checkpoint['source_sha256'],reason='checkpoint write interrupted; native results preserved')
    raw={};traces={}

    def save(final=False):
        record['sources_unchanged']=all(digest(root/'tools'/n)==s for n,s in sources.items())
        payload=json.dumps(record,indent=2,allow_nan=False)+'\n'
        # Keep frequent updates out of the cloud-synced final report file.
        target=args.report if final else args.local/'progress.json'
        temporary=target.with_suffix('.pending')
        for attempt in range(10):
            try:
                temporary.write_text(payload,encoding='utf-8');temporary.replace(target)
                return
            except OSError:
                if attempt==9:raise
                time.sleep(.1*(attempt+1))

    def run(label,f,order,trace=False,bench=False):
        if label in cache:
            entry=cache[label].copy();command=entry['command'];rate=int(f['parameters'][4])
            expected_mode=['bench','1',str(order),'64',f'--buffer-microframes={rate//8000}'] if bench else ['verify','1',str(order)]
            if command[2:2+len(expected_mode)]!=expected_mode or entry['diagnostic_trace']!=trace:
                raise ValueError('cached command changed')
            for prefix in ['coefficient','result']:
                if digest(entry[prefix+'_path'])!=entry[prefix+'_sha256']:raise ValueError('cached data changed')
            with tempfile.TemporaryDirectory() as temporary:
                check=Path(temporary)/'input.bin';write_fixture(check,f)
                if digest(check)!=entry['coefficient_sha256']:raise ValueError('cached physical input changed')
            data=json.loads(Path(entry['result_path']).read_text(encoding='utf-8'));raw[label]=data
            if trace:
                if digest(entry['observation_path'])!=entry['observation_sha256']:raise ValueError('cached observation changed')
                traces[label]=common_signal(json.loads(Path(entry['observation_path']).read_text(encoding='utf-8')),rate)
            entry['reused_from_checkpoint']=True
            record['runs'].append(entry);save()
            print(label,'authenticated cached result',flush=True)
            return data
        coefficient=args.local/(label+'.bin');output=args.local/(label+'.json');observation=args.local/(label+'-observation.json')
        if coefficient.exists() or output.exists():
            if not checkpoint:raise ValueError('duplicate run label')
            coefficient=args.local/(label+'-resumed.bin');output=args.local/(label+'-resumed.json');observation=args.local/(label+'-resumed-observation.json')
            if coefficient.exists() or output.exists():raise ValueError('duplicate resumed label')
        write_fixture(coefficient,f);rate=int(f['parameters'][4])
        mode=['bench','1',str(order),'64',f'--buffer-microframes={rate//8000}'] if bench else ['verify','1',str(order)]
        command=[str(exe),str(coefficient),*mode,'--collocation']
        if trace:command.append('--observation-trace='+str(observation))
        process=subprocess.run(command,capture_output=True,text=True,check=True)
        output.write_text(process.stdout,encoding='utf-8');data=json.loads(process.stdout)
        initial=audit.wire_energies(f,f['q'],f['v']);final=audit.wire_energies(f,data['q'],data['v'])
        change=np.abs(final-initial)/np.maximum(np.abs(initial),1e-30)
        entry={k:v for k,v in data.items() if k not in ['q','v']}
        entry.update(label=label,transverse=f['transverse'],axial=f['axial'],rate_hz=rate,order=order,
            command=command,coefficient_path=str(coefficient.resolve()),coefficient_sha256=digest(coefficient),
            result_path=str(output.resolve()),result_sha256=digest(output),
            independent_wire_energy_relative=change.tolist(),maximum_wire_energy_relative=float(max(change)),
            energy_gate_passed=bool(data['completed'] and data['work_relative_max']<=1e-9 and max(change)<=1e-9),
            diagnostic_trace=trace,benchmark=bench)
        raw[label]=data
        if trace:
            entry.update(observation_path=str(observation.resolve()),observation_sha256=digest(observation))
            observed=json.loads(observation.read_text(encoding='utf-8'))
            if len(observed['rows'])!=data['completed_microframes']+1:raise ValueError('incomplete observation trace')
            traces[label]=common_signal(observed,rate)
        record['runs'].append(entry);save()
        print(label,'completed',data['completed'],'mean',data['mean_ms'],'max',data['max_ms'],flush=True)
        if not entry['energy_gate_passed'] or data['allocations']!=0:raise ValueError('native or independent energy/allocation gate failed')
        return data

    # Check that optional diagnostics did not change the established full model.
    control=next(c for c in old['cases'] if c['label']=='plain-512000')
    if digest(control['result_path'])!=control['result_sha256'] or digest(control['coefficient_path'])!=control['coefficient_sha256']:
        raise ValueError('old full-model control changed')
    mixed=fixture(transverse=96,rate=512000,seed=83,duration=.006);mixed['launch_speeds'][:]=0.
    current=run('mixed-full-control',mixed,8);frozen=json.loads(Path(control['result_path']).read_text(encoding='utf-8'))
    record['full_control_exact']=all(current[k]==frozen[k] for k in ['q','v','work_relative_max',
        'maximum_iterations','numerical_lambda_abs_max','midpoint_residual_relative_max'])
    if not record['full_control_exact']:raise ValueError('instrumentation changed physical state')
    mixed_small,indices=project(mixed,12,16000)
    reduced=run('mixed-reduced-stress',mixed_small,8)
    fine=old['teacher_cases'][-1]
    if digest(fine['result_path'])!=fine['result_sha256']:raise ValueError('frozen fine reference changed')
    record['mixed_stress']=state_errors(mixed,json.loads(Path(fine['result_path']).read_text(encoding='utf-8')),reduced,indices)

    grid=[(4,16000),(8,16000),(12,16000),(16,16000),(24,32000),(32,64000),(48,64000),(96,16000),(96,64000)]
    for peak in [10.,50.]:
        prefix=f'pulse-{int(peak)}N';full=pulse_fixture(peak_force=peak);check=pulse_fixture(peak_force=peak,order=512)
        metric=lambda f:np.r_[omega(f)*f['q'][:-1],f['v'][:-1]]
        pulse_error=float(np.linalg.norm(metric(full)-metric(check))/max(np.linalg.norm(metric(check)),1e-30))
        if pulse_error>1e-10:raise ValueError('prescribed initial pulse is unresolved')
        a=prefix+'-full-O8';b=prefix+'-full-O16'
        run(a,full,8,trace=True);run(b,full,16,trace=True)
        reference=b;pair=signal_errors(traces[a],traces[b])
        full_indices=np.arange(len(full['q'])-1)
        state_pair=state_errors(full,raw[b],raw[a],full_indices)
        resolved=pair['maximum_probe_velocity_relative']<=.001 and state_pair['full_state_relative']<=.001
        if not resolved:
            c=prefix+'-full-O32';run(c,full,32,trace=True)
            pair=signal_errors(traces[b],traces[c]);state_pair=state_errors(full,raw[c],raw[b],full_indices)
            resolved=pair['maximum_probe_velocity_relative']<=.001 and state_pair['full_state_relative']<=.001
            reference=c
        experiment=dict(peak_force_n=peak,prescribed_pulse_duration_s=.001,initial_pulse_quadrature_relative=pulse_error,
            initial_total_energy_j=float(sum(audit.wire_energies(full,full['q'],full['v']))),
            reference_label=reference,reference_refinement=pair,reference_state_refinement=state_pair,
            numerical_reference_resolved=bool(resolved),cases=[])
        record['experiments'].append(experiment);save()
        benchmark=run(prefix+'-full-benchmark',full,8,bench=True)
        experiment['full_benchmark']=dict(mean_ms=benchmark['mean_ms'],max_ms=benchmark['max_ms'])
        for nt,rate in grid:
            f,indices=project(full,nt,rate);label=prefix+f'-N{nt}-R{rate}'
            candidate=run(label,f,8,trace=True)
            refined,_=project(full,nt,2*rate);ref_label=label+'-refined'
            run(ref_label,refined,16,trace=True)
            signal=signal_errors(traces[reference],traces[label])
            temporal=signal_errors(traces[ref_label],traces[label])
            final_error=state_errors(full,raw[reference],candidate,indices)
            temporal_state=state_errors(f,raw[ref_label],candidate,np.arange(len(f['q'])-1))
            first=run(label+'-benchmark',f,8,bench=True)
            repeated=run(label+'-repeat',f,8,bench=True)
            continued,_=project(full,nt,rate)
            continued['q']=np.asarray(candidate['q']);continued['v']=np.asarray(candidate['v']);continued['endpoint']=continued['q'].copy()
            tail=run(label+'-continued-benchmark',continued,8,bench=True)
            worst=max(x['max_ms'] for x in [first,repeated,tail]);worst_mean=max(x['mean_ms'] for x in [first,repeated,tail])
            temporal_pass=temporal['maximum_probe_velocity_relative']<=.001 and temporal_state['full_state_relative']<=.001
            mechanical_pass=signal['maximum_probe_velocity_relative']<=.05 and signal['maximum_probe_velocity_relative_including_boundaries']<=.05
            case=dict(transverse=nt,axial=2*nt,rate_hz=rate,label=label,signal_error=signal,
                final_state_error=final_error,temporal_refinement=temporal,temporal_state_refinement=temporal_state,
                temporal_refinement_passed=bool(temporal_pass),mechanical_5percent_screen_passed=bool(mechanical_pass),
                mean_ms=first['mean_ms'],repeat_mean_ms=repeated['mean_ms'],continued_mean_ms=tail['mean_ms'],
                worst_mean_ms=worst_mean,worst_max_ms=worst,component_deadline_passed=worst<=.125,
                component_half_budget_passed=worst<=.0625,estimated_sequential_mean_budget_for_8_notes=8*worst_mean/.125,
                provisional_component_candidate=bool(resolved and temporal_pass and mechanical_pass and worst<=.125),
                full_piano_realtime=False)
            experiment['cases'].append(case);save()
            print('RESULT',peak,nt,rate,'probe error',signal['maximum_probe_velocity_relative'],
                  'worst ms',worst,'candidate',case['provisional_component_candidate'],flush=True)
    save(final=True)


if __name__=='__main__':
    with threadpool_limits(limits=1):main()
