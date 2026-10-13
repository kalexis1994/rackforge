"""Measure grouped axial contractions against authenticated scalar trajectories."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import time
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_native_strings import fixture, write_fixture
from concert_grand_phase_forces import relative
from concert_grand_wave_cache import digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference-report', type=Path, required=True)
    parser.add_argument('--reference-ref', required=True)
    parser.add_argument('--local', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    if args.local.exists() or args.report.exists():
        raise ValueError('fresh paths required')
    root = Path(__file__).resolve().parents[1]
    old = json.loads(args.reference_report.read_text())
    report_path = args.reference_report.resolve().relative_to(root).as_posix()
    frozen = subprocess.check_output(['git', 'show', args.reference_ref+':'+report_path], cwd=root)
    if frozen != args.reference_report.read_bytes():
        raise ValueError('reference report changed')
    for name, sha in old['source_sha256'].items():
        source = subprocess.check_output(['git', 'show', args.reference_ref+':tools/'+name], cwd=root)
        if hashlib.sha256(source).hexdigest() != sha:
            raise ValueError('frozen source mismatch: '+name)
    baseline = next(c for c in old['cases'] if c['label'] == 'independent-wire-roots')
    for entry in [baseline, *old['teacher_cases']]:
        for prefix in ['result', 'coefficient']:
            if digest(Path(entry[prefix+'_path'])) != entry[prefix+'_sha256']:
                raise ValueError('reference data changed')
    if digest(Path(old['executable_path'])) != old['executable_sha256']:
        raise ValueError('baseline executable changed')
    names = list(old['source_sha256'])+[Path(__file__).name]
    source = {name: digest(root/'tools'/name) for name in names}
    spec = importlib.util.spec_from_file_location('wire_audit', root/'tools/audit-concert-grand-phase-wire-energy.py')
    audit = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(audit)
    args.local.mkdir(parents=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    exe = args.local/'trajectory.exe'
    subprocess.run(['rustc', '--edition=2021', '-O', '-C', 'target-cpu=native', '--cfg',
                    'concert_phase_library', str(root/'tools/concert-grand-phase-trajectory.rs'), '-o', str(exe)], check=True)
    record = dict(stage='cached amplitudes and grouped axial waveform contraction', source_sha256=source,
                  reference_report_path=str(args.reference_report.resolve()), reference_report_sha256=digest(args.reference_report),
                  reference_ref=args.reference_ref, reference_sources_authenticated_from_git=True,
                  reference_reintegrated=False, baseline_case=baseline,
                  baseline_executable_path=old['executable_path'], baseline_executable_sha256=old['executable_sha256'],
                  executable_path=str(exe.resolve()), executable_sha256=digest(exe),
                  teacher_cases=old['teacher_cases'], reference_pair_passed=old['reference_pair_passed'],
                  reference_state_refinement_relative=old['reference_state_refinement_relative'],
                  transverse=96, axial=192, note=60, seed=83, rate_hz=2048000, frames=1, order=8, duration_s=.006,
                  contact_included=False, pressure_included=False, pedals_included=False,
                  factory_updated=False, full_piano_realtime=False, global_physics_approved=False,
                  numerical_energy_constraint=True, physical_dissipation=False, heat_j=0., cases=[], benchmarks=[])
    started = time.perf_counter()

    def save():
        record['seconds'] = time.perf_counter()-started
        args.report.write_text(json.dumps(record, indent=2, allow_nan=False)+'\n', encoding='utf-8')

    with threadpool_limits(limits=1):
        f = fixture(transverse=96, rate=2048000, seed=83, duration=.006)
        f['launch_speeds'][:] = 0.
        path = args.local/'coast-input.bin'
        write_fixture(path, f)
        if digest(path) != baseline['coefficient_sha256']:
            raise ValueError('physical input changed')
        omega = np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
        initial = audit.wire_energies(f, f['q'], f['v'])
        previous = json.loads(Path(baseline['result_path']).read_text())
        teacher = json.loads(Path(old['teacher_cases'][-1]['result_path']).read_text())

        def metric(data):
            return np.r_[omega*np.asarray(data['q'])[:-1], data['v'][:-1]]

        def run(label, executable, mode, flags=()):
            command = [str(executable), str(path), *map(str, mode), *flags]
            begin = time.perf_counter()
            process = subprocess.run(command, capture_output=True, text=True, check=True)
            data = json.loads(process.stdout)
            output = args.local/(label+'.json')
            output.write_text(process.stdout, encoding='utf-8')
            entry = dict(label=label, command=command, elapsed_s=time.perf_counter()-begin,
                         executable_sha256=digest(Path(executable)), coefficient_path=str(path.resolve()),
                         coefficient_sha256=digest(path), result_path=str(output.resolve()), result_sha256=digest(output))
            for key in ['completed', 'failure', 'work_relative_max', 'raw_local_work_relative_max',
                        'numerical_lambda_abs_max', 'midpoint_residual_relative_max', 'maximum_iterations',
                        'allocations', 'known_prepared_bytes', 'per_wire_energy_constraint']:
                entry[key] = data[key]
            final = audit.wire_energies(f, data['q'], data['v'])
            change = np.abs(final-initial)/np.maximum(np.abs(initial), 1e-30)
            entry.update(initial_wire_energy_j=initial.tolist(), final_wire_energy_j=final.tolist(),
                         wire_energy_relative=change.tolist(), maximum_wire_energy_relative=float(max(change)),
                         per_wire_1e9_gate_passed=bool(data['completed'] and max(change) <= 1e-9),
                         total_work_1e9_gate_passed=data['completed'] and data['work_relative_max'] <= 1e-9)
            if mode[0] == 'verify':
                error = relative(metric(data), metric(teacher))
                control_error = relative(metric(data), metric(previous))
                entry.update(state_relative=error, baseline_state_relative=control_error,
                             baseline_1e11_gate_passed=control_error <= 1e-11,
                             frozen_baseline_arrays_and_work_equal=all(data[k] == previous[k] for k in
                                                                      ['q', 'v', 'work_relative_max', 'numerical_lambda_abs_max']),
                             scoped_coast_state_and_energy_passed=old['reference_pair_passed'] and data['completed']
                                 and error <= .01 and entry['per_wire_1e9_gate_passed'] and entry['total_work_1e9_gate_passed'])
                record['cases'].append(entry)
                print(label, 'state', error, 'baseline change', control_error, 'wire work', max(change), flush=True)
            else:
                entry['benchmark'] = {k: data[k] for k in ['buffers', 'mean_ms', 'p99_ms', 'max_ms', 'allocations']}
                entry['benchmark'].update(deadline_ms=.125, physical_duration_s=.001, microframes_per_buffer=256,
                                          isolated_component_budget_passed=data['completed'] and data['max_ms'] <= .125
                                              and data['buffers'] == 8 and data['allocations'] == 0)
                record['benchmarks'].append(entry)
                print(label, entry['benchmark'], flush=True)
            save()

        save()
        run('grouped-waveform', exe, ['verify', 1, 8])
        run('cached-scalar-control', exe, ['verify', 1, 8], ['--scalar-axial-waveform'])
        # Sequential advancing runs on identical state/input; no tests or other
        # benchmarks run concurrently. The scalar flag changes only the waveform.
        run('frozen-baseline-benchmark', old['executable_path'], ['bench', 1, 8, 8])
        run('cached-scalar-benchmark', exe, ['bench', 1, 8, 8], ['--scalar-axial-waveform'])
        run('grouped-benchmark', exe, ['bench', 1, 8, 8])
        run('grouped-repeat-benchmark', exe, ['bench', 1, 8, 8])
    record['sources_unchanged'] = all(digest(root/'tools'/name) == sha for name, sha in source.items())
    record['reference_report_unchanged'] = digest(args.reference_report) == record['reference_report_sha256']
    save()


if __name__ == '__main__':
    main()
