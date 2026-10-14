"""Longer sequential CPU comparison for the measured four-node gather workspace."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--case-report', type=Path, required=True)
    parser.add_argument('--local', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--buffers', type=int, default=64)
    args = parser.parse_args()
    if args.local.exists() or args.report.exists() or not 8 <= args.buffers <= 2048:
        raise ValueError('fresh paths and 8..2048 buffers required')
    root = Path(__file__).resolve().parents[1]
    source = json.loads(args.case_report.read_text())
    if not source['sources_unchanged']:
        raise ValueError('incomplete case report')
    for name, sha in source['source_sha256'].items():
        if digest(root/'tools'/name) != sha:
            raise ValueError('measured source changed: '+name)
    current = source['executable_path']
    frozen = source['baseline_executable_path']
    coefficient = next(c for c in source['cases'] if c['label']=='gathered-512000-O8')
    authenticated = [(current, source['executable_sha256']), (frozen, source['baseline_executable_sha256']),
                     (coefficient['coefficient_path'], coefficient['coefficient_sha256'])]
    if any(digest(path) != sha for path, sha in authenticated):
        raise ValueError('measured executable or input changed')
    args.local.mkdir(parents=True)
    record = dict(producer_path=str(Path(__file__).resolve()), producer_sha256=digest(__file__),
                  case_report_path=str(args.case_report.resolve()), case_report_sha256=digest(args.case_report),
                  source_sha256=source['source_sha256'], buffers=args.buffers, microframes_per_buffer=64,
                  physical_duration_s=args.buffers*64/512000, deadline_ms=.125,
                  fidelity_checked_duration_s=.006, longer_duration_fidelity_approved=False,
                  full_piano_realtime=False, cases=[])
    keys = ['q','v','work_relative_max','numerical_lambda_abs_max','maximum_iterations',
            'midpoint_residual_relative_max','initial_wire_energy_j','final_wire_energy_j']
    control = None
    schedule = [('frozen',frozen,[]),('direct',current,['--direct-batch-gathers']),
                ('gathered',current,[]),('gathered-repeat',current,[]),
                ('direct-repeat',current,['--direct-batch-gathers']),('frozen-repeat',frozen,[])]
    for label, exe, flags in schedule:
        command = [exe, coefficient['coefficient_path'], 'bench', '1', '8', str(args.buffers),
                   '--collocation','--buffer-microframes=64',*flags]
        process = subprocess.run(command, capture_output=True, text=True, check=True)
        data = json.loads(process.stdout)
        output = args.local/(label+'.json'); output.write_text(process.stdout, encoding='utf-8')
        if control is None:
            control = {k:data[k] for k in keys}
        entry = {k:v for k,v in data.items() if k not in ['q','v']}
        entry.update(label=label, command=command, executable_sha256=digest(exe),
                     coefficient_path=coefficient['coefficient_path'], coefficient_sha256=coefficient['coefficient_sha256'],
                     result_path=str(output.resolve()), result_sha256=digest(output),
                     arrays_and_work_equal=all(data[k]==control[k] for k in keys))
        entry['native_wire_energy_relative'] = [abs(b-a)/max(abs(a),1e-30)
                                              for a,b in zip(data['initial_wire_energy_j'],data['final_wire_energy_j'])]
        entry['energy_and_completion_passed'] = (data['completed'] and data['buffers']==args.buffers
            and data['allocations']==0 and data['work_relative_max']<=1e-9
            and max(entry['native_wire_energy_relative'])<=1e-9)
        record['cases'].append(entry)
        record['sources_unchanged'] = all(digest(root/'tools'/name)==sha for name,sha in source['source_sha256'].items())
        record['inputs_unchanged'] = all(digest(path)==sha for path,sha in authenticated)
        args.report.write_text(json.dumps(record, indent=2, allow_nan=False)+'\n', encoding='utf-8')
        print(label, data['mean_ms'], data['max_ms'], entry['arrays_and_work_equal'],
              entry['energy_and_completion_passed'], flush=True)


if __name__ == '__main__':
    main()
