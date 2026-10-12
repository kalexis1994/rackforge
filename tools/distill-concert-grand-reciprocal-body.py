"""Same all-mode spatial student, one reciprocal row and fused arithmetic."""
import argparse
import importlib.util
import json
from pathlib import Path
import struct
import subprocess
import time
import numpy as np
from scipy.linalg import solve, svd
from concert_grand_projected_support import ModalSupportMap, projected_matrices
from concert_grand_realtime_body import Body, modal_response
from concert_grand_modal_runtime import ModalRuntime
from concert_grand_reciprocal_runtime import ReciprocalRuntime
import concert_grand_orthotropic_plate as plate

spec = importlib.util.spec_from_file_location('spatial_pilot', Path(__file__).with_name('distill-concert-grand-spatial-body.py'))
prior = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prior)
digest, run, verify = prior.digest, prior.run, prior.verify


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--local', type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists() or args.local.exists():
        raise ValueError('preserve previous measured outputs')
    root = Path(__file__).resolve().parent.parent
    docs = root/'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P7-cuerpo-realtime-2026-10-12'
    parent_path = docs/'espacial.json'
    parent = json.loads(parent_path.read_text())
    for name, sha in parent['source_sha256'].items():
        if digest(root/'tools'/name) != sha:
            raise ValueError('spatial parent changed')
    dense = json.loads((docs/'destilacion.json').read_text())
    for name, sha in dense['sources_sha256'].items():
        if digest(root/'tools'/name) != sha:
            raise ValueError('reference construction changed')
    source = root/'local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    if digest(source) != parent['teacher_sha256']:
        raise ValueError('teacher changed')
    names = [Path(__file__).name, 'concert_grand_reciprocal_runtime.py', 'concert-grand-reciprocal-body.rs', 'concert_grand_modal_runtime.py']
    hashes = {name: digest(root/'tools'/name) for name in names}
    args.local.mkdir(parents=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    began = time.perf_counter()
    report = dict(stage='preparation', parent_report_sha256=digest(parent_path), source_sha256=hashes,
        teacher_sha256=digest(source), scope=parent['scope'], body_callback_budget_fraction=.20,
        full_piano_realtime=False, global_physics_approved=False, mechanical_poles_removed=0,
        compiler_flags=parent['compiler_flags'], force_and_velocity_same_spatial_projection=True,
        arithmetic='F64 states and coefficients; fused multiply-add; no fast-math', cases=[])
    def save():
        report['seconds'] = time.perf_counter()-began
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8')
    save()
    core = (root/'tools/concert-grand-reciprocal-body.rs').read_text()
    wrapper = (root/'tools/concert-grand-spatial-body.rs').read_text()
    include = 'include!("concert-grand-modal-body.rs");'
    if wrapper.count(include) != 1:
        raise ValueError('wrapper include changed')
    generated = args.local/'native-reciprocal.rs'
    generated.write_text(wrapper.replace(include, core), encoding='utf-8')
    report['expanded_native_source_sha256'] = digest(generated)
    executable = args.local/'reciprocal-body-bench.exe'
    subprocess.run(['rustc', '--edition=2021', '-O', '-C', 'target-cpu=native', str(generated), '-o', str(executable)], check=True)
    generated.unlink()
    report['rustc'] = subprocess.check_output(['rustc', '--version'], text=True).strip()
    with np.load(source) as data:
        support = ModalSupportMap.from_matrices(*projected_matrices(data))
        geometry = dict(panel=json.loads(str(data['panel'])), boundary=str(data['boundary']),
                        nx=int(data['nx']), ny=int(data['ny']), vectors=data['ritz_vectors'])
        xs = geometry['panel']['length']*(.1+.8*(np.arange(21, 109)-21)/87)
        ports = (plate.shape(geometry, xs, np.full(88, geometry['panel']['width']/2)) @ support.basis).T
    omega = 2*np.pi*support.frequency
    full = Body.mechanical(omega, support.damping, ports)
    full_modal = ModalRuntime.from_body(full)
    train = ports[:, np.array(parent['training_keys'])-21]
    snapshots = []
    for hz in parent['training_frequency_hz']:
        s = 2j*np.pi*hz
        velocity = solve(np.diag(omega**2+s*s)+s*support.damping, s*train, check_finite=False)
        response = ports.T @ velocity
        scale = max(np.linalg.norm(response), 1e-30)
        snapshots.extend([response.real/scale, response.imag/scale])
    spatial, _, _ = svd(np.hstack(snapshots), full_matrices=False, check_finite=False)
    held = np.array(parent['held_out_input_keys'])-21
    frequencies = np.array(parent['held_out_frequency_hz'])
    reference = modal_response(omega, support.damping, ports[:, held], frequencies)
    norm = np.linalg.norm(reference, axis=(1, 2))
    report.update(training_keys=parent['training_keys'], held_out_input_keys=parent['held_out_input_keys'],
        training_frequency_hz=parent['training_frequency_hz'], held_out_frequency_hz=frequencies.tolist(),
        training_output_points=parent['training_output_points'])
    for latent in [16, 20, 24, 32]:
        mapping = spatial[:, :latent]
        modal = ModalRuntime(full_modal.roots, full_modal.vectors, full_modal.drive @ mapping,
                            (mapping.T @ full_modal.output.T).T, full_modal.multiplicity,
                            full_modal.condition, full_modal.eigen_residual)
        canonical = ReciprocalRuntime(modal)
        response = canonical.modal.response(frequencies, list(range(latent)))
        unchanged = modal.response(frequencies, list(range(latent)))
        equivalent = float(np.linalg.norm(response-unchanged)/np.linalg.norm(unchanged))
        if equivalent > 1e-10:
            raise ValueError('reciprocal normalization altered transfer')
        projected = np.einsum('pk,fkl,ql->fpq', mapping[held], response, mapping[held], optimize=True)
        difference = np.linalg.norm(projected-reference, axis=(1, 2))
        errors = difference/np.maximum(norm, 1e-30)
        rms = float(np.sqrt(np.trapezoid(difference**2, frequencies)/np.trapezoid(norm**2, frequencies)))
        worst = float(errors.max())
        row = dict(latent_ports=latent, mechanical_coordinates=606, energy_states=1212,
            mobility_rms_relative=rms, mobility_worst_relative=worst,
            mobility_relative_per_frequency=errors.tolist(), transfer_gate_passed=rms<=.05 and worst<=.10,
            reciprocal_normalization_transfer_relative=equivalent,
            symmetric_residue_defect_relative=canonical.reciprocity_defect_relative, rates=[])
        mapping_path = args.local/f'ports-{latent}.bin'
        with mapping_path.open('xb') as stream:
            stream.write(struct.pack('<8sII', b'RFPRT001', 88, latent))
            stream.write(np.asarray(mapping, dtype='<f8').tobytes())
        row.update(mapping_path=str(mapping_path.resolve()), mapping_sha256=digest(mapping_path), mapping_bytes=mapping_path.stat().st_size)
        for rate in [44100, 48000]:
            bundle = args.local/f'reciprocal-body-{latent}-{rate}.bin'
            step = canonical.write(bundle, rate)
            verification = verify(run(executable, bundle, mapping_path, 'verify'), canonical.modal,
                                  canonical.modal, step, mapping)
            benchmark = run(executable, bundle, mapping_path, 'bench')
            fraction = benchmark['p99_ms']/benchmark['deadline_ms']
            benchmark.update(p99_deadline_fraction=fraction, observed_body_budget_passed=fraction<=.20
                and benchmark['max_ms']<=benchmark['deadline_ms'] and benchmark['callback_allocations']==0)
            row['rates'].append(dict(coefficient_path=str(bundle.resolve()), coefficient_sha256=digest(bundle),
                coefficient_bytes=bundle.stat().st_size, verification=verification, benchmark=benchmark))
        row['body_budget_passed_both_rates'] = all(r['benchmark']['observed_body_budget_passed'] for r in row['rates'])
        row['both_fidelity_and_body_budget_passed'] = row['transfer_gate_passed'] and row['body_budget_passed_both_rates']
        report['cases'].append(row)
        report['stage'] = 'measuring shared reciprocal mode rows'
        save()
        print('reciprocal ports', latent, 'rms', rms, 'worst', worst, 'p99 fractions', [r['benchmark']['p99_deadline_fraction'] for r in row['rates']], flush=True)
    joint = [r for r in report['cases'] if r['both_fidelity_and_body_budget_passed']]
    budget = [r for r in report['cases'] if r['body_budget_passed_both_rates']]
    chosen = min(joint, key=lambda r:r['latent_ports']) if joint else (min(budget, key=lambda r:r['mobility_rms_relative']) if budget else None)
    report['candidate_latent_ports'] = chosen['latent_ports'] if chosen else None
    report['both_gates_passed'] = bool(joint)
    for row in report['cases']:
        row['mapping_retained'] = row is chosen
        if row is not chosen:
            Path(row['mapping_path']).unlink()
        for rate in row['rates']:
            rate['coefficient_retained'] = row is chosen
            if row is not chosen:
                Path(rate['coefficient_path']).unlink()
    report.update(stage='reciprocal single-row dry body measured',
        source_and_teacher_unchanged=digest(source)==report['teacher_sha256'] and all(digest(root/'tools'/name)==sha for name,sha in hashes.items()),
        native_python_and_energy_verification_scope='Native final state/output checksum/work versus Python; per-step energy on Python modal trajectory reconstructed in mechanical energy coordinates.',
        remaining=dense['remaining'])
    save()
    print('reciprocal stage complete', report['seconds'], 's; candidate', report['candidate_latent_ports'], 'both gates', report['both_gates_passed'], flush=True)


if __name__ == '__main__':
    main()
