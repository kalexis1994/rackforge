"""Measure linear full-mode carriers with a low-dimensional nonlinear Hamiltonian."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_native_strings import fixture,write_fixture
from concert_grand_modal_viability import pulse_fixture,project,modal_indices,omega,common_signal,signal_errors,state_errors
from concert_grand_hybrid import hybrid_signal


def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--parent',type=Path,required=True)
    parser.add_argument('--local',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    args=parser.parse_args();root=Path(__file__).resolve().parents[1];old='5a4c4d64'
    if args.local.exists() or args.report.exists():raise ValueError('fresh paths required')
    parent=json.loads(args.parent.read_text(encoding='utf-8'))
    # Old references stay reproducible even while the experimental solver grows.
    git_bytes=subprocess.check_output(['git','show',old+':'+args.parent.resolve().relative_to(root).as_posix()],cwd=root)
    if hashlib.sha256(git_bytes).hexdigest()!=digest(args.parent):raise ValueError('parent report differs from committed reference')
    for name,sha in parent['source_sha256'].items():
        blob=subprocess.check_output(['git','show',old+':tools/'+name],cwd=root)
        if hashlib.sha256(blob).hexdigest()!=sha:raise ValueError('historical source mismatch: '+name)
    sources=set(parent['source_sha256'])|{'concert-grand-hybrid.rs','concert_grand_hybrid.py',Path(__file__).name,'tests/test_concert_hybrid.py'}
    hashes={name:digest(root/'tools'/name) for name in sorted(sources)}
    args.local.mkdir(parents=True)
    exe=args.local.resolve()/'trajectory.exe'
    command=['rustc','--edition=2021','-O','-C','target-cpu=native','--cfg','concert_phase_library',
        str(root/'tools/concert-grand-phase-trajectory.rs'),'-o',str(exe)]
    subprocess.run(command,check=True,cwd=root)
    spec=importlib.util.spec_from_file_location('wire_audit',root/'tools/audit-concert-grand-phase-wire-energy.py')
    audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)
    record=dict(stage='hybrid-linear-highs',hamiltonian='H_linear(full) + V_nonlinear(P_low q)',
        reference_commit=old,parent_path=str(args.parent.resolve()),parent_sha256=digest(args.parent),
        source_sha256=hashes,compile_command=command,executable_path=str(exe),executable_sha256=digest(exe),
        duration_s=.096,deadline_ms=.125,mechanical_screening_tolerance=.05,numerical_refinement_tolerance=.001,
        physical_dissipation=False,contact_simulated=False,soundboard_included=False,acoustic_pressure=False,
        perceptual_quality_approved=False,full_piano_realtime=False,audio_output_benchmarked=False,
        observation='low trace interpolated to 512 kHz plus exact high linear carriers, then anti-alias to 48 kHz',
        runs=[],cases=[],references=[])

    def run(label,f,low,order,trace=False,bench=False):
        inp=args.local/(label+'.bin');out=args.local/(label+'.json');obs=args.local/(label+'-low-observation.json')
        write_fixture(inp,f);rate=int(f['parameters'][4])
        mode=['bench','1',str(order),'64',f'--buffer-microframes={rate//8000}'] if bench else ['verify','1',str(order)]
        command=[str(exe),str(inp.resolve()),*mode,'--collocation',f'--linear-highs={low}']
        if trace:command.append('--observation-trace='+str(obs.resolve()))
        result=subprocess.run(command,capture_output=True,text=True,check=True)
        out.write_text(result.stdout,encoding='utf-8');data=json.loads(result.stdout)
        # Independent hybrid Hamiltonian audit and a separate ORIGINAL-model ledger.
        small,indices=project(f,low,rate)
        high=np.setdiff1d(np.arange(len(f['q'])-1),indices);w2=omega(f)**2
        def energies(q,v):
            q=np.asarray(q);v=np.asarray(v)
            low_e=audit.wire_energies(small,np.r_[q[indices],q[-1]],np.r_[v[indices],v[-1]])
            hi=.5*(v[high]**2+w2[high]*q[high]**2)
            return low_e+np.bincount(high//(4*f['transverse']),weights=hi,minlength=3)
        initial=energies(f['q'],f['v']);final=energies(data['q'],data['v'])
        relative=float(max(abs(final-initial)/np.maximum(abs(initial),1e-30)))
        original_start=audit.wire_energies(f,f['q'],f['v']);original_end=audit.wire_energies(f,data['q'],data['v'])
        original_change=float(max(abs(original_end-original_start)/np.maximum(abs(original_start),1e-30)))
        if not data['completed'] or relative>1e-9 or data['work_relative_max']>1e-9 or data['allocations']!=0:
            raise ValueError('hybrid energy, completion or allocation gate failed: '+label)
        t=data['completed_microframes']/rate;w=np.sqrt(w2[high]);phase=w*t
        expected_q=np.cos(phase)*f['q'][high]+np.sin(phase)*f['v'][high]/w
        expected_v=-w*np.sin(phase)*f['q'][high]+np.cos(phase)*f['v'][high]
        delta=np.r_[w*(np.asarray(data['q'])[high]-expected_q),np.asarray(data['v'])[high]-expected_v]
        norm=np.linalg.norm(np.r_[w*expected_q,expected_v])
        linear_error=float(np.linalg.norm(delta)/max(norm,1e-30))
        if linear_error>1e-9:raise ValueError('high linear carriers changed')
        entry=dict(label=label,rate_hz=rate,nonlinear_transverse=low,order=order,benchmark=bench,command=command,
            coefficient_path=str(inp.resolve()),coefficient_sha256=digest(inp),result_path=str(out.resolve()),result_sha256=digest(out),
            hybrid_wire_energy_relative_max=relative,original_wire_energy_relative_change=original_change,
            high_linear_state_relative=linear_error,mean_ms=data['mean_ms'],max_ms=data['max_ms'],
            known_prepared_bytes=data['known_prepared_bytes'],allocations=data['allocations'])
        if trace:entry.update(observation_path=str(obs.resolve()),observation_sha256=digest(obs))
        record['runs'].append(entry)
        signal=hybrid_signal(f,low,json.loads(obs.read_text(encoding='utf-8')),rate) if trace else None
        return data,signal,entry

    for peak in [10.,50.]:
        full=pulse_fixture(peak_force=peak);experiment=next(e for e in parent['experiments'] if e['peak_force_n']==peak)
        ref=next(r for r in parent['runs'] if r['label']==experiment['reference_label'])
        for field in ['result','observation','coefficient']:
            if digest(ref[field+'_path'])!=ref[field+'_sha256']:raise ValueError('reference artifact changed')
        record['references'].append(ref)
        reference=json.loads(Path(ref['result_path']).read_text(encoding='utf-8'))
        ref_signal=common_signal(json.loads(Path(ref['observation_path']).read_text(encoding='utf-8')),ref['rate_hz'])
        for low,rate,order in [(8,16000,16),(12,16000,16),(24,32000,16)]:
            label=f'pulse-{int(peak)}N-L96-NL{low}-R{rate}-O{order}'
            f,_=project(full,96,rate)
            data,signal,entry=run(label,f,low,order,trace=True)
            fine,_=project(full,96,2*rate)
            refined,refined_signal,_=run(label+'-refined',fine,low,2*order,trace=True)
            quality=signal_errors(ref_signal,signal);temporal=signal_errors(refined_signal,signal)
            state=state_errors(full,reference,data,np.arange(len(full['q'])-1))
            temporal_state=state_errors(f,refined,data,np.arange(len(f['q'])-1))
            timings=[]
            for suffix in ['benchmark','repeat','continued']:
                current,_=project(full,96,rate)
                if suffix=='continued':
                    current['q']=np.asarray(data['q']);current['v']=np.asarray(data['v']);current['endpoint']=current['q'].copy()
                timed,_,_=run(label+'-'+suffix,current,low,order,bench=True);timings.append(timed)
            mean=max(t['mean_ms'] for t in timings);maximum=max(t['max_ms'] for t in timings)
            resolved=temporal['maximum_probe_velocity_relative']<=.001 and temporal_state['full_state_relative']<=.001
            record['cases'].append(dict(label=label,peak_force_n=peak,linear_transverse=96,nonlinear_transverse=low,
                rate_hz=rate,order=order,signal_error=quality,final_state_error=state,
                temporal_refinement=temporal,temporal_state_refinement=temporal_state,temporal_refinement_passed=bool(resolved),
                original_wire_energy_relative_change=entry['original_wire_energy_relative_change'],
                worst_mean_ms=mean,worst_max_ms=maximum,solver_deadline_passed=maximum<=.125,
                mechanical_5percent_screen_passed=bool(quality['maximum_probe_velocity_relative']<=.05 and
                    quality['maximum_probe_velocity_relative_including_boundaries']<=.05),
                audio_output_benchmarked=False,full_piano_realtime=False))
            print(label,'error',quality['maximum_probe_velocity_relative'],'bands',
                [b['complex_spectrum_relative'] for b in quality['bands']],
                'refinement',temporal_state['full_state_relative'],'mean/max',mean,maximum,flush=True)
    # Original broadband stress must not be reclassified as a suitable low-mode initial state.
    mixed=fixture(transverse=96,rate=512000,seed=83,duration=.006);mixed['launch_speeds'][:]=0.
    stress,_,entry=run('mixed-L96-NL8-R512000-O8',mixed,8,8)
    old_run=next(r for r in parent['runs'] if r['label']=='mixed-full-control')
    if digest(old_run['result_path'])!=old_run['result_sha256']:raise ValueError('stress reference changed')
    record['stress_reference']=old_run
    record['mixed_stress']=state_errors(mixed,json.loads(Path(old_run['result_path']).read_text(encoding='utf-8')),
        stress,np.arange(len(mixed['q'])-1))|{'original_wire_energy_relative_change':entry['original_wire_energy_relative_change']}
    record['sources_unchanged']=all(digest(root/'tools'/name)==sha for name,sha in hashes.items())
    if not record['sources_unchanged']:raise ValueError('measured sources changed')
    args.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')


if __name__=='__main__':
    with threadpool_limits(limits=1):main()
