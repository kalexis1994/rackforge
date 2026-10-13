"""Authenticate bounded ramp recovery against manual subdivision and AAVF.

The seeded sustained fixture reproduces the previously rejected 254.875 ms
block. Completion, temporal accuracy, energy and CPU have separate gates.
Only small final states are saved locally; no audio or factory promotion.
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
    parser.add_argument('--control-measurement', type=Path, required=True)
    parser.add_argument('--local', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    if args.local.exists() or args.report.exists():
        raise ValueError('fresh recovery verification paths required')
    root = Path(__file__).resolve().parent.parent
    parent = json.loads(args.measurement.read_text(encoding='utf-8'))
    control = json.loads(args.control_measurement.read_text(encoding='utf-8'))
    for name, sha in parent['source_sha256'].items():
        if digest(root / 'tools' / name) != sha:
            raise ValueError(f'measured source changed: {name}')
        if name not in {'measure-concert-grand-native-strings.py', 'concert-grand-native-strings.rs'}:
            if control['source_sha256'].get(name) != sha:
                raise ValueError(f'control physics changed: {name}')
    if not (parent['ramped_nonlinearity'] and parent.get('adaptive_ramp')
            and parent['nonlinear_micro_block'] == 16
            and parent['correction_direction'] == 'parallel to string displacement'):
        raise ValueError('determined adaptive N16 measurement required')
    if parent.get('math_backend') != 'prepared CPU dispatch':
        raise ValueError('dispatch measurement required; portable is checked separately')
    exe = Path(parent['executable_path'])
    old_exe = Path(control['executable_path'])
    if digest(exe) != parent['executable_sha256'] or digest(old_exe) != control['executable_sha256']:
        raise ValueError('measured executable changed')
    args.local.mkdir(parents=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    started = time.perf_counter()
    record = dict(
        parent_path=str(args.measurement.resolve()), parent_sha256=digest(args.measurement),
        control_parent_path=str(args.control_measurement.resolve()),
        control_parent_sha256=digest(args.control_measurement),
        source_sha256=parent['source_sha256'], verifier_sha256=digest(Path(__file__)),
        executable_sha256=digest(exe), control_executable_sha256=digest(old_exe),
        fixed_bridge=True, moving_body_attached=False, pressure_included=False,
        pedals_included=False, full_piano_realtime=False, global_physics_approved=False,
        seed=83, rate_hz=128000, transverse=96, axial=192, micro_block=16,
        duration_s=.256, restrike_speed_m_s=0., mechanical_coordinates=1153,
        rejected_frame=32624, manual_boundaries=[32632, 32640], cases=[], benchmarks=[],
    )

    def save():
        record['seconds'] = time.perf_counter() - started
        args.report.write_text(json.dumps(record, indent=2, allow_nan=False) + '\n', encoding='utf-8')

    def run(label, executable, path, mode, *flags):
        started_case = time.perf_counter()
        output = subprocess.run([str(executable), str(path), mode, *map(str, flags)],
                                capture_output=True, text=True)
        result = dict(label=label, mode=mode, flags=list(flags), completed=output.returncode == 0,
                      seconds=time.perf_counter() - started_case)
        if output.returncode:
            result.update(returncode=output.returncode, failure=output.stderr)
            return result, None
        data = json.loads(output.stdout)
        final_path = args.local / f'{label}.json'
        final_path.write_text(json.dumps(data, allow_nan=False) + '\n', encoding='utf-8')
        result.update(result_path=str(final_path.resolve()), result_sha256=digest(final_path))
        return result, data

    save()
    with threadpool_limits(limits=1):
        f = fixture(transverse=96, rate=128000, seed=83, duration=.256, second_speed=0.)
        path = args.local / 'sustained-mixed.bin'
        write_fixture(path, f)
        record.update(coefficient_path=str(path.resolve()), coefficient_sha256=digest(path))
        omega = np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]

        def metric(data):
            return np.r_[omega * np.asarray(data['q'])[:-1], data['v']]

        def change(a, b):
            return float(np.linalg.norm(metric(a) - metric(b)) / np.linalg.norm(metric(b)))

        frozen, _ = run('frozen-rejection', old_exe, path, 'verify-ramp', 16)
        disabled, _ = run('disabled-rejection', exe, path, 'verify-ramp', 16, '--no-ramp-refinement')
        record['cases'].extend([frozen, disabled])
        save()
        recovered_case, recovered = run('recovered', exe, path, 'verify-ramp', 16)
        manual_case, manual = run('manual-split', exe, path, 'verify-ramp', 16,
                                  '--no-ramp-refinement', '--ramp-boundaries=32632,32640')
        record['cases'].extend([recovered_case, manual_case])
        save()
        if recovered is not None and manual is not None:
            record.update(
                manual_state_change_relative=change(recovered, manual),
                manual_bitwise_equal=all(recovered[k] == manual[k]
                                         for k in ['q', 'v', 'force_trace', 'heat_j', 'launch_work_j']),
                mechanical_work_relative_max=recovered['work_relative_max'],
                mechanical_work_1e9_gate_passed=recovered['work_relative_max'] <= 1e-9,
                rejected_attempts=recovered['ramp_rejected_attempts'],
                refined_requests=recovered['ramp_refined_requests'],
                refined_children=recovered['ramp_refined_child_blocks'],
                smallest_refined_block=recovered['smallest_refined_block'],
                maximum_ramp_attempts=recovered['maximum_ramp_attempts'],
                force_peak_n=recovered['force_peak_n'], felt_heat_j=recovered['heat_j'],
            )
        save()
        print('recovery and explicit subdivision saved; checking independent microclock', flush=True)
        reference_case, reference = run('monolithic-reference', exe, path, 'verify')
        record['cases'].append(reference_case)
        save()
        if reference is not None and recovered is not None:
            force = float(np.linalg.norm(np.asarray(recovered['force_trace']) - reference['force_trace'])
                          / np.linalg.norm(reference['force_trace']))
            state = change(recovered, reference)
            record.update(blocked_state_relative=state, attack_force_relative=force,
                          accuracy_1percent_gate_passed=state <= .01 and force <= .01,
                          reference_work_relative_max=reference['work_relative_max'])
            print('mixed sustained temporal error', state, 'force', force, flush=True)
            finer_case, finer = run('finer-N8', exe, path, 'verify-ramp', 8)
            record['cases'].append(finer_case)
            if finer is not None:
                finer_state = change(finer, reference)
                finer_case.update(blocked_state_relative=finer_state,
                                  mechanical_work_relative_max=finer['work_relative_max'],
                                  state_1percent_gate_passed=finer_state <= .01,
                                  refined_requests=finer['ramp_refined_requests'])
                print('finer N8 temporal error', finer_state, flush=True)
        save()
        for buffers in [128, 512]:
            label = f'recovered-benchmark-{buffers}'
            result, bench = run(label, exe, path, 'bench-ramp', buffers, 16)
            if bench is not None:
                result.update(benchmark=bench,
                              isolated_full_buffer_budget_passed=bench['max_ms'] <= bench['deadline_ms']
                              and bench['allocations'] == 0)
                print(label, 'mean', bench['mean_ms'], 'max', bench['max_ms'],
                      'refinements', bench['ramp_refined_requests'], flush=True)
            record['benchmarks'].append(result)
            save()
    record['sources_unchanged'] = all(digest(root / 'tools' / name) == sha
                                      for name, sha in parent['source_sha256'].items())
    record['verifier_unchanged'] = digest(Path(__file__)) == record['verifier_sha256']
    save()


if __name__ == '__main__':
    main()
