"""Canonical fine-contact local Hann control and reusable load directions.

One unchanged 3072-kHz reconstruction; authenticate its exact four-array
endpoint. Original nine-frequency oracles and unchanged 1e-10 threshold.
Save only seven 606-complex vectors per method, then refine Rayleigh cells.
Does not certify the unmeasured coarse trajectory or old temporal failures.
"""
import argparse
import importlib.util
import json
import time
from pathlib import Path
import numpy as np
from concert_grand_wave_cache import digest
from concert_grand_seed_resume import authenticated_seed
from concert_grand_string_resolution import capture_contact
from concert_grand_uniform_spectra import uniform_window_spectra
from concert_grand_local_hann import local_hann_spectra, truncation_bounds
from concert_grand_window_spectra import velocity_spectrum, hann_acceleration_spectrum
from concert_grand_matrixfree_air import CellAir
from concert_grand_real_surface import RealArithmeticSurface

spec = importlib.util.spec_from_file_location('local_contact_parent',
    Path(__file__).with_name('audit-concert-grand-contact-band.py'))
parent = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parent)
prior, field = parent.prior, parent.field


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--parent', type=Path, required=True)
    p.add_argument('--report', type=Path, required=True)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
    old = json.loads(a.parent.read_text(encoding='utf-8'))
    if (old['stage'] != 'full-contact high-band source and directional load diagnostic completed'
            or old['rates'] != [1536000, 3072000] or old['duration_s'] != .003
            or not all(old['canonical_endpoint_exact'])):
        raise ValueError('completed canonical contact parent required')
    prior.verify(old)
    checkpoint_path = prior.DOC/'medidas-P6-coast-campo/coast-512k.json'
    seed_path = prior.LOCAL/'P6-contacto96-seed-2026-10-07/Contacto-96-3072k.npz'
    expected = json.loads(checkpoint_path.read_text(encoding='utf-8'))
    a.full_report = prior.DOC/'medidas-P6-aire-bloques/carga128-pade2.json'
    a.board = prior.LOCAL/'P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    a.cache = prior.LOCAL/'P6-carga-causal-2026-10-06'
    a.direction_block = 32
    start = time.perf_counter()
    sources = [Path(__file__).name, 'concert_grand_local_hann.py']
    r = dict(stage='construction', all_gates_passed=False, preflight_only=True,
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in sources},
        parent_sha256={str(a.parent):digest(a.parent)},
        board_sha256=digest(a.board), cache_manifest_sha256=digest(a.cache/'manifest.json'),
        geometry=old['geometry'], microphones=old['microphones'], rate=3072000,
        duration_s=.003, incident_speed_m_s=5., transverse=96, axial=192,
        criteria=old['criteria'], original_diagnostic_gates=old['diagnostic_gates'],
        scope='Fine contact only; local Hann V/A versus untouched original oracles. '
              'No replacement of old two-rate analytic or temporal gates. '
              'Harmonic directional load refinement only; no live-memory or angular certificate.')
    def save():
        r['seconds'] = time.perf_counter()-start
        r['peak_working_set_bytes'] = field.full.reference.peak_memory()
        a.report.parent.mkdir(parents=True, exist_ok=True)
        a.report.write_text(json.dumps(r, indent=2, allow_nan=False), encoding='utf-8')
    save()
    if r['board_sha256'] != old['board_sha256'] or r['cache_manifest_sha256'] != old['cache_manifest_sha256']:
        raise ValueError('changed board/cache manifest')
    approved, modal, Bsource, notes, surface, provider, identity, _ = field.full.prepare_reference(a)
    # Same conservative native-trace/oracle allowance as the full-band parent;
    # this run has fewer frequencies and no full pressure-transfer grid.
    r['array_estimates'] = dict(estimated_bytes=old['array_estimates']['estimated_bytes'],
        maximum_bytes=12*2**30, angular_history_bytes=0,
        explanation='Inherited conservative full-contact allowance; histories only in RAM.')
    if r['array_estimates']['estimated_bytes'] > 12*2**30:
        raise MemoryError('array budget exceeded')
    _, wave, air = field.full.construct_air(a, approved, provider, surface, identity, 12*2**30, r, save)
    if r['cache_R_sha256'] != old['cache_R_sha256']:
        raise ValueError('changed acoustic load')
    seed, _, proof = authenticated_seed(checkpoint_path, seed_path,
        '90cfd00a7363b12bb404f607c85c7955cab0876e', expected)
    r['seed_provenance'] = proof
    system = field.BridgeHammer(field.MovingBridge(field.Wire(.62, .0081, 850),
        modal.support(Bsource[:, notes.index(60)]), transverse=96, axial=192),
        field.Felt(.002, 7.5e8, 2.6, .125, .02, .1))
    r['stage'] = 'canonical fine contact'; save()
    pilot = parent.ProgressContact(field.FastBoundedContact(system, air), r['rate'], r, save)
    state, row, t, B, V = capture_contact(pilot, 5., r['rate'], .003)
    previous = old['cases'][-1]
    r['contact_reproduction_relative'] = {key:prior.parent.anchor.reference.difference(row[key], value)
        for key, value in previous.items() if key.endswith('_j') or key in ['final_wave_norm',
            'compression_final_m', 'peak_centered_contact_force_n', 'contact_impulse_n_s',
            'hammer_momentum_change_n_s']}
    r['canonical_endpoint_exact'] = [bool(np.array_equal(x, y)) for x, y in zip(state, seed)]
    if not all(r['canonical_endpoint_exact']):
        raise ValueError('reconstructed endpoint differs')
    r['contact'] = row
    r.pop('active_contact', None)
    del state, pilot, seed
    r['stage'] = 'local Hann and independent oracles'; save()
    frequencies = np.r_[0., 60., parent.PROBES]
    r['oracle_frequency_hz'] = frequencies.tolist()
    begin = time.perf_counter()
    actual = local_hann_spectra(t, B, V, frequencies)
    r['local_spectra_s'] = time.perf_counter()-begin
    r['local_truncation_bounds'] = truncation_bounds(t, frequencies)
    original = uniform_window_spectra(t, B, V, frequencies)['hann']
    v_oracle = velocity_spectrum(t, B, V, frequencies, True, 12)
    a_oracle = hann_acceleration_spectrum(t, B, V, frequencies)
    r['original_global_oracle_relative'] = {name:[field.relative(x, y) for x, y in zip(values, oracle)]
        for name, values, oracle in [('velocity', original[0], v_oracle), ('acceleration', original[1], a_oracle)]}
    r['local_oracle_relative'] = {name:[field.relative(x, y) for x, y in zip(values, oracle)]
        for name, values, oracle in [('velocity', actual[0], v_oracle), ('acceleration', actual[1], a_oracle)]}
    # Same original implementation with a higher quadrature order: diagnostic
    # of oracle roundoff, never a replacement for its declared order-12 test.
    v16 = velocity_spectrum(t, B, V, frequencies, True, 16)
    r['original_velocity_oracle_order12_16_relative'] = [field.relative(x, y) for x, y in zip(v_oracle, v16)]
    order12 = local_hann_spectra(t, B, V, frequencies, order=12)
    blocked = local_hann_spectra(t, B, V, frequencies, segment_block=127, frequency_block=1)
    r['local_order8_12_relative'] = [[field.relative(x, y) for x, y in zip(a0, a1)] for a0, a1 in zip(actual, order12)]
    r['local_block_relative'] = [[field.relative(x, y) for x, y in zip(a0, a1)] for a0, a1 in zip(actual, blocked)]
    r['source_vectors'] = {label:[dict(frequency_hz=float(hz),
        transform=[[float(z.real), float(z.imag)] for z in vector])
        for hz, vector in zip(parent.PROBES, values[2:])]
        for label, values in [('original_global_hann_velocity', original[0]), ('local_hann_velocity', actual[0])]}
    r['source_vector_binding'] = dict(coordinates=provider.modes,
        basis='full physical modal board basis from authenticated board and prepare_reference',
        board_sha256=r['board_sha256'], units='integral of modal velocity in seconds',
        absolute_time_window_s=[float(t[0]), float(t[-1])], normalized=False)
    # Delete all trajectories; only small spectral vectors survive.
    del B, V, t, order12, blocked, v_oracle, a_oracle, v16
    save()
    with np.load(a.board) as data:
        geometry = dict(panel=json.loads(str(data['panel'])), boundary=str(data['boundary']),
            nx=int(data['nx']), ny=int(data['ny']), vectors=data['ritz_vectors']@modal.basis)
    fine = RealArithmeticSurface(geometry, [2*n for n in approved['cells']])
    finer = RealArithmeticSurface(geometry, [4*n for n in approved['cells']])
    r['load_cells'] = [approved['cells'], [2*n for n in approved['cells']], [4*n for n in approved['cells']]]
    r['load_probes'] = []
    for i, hz in enumerate(parent.PROBES):
        r['stage'] = f'cell refinement at {hz:g} Hz'; save()
        source = original[0][i+2]
        norm = float(np.linalg.norm(source))
        direction = source/norm
        s = 2j*np.pi*hz
        base12 = CellAir(surface, s, 12).force(direction)
        fine12 = CellAir(fine, s, 12).force(direction)
        finer12 = CellAir(finer, s, 12).force(direction)
        finer16 = CellAir(finer, s, 16).force(direction)
        approximate = air.impedance(s)@direction
        measured = dict(frequency_hz=float(hz), source_velocity_transform_norm=norm,
            load_cell_refinement_relative=field.relative(base12, fine12),
            load_fine_refinement_relative=field.relative(fine12, finer12),
            load_finest_quadrature_relative=field.relative(finer12, finer16),
            causal_vs_same_cells_relative=field.relative(approximate, base12),
            causal_vs_refined_cells_relative=field.relative(approximate, fine12),
            causal_vs_finest_cells_relative=field.relative(approximate, finer12),
            local_vs_global_direction_relative=field.relative(actual[0][i+2]/np.linalg.norm(actual[0][i+2]), direction))
        inherited = old['load_probes'][i]
        measured['parent_reproduction_relative'] = {key:prior.parent.anchor.reference.difference(measured[key], inherited[key])
            for key in ['source_velocity_transform_norm', 'load_cell_refinement_relative',
                        'causal_vs_same_cells_relative', 'causal_vs_refined_cells_relative']}
        r['load_probes'].append(measured)
        print('load refinement', json.dumps(measured), flush=True); save()
    prior.verify(old); prior.verify(r)
    unchanged = (digest(a.board) == r['board_sha256'] and digest(a.cache/'R.npy') == r['cache_R_sha256']
        and digest(a.cache/'manifest.json') == r['cache_manifest_sha256']
        and digest(seed_path) == proof['payload_sha256']
        and digest(seed_path.with_suffix('.json')) == proof['manifest_sha256'])
    maximum = parent.reference.maximum_nested
    c = r['criteria']
    r['local_controls'] = dict(
        contact_reproduction=maximum(r['contact_reproduction_relative']) < c['reproduction_relative'],
        endpoint_exact=all(r['canonical_endpoint_exact']),
        fine_local_oracle=maximum(r['local_oracle_relative']) < c['analytic_oracle_relative'],
        local_order=maximum(r['local_order8_12_relative']) < c['analytic_oracle_relative'],
        local_blocks=maximum(r['local_block_relative']) < c['analytic_oracle_relative'],
        load_reproduction=max(maximum(p['parent_reproduction_relative']) for p in r['load_probes']) < c['reproduction_relative'],
        finest_load_quadrature=max(p['load_finest_quadrature_relative'] for p in r['load_probes']) < c['load_cell_quadrature_relative'],
        fine_load_cells=max(p['load_fine_refinement_relative'] for p in r['load_probes']) < c['load_cell_refinement_relative'],
        source_and_cache_unchanged=unchanged)
    r['local_controls'] = {k:bool(v) for k, v in r['local_controls'].items()}
    r['local_controls_passed'] = all(r['local_controls'].values())
    r['stage'] = 'canonical fine contact local Hann and mesh diagnostic completed'
    save()
    print('finished', json.dumps(r['local_controls']), r['seconds'], flush=True)


if __name__ == '__main__':
    main()
