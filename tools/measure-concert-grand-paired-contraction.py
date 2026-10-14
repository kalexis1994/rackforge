"""Interleave frozen, streamed and blocked contraction timing runs."""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--case-report',type=Path,required=True)
    parser.add_argument('--local',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    args=parser.parse_args();root=Path(__file__).resolve().parents[1]
    if args.local.exists() or args.report.exists():
        raise ValueError('fresh paths required')
    old=json.loads(args.case_report.read_text())
    if not old['sources_unchanged'] or any(digest(root/'tools'/n)!=s for n,s in old['source_sha256'].items()):
        raise ValueError('measured sources changed')
    frozen=old['frozen_executable_path']; current=old['plain_executable_path']
    fixture=next(c for c in old['cases'] if c['label']=='plain-512000')
    inputs=[(frozen,old['frozen_executable_sha256']),(current,old['plain_executable_sha256']),
            (fixture['coefficient_path'],fixture['coefficient_sha256'])]
    if any(digest(p)!=s for p,s in inputs):
        raise ValueError('executable or input changed')
    args.local.mkdir(parents=True)
    record=dict(producer_path=str(Path(__file__).resolve()),producer_sha256=digest(__file__),
                case_report_path=str(args.case_report.resolve()),case_report_sha256=digest(args.case_report),
                source_sha256=old['source_sha256'],physical_duration_s=.002,buffers=16,
                deadline_ms=.125,full_piano_realtime=False,cases=[])
    control=None
    keys=['q','v','work_relative_max','numerical_lambda_abs_max','maximum_iterations',
          'midpoint_residual_relative_max','initial_wire_energy_j','final_wire_energy_j']
    schedule=[('frozen',frozen,[]),('streamed',current,['--streamed-node-contraction']),
              ('blocked',current,[]),('blocked',current,[]),
              ('streamed',current,['--streamed-node-contraction']),('frozen',frozen,[])]
    for cycle in range(3):
        for index,(route,exe,flags) in enumerate(schedule):
            label=f'{cycle}-{index}-{route}'
            command=[exe,fixture['coefficient_path'],'bench','1','8','16','--collocation',
                     '--buffer-microframes=64',*flags]
            process=subprocess.run(command,capture_output=True,text=True,check=True)
            data=json.loads(process.stdout);output=args.local/(label+'.json')
            output.write_text(process.stdout,encoding='utf-8')
            if control is None:control={k:data[k] for k in keys}
            same=all(data[k]==control[k] for k in keys)
            if not same or not data['completed'] or data['allocations']!=0:
                raise ValueError('state mismatch or processing failure')
            record['cases'].append(dict(label=label,route=route,command=command,
                executable_sha256=digest(exe),coefficient_sha256=fixture['coefficient_sha256'],
                result_path=str(output.resolve()),result_sha256=digest(output),
                mean_ms=data['mean_ms'],p99_ms=data['p99_ms'],max_ms=data['max_ms'],
                allocations=data['allocations'],arrays_and_work_equal=same))
            record['summary']={r:dict(mean_of_means_ms=statistics.mean(c['mean_ms'] for c in record['cases'] if c['route']==r),
                median_of_means_ms=statistics.median(c['mean_ms'] for c in record['cases'] if c['route']==r),
                max_ms=max(c['max_ms'] for c in record['cases'] if c['route']==r))
                for r in {c['route'] for c in record['cases']}}
            record['sources_unchanged']=all(digest(root/'tools'/n)==s for n,s in old['source_sha256'].items())
            record['inputs_unchanged']=all(digest(p)==s for p,s in inputs)
            args.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')
            print(label,data['mean_ms'],data['max_ms'],flush=True)
    print(record['summary'],flush=True)


if __name__=='__main__':main()
