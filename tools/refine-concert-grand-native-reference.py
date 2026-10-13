"""Refine the work-checked AAVF clock without approving a whole piano.

Keep all modal states and the same seeded initial condition. Compare final
states and time-bin mean contact forces; state/work/force are separate gates.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time

import numpy as np
from threadpoolctl import threadpool_limits

from concert_grand_native_strings import fixture, write_fixture
from concert_grand_wave_cache import digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--measurement', type=Path, required=True)
    parser.add_argument('--local', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--rates', nargs='+', type=int,
                        default=[128000, 256000, 512000, 1024000, 2048000, 4096000])
    parser.add_argument('--duration', type=float, default=.006)
    args = parser.parse_args()
    rates = sorted(set(args.rates))
    common = 128000
    if (args.local.exists() or args.report.exists() or len(rates) < 2
            or any(rate < common or rate % common for rate in rates)
            or not 0 < args.duration <= .256
            or any(abs(args.duration*rate-round(args.duration*rate)) > 1e-8 for rate in rates)):
        raise ValueError('fresh paths, integral nested clocks and positive duration required')
    root = Path(__file__).resolve().parent.parent
    parent = json.loads(args.measurement.read_text(encoding='utf-8'))
    for name, sha in parent['source_sha256'].items():
        if digest(root/'tools'/name) != sha:
            raise ValueError(f'measured source changed: {name}')
    if not parent.get('checked_reference'):
        raise ValueError('work-checked reference required')
    exe = Path(parent['executable_path'])
    if digest(exe) != parent['executable_sha256']:
        raise ValueError('measured executable changed')
    args.local.mkdir(parents=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    started = time.perf_counter()
    record = dict(parent_path=str(args.measurement.resolve()), parent_sha256=digest(args.measurement),
                  source_sha256=parent['source_sha256'], verifier_sha256=digest(Path(__file__)),
                  executable_sha256=digest(exe), duration_s=args.duration, seed=83,
                  transverse=96, axial=192, common_force_bins_hz=common,
                  force_window_s=min(.006, args.duration), checked_reference=True,
                  fixed_bridge=True, moving_body_attached=False, pressure_included=False,
                  pedals_included=False, full_piano_realtime=False, global_physics_approved=False,
                  cases=[], comparisons=[], ramp_comparisons=[])

    def save():
        record['seconds'] = time.perf_counter()-started
        args.report.write_text(json.dumps(record, indent=2, allow_nan=False)+'\n', encoding='utf-8')

    def run(label, path, *mode):
        begin = time.perf_counter()
        output = subprocess.run([str(exe), str(path), *map(str, mode)], capture_output=True, text=True)
        case = dict(label=label, mode=list(mode), completed=output.returncode == 0,
                    seconds=time.perf_counter()-begin)
        if output.returncode:
            case.update(returncode=output.returncode, failure=output.stderr)
            return case, None
        data = json.loads(output.stdout)
        result = args.local/(label+'.json')
        result.write_text(json.dumps(data, allow_nan=False)+'\n', encoding='utf-8')
        case.update(result_path=str(result.resolve()), result_sha256=digest(result),
                    mechanical_work_relative_max=data['work_relative_max'],
                    mechanical_work_1e9_gate_passed=data['work_relative_max'] <= 1e-9,
                    reference_work_rejections=data['reference_work_rejections'],
                    reference_local_work_max=data['reference_local_work_max'])
        return case, data

    save()
    previous = None
    highest = None
    with threadpool_limits(limits=1):
        for rate in rates:
            f = fixture(transverse=96, rate=rate, seed=83, duration=args.duration, second_speed=0.)
            path = args.local/f'initial-96-{rate}.bin'
            write_fixture(path, f)
            # Hash every physical input except the varied clock. This also
            # authenticates that q/v, geometry, contact and launch stay fixed.
            physical = hashlib.sha256()
            for key in ['parameters', 'shapes', 'tensions', 'launch_speeds', 'times', 'q', 'endpoint', 'v']:
                values = np.asarray(f[key], dtype='<f8')
                if key == 'parameters':
                    values = np.delete(values, 4)
                physical.update(values.tobytes())
            signature = physical.hexdigest()
            if record.get('physical_input_sha256', signature) != signature:
                raise ValueError('clock refinement changed the physical fixture')
            record['physical_input_sha256'] = signature
            omega = np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]

            def state(data):
                return np.r_[omega*np.asarray(data['q'])[:-1], data['v']]

            def force_bins(data, hz):
                return np.asarray(data['force_trace']).reshape(-1, hz//common).mean(axis=1)

            case, data = run(f'checked-{rate}', path, 'verify')
            case.update(rate_hz=rate, coefficient_path=str(path.resolve()), coefficient_sha256=digest(path))
            record['cases'].append(case)
            if data is not None and previous is not None:
                before_rate, before_case, before = previous
                change = float(np.linalg.norm(state(data)-state(before))/np.linalg.norm(state(data)))
                forces = force_bins(data, rate)
                force_change = float(np.linalg.norm(forces-force_bins(before, before_rate))/np.linalg.norm(forces))
                work_passed = case['mechanical_work_1e9_gate_passed'] and before_case['mechanical_work_1e9_gate_passed']
                record['comparisons'].append(dict(
                    coarse_rate_hz=before_rate, fine_rate_hz=rate,
                    state_change_relative=change, force_bin_change_relative=force_change,
                    work_1e9_gate_passed=work_passed,
                    state_force_1percent_gate_passed=change <= .01 and force_change <= .01,
                    scoped_reference_pair_passed=work_passed and change <= .01 and force_change <= .01))
                print('reference refinement', before_rate, rate, 'state', change,
                      'force', force_change, 'work', case['mechanical_work_relative_max'], flush=True)
            if data is not None:
                previous = (rate, case, data)
                highest = (rate, path, data)
            save()
        if highest is not None:
            rate, path, reference = highest
            for block in [1, 16]:
                case, data = run(f'ramp-{rate}-N{block}', path, 'verify-ramp', block)
                case.update(rate_hz=rate, micro_block=block, macro_duration_s=block/rate)
                if data is not None:
                    change = float(np.linalg.norm(state(data)-state(reference))/np.linalg.norm(state(reference)))
                    force_change = float(np.linalg.norm(force_bins(data, rate)-force_bins(reference, rate))
                                         /np.linalg.norm(force_bins(reference, rate)))
                    case.update(state_change_relative=change, force_bin_change_relative=force_change,
                                state_force_1percent_gate_passed=change <= .01 and force_change <= .01)
                    print('ramp versus fine reference', rate, block, 'state', change, 'force', force_change, flush=True)
                record['ramp_comparisons'].append(case)
                save()
    record['highest_requested_rate_completed'] = record['cases'][-1]['completed']
    record['highest_pair_passed'] = (record['highest_requested_rate_completed']
                                   and bool(record['comparisons'])
                                   and record['comparisons'][-1]['coarse_rate_hz'] == rates[-2]
                                   and record['comparisons'][-1]['fine_rate_hz'] == rates[-1]
                                   and record['comparisons'][-1]['scoped_reference_pair_passed'])
    record['sources_unchanged'] = all(digest(root/'tools'/name) == sha
                                      for name, sha in parent['source_sha256'].items())
    record['verifier_unchanged'] = digest(Path(__file__)) == record['verifier_sha256']
    save()


if __name__ == '__main__':
    main()
