"""Measure full-gradient interaction collocation, including rejected orders."""
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
    parser.add_argument('--reuse-forces', action='store_true')
    parser.add_argument('--batch-forces', action='store_true')
    args = parser.parse_args()
    if args.reuse_forces and args.batch_forces:
        raise ValueError('choose a single comparison stage')
    compare_collocation = args.reuse_forces or args.batch_forces
    if args.local.exists() or args.report.exists():
        raise ValueError('fresh paths required')
    root = Path(__file__).resolve().parents[1]
    old = json.loads(args.reference_report.read_text())
    frozen = subprocess.check_output(['git', 'show', args.reference_ref+':'+args.reference_report.resolve().relative_to(root).as_posix()], cwd=root)
    if frozen != args.reference_report.read_bytes():
        raise ValueError('reference report changed')
    for name, sha in old['source_sha256'].items():
        if hashlib.sha256(subprocess.check_output(['git', 'show', args.reference_ref+':tools/'+name], cwd=root)).hexdigest() != sha:
            raise ValueError('frozen source mismatch: '+name)
    baseline_label = 'reused-512000-O8' if args.batch_forces else ('collocation-512000-O8' if args.reuse_forces else 'optimized-trajectory')
    baseline = next(c for c in old['cases'] if c['label'] == baseline_label)
    labels = ['reused-128000-O24','reused-256000-O16','reused-512000-O8'] if args.batch_forces else ['collocation-128000-O24','collocation-256000-O16','collocation-512000-O8']
    controls = [c for c in old['cases'] if c['label'] in labels] if compare_collocation else [baseline]
    for entry in [*controls, *old['teacher_cases']]:
        for prefix in ['result', 'coefficient']:
            if digest(Path(entry[prefix+'_path'])) != entry[prefix+'_sha256']:
                raise ValueError('reference data changed')
    if digest(Path(old['executable_path'])) != old['executable_sha256']:
        raise ValueError('baseline executable changed')
    names = list(old['source_sha256'])+['concert-grand-collocation.rs','concert-grand-batch-cosine.rs', Path(__file__).name]
    sources = {name: digest(root/'tools'/name) for name in names}
    spec = importlib.util.spec_from_file_location('wire_audit', root/'tools/audit-concert-grand-phase-wire-energy.py')
    audit = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(audit)
    args.local.mkdir(parents=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    exe = args.local/'trajectory.exe'
    subprocess.run(['rustc', '--edition=2021', '-O', '-C', 'target-cpu=native', '--cfg', 'concert_phase_library',
                    str(root/'tools/concert-grand-phase-trajectory.rs'), '-o', str(exe)], check=True)
    stage = 'four-node batched cosine transforms' if args.batch_forces else ('reuse forces at converged collocation nodes' if args.reuse_forces else 'full nonlinear gradient at interaction collocation nodes')
    record = dict(stage=stage, source_sha256=sources,
                  reference_ref=args.reference_ref, reference_sources_authenticated_from_git=True,
                  reference_report_path=str(args.reference_report.resolve()), reference_report_sha256=digest(args.reference_report),
                  reference_reintegrated=False, baseline_case=baseline, baseline_executable_path=old['executable_path'],
                  baseline_executable_sha256=old['executable_sha256'], executable_path=str(exe.resolve()), executable_sha256=digest(exe),
                  teacher_cases=old['teacher_cases'], reference_pair_passed=old['reference_pair_passed'],
                  reference_state_refinement_relative=old['reference_state_refinement_relative'],
                  note=60, seed=83, transverse=96, axial=192, duration_s=.006,
                  contact_included=False, pressure_included=False, pedals_included=False, physical_dissipation=False,
                  full_piano_realtime=False, global_physics_approved=False, factory_updated=False, heat_j=0., cases=[], benchmarks=[])
    started = time.perf_counter()
    states = {}

    def save():
        record['seconds'] = time.perf_counter()-started
        args.report.write_text(json.dumps(record, indent=2, allow_nan=False)+'\n', encoding='utf-8')

    with threadpool_limits(limits=1):
        control_fixture = fixture(transverse=96, rate=2048000, seed=83, duration=.006)
        control_fixture['launch_speeds'][:] = 0.
        omega = np.sqrt(np.diag(control_fixture['system'].string.K))[control_fixture['keep'][:-1]]
        teacher = json.loads(Path(old['teacher_cases'][-1]['result_path']).read_text())
        previous = json.loads(Path(baseline['result_path']).read_text())

        def metric(data):
            return np.r_[omega*np.asarray(data['q'])[:-1], data['v'][:-1]]

        def run(label, rate, order, flags, bench=False, executable=exe):
            f = fixture(transverse=96, rate=rate, seed=83, duration=.006)
            f['launch_speeds'][:] = 0.
            for key in ['q', 'v', 'launch_speeds']:
                np.testing.assert_array_equal(f[key], control_fixture[key])
            np.testing.assert_array_equal(np.delete(f['parameters'],4), np.delete(control_fixture['parameters'],4))
            path = args.local/(label+'.bin')
            write_fixture(path, f)
            frozen_control = next((c for c in controls if c.get('rate_hz')==rate and c.get('order')==order),None) if compare_collocation else (baseline if rate==2048000 else None)
            if frozen_control and digest(path) != frozen_control['coefficient_sha256']:
                raise ValueError('control physical input changed')
            microframes = rate//8000
            mode = ['bench', 1, order, 8] if bench else ['verify', 1, order]
            extra = [f'--buffer-microframes={microframes}'] if bench and (executable == exe or compare_collocation) else []
            command = [str(executable), str(path), *map(str, mode), *flags, *extra]
            process = subprocess.run(command, capture_output=True, text=True, check=True)
            data = json.loads(process.stdout)
            output = args.local/(label+'.json')
            output.write_text(process.stdout, encoding='utf-8')
            entry = dict(label=label, rate_hz=rate, order=order, frames=1, command=command,
                         coefficient_path=str(path.resolve()), coefficient_sha256=digest(path),
                         result_path=str(output.resolve()), result_sha256=digest(output), executable_sha256=digest(Path(executable)))
            for key in ['completed', 'failure', 'seconds', 'work_relative_max', 'raw_local_work_relative_max',
                        'numerical_lambda_abs_max', 'midpoint_residual_relative_max', 'maximum_iterations', 'allocations',
                        'known_prepared_bytes', 'per_wire_energy_constraint', 'numerical_energy_constraint']:
                entry[key] = data[key]
            initial = audit.wire_energies(f,f['q'],f['v'])
            final = audit.wire_energies(f,data['q'],data['v'])
            change = np.abs(final-initial)/np.maximum(np.abs(initial),1e-30)
            entry.update(initial_wire_energy_j=initial.tolist(), final_wire_energy_j=final.tolist(),
                         wire_energy_relative=change.tolist(), maximum_wire_energy_relative=float(max(change)),
                         per_wire_1e9_gate_passed=bool(data['completed'] and max(change) <= 1e-9),
                         total_work_1e9_gate_passed=data['completed'] and data['work_relative_max'] <= 1e-9)
            if not bench:
                error = relative(metric(data), metric(teacher))
                entry.update(state_relative=error, state_1percent_gate_passed=data['completed'] and error <= .01,
                             scoped_coast_state_and_energy_passed=old['reference_pair_passed'] and data['completed'] and error <= .01
                                 and entry['per_wire_1e9_gate_passed'] and entry['total_work_1e9_gate_passed'])
                if not flags:
                    entry['frozen_phase_arrays_and_work_equal'] = all(data[k] == previous[k] for k in
                                                                     ['q', 'v', 'work_relative_max', 'numerical_lambda_abs_max'])
                if compare_collocation and frozen_control:
                    control_data = json.loads(Path(frozen_control['result_path']).read_text())
                    control_error = relative(metric(data),metric(control_data))
                    entry.update(frozen_control_state_relative=control_error,control_1e9_gate_passed=control_error<=1e-9,
                                 frozen_control_arrays_and_work_equal=all(data[k]==control_data[k] for k in
                                                                         ['q','v','work_relative_max','numerical_lambda_abs_max']))
                states[label] = metric(data)
                record['cases'].append(entry)
                print(label, 'state',error,'wire work',max(change),'iterations',data['maximum_iterations'],flush=True)
            else:
                entry['benchmark'] = {k:data[k] for k in ['buffers','mean_ms','p99_ms','max_ms','allocations']}
                entry['benchmark'].update(microframes_per_buffer=microframes, deadline_ms=.125, physical_duration_s=.001,
                                          matched_physical_duration=True, isolated_component_budget_passed=data['completed']
                                              and data['buffers']==8 and data['max_ms']<=.125 and data['allocations']==0)
                record['benchmarks'].append(entry)
                print(label,entry['benchmark'],flush=True)
            save()

        save()
        if args.batch_forces:
            for rate,order in [(128000,24),(256000,16),(512000,8)]:
                run(f'batched-{rate}-O{order}',rate,order,['--collocation'])
                run(f'single-{rate}-O{order}',rate,order,['--collocation','--scalar-collocation-forces'])
            run('frozen-benchmark',512000,8,['--collocation'],True,old['executable_path'])
            run('single-benchmark',512000,8,['--collocation','--scalar-collocation-forces'],True)
            run('batched-benchmark',512000,8,['--collocation'],True)
            run('batched-repeat',512000,8,['--collocation'],True)
            run('frozen-repeat',512000,8,['--collocation'],True,old['executable_path'])
            run('batched-256k-benchmark',256000,16,['--collocation'],True)
            record['sources_unchanged'] = all(digest(root/'tools'/name)==sha for name,sha in sources.items())
            record['reference_report_unchanged'] = digest(args.reference_report)==record['reference_report_sha256']
            save()
            return
        if args.reuse_forces:
            for rate,order in [(128000,24),(256000,16),(512000,8)]:
                run(f'reused-{rate}-O{order}',rate,order,['--collocation'])
                run(f'refreshed-{rate}-O{order}',rate,order,['--collocation','--refresh-converged-forces'])
            run('frozen-collocation-benchmark',512000,8,['--collocation'],True,old['executable_path'])
            run('refreshed-benchmark',512000,8,['--collocation','--refresh-converged-forces'],True)
            run('reused-benchmark',512000,8,['--collocation'],True)
            run('reused-repeat',512000,8,['--collocation'],True)
            run('reused-256k-benchmark',256000,16,['--collocation'],True)
            record['sources_unchanged'] = all(digest(root/'tools'/name)==sha for name,sha in sources.items())
            record['reference_report_unchanged'] = digest(args.reference_report)==record['reference_report_sha256']
            save()
            return
        run('preserved-phase-control',2048000,8,[])
        for rate,order in [(128000,16),(128000,24),(128000,32),(256000,16),(256000,24),(512000,8),(512000,16)]:
            run(f'collocation-{rate}-O{order}',rate,order,['--collocation'])
        run('unconstrained-256000-O24',256000,24,['--collocation','--unconstrained'])
        pairs = [('128k-order24-to32','collocation-128000-O24','collocation-128000-O32'),
                 ('256k-order16-to24','collocation-256000-O16','collocation-256000-O24'),
                 ('512k-order8-to16','collocation-512000-O8','collocation-512000-O16'),
                 ('128k-to256k','collocation-128000-O32','collocation-256000-O24'),
                 ('256k-to512k','collocation-256000-O24','collocation-512000-O16')]
        record['collocation_refinement_pairs'] = [dict(label=label,left=a,right=b,state_relative=relative(states[a],states[b]))
                                                 for label,a,b in pairs]
        save()
        run('frozen-phase-benchmark',2048000,8,[],True,old['executable_path'])
        for rate,order in [(128000,24),(256000,16),(512000,8)]:
            run(f'collocation-benchmark-{rate}-O{order}',rate,order,['--collocation'],True)
        run('collocation-repeat-512000-O8',512000,8,['--collocation'],True)
    record['sources_unchanged'] = all(digest(root/'tools'/name)==sha for name,sha in sources.items())
    record['reference_report_unchanged'] = digest(args.reference_report)==record['reference_report_sha256']
    save()


if __name__ == '__main__':
    main()
