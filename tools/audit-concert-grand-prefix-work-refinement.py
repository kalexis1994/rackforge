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
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
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
    a.report.parent.mkdir(parents=True,exist_ok=True)
    a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')
    print('work refinement',r['finest_pair_gates'],flush=True)
    for row in finest_pair:
        print(row['phase'],{name:item['relative'] for name,item in row['changes'].items()},flush=True)


if __name__=='__main__':
    main()
