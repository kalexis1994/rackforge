"""Keep all mechanical poles; reduce only reciprocal force/velocity geometry."""
import argparse
import hashlib
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
import concert_grand_orthotropic_plate as plate


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def run(executable, bundle, mapping, mode):
    return json.loads(subprocess.check_output([str(executable), str(bundle), str(mapping), mode], text=True))


def verify(native, full_modal, modal, step, mapping):
    w = np.zeros(len(modal.roots), dtype=complex)
    checksum = work = old_energy = maximum_excess = 0.
    for frame in range(1200):
        force = ((frame*17+np.arange(88)*13)%97-48)/48 if frame < 400 else np.zeros(88)
        w, latent_velocity = step.step(w, mapping.T @ force)
        y = mapping @ latent_velocity
        x = full_modal.reconstruct(w)
        energy = float(x @ x / 2)
        injected = float(step.dt*force @ y)
        maximum_excess = max(maximum_excess, energy-old_energy-injected)
        old_energy = energy
        work += injected
        checksum += (np.arange(88)+1) @ y
    nw = np.asarray(native['real'])+1j*np.asarray(native['imag'])
    np.testing.assert_allclose(nw, w, rtol=1e-8, atol=1e-14)
    np.testing.assert_allclose([native['checksum'], native['work_j']], [checksum, work], rtol=2e-10, atol=1e-13)
    relative = float(maximum_excess/max(abs(work), energy, 1e-30))
    if relative > 1e-10:
        raise ValueError('spatial port energy excess')
    return dict(native_python_final_states_outputs_and_work_passed=True,
        python_reconstructed_step_energy_excess_relative=relative,
        eigenvectors_condition=full_modal.condition, eigen_equation_relative=full_modal.eigen_residual)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--local', type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists() or args.local.exists():
        raise ValueError('preserve prior measurements')
    root = Path(__file__).resolve().parent.parent
    docs = root/'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P7-cuerpo-realtime-2026-10-12'
    parent_path = docs/'destilacion.json'
    modal_parent = docs/'modal.json'
    parent = json.loads(parent_path.read_text())
    for name, sha in parent['sources_sha256'].items():
        if digest(root/'tools'/name) != sha:
            raise ValueError('dense reference sources changed')
    for name, sha in json.loads(modal_parent.read_text())['source_sha256'].items():
        if digest(root/'tools'/name) != sha:
            raise ValueError('modal reference sources changed')
    source = root/'local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    if digest(source) != parent['teacher_sha256']:
        raise ValueError('teacher changed')
    names = [Path(__file__).name, 'concert-grand-spatial-body.rs']
    hashes = {name: digest(root/'tools'/name) for name in names}
    args.local.mkdir(parents=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    began = time.perf_counter()
    report = dict(stage='preparation', parent_report_sha256=digest(parent_path), modal_parent_sha256=digest(modal_parent),
        source_sha256=hashes, teacher_sha256=digest(source), scope=parent['scope'],
        full_piano_realtime=False, global_physics_approved=False, body_callback_budget_fraction=.20,
        force_and_velocity_same_spatial_projection=True, mechanical_poles_removed=0, cases=[],
        compiler_flags=['--edition=2021', '-O', '-C', 'target-cpu=native'],
        compiler_portability='Host-native x86 CPU build only; portable/WASM fallback not measured.')
    def save():
        report['seconds'] = time.perf_counter()-began
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8')
    save()
    core = (root/'tools/concert-grand-modal-body.rs').read_text()
    # Include the measured kernel without changing an executable statement.
    core = '\n'.join(line.replace('//!', '//', 1) if line.startswith('//!') else line for line in core.splitlines())
    wrapper = (root/'tools/concert-grand-spatial-body.rs').read_text()
    include = 'include!("concert-grand-modal-body.rs");'
    if wrapper.count(include) != 1:
        raise ValueError('kernel include marker changed')
    generated = args.local/'native-spatial.rs'
    generated.write_text(wrapper.replace(include, core), encoding='utf-8')
    report['expanded_native_source_sha256'] = digest(generated)
    executable = args.local/'spatial-body-bench.exe'
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
    report['teacher_frequency_range_hz'] = [float(support.frequency.min()), float(support.frequency.max())]
    report['mechanical_modes_above_22050_hz'] = int(np.sum(support.frequency > 22050))
    print('full mechanical modal realization and force geometry', flush=True)
    full_modal = ModalRuntime.from_body(full)
    train = ports[:, np.array(parent['training_keys'])-21]
    snapshots = []
    for hz in parent['training_frequency_hz']:
        s = 2j*np.pi*hz
        q = solve(np.diag(omega**2+s*s)+s*support.damping, s*train, check_finite=False)
        response = ports.T @ q
        scale = max(np.linalg.norm(response), 1e-30)
        snapshots.extend([response.real/scale, response.imag/scale])
    spatial, _, _ = svd(np.hstack(snapshots), full_matrices=False, check_finite=False)
    held = np.array(parent['held_out_keys'])-21
    frequencies = np.array(parent['held_out_frequency_hz'])
    reference = modal_response(omega, support.damping, ports[:, held], frequencies)
    norm = np.linalg.norm(reference, axis=(1, 2))
    report.update(training_keys=parent['training_keys'], held_out_input_keys=parent['held_out_keys'],
        training_output_points='All 88 force/velocity points; held-out frequencies and input keys were not driven in training.',
        training_frequency_hz=parent['training_frequency_hz'], held_out_frequency_hz=frequencies.tolist())
    for latent in [8, 16, 24, 32, 48, 64, 88]:
        mapping = spatial[:, :latent]
        np.testing.assert_allclose(mapping.T @ mapping, np.eye(latent), atol=2e-12)
        modal = ModalRuntime(full_modal.roots, full_modal.vectors, full_modal.drive @ mapping,
                             mapping.T @ full_modal.output.T, full_modal.multiplicity,
                             full_modal.condition, full_modal.eigen_residual)
        # Output expects modes x ports, not ports x modes.
        modal.output = modal.output.T
        response = modal.response(frequencies, list(range(latent)))
        held_map = mapping[held]
        projected = np.einsum('pk,fkl,ql->fpq', held_map, response, held_map, optimize=True)
        difference = np.linalg.norm(projected-reference, axis=(1, 2))
        per_frequency = difference/np.maximum(norm, 1e-30)
        rms = float(np.sqrt(np.trapezoid(difference**2, frequencies)/np.trapezoid(norm**2, frequencies)))
        worst = float(per_frequency.max())
        row = dict(latent_ports=latent, full_mechanical_coordinates=606, full_energy_states=1212,
            mobility_rms_relative=rms, mobility_worst_relative=worst,
            mobility_relative_per_frequency=per_frequency.tolist(),
            transfer_gate_passed=rms<=.05 and worst<=.10, rates=[])
        mapping_path = args.local/f'ports-{latent}.bin'
        with mapping_path.open('xb') as stream:
            stream.write(struct.pack('<8sII', b'RFPRT001', 88, latent))
            stream.write(np.asarray(mapping, dtype='<f8').tobytes())
        row.update(mapping_path=str(mapping_path.resolve()), mapping_sha256=digest(mapping_path), mapping_bytes=mapping_path.stat().st_size)
        for rate in [44100, 48000]:
            step = modal.coefficients(rate)
            bundle = args.local/f'spatial-body-{latent}-{rate}.bin'
            step.write(bundle)
            verification = verify(run(executable, bundle, mapping_path, 'verify'), full_modal, modal, step, mapping)
            benchmark = run(executable, bundle, mapping_path, 'bench')
            fraction = benchmark['p99_ms']/benchmark['deadline_ms']
            benchmark.update(p99_deadline_fraction=fraction, observed_body_budget_passed=fraction<=.20
                and benchmark['max_ms']<=benchmark['deadline_ms'] and benchmark['callback_allocations']==0)
            row['rates'].append(dict(coefficient_path=str(bundle.resolve()), coefficient_sha256=digest(bundle),
                coefficient_bytes=bundle.stat().st_size, verification=verification, benchmark=benchmark))
        row['body_budget_passed_both_rates'] = all(r['benchmark']['observed_body_budget_passed'] for r in row['rates'])
        row['both_fidelity_and_body_budget_passed'] = row['transfer_gate_passed'] and row['body_budget_passed_both_rates']
        report['cases'].append(row)
        report['stage'] = 'measuring spatial latent port runtime'
        save()
        print('spatial ports', latent, 'rms', rms, 'worst', worst, 'p99 fractions', [r['benchmark']['p99_deadline_fraction'] for r in row['rates']], flush=True)
    joint = [row for row in report['cases'] if row['both_fidelity_and_body_budget_passed']]
    budget = [row for row in report['cases'] if row['body_budget_passed_both_rates']]
    chosen = min(joint, key=lambda r:r['latent_ports']) if joint else (min(budget, key=lambda r:r['mobility_rms_relative']) if budget else None)
    report['candidate_latent_ports'] = chosen['latent_ports'] if chosen else None
    report['candidate_transfer_gate_passed'] = chosen['transfer_gate_passed'] if chosen else False
    report['both_gates_passed'] = bool(joint)
    for row in report['cases']:
        retained = row is chosen
        row['mapping_retained'] = retained
        if not retained:
            Path(row['mapping_path']).unlink()
        for rate in row['rates']:
            rate['coefficient_retained'] = retained
            if not retained:
                Path(rate['coefficient_path']).unlink()
    report.update(stage='spatially distilled reciprocal dry body measured',
        source_and_teacher_unchanged=digest(source)==report['teacher_sha256'] and all(digest(root/'tools'/name)==sha for name,sha in hashes.items()),
        continuous_transfer_and_runtime_equivalence_separate=True,
        no_samples_no_audio_no_new_R=True)
    save()
    print('spatial stage complete', report['seconds'], 's; candidate', report['candidate_latent_ports'], flush=True)


if __name__ == '__main__':
    main()
