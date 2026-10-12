"""Repeat native timing after offline fit processes have finished.

Coefficient/source parity and work tests are inherited by hash from the
original diagnostic. This report measures timing only, without another fit.
"""
import argparse
import json
from pathlib import Path
import subprocess
from concert_grand_wave_cache import digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--parent',type=Path,required=True)
    parser.add_argument('--local',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    args = parser.parse_args()
    if args.report.exists():
        raise ValueError('preserve old timing')
    parent = json.loads(args.parent.read_text())
    executable = args.local/'loaded-mobility-bench.exe'
    mapping = args.local/'ports.bin'
    if digest(executable)!=parent['native_executable_sha256'] or digest(mapping)!=parent['mapping_sha256']:
        raise ValueError('native executable or geometry changed')
    rates = []
    for original in parent['rates']:
        bundle = Path(original['coefficient_path'])
        if digest(bundle)!=original['coefficient_sha256']:
            raise ValueError('native bundle changed')
        bench = json.loads(subprocess.check_output([str(executable),str(bundle),str(mapping),'bench'],text=True))
        fraction = bench['p99_ms']/bench['deadline_ms']
        bench.update(p99_deadline_fraction=fraction,
            observed_body_budget_passed=fraction<=.20 and bench['max_ms']<=bench['deadline_ms'] and bench['callback_allocations']==0)
        rates.append(dict(coefficient_sha256=digest(bundle),benchmark=bench))
    report = dict(stage='native loaded diagnostic retimed after fit completion',
        producer_sha256=digest(__file__),parent_report_sha256=digest(args.parent),
        native_executable_sha256=digest(executable),mapping_sha256=digest(mapping),rates=rates,
        body_20percent_budget_passed_both_rates=all(r['benchmark']['observed_body_budget_passed'] for r in rates),
        no_fit_running_in_this_task=True,full_piano_realtime=False,
        scope='Timing only; inherited native/Python/work controls on byte-identical coefficients. Pressure excluded; no CPU isolation or host realtime guarantee.')
    args.report.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print([(r['benchmark']['rate_hz'],r['benchmark']['p99_ms'],r['benchmark']['max_ms']) for r in rates])


if __name__ == '__main__':
    main()
