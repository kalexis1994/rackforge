"""Reciprocal spatial ports + retained modes + exact static-compliance tail."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import subprocess
import time
import numpy as np
from scipy.linalg import qr, solve, svd
from concert_grand_projected_support import ModalSupportMap, projected_matrices
from concert_grand_realtime_body import Body, modal_response
from concert_grand_modal_runtime import ModalRuntime
import concert_grand_orthotropic_plate as plate

spec = importlib.util.spec_from_file_location('spatial_pilot', Path(__file__).with_name('distill-concert-grand-spatial-body.py'))
prior = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prior)
digest = prior.digest
run = prior.run
verify = prior.verify


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--local', type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists() or args.local.exists():
        raise ValueError('preserve measured coefficients/reports')
    root = Path(__file__).resolve().parent.parent
    docs = root/'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P7-cuerpo-realtime-2026-10-12'
    parent_path = docs/'espacial.json'
    parent = json.loads(parent_path.read_text())
    for name, sha in parent['source_sha256'].items():
        if digest(root/'tools'/name) != sha:
            raise ValueError('spatial parent changed')
    dense_path = docs/'destilacion.json'
    dense = json.loads(dense_path.read_text())
    for name, sha in dense['sources_sha256'].items():
        if digest(root/'tools'/name) != sha:
            raise ValueError('teacher construction changed')
    source = root/'local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    if digest(source) != parent['teacher_sha256']:
        raise ValueError('teacher changed')
    names = [Path(__file__).name, 'concert_grand_modal_runtime.py', 'concert-grand-modal-body.rs']
    hashes = {name: digest(root/'tools'/name) for name in names}
    args.local.mkdir(parents=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    began = time.perf_counter()
    report = dict(stage='preparation', parent_report_sha256=digest(parent_path),
        source_sha256=hashes, teacher_sha256=digest(source), scope=parent['scope'],
        body_callback_budget_fraction=.20, full_piano_realtime=False, global_physics_approved=False,
        compiler_flags=parent['compiler_flags'], same_material_loss_projected=True,
        static_compliance_tail_not_deleted=True, latent_ports=24, cases=[])
    def save():
        report['seconds'] = time.perf_counter()-began
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8')
    save()
    core = (root/'tools/concert-grand-modal-body.rs').read_text()
    core = '\n'.join(line.replace('//!', '//', 1) if line.startswith('//!') else line for line in core.splitlines())
    wrapper = (root/'tools/concert-grand-spatial-body.rs').read_text()
    include = 'include!("concert-grand-modal-body.rs");'
    if wrapper.count(include) != 1:
        raise ValueError('native kernel include changed')
    generated = args.local/'native-static.rs'
    generated.write_text(wrapper.replace(include, core), encoding='utf-8')
    report['expanded_native_source_sha256'] = digest(generated)
    executable = args.local/'static-body-bench.exe'
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
    train = ports[:, np.array(parent['training_keys'])-21]
    snapshots = []
    for hz in parent['training_frequency_hz']:
        s = 2j*np.pi*hz
        q = solve(np.diag(omega**2+s*s)+s*support.damping, s*train, check_finite=False)
        response = ports.T @ q
        scale = max(np.linalg.norm(response), 1e-30)
        snapshots.extend([response.real/scale, response.imag/scale])
    mapping, _, _ = svd(np.hstack(snapshots), full_matrices=False, check_finite=False)
    mapping = mapping[:, :24]
    latent_ports = ports @ mapping
    training_frequencies = np.unique(np.r_[parent['training_frequency_hz'],
                         support.frequency[support.frequency <= 6500]])
    # Rank physical coordinates by their maximum training contribution.
    # Validation frequencies/forces are never used to choose the basis.
    score = np.zeros(606)
    for hz in training_frequencies:
        s = 2j*np.pi*hz
        velocity = solve(np.diag(omega**2+s*s)+s*support.damping, s*latent_ports, check_finite=False)
        response = latent_ports.T @ velocity
        score = np.maximum(score, np.linalg.norm(latent_ports, axis=1)*np.linalg.norm(velocity, axis=1)
                           /max(np.linalg.norm(response), 1e-30))
    order = np.argsort(score)[::-1]
    held = np.array(parent['held_out_input_keys'])-21
    frequencies = np.array(parent['held_out_frequency_hz'])
    reference = modal_response(omega, support.damping, ports[:, held], frequencies)
    norm = np.linalg.norm(reference, axis=(1, 2))
    report.update(training_keys=parent['training_keys'], held_out_input_keys=parent['held_out_input_keys'],
        training_frequency_hz=training_frequencies.tolist(), held_out_frequency_hz=frequencies.tolist(),
        training_output_points=parent['training_output_points'], spatial_mapping_sha256=hashlib.sha256(mapping.tobytes()).hexdigest())
    mapping_path = args.local/'ports-24.bin'
    with mapping_path.open('xb') as stream:
        stream.write(struct.pack('<8sII', b'RFPRT001', 88, 24))
        stream.write(np.asarray(mapping, dtype='<f8').tobytes())
    report['mapping_path'] = str(mapping_path.resolve())
    report['mapping_sha256'] = digest(mapping_path)
    for retained in [32, 64, 96, 128, 160, 192, 256]:
        exact = np.sort(order[:retained])
        remaining = np.setdiff1d(np.arange(606), exact)
        tail = latent_ports[remaining]/omega[remaining, None]**2
        tail_basis, r = qr(tail, mode='economic')
        if np.min(abs(np.diag(r))) < np.max(abs(np.diag(r)))*1e-12:
            raise ValueError('rank-deficient static tail; use a rank-revealing basis')
        spatial = np.zeros((606, retained+24))
        spatial[exact, np.arange(retained)] = 1.
        spatial[remaining, retained:] = tail_basis
        # Exact static compliance for every LATENT force direction.
        stiffness = spatial.T @ (omega[:, None]**2*spatial)
        reduced_ports = spatial.T @ latent_ports
        compliance = reduced_ports.T @ solve(stiffness, reduced_ports)
        expected = latent_ports.T @ (latent_ports/omega[:, None]**2)
        static_error = float(np.linalg.norm(compliance-expected)/np.linalg.norm(expected))
        if static_error > 1e-10:
            raise ValueError('static compliance tail changed')
        body = full.project_mechanical(spatial)
        # The same spatial force/velocity projection, now in reduced mechanics.
        latent_body = Body(body.a, body.b @ mapping)
        modal = ModalRuntime.from_body(latent_body)
        response = modal.response(frequencies, list(range(24)))
        projected = np.einsum('pk,fkl,ql->fpq', mapping[held], response, mapping[held], optimize=True)
        difference = np.linalg.norm(projected-reference, axis=(1, 2))
        per_frequency = difference/np.maximum(norm, 1e-30)
        rms = float(np.sqrt(np.trapezoid(difference**2, frequencies)/np.trapezoid(norm**2, frequencies)))
        worst = float(per_frequency.max())
        row = dict(retained_original_modes=retained, static_tail_coordinates=24,
            mechanical_coordinates=retained+24, energy_states=2*(retained+24),
            retained_mode_indices=exact.tolist(), retained_mode_frequency_hz=support.frequency[exact].tolist(),
            static_compliance_relative=static_error, mobility_rms_relative=rms,
            mobility_worst_relative=worst, mobility_relative_per_frequency=per_frequency.tolist(),
            transfer_gate_passed=rms<=.05 and worst<=.10, rates=[])
        for rate in [44100, 48000]:
            step = modal.coefficients(rate)
            bundle = args.local/f'static-body-{retained}-{rate}.bin'
            step.write(bundle)
            verification = verify(run(executable, bundle, mapping_path, 'verify'), modal, modal, step, mapping)
            benchmark = run(executable, bundle, mapping_path, 'bench')
            fraction = benchmark['p99_ms']/benchmark['deadline_ms']
            benchmark.update(p99_deadline_fraction=fraction, observed_body_budget_passed=fraction<=.20
                and benchmark['max_ms']<=benchmark['deadline_ms'] and benchmark['callback_allocations']==0)
            row['rates'].append(dict(coefficient_path=str(bundle.resolve()), coefficient_sha256=digest(bundle),
                coefficient_bytes=bundle.stat().st_size, verification=verification, benchmark=benchmark))
        row['body_budget_passed_both_rates'] = all(r['benchmark']['observed_body_budget_passed'] for r in row['rates'])
        row['both_fidelity_and_body_budget_passed'] = row['transfer_gate_passed'] and row['body_budget_passed_both_rates']
        report['cases'].append(row)
        report['stage'] = 'measuring static-tail reciprocal body'
        save()
        print('retained', retained, 'rms', rms, 'worst', worst, 'p99 fractions', [r['benchmark']['p99_deadline_fraction'] for r in row['rates']], flush=True)
    joint = [row for row in report['cases'] if row['both_fidelity_and_body_budget_passed']]
    budget = [row for row in report['cases'] if row['body_budget_passed_both_rates']]
    chosen = min(joint, key=lambda r:r['mechanical_coordinates']) if joint else (min(budget, key=lambda r:r['mobility_rms_relative']) if budget else None)
    report['candidate_retained_modes'] = chosen['retained_original_modes'] if chosen else None
    report['both_gates_passed'] = bool(joint)
    for row in report['cases']:
        for rate in row['rates']:
            rate['coefficient_retained'] = row is chosen
            if row is not chosen:
                Path(rate['coefficient_path']).unlink()
    report.update(stage='static-tail reciprocal dry body measured',
        source_and_teacher_unchanged=digest(source)==report['teacher_sha256'] and all(digest(root/'tools'/name)==sha for name,sha in hashes.items()),
        native_python_and_energy_verification_scope='Native final modal state/output checksum/work versus Python; per-step energy checked on Python trajectory reconstructed in reduced energy coordinates.',
        remaining=dense['remaining'])
    save()
    print('static-tail stage complete', report['seconds'], 's; candidate', report['candidate_retained_modes'], 'both gates', report['both_gates_passed'], flush=True)


if __name__ == '__main__':
    main()
