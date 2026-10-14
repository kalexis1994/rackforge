"""Authenticate the feasibility artifacts and recompute their screening decisions."""
import argparse
import hashlib
import json
from pathlib import Path


def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--study',type=Path,required=True);p.add_argument('--orders',type=Path,required=True)
    p.add_argument('--report',type=Path,required=True);a=p.parse_args();root=Path(__file__).resolve().parents[1]
    if a.report.exists():raise ValueError('fresh audit path required')
    study=json.loads(a.study.read_text(encoding='utf-8'));orders=json.loads(a.orders.read_text(encoding='utf-8'))
    if digest(a.study)!=orders['parent_sha256']:raise ValueError('study hash mismatch')
    sources=study['source_sha256']|orders['source_sha256']|{Path(__file__).name:digest(__file__)}
    for name,sha in sources.items():
        if digest(root/'tools'/name)!=sha:raise ValueError('source changed: '+name)
    resume=study.get('resumed_checkpoint');cached={}
    if resume:
        if hashlib.sha256(resume['producer_source'].encode('utf-8')).hexdigest()!=resume['producer_sha256']:
            raise ValueError('original producer source mismatch')
        if digest(resume['path'])!=resume['sha256']:raise ValueError('original checkpoint changed')
        checkpoint=json.loads(Path(resume['path']).read_text(encoding='utf-8'))
        for name,sha in resume['original_source_sha256'].items():
            actual=resume['producer_sha256'] if name=='measure-concert-grand-modal-viability.py' else digest(root/'tools'/name)
            if actual!=sha:raise ValueError('cached runtime source changed: '+name)
        cached={r['label']:r for r in checkpoint['runs']}
    for report in [study,orders]:
        if not report['sources_unchanged'] or digest(report['executable_path'])!=report['executable_sha256']:
            raise ValueError('native executable changed')
        labels=set();paths=set()
        for run in report['runs']:
            if run['label'] in labels or run['result_path'] in paths:raise ValueError('duplicate artifact')
            labels.add(run['label']);paths.add(run['result_path'])
            for prefix in ['coefficient','result','observation']:
                if prefix+'_path' in run and digest(run[prefix+'_path'])!=run[prefix+'_sha256']:
                    raise ValueError('artifact changed: '+run['label'])
            if run['allocations']!=0:raise ValueError('processing allocation')
            if run.get('reused_from_checkpoint'):
                original={k:v for k,v in run.items() if k!='reused_from_checkpoint'}
                if original!=cached[run['label']]:raise ValueError('cached metadata changed')
            if 'energy_gate_passed' in run and not run['energy_gate_passed']:raise ValueError('energy failure')
            if 'independent_wire_energy_relative_max' in run and run['independent_wire_energy_relative_max']>1e-9:
                raise ValueError('supplement energy failure')
    accepted={10.:set(),50.:set()}
    for experiment in study['experiments']:
        for case in experiment['cases']:
            quality=case['signal_error'];temporal=case['temporal_refinement'];state=case['temporal_state_refinement']
            expected=(experiment['numerical_reference_resolved'] and temporal['maximum_probe_velocity_relative']<=.001
                and state['full_state_relative']<=.001 and quality['maximum_probe_velocity_relative']<=.05
                and quality['maximum_probe_velocity_relative_including_boundaries']<=.05 and case['worst_max_ms']<=.125)
            if bool(expected)!=case['provisional_component_candidate']:raise ValueError('primary decision mismatch')
            if expected:accepted[experiment['peak_force_n']].add((case['transverse'],case['rate_hz'],8))
    resolved_by_peak={e['peak_force_n']:e['numerical_reference_resolved'] for e in study['experiments']}
    for case in orders['cases']:
        quality=case['signal_error'];temporal=case['temporal_refinement'];state=case['temporal_state_refinement']
        expected=(resolved_by_peak[case['peak_force_n']] and temporal['maximum_probe_velocity_relative']<=.001 and state['full_state_relative']<=.001
            and quality['maximum_probe_velocity_relative']<=.05 and quality['maximum_probe_velocity_relative_including_boundaries']<=.05
            and case['worst_max_ms']<=.125)
        if bool(expected)!=case['provisional_component_candidate']:raise ValueError('order decision mismatch')
        if expected:accepted[case['peak_force_n']].add((case['transverse'],case['rate_hz'],case['order']))
    record=dict(source_sha256=sources,study_path=str(a.study.resolve()),study_sha256=digest(a.study),
        orders_path=str(a.orders.resolve()),orders_sha256=digest(a.orders),
        native_runs_verified=len(study['runs'])+len(orders['runs']),
        reused_native_runs_verified=sum(r.get('reused_from_checkpoint',False) for r in study['runs']),
        candidates_10N=sorted(accepted[10.]),candidates_50N=sorted(accepted[50.]),
        candidates_passing_both_intensities=sorted(accepted[10.]&accepted[50.]),
        conclusion=('component candidates found; complete instrument unverified' if accepted[10.]&accepted[50.] else
            'no tested configuration passes both intensity screens and the measured component deadline'),
        acoustic_pressure=False,perceptual_quality_approved=False,polyphony_measured=False,full_piano_realtime=False)
    a.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    print(record['conclusion']);print('Verified native runs:',record['native_runs_verified'])


if __name__=='__main__':main()
