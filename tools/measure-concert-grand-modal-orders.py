"""Resolve the temporal-order tradeoff of promising reduced C4 models."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_native_strings import write_fixture
from concert_grand_modal_viability import pulse_fixture, project, common_signal, signal_errors, state_errors


def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--parent',type=Path,required=True)
    parser.add_argument('--local',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    args=parser.parse_args();root=Path(__file__).resolve().parents[1]
    if args.local.exists() or args.report.exists():raise ValueError('fresh paths required')
    parent=json.loads(args.parent.read_text(encoding='utf-8'))
    if any(digest(root/'tools'/name)!=sha for name,sha in parent['source_sha256'].items()):raise ValueError('parent sources changed')
    exe=Path(parent['executable_path'])
    if digest(exe)!=parent['executable_sha256']:raise ValueError('native executable changed')
    args.local.mkdir(parents=True)
    spec=importlib.util.spec_from_file_location('wire_audit',root/'tools/audit-concert-grand-phase-wire-energy.py')
    audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)
    record=dict(parent_path=str(args.parent.resolve()),parent_sha256=digest(args.parent),
        source_sha256=parent['source_sha256']|{Path(__file__).name:digest(__file__),'tests/test_concert_modal_signal.py':digest(root/'tools/tests/test_concert_modal_signal.py')},
        executable_path=str(exe),executable_sha256=digest(exe),duration_s=.096,deadline_ms=.125,
        acoustic_pressure=False,perceptual_quality_approved=False,full_piano_realtime=False,cases=[],runs=[])

    def run(label,f,order,trace=False,bench=False):
        inp=args.local/(label+'.bin');out=args.local/(label+'.json');obs=args.local/(label+'-observation.json');write_fixture(inp,f)
        rate=int(f['parameters'][4]);mode=['bench','1',str(order),'64',f'--buffer-microframes={rate//8000}'] if bench else ['verify','1',str(order)]
        command=[str(exe),str(inp),*mode,'--collocation']
        if trace:command.append('--observation-trace='+str(obs))
        p=subprocess.run(command,capture_output=True,text=True,check=True);out.write_text(p.stdout,encoding='utf-8');data=json.loads(p.stdout)
        initial=audit.wire_energies(f,f['q'],f['v']);final=audit.wire_energies(f,data['q'],data['v'])
        energy=float(max(abs(final-initial)/np.maximum(abs(initial),1e-30)))
        if not data['completed'] or energy>1e-9 or data['work_relative_max']>1e-9 or data['allocations']!=0:raise ValueError('energy or allocation gate failed')
        entry=dict(label=label,command=command,coefficient_path=str(inp.resolve()),coefficient_sha256=digest(inp),
            result_path=str(out.resolve()),result_sha256=digest(out),mean_ms=data['mean_ms'],max_ms=data['max_ms'],
            independent_wire_energy_relative_max=energy,allocations=data['allocations'])
        if trace:entry.update(observation_path=str(obs.resolve()),observation_sha256=digest(obs))
        record['runs'].append(entry)
        return data,common_signal(json.loads(obs.read_text(encoding='utf-8')),rate) if trace else None

    for peak in [10.,50.]:
        full=pulse_fixture(peak_force=peak);experiment=next(e for e in parent['experiments'] if e['peak_force_n']==peak)
        full_run=next(r for r in parent['runs'] if r['label']==experiment['reference_label'])
        if digest(full_run['observation_path'])!=full_run['observation_sha256']:raise ValueError('full reference changed')
        full_signal=common_signal(json.loads(Path(full_run['observation_path']).read_text(encoding='utf-8')),full_run['rate_hz'])
        full_state=json.loads(Path(full_run['result_path']).read_text(encoding='utf-8'))
        for nt,rate,order in [(8,16000,16),(8,16000,24),(8,8000,24),(12,16000,16)]:
            f,indices=project(full,nt,rate);label=f'pulse-{int(peak)}N-N{nt}-R{rate}-O{order}'
            data,signal=run(label,f,order,trace=True)
            refined=next(r for r in parent['runs'] if r['label']==f'pulse-{int(peak)}N-N{nt}-R16000-refined')
            if digest(refined['observation_path'])!=refined['observation_sha256']:raise ValueError('reduced reference changed')
            refined_signal=common_signal(json.loads(Path(refined['observation_path']).read_text(encoding='utf-8')),refined['rate_hz'])
            refined_state=json.loads(Path(refined['result_path']).read_text(encoding='utf-8'))
            temporal=signal_errors(refined_signal,signal);quality=signal_errors(full_signal,signal)
            temporal_state=state_errors(f,refined_state,data,np.arange(len(f['q'])-1))
            benchmarks=[]
            for suffix in ['benchmark','repeat','continued']:
                current,_=project(full,nt,rate)
                if suffix=='continued':
                    current['q']=np.asarray(data['q']);current['v']=np.asarray(data['v']);current['endpoint']=current['q'].copy()
                result,_=run(label+'-'+suffix,current,order,bench=True);benchmarks.append(result)
            worst=max(b['max_ms'] for b in benchmarks);mean=max(b['mean_ms'] for b in benchmarks)
            resolved=temporal['maximum_probe_velocity_relative']<=.001 and temporal_state['full_state_relative']<=.001
            case=dict(label=label,peak_force_n=peak,transverse=nt,axial=2*nt,rate_hz=rate,order=order,
                signal_error=quality,temporal_refinement=temporal,temporal_state_refinement=temporal_state,
                temporal_refinement_passed=bool(resolved),worst_max_ms=worst,worst_mean_ms=mean,
                component_deadline_passed=worst<=.125,estimated_sequential_mean_budget_for_8_notes=8*mean/.125,
                provisional_component_candidate=bool(experiment['numerical_reference_resolved'] and resolved and worst<=.125
                    and quality['maximum_probe_velocity_relative']<=.05 and quality['maximum_probe_velocity_relative_including_boundaries']<=.05))
            record['cases'].append(case)
            print(label,'error',quality['maximum_probe_velocity_relative'],'refinement',temporal['maximum_probe_velocity_relative'],
                  'state refinement',temporal_state['full_state_relative'],'max ms',worst,'candidate',case['provisional_component_candidate'],flush=True)
    record['sources_unchanged']=all(digest(root/'tools'/n)==sha for n,sha in record['source_sha256'].items())
    args.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')


if __name__=='__main__':
    with threadpool_limits(limits=1):main()
