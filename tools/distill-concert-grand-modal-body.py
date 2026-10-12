"""Measure an exact resonator realization after the dense dry-body pilot."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time
import numpy as np
from concert_grand_projected_support import ModalSupportMap, projected_matrices
from concert_grand_realtime_body import Body, modal_snapshots, modal_response
from concert_grand_modal_runtime import ModalRuntime
import concert_grand_orthotropic_plate as plate


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def run(executable, bundle, mode):
    return json.loads(subprocess.check_output([str(executable), str(bundle), mode], text=True))


def verify(native, modal, step):
    w = np.zeros(len(modal.roots), dtype=complex)
    checksum = work = previous_energy = maximum_excess = 0.
    for frame in range(1200):
        f = ((frame*17+np.arange(step.drive.shape[1])*13)%97-48)/48 if frame < 400 else np.zeros(step.drive.shape[1])
        w, y = step.step(w, f)
        x = modal.reconstruct(w)
        energy = x @ x / 2
        injected = step.dt*f @ y
        maximum_excess = max(maximum_excess, energy-previous_energy-injected)
        work += injected
        checksum += (np.arange(len(y))+1) @ y
        previous_energy = energy
    nw = np.asarray(native['real'])+1j*np.asarray(native['imag'])
    np.testing.assert_allclose(nw, w, rtol=1e-8, atol=1e-14)
    np.testing.assert_allclose([native['checksum'], native['work_j']], [checksum, work], rtol=2e-10, atol=1e-13)
    error = float(maximum_excess/max(abs(work), energy, 1e-30))
    if error > 1e-10:
        raise ValueError('energy excess in modal held-force trajectory')
    native_energy = float(modal.reconstruct(nw) @ modal.reconstruct(nw)/2)
    return dict(native_python_modal_states_outputs_and_work_passed=True,
                maximum_step_energy_excess_relative=error,
                final_reconstructed_energy_j=native_energy,
                eigenvectors_condition=modal.condition, eigen_equation_relative=modal.eigen_residual)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--local', type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists() or args.local.exists():
        raise ValueError('preserve existing measurement')
    root = Path(__file__).resolve().parent.parent
    parent_path = root/'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P7-cuerpo-realtime-2026-10-12/destilacion.json'
    parent = json.loads(parent_path.read_text())
    for name, expected in parent['sources_sha256'].items():
        if digest(root/'tools'/name) != expected:
            raise ValueError('dense pilot source changed')
    source = root/'local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    if digest(source) != parent['teacher_sha256']:
        raise ValueError('teacher changed')
    names = [Path(__file__).name, 'concert_grand_modal_runtime.py', 'concert-grand-modal-body.rs']
    hashes = {name: digest(root/'tools'/name) for name in names}
    args.local.mkdir(parents=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    began = time.perf_counter()
    report = dict(stage='preparation', parent_report_sha256=digest(parent_path),
                  source_sha256=hashes, teacher_sha256=digest(source),
                  scope=parent['scope'], full_piano_realtime=False, global_physics_approved=False,
                  body_callback_budget_fraction=.20, cases=[], compiler_flags=['--edition=2021', '-O'])
    def save():
        report['seconds'] = time.perf_counter()-began
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8')
    save()
    executable = args.local/'modal-body-bench.exe'
    subprocess.run(['rustc', '--edition=2021', '-O', str(root/'tools/concert-grand-modal-body.rs'), '-o', str(executable)], check=True)
    report['rustc'] = subprocess.check_output(['rustc', '--version'], text=True).strip()
    with np.load(source) as data:
        support = ModalSupportMap.from_matrices(*projected_matrices(data))
        geometry = dict(panel=json.loads(str(data['panel'])), boundary=str(data['boundary']),
                        nx=int(data['nx']), ny=int(data['ny']), vectors=data['ritz_vectors'])
        xs = geometry['panel']['length']*(.1+.8*(np.arange(21, 109)-21)/87)
        ports = (plate.shape(geometry, xs, np.full(88, geometry['panel']['width']/2)) @ support.basis).T
    omega = 2*np.pi*support.frequency
    full = Body.mechanical(omega, support.damping, ports)
    held_indices = np.array(parent['held_out_keys'])-21
    frequencies = np.array(parent['held_out_frequency_hz'])
    basis, _ = modal_snapshots(omega, support.damping, ports[:, np.array(parent['training_keys'])-21], parent['training_frequency_hz'])
    reference = modal_response(omega, support.damping, ports[:, held_indices], frequencies)
    norms = np.linalg.norm(reference, axis=(1, 2))
    for states in [128, 256, 512, 1212]:
        print('modal realization', states, flush=True)
        body = full if states == 1212 else full.project_mechanical(basis[:, :states//2])
        modal = ModalRuntime.from_body(body)
        response = modal.response(frequencies, held_indices)
        difference = np.linalg.norm(response-reference, axis=(1, 2))
        rms = float(np.sqrt(np.trapezoid(difference**2, frequencies)/np.trapezoid(norms**2, frequencies)))
        worst = float(np.max(difference/np.maximum(norms, 1e-30)))
        row = dict(states=states, resonators=len(modal.roots), reduction_used=states!=1212,
                   mobility_rms_relative=rms, mobility_worst_relative=worst,
                   transfer_gate_passed=rms<=.05 and worst<=.10, rates=[])
        for rate in [44100, 48000]:
            step = modal.coefficients(rate)
            bundle = args.local/f'modal-body-{states}-{rate}.bin'
            step.write(bundle)
            verification = verify(run(executable, bundle, 'verify'), modal, step)
            benchmark = run(executable, bundle, 'bench')
            fraction = benchmark['p99_ms']/benchmark['deadline_ms']
            benchmark.update(p99_deadline_fraction=fraction,
                observed_body_budget_passed=fraction<=.20 and benchmark['max_ms']<=benchmark['deadline_ms'] and benchmark['callback_allocations']==0)
            row['rates'].append(dict(rate_hz=rate, coefficient_path=str(bundle.resolve()),
                coefficient_sha256=digest(bundle), coefficient_bytes=bundle.stat().st_size,
                verification=verification, benchmark=benchmark))
        row['body_budget_passed_both_rates'] = all(r['benchmark']['observed_body_budget_passed'] for r in row['rates'])
        row['both_fidelity_and_body_budget_passed'] = row['transfer_gate_passed'] and row['body_budget_passed_both_rates']
        report['cases'].append(row)
        report['stage'] = 'measuring exact modal implementations'
        save()
        print('modal', states, 'rms', rms, 'worst', worst, 'p99 fractions', [r['benchmark']['p99_deadline_fraction'] for r in row['rates']], flush=True)
    eligible = [row for row in report['cases'] if row['body_budget_passed_both_rates']]
    chosen = min(eligible, key=lambda row: row['mobility_rms_relative']) if eligible else None
    report['budget_candidate_states'] = chosen['states'] if chosen else None
    report['candidate_fidelity_gate_passed'] = chosen['transfer_gate_passed'] if chosen else False
    for row in report['cases']:
        for rate in row['rates']:
            retained = row is chosen
            rate['coefficient_retained'] = retained
            if not retained:
                Path(rate['coefficient_path']).unlink()
    report.update(stage='exact modal dry-body runtime measured',
        source_and_teacher_unchanged=digest(source)==report['teacher_sha256'] and all(digest(root/'tools'/name)==sha for name, sha in hashes.items()),
        same_loss_matrix_and_held_force_equations=True,
        no_pole_clipping_or_independent_decay_refit=True)
    save()
    print('modal stage complete', report['seconds'], 's; budget candidate', report['budget_candidate_states'], flush=True)


if __name__ == '__main__':
    main()
