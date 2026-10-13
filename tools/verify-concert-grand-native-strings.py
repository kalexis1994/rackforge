"""Authenticate the measured choir and check a longer live restrike.

Compare the blocked solver against the original native microclock, retaining
all 96/192 modes. This is a fixed-bridge component check, not piano approval.
"""
import argparse
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
    parser.add_argument('--micro-blocks', type=int, nargs='+')
    args = parser.parse_args()
    if args.local.exists() or args.report.exists():
        raise ValueError('fresh verification paths required')
    root = Path(__file__).resolve().parent.parent
    parent = json.loads(args.measurement.read_text(encoding='utf-8'))
    for name, sha in parent['source_sha256'].items():
        if digest(root / 'tools' / name) != sha:
            raise ValueError(f'measured source changed: {name}')
    exe = Path(parent['executable_path'])
    if digest(exe) != parent['executable_sha256']:
        raise ValueError('measured executable changed')
    if not parent['ramped_nonlinearity']:
        raise ValueError('ramp measurement required')
    blocks = args.micro_blocks or [parent['nonlinear_micro_block']]
    if any(not 1 <= block <= 64 for block in blocks):
        raise ValueError('blocks must contain 1..64 microsteps')
    args.local.mkdir(parents=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    start = time.perf_counter()
    record = dict(
        parent_path=str(args.measurement.resolve()),
        parent_sha256=digest(args.measurement),
        verifier_sha256=digest(Path(__file__)),
        source_sha256=parent['source_sha256'],
        executable_sha256=digest(exe),
        duration_s=.25, restrike_s=.125, restrike_speed_m_s=.1,
        fixed_bridge=True, moving_body_attached=False,
        pressure_included=False, pedals_included=False,
        full_piano_realtime=False, global_physics_approved=False,
        cases=[],
    )

    def save():
        record['seconds'] = time.perf_counter() - start
        args.report.write_text(json.dumps(record, indent=2, allow_nan=False) + '\n', encoding='utf-8')

    save()
    with threadpool_limits(limits=1):
        for rate in [44100, 128000]:
            print('extended live choir', rate, flush=True)
            f = fixture(transverse=96, rate=rate, duration=.25, second_time=.125, second_speed=.1)
            path = args.local / f'live-96-{rate}.bin'
            write_fixture(path, f)
            reference = json.loads(subprocess.check_output(
                [str(exe), str(path), 'verify'], text=True, stderr=subprocess.STDOUT))
            # Only final states and the original 6 ms force probe, no histories.
            reference_path = args.local / f'reference-{rate}.json'
            reference_path.write_text(json.dumps(reference, allow_nan=False) + '\n', encoding='utf-8')
            omega = np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
            expected = np.r_[omega * np.asarray(reference['q'])[:-1], reference['v']]
            for block in blocks:
                case = dict(rate_hz=rate, transverse=96, axial=192, micro_block=block,
                            coefficient_path=str(path.resolve()), coefficient_sha256=digest(path),
                            reference_path=str(reference_path.resolve()), reference_sha256=digest(reference_path))
                try:
                    native = json.loads(subprocess.check_output(
                        [str(exe), str(path), 'verify-ramp', str(block)],
                        text=True, stderr=subprocess.STDOUT))
                    observed = np.r_[omega * np.asarray(native['q'])[:-1], native['v']]
                    state = float(np.linalg.norm(observed - expected) / np.linalg.norm(expected))
                    force = float(np.linalg.norm(np.asarray(native['force_trace']) - reference['force_trace'])
                                  / np.linalg.norm(reference['force_trace']))
                    case.update(
                        completed=True, blocked_state_relative=state, attack_force_relative=force,
                        accuracy_1percent_gate_passed=state <= .01 and force <= .01,
                        mechanical_work_relative_max=native['work_relative_max'],
                        mechanical_work_1e9_gate_passed=native['work_relative_max'] <= 1e-9,
                        reference_work_relative_max=reference['work_relative_max'],
                        launch_work_j=native['launch_work_j'], felt_heat_j=native['heat_j'],
                        maximum_outer_iterations=native['maximum_outer_iterations'],
                        note='Force trace covers first 6 ms; state/work cover the full 250 ms and live restrike.',
                    )
                    print('extended result', rate, block, 'state', state, 'work', native['work_relative_max'], flush=True)
                except subprocess.CalledProcessError as error:
                    case.update(completed=False, failure=error.output)
                    print('extended failure', rate, block, error.output, flush=True)
                record['cases'].append(case)
                save()
    record['sources_unchanged'] = all(digest(root / 'tools' / name) == sha
                                      for name, sha in parent['source_sha256'].items())
    record['verifier_unchanged'] = digest(Path(__file__)) == record['verifier_sha256']
    save()


if __name__ == '__main__':
    main()
