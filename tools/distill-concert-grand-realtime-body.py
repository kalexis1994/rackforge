"""Distill and benchmark ONE reciprocal dry board; acoustic/string stages pending."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import time

import numpy as np
from scipy.linalg import solve
from concert_grand_projected_support import ModalSupportMap, projected_matrices
from concert_grand_realtime_body import Body, modal_snapshots, modal_response
import concert_grand_orthotropic_plate as plate


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def run(executable, bundle, mode='bench'):
    result = subprocess.run([str(executable), str(bundle), mode], check=True,
                            capture_output=True, text=True)
    return json.loads(result.stdout)


def crosscheck(native, step):
    x = np.zeros(len(step.transition))
    work = checksum = 0.
    maximum_excess = 0.
    for frame in range(1200):
        f = ((frame*17+np.arange(step.ports.shape[1])*13)%97-48)/48 if frame < 400 else np.zeros(step.ports.shape[1])
        before = x @ x / 2
        x, y = step.step(x, f)
        injected = step.dt * f @ y
        maximum_excess = max(maximum_excess, x @ x / 2 - before - injected)
        work += injected
        checksum += (np.arange(len(y))+1) @ y
    np.testing.assert_allclose(native['state'], x, rtol=2e-8, atol=1e-14)
    np.testing.assert_allclose([native['work_j'], native['energy_j'], native['checksum']],
                               [work, x @ x / 2, checksum], rtol=2e-10, atol=1e-13)
    scale = max(abs(work), x @ x / 2, 1e-30)
    excess = max(maximum_excess, native['maximum_energy_excess_j'])/scale
    if excess > 1e-10:
        raise ValueError('native/Python discrete passivity failed')
    return dict(native_python_state_and_output_passed=True, maximum_step_energy_excess_relative=excess,
                driven_then_free_frames=1200, force_held_per_sample=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--local', type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists() or args.local.exists():
        raise ValueError('preserve existing measured report and coefficient folder')
    root = Path(__file__).resolve().parent.parent
    source = root/'local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    reference = root/'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-motivo-MIDI-2026-10-11/render.json'
    if digest(source) != json.loads(reference.read_text())['board_sha256']:
        raise ValueError('changed teacher board')
    names = [Path(__file__).name, 'concert_grand_realtime_body.py', 'concert-grand-realtime-body.rs',
             'concert_grand_projected_support.py', 'concert_grand_orthotropic_plate.py']
    hashes = {name: digest(Path(__file__).with_name(name)) for name in names}
    args.local.mkdir(parents=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    began = time.perf_counter()
    report = dict(stage='preparation', preceding_stage_commit='f0190e32', teacher_sha256=digest(source),
        sources_sha256=hashes, reference_render_sha256=digest(reference),
        scope='Dry 606-coordinate structural board and full material damping only. No air loading, strings, contact, pedals, microphone, audio, factory update or full-piano realtime approval.',
        teacher_linear_coordinates=606, ports=list(range(21, 109)),
        global_physics_approved=False, full_piano_realtime=False, auditory_fidelity_approved=False,
        body_callback_budget_fraction=.20, transfer_rms_limit=.05, transfer_worst_limit=.10,
        sampled_notes_used=False, neural_training=False, compiler_flags=['--edition=2021', '-O'],
        environment=dict(platform=platform.platform(), processor=platform.processor()), cases=[])
    def save():
        report['seconds'] = time.perf_counter()-began
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8')
    save()
    executable = args.local/'body-bench.exe'
    subprocess.run(['rustc', '--edition=2021', '-O', str(root/'tools/concert-grand-realtime-body.rs'),
                    '-o', str(executable)], check=True)
    report['rustc'] = subprocess.check_output(['rustc', '--version'], text=True).strip()
    with np.load(source) as data:
        matrices = projected_matrices(data)
        modal = ModalSupportMap.from_matrices(*matrices)
        geometry = dict(panel=json.loads(str(data['panel'])), boundary=str(data['boundary']),
                        nx=int(data['nx']), ny=int(data['ny']), vectors=data['ritz_vectors'])
        keys = np.arange(21, 109)
        xs = geometry['panel']['length']*(.1+.8*(keys-21)/87)
        # Associative projection: avoid a second huge Ritz-vector matrix.
        ports = (plate.shape(geometry, xs, np.full(88, geometry['panel']['width']/2)) @ modal.basis).T
    omega = 2*np.pi*modal.frequency
    full = Body.mechanical(omega, modal.damping, ports)
    train_keys = np.array([21, 29, 37, 45, 53, 60, 63, 69, 75, 81, 87, 93, 99, 108])
    held_keys = np.array([24, 33, 42, 50, 57, 65, 72, 79, 85, 96, 104])
    train_ports = ports[:, train_keys-21]
    relevance = np.sum(train_ports**2, axis=1)/np.maximum(np.diag(modal.damping), 1e-12)**2
    admissible = np.flatnonzero((modal.frequency >= 20) & (modal.frequency <= 6000))
    pole_indices = admissible[np.argsort(relevance[admissible])[-24:]]
    train_hz = np.unique(np.r_[np.geomspace(20., 6000., 64), modal.frequency[pole_indices]])
    held_hz = np.geomspace(23., 5970., 111)
    report.update(training_keys=train_keys.tolist(), held_out_keys=held_keys.tolist(),
                  training_frequency_hz=train_hz.tolist(), held_out_frequency_hz=held_hz.tolist(),
                  stage='building paired spatial POD')
    save()
    print('building reciprocal 606-coordinate spatial basis', flush=True)
    basis, singular = modal_snapshots(omega, modal.damping, train_ports, train_hz)
    reference_response = modal_response(omega, modal.damping, ports[:, held_keys-21], held_hz)
    norms = np.linalg.norm(reference_response, axis=(1, 2))
    for states in [16, 32, 64, 96, 128]:
        reduced = full.project_mechanical(basis[:, :states//2])
        reduced_held = Body(reduced.a, reduced.b[:, held_keys-21])
        response = reduced_held.response(held_hz)
        difference = np.linalg.norm(response-reference_response, axis=(1, 2))
        error = float(np.sqrt(np.trapezoid(difference**2, held_hz)/np.trapezoid(norms**2, held_hz)))
        worst = float(np.max(difference/np.maximum(norms, 1e-30)))
        reciprocity = float(np.max(np.linalg.norm(response-response.transpose(0, 2, 1), axis=(1, 2))/np.maximum(norms, 1e-30)))
        if reciprocity > 1e-10:
            raise ValueError('reciprocity lost')
        row = dict(states=states, structural_coordinates=states//2,
            complex_mobility_rms_relative=error, complex_mobility_worst_relative=worst,
            reciprocity_relative=reciprocity, transfer_gate_passed=error<=.05 and worst<=.10,
            normalized_snapshot_singular=singular[states//2-1]/singular[0], rates=[])
        for rate in [44100, 48000]:
            step = reduced.held_step(rate)
            bundle = args.local/f'body-{states}-{rate}.bin'
            step.write(bundle)
            verification = crosscheck(run(executable, bundle, 'verify'), step)
            budget = run(executable, bundle)
            budget['p99_deadline_fraction'] = budget['p99_ms']/budget['deadline_ms']
            budget['observed_body_budget_passed'] = (budget['p99_deadline_fraction'] <= .20
                and budget['max_ms'] <= budget['deadline_ms'] and budget['callback_allocations'] == 0)
            row['rates'].append(dict(coefficient_bytes=bundle.stat().st_size,
                coefficient_sha256=digest(bundle), coefficient_path=str(bundle.resolve()),
                verification=verification, benchmark=budget))
        row['body_budget_passed_both_rates'] = all(r['benchmark']['observed_body_budget_passed'] for r in row['rates'])
        report['cases'].append(row)
        report['stage'] = 'measuring native body budgets and held-out mobility'
        save()
        print('body', states, 'mobility rms', error, 'worst', worst,
              'p99 fractions', [r['benchmark']['p99_deadline_fraction'] for r in row['rates']], flush=True)
    eligible = [row for row in report['cases'] if row['body_budget_passed_both_rates']]
    if eligible:
        chosen = min(eligible, key=lambda row: row['complex_mobility_rms_relative'])
        report['budget_candidate_states'] = chosen['states']
        report['budget_candidate_transfer_gate_passed'] = chosen['transfer_gate_passed']
    else:
        chosen = None
        report['budget_candidate_states'] = None
        report['budget_candidate_transfer_gate_passed'] = False
    # Retain only one student's rate-specific coefficients, no trial copies.
    for row in report['cases']:
        for rate in row['rates']:
            retained = chosen is row
            rate['coefficient_retained'] = retained
            if not retained:
                Path(rate['coefficient_path']).unlink()
    report.update(stage='dry reciprocal body distillation measured',
        source_and_teacher_unchanged=digest(source)==report['teacher_sha256']
            and all(digest(Path(__file__).with_name(name))==sha for name, sha in hashes.items()),
        implementation='exact ZOH energy-state update; collocated interval-average velocity; paired Galerkin spatial POD',
        continuous_reduction_versus_discrete_runtime_separated=True,
        remaining=['causal reciprocal acoustic load', 'strings and bridge inertia',
                   'live nonlinear felt/contact and restrikes', 'pedals and distributed dampers',
                   'microphone pressure and propagation', 'held-out polyphonic pressure listening',
                   'coupled port closure', 'WASM and complete audio callback timing'])
    save()
    print('body distillation complete', report['seconds'], 's; budget candidate', report['budget_candidate_states'], flush=True)


if __name__ == '__main__':
    main()
