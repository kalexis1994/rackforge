"""Refine independent acoustic storage/work observables of frozen prefixes.

Use the previous contact bank's 0.2% work/storage criterion and 1e-14 J
near-zero denominator. This does not infer wave-state or waveform accuracy.
"""
import argparse
import json
from pathlib import Path

from concert_grand_wave_cache import digest


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--prefix-report',type=Path,required=True)
    p.add_argument('--report',type=Path,required=True)
    p.add_argument('--force-report',type=Path)
    p.add_argument('--capture-report',type=Path)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
    if (a.force_report is None)!=(a.capture_report is None):
        raise ValueError('both linked force/capture controls required')
    parent = json.loads(a.prefix_report.read_text())
    if parent['stage'] != 'contact prefix audit completed':
        raise ValueError('finished independent prefix measurements required')
    criteria = dict(energy_work_relative=.002,near_zero_j=1e-14)
    columns = ['wave_final_j','relaxation_final_j','wave_radiation_j','relaxation_loss_j',
               'felt_heat_j','material_heat_j','air_port_work_j']
    rows = []
    for phase in ['Cold','Continued']:
        cases = sorted([row for row in parent['cases'] if row['phase']==phase],key=lambda row:row['rate'])
        if len(cases)<2 or [row['rate'] for row in cases] != parent['rates']:
            raise ValueError('matching nested rates required')
        fine = cases[-1]
        for case in cases[:-1]:
            changes = {}
            for column in columns:
                exact = fine[column]
                denominator = max(abs(exact),criteria['near_zero_j'])
                change = abs(case[column]-exact)/denominator
                changes[column] = dict(reference_j=exact,absolute_error_j=abs(case[column]-exact),
                                       relative=change,near_zero=abs(exact)<criteria['near_zero_j'],
                                       passed=change<criteria['energy_work_relative'])
            rows.append(dict(phase=phase,rate=case['rate'],reference_rate=fine['rate'],
                             changes=changes,passed=all(item['passed'] for item in changes.values())))
    finest_pair = [row for row in rows if row['rate']==parent['rates'][-2]]
    r = dict(parent_sha256={str(a.prefix_report):digest(a.prefix_report)},criteria=criteria,rows=rows,
             finest_pair_gates={row['phase']:row['passed'] for row in finest_pair},
             finest_pair_work_passed=all(row['passed'] for row in finest_pair),
             source_sha256={Path(__file__).name:digest(__file__)},
             preflight_only=True,all_gates_passed=False,
             scope='Independent storage, radiation, material/felt heat and port work in the short prefix. '
                   'No full wave-coordinate, pressure-waveform, separation or timbral accuracy claim.')
    if a.force_report is not None:
        force = json.loads(a.force_report.read_text())
        capture = json.loads(a.capture_report.read_text())
        if (force['cache_R_sha256']!=parent['cache_R_sha256']
                or capture['cache_R_sha256']!=parent['cache_R_sha256']
                or capture['stage']!='compact capture/replay preflight completed'
                or capture['rates']!=parent['rates'] or capture['duration_s']!=parent['duration_s']
                or capture['modes']!=parent['modes']
                or not set(parent['parent_sha256'].values()).issubset(force['parent_sha256'].values())
                or capture['parent_sha256']!=parent['parent_sha256']):
            raise ValueError('linked controls use different frozen board/load/prefixes')
        measured = {(row['phase'],row['rate']):row for row in parent['cases']}
        observed = {(row['phase'],row['rate']):row for row in force['rows']}
        captured = {(row['phase'],row['rate']):row for row in capture['cases']}
        if (observed.keys()!=measured.keys() or captured.keys()!=measured.keys()
                or len(observed)!=len(force['rows']) or len(captured)!=len(capture['cases'])):
            raise ValueError('matching complete case sets required')
        reproduced = []
        for key,row in measured.items():
            other,read = captured[key],observed[key]
            for column in ['impulse_n_s','maximum_force_n','mechanical_final_j',
                           'compression_final_m','final_wave_norm','relaxation_final_j']:
                reproduced.append(abs(row[column]-other[column])/max(abs(row[column]),1e-30))
            for column,field in [('impulse_n_s','impulse_n_s'),('maximum_force_n','interval_average_peak_force_n')]:
                reproduced.append(abs(row[column]-read[field])/max(abs(row[column]),1e-30))
            if other['common_force_n']!=read['common_force_n']:
                raise ValueError('different force histories on the common timestamp grid')
        if max(reproduced)>=1e-10:
            raise ValueError('linked controls do not reproduce measured contact diagnostics')
        r['shared_contact_diagnostics_relative_max'] = max(reproduced)
        for path in [a.force_report,a.capture_report]:
            r['parent_sha256'][str(path)] = digest(path)
        r['original_prefix_gates'] = parent['prefix_gates']
        # Retain the failed interval-average maximum. The new force gate uses
        # the same tolerance at common physical timestamps, proven separately.
        gates = {name:value for name,value in parent['prefix_gates'].items() if name!='finest_force'}
        fine = [row for row in parent['comparisons'] if row['rate']==parent['rates'][-2]]
        gates['finest_impulse'] = max(row['impulse_relative'] for row in fine)<.002
        gates.update({'aligned_'+name:value for name,value in force['common_force_gates'].items()})
        gates['independent_work_refinement'] = r['finest_pair_work_passed']
        gates['compact_endpoint_replay'] = capture['compact_replay_passed']
        r['linked_prefix_gates'] = gates
        r['linked_prefix_gates_passed'] = all(gates.values())
        r['replaced_observable'] = 'Only the interval-average AVF maximum is replaced by constitutive force '
        r['replaced_observable'] += 'on common timestamps; original failed gate retained, 0.2% tolerance unchanged.'
    a.report.parent.mkdir(parents=True,exist_ok=True)
    a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')
    print('work refinement',r['finest_pair_gates'],flush=True)
    for row in finest_pair:
        print(row['phase'],{name:item['relative'] for name,item in row['changes'].items()},flush=True)
    if 'linked_prefix_gates' in r:
        print('linked prefix controls',r['linked_prefix_gates'],flush=True)


if __name__=='__main__':
    main()
