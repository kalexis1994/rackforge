"""Full-coordinate harmonic/Laplace audit of the higher-order causal air.

Require a positive, unchanged full-board mass preflight before allocating
correlations. Check the three diagnostic points first, then the frozen 83
frequencies and three open right-half-plane points if they pass. Checkpoint
one report in place; no factor, WAV, eigenvalue repair or audio promotion.
"""
import argparse
import importlib.util
import json
import time
from pathlib import Path
from types import SimpleNamespace

import numpy as np

from concert_grand_pade_air import FourthFactorBlocks, PadeAir, PadeBoundedAir
from concert_grand_projected_support import ModalSupportMap, projected_matrices
from concert_grand_real_surface import RealArithmeticSurface
from concert_grand_streaming_air import SurfaceFactorBlocks, StreamingRationalAir
import concert_grand_orthotropic_plate as plate


spec = importlib.util.spec_from_file_location(
    'full_air_reference', Path(__file__).with_name('audit-concert-grand-streaming-air.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['loaded-report', 'support-report', 'board', 'mass-report', 'report']:
        p.add_argument('--' + name, type=Path, required=True)
    p.add_argument('--rate', type=int, default=128000)
    p.add_argument('--polar', type=int, default=128)
    p.add_argument('--azimuth', type=int, default=384)
    p.add_argument('--direction-block', type=int, default=8)
    p.add_argument('--max-array-gib', type=float, default=8.)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
    if not np.isfinite(a.max_array_gib) or a.max_array_gib <= 0:
        raise ValueError('positive memory budget')
    start = time.monotonic()
    parent = json.loads(a.loaded_report.read_text())
    support = json.loads(a.support_report.read_text())
    mass = json.loads(a.mass_report.read_text())
    if not parent['all_gates_passed'] or not support['all_gates_passed']:
        raise ValueError('approved continuous/mechanical parents required')
    if (reference.digest(a.loaded_report) != support['parent_sha256']
            or reference.digest(a.board) != support['board_sha256']
            or mass['board_sha256'] != support['board_sha256']
            or mass['cells'] != parent['cells']):
        raise ValueError('changed parent chain')
    for path in [a.loaded_report, a.support_report]:
        if mass['parent_sha256'].get(str(path)) != reference.digest(path):
            raise ValueError('mass preflight used different parents')
    for name, sha in mass['source_sha256'].items():
        if reference.digest(Path(__file__).with_name(name)) != sha:
            raise ValueError('mass preflight source changed: ' + name)
    mass_row = next((row for row in mass['rows']
                     if row['rate'] == a.rate and row['angular'] == [a.polar, a.azimuth]
                     and row['derivative'] == 'fourth'), None)
    if mass_row is None or not mass_row['finite_mass_possible']:
        raise ValueError('unmodified full-board positive mass preflight required')
    with np.load(a.board) as d:
        mechanical = {name: d[name] for name in [
            'frequency', 'low_count', 'tail_mass', 'tail_stiffness', 'tail_damping', 'low_damping']}
        model = dict(panel=json.loads(str(d['panel'])), boundary=str(d['boundary']),
                     nx=int(d['nx']), ny=int(d['ny']), vectors=d['ritz_vectors'])
        notes = d['test_notes'].tolist()
    modal = ModalSupportMap.from_matrices(*projected_matrices(mechanical))
    model['vectors'] = model['vectors'] @ modal.basis
    surface = RealArithmeticSurface(model, parent['cells'])
    n = surface.modes
    if n != mass['modes'] or notes != parent['notes']:
        raise ValueError('unchanged full coordinates and ports required')
    x = model['panel']['length'] * (.1 + .8 * (np.array(notes) - 21) / 87)
    B = plate.shape(model, x, np.full(len(x), model['panel']['width'] / 2)).T
    del model, mechanical
    shapes = surface.forward(np.eye(n))
    plain = SimpleNamespace(shapes=shapes, modes=n, nx=surface.nx, ny=surface.ny,
                            hx=surface.hx, hy=surface.hy, length=surface.length,
                            width=surface.width, density=surface.density, speed=surface.speed)
    provider = FourthFactorBlocks(SurfaceFactorBlocks(
        plain, a.rate, a.polar, a.azimuth, a.direction_block, fold_y=True))
    sources = sorted(set(mass['source_sha256']) | {
        Path(__file__).name, 'concert_grand_rational_air.py', 'concert_grand_bounded_air.py',
        'concert_grand_pade_air.py', 'concert_grand_orthotropic_plate.py'})
    r = dict(rate=a.rate, angular=[a.polar, a.azimuth], derivative='fourth', delay='Pade2',
             cells=parent['cells'], modes=n, notes=notes,
             budget=provider.budget(real_gram=True), max_array_bytes=int(a.max_array_gib * 2**30),
             real_gram=True, fold_y=True, symmetry_relative=provider.symmetry_relative,
             logical_outputs=provider.logical_outputs, evaluated_outputs=provider.outputs,
             parent_sha256={str(path): reference.digest(path) for path in [
                 a.loaded_report, a.support_report, a.mass_report]},
             board_sha256=reference.digest(a.board),
             source_sha256={name: reference.digest(Path(__file__).with_name(name)) for name in sources},
             criteria=dict(impedance_ZRX_relative=.02, loaded_port_relative=.01,
                           Laplace_relative=.02, passivity_relative=-1e-10,
                           mass_control_relative=1e-10, realized_limits_relative=1e-10),
             rows=[], laplace=[], stage='construction', preflight_only=True,
             all_gates_passed=False, harmonic_gates_passed=False,
             scope='All 606 coordinates, unchanged mechanical loss, 11 ports and two pressure observers. '
                   'Harmonic/Laplace audit only; no coupled-time, timbral or runtime approval.')

    def checkpoint():
        r['seconds'] = time.monotonic() - start
        r['peak_working_set_bytes'] = reference.peak_memory()
        a.report.parent.mkdir(parents=True, exist_ok=True)
        a.report.write_text(json.dumps(r, indent=2, allow_nan=False), encoding='utf-8')

    print('budget', json.dumps(r['budget']), flush=True)
    checkpoint()
    wave = StreamingRationalAir(provider, r['max_array_bytes'], real_gram=True,
                               progress=lambda i: print('wave block', i, flush=True) if i % 32 == 0 else None)
    candidate_wave = PadeAir(wave)
    r['wave_reciprocity_relative'] = float(wave.reciprocity_error)
    r['wave_dc_relative'] = float(wave.dc_error)
    r['wave_states'] = candidate_wave.states
    print('static mass reference', flush=True)
    target = reference.direct_matrix(surface, 0).real
    target = (target + target.T) / 2
    limit = surface.density * surface.speed * surface.hx * surface.hy * (provider.shapes.T @ provider.shapes)
    residue = target - candidate_wave.static_mass()
    eig = np.linalg.eigvalsh((residue + residue.T) / 2)
    r['mass_residual_minimum'] = float(eig.min())
    r['mass_residual_maximum'] = float(eig.max())
    r['mass_control_relative'] = float(max(abs(eig.min() - mass_row['mass_residual_minimum']),
                                          abs(eig.max() - mass_row['mass_residual_maximum']))
                                      / max(abs(eig).max(), 1e-40))
    if r['mass_control_relative'] > r['criteria']['mass_control_relative']:
        r['stage'] = 'rejected mass equivalence'
        checkpoint()
        raise AssertionError('correlation mass differs from cumulative factor mass')
    try:
        candidate = PadeBoundedAir(candidate_wave, target, limit)
    except ValueError as error:
        r['stage'] = 'rejected finite completion'
        r['completion_rejection'] = str(error)
        r['finite_completion'] = False
        checkpoint()
        print('completion rejected', str(error), flush=True)
        return
    r['finite_completion'] = True
    r['states'] = candidate.states
    r['realized_mass_relative'] = reference.relative(candidate.static_mass(), target)
    r['realized_limit_relative'] = reference.relative(candidate.high_limit(), limit)
    r['relaxation_tau_range_s'] = [float(candidate.relaxation.tau.min()), float(candidate.relaxation.tau.max())]
    w2 = (2 * np.pi * modal.frequency)**2

    def transfer(hz):
        s = 2j * np.pi * hz
        z = candidate.impedance(s)
        exact_z = reference.direct_matrix(surface, s)
        dynamic = np.diag(w2 + s*s) + s * modal.damping
        q = np.linalg.solve(dynamic + s*z, B)
        exact_q = np.linalg.solve(dynamic + s*exact_z, B)
        # Every port error keeps the other ten output ports; no fitted port weights.
        mobility = B.T @ (s*q)
        exact_mobility = B.T @ (s*exact_q)
        mic = [[.55, .5, .66], [.85, .5, .66]]
        pressure = surface.pressure(hz, q, mic)
        exact_pressure = surface.pressure(hz, exact_q, mic)
        row = dict(frequency_hz=float(hz), impedance_ZRX_relative=[
            reference.relative(z, exact_z), reference.relative(z.real, exact_z.real),
            reference.relative(z.imag, exact_z.imag)],
            mobility_relative=(np.linalg.norm(mobility - exact_mobility, axis=0)
                               / np.maximum(np.linalg.norm(exact_mobility, axis=0), 1e-40)).tolist(),
            pressure_relative=(np.linalg.norm(pressure - exact_pressure, axis=0)
                               / np.maximum(np.linalg.norm(exact_pressure, axis=0), 1e-40)).tolist(),
            hermitian_minimum_relative=float(np.linalg.eigvalsh((z + z.conj().T) / 2).min()
                                             / max(np.linalg.norm(z), 1e-40)))
        r['rows'].append(row)
        print('transfer', json.dumps(row), flush=True)
        checkpoint()

    def gates():
        return dict(finite_completion=r['finite_completion'],
                    mass_equivalence=r['mass_control_relative'] < r['criteria']['mass_control_relative'],
                    finite_limits=max(r['realized_mass_relative'], r['realized_limit_relative'])
                                  < r['criteria']['realized_limits_relative'],
                    impedance=max(max(row['impedance_ZRX_relative']) for row in r['rows']) < .02,
                    loaded=max(max(row[key]) for row in r['rows']
                               for key in ['mobility_relative', 'pressure_relative']) < .01,
                    passive=min(row['hermitian_minimum_relative'] for row in r['rows']) >= -1e-10)

    r['stage'] = 'three-point preflight'
    for hz in [60., 1881.9607122235952, 6000.]:
        transfer(hz)
    r['preflight_gates'] = gates()
    if not all(r['preflight_gates'].values()):
        r['stage'] = 'rejected three-point preflight'
        checkpoint()
        print('preflight rejected', json.dumps(r['preflight_gates']), flush=True)
        return
    r['stage'] = 'frozen harmonic grid'
    for hz in parent['frequencies_hz']:
        if not any(abs(row['frequency_hz'] - hz) < 1e-9 for row in r['rows']):
            transfer(hz)
    r['stage'] = 'open right-half-plane'
    for s in [100+4000j, 1000+20000j, 5000+40000j]:
        z = candidate.impedance(s)
        exact_z = reference.direct_matrix(surface, s)
        row = dict(point=[s.real, s.imag], impedance_relative=reference.relative(z, exact_z),
                   hermitian_minimum_relative=float(np.linalg.eigvalsh((z + z.conj().T) / 2).min()
                                                    / max(np.linalg.norm(z), 1e-40)))
        r['laplace'].append(row)
        print('Laplace', json.dumps(row), flush=True)
        checkpoint()
    r['harmonic_gates'] = gates()
    r['harmonic_gates']['frozen_grid_complete'] = all(
        any(abs(row['frequency_hz'] - hz) < 1e-9 for row in r['rows']) for hz in parent['frequencies_hz'])
    r['harmonic_gates']['Laplace'] = max(row['impedance_relative'] for row in r['laplace']) < .02
    r['harmonic_gates']['Laplace_passive'] = min(row['hermitian_minimum_relative'] for row in r['laplace']) >= -1e-10
    r['harmonic_gates_passed'] = all(r['harmonic_gates'].values())
    r['stage'] = 'harmonic audit completed'
    r['next'] = 'Validate coupled transients and two-state fast contact/coast before rendering. '
    r['next'] += 'Harmonic precision and algebraic passivity do not identify the piano or approve its sound.'
    checkpoint()
    print('audit finished', json.dumps(r['harmonic_gates']), r['seconds'], flush=True)


if __name__ == '__main__':
    main()
