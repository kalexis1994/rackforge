"""Close the coarse local-Hann control and refine the failed 16-kHz mesh.

Frozen physical sources, one existing R/capsule, no disk trajectories.
Authenticate the fine report and reuse its source instead of rerunning fine
contact. Preserve all original temporal, integration and spatial failures.
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
from concert_grand_local_hann_controls import maximum_nested
from concert_grand_saved_source import saved_source

spec = importlib.util.spec_from_file_location('coarse_hann_parent',
    Path(__file__).with_name('audit-concert-grand-contact-band.py'))
parent = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parent)
prior, field = parent.prior, parent.field


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report', type=Path, required=True)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
    folder = prior.DOC/'medidas-P6-contacto-banda-superior'
    paths = [folder/'contacto-24k.json', folder/'Hann-local-malla.json',
             folder/'Hann-local-malla-validado.json']
    old, fine, completed = [json.loads(p.read_text(encoding='utf-8')) for p in paths]
    for report in [old, fine, completed]:
        prior.verify(report)
    if (old['stage'] != 'full-contact high-band source and directional load diagnostic completed'
            or old['rates'] != [1536000, 3072000] or fine['rate'] != 3072000
            or completed['stage'] != 'canonical fine contact local Hann and mesh diagnostic completed'
            or not completed['local_controls']['fine_local_oracle']
            or not completed['local_controls']['source_and_cache_unchanged']
            or not all(fine['canonical_endpoint_exact'])
            or completed['parent_sha256'][str(paths[1].relative_to(prior.ROOT))] != digest(paths[1])):
        raise ValueError('authenticated matched fine local-Hann control required')
    if old['criteria'] != fine['criteria'] or fine['criteria'] != completed['criteria']:
        raise ValueError('unchanged criteria required')
    a.full_report = prior.DOC/'medidas-P6-aire-bloques/carga128-pade2.json'
    a.board = prior.LOCAL/'P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    a.cache = prior.LOCAL/'P6-carga-causal-2026-10-06'
    a.direction_block = 32
    if not (a.cache/'R.npy').is_file() or not (a.cache/'manifest.json').is_file():
        raise ValueError('existing single acoustic cache required')
    checkpoint_path = prior.DOC/'medidas-P6-coast-campo/coast-512k.json'
    seed_path = prior.LOCAL/'P6-contacto96-seed-2026-10-07/Contacto-96-3072k.npz'
    start = time.perf_counter()
    r = dict(stage='construction', all_gates_passed=False, preflight_only=True,
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in [
            Path(__file__).name, 'concert_grand_saved_source.py',
            'concert_grand_local_hann.py', 'concert_grand_local_hann_controls.py']},
        parent_sha256={str(p):digest(p) for p in paths},
        criteria=old['criteria'], board_sha256=digest(a.board),
        cache_manifest_sha256=digest(a.cache/'manifest.json'), rate=1536000,
        duration_s=.003, incident_speed_m_s=5., transverse=96, axial=192,
        scope='Coarse nine-frequency local-Hann control; fine controls inherited. '
              '16-kHz harmonic action on a saved original source, not a new physical '
              'trajectory, full spectral convergence, operator or live-memory certificate.',
        original_diagnostic_gates=old['diagnostic_gates'])
    def save():
        r['seconds'] = time.perf_counter()-start
        r['peak_working_set_bytes'] = field.full.reference.peak_memory()
        a.report.parent.mkdir(parents=True, exist_ok=True)
        a.report.write_text(json.dumps(r, indent=2, allow_nan=False), encoding='utf-8')
    save()
    if r['board_sha256'] != old['board_sha256'] or r['board_sha256'] != fine['board_sha256']:
        raise ValueError('changed board')
    approved, modal, Bsource, notes, surface, provider, identity, _ = field.full.prepare_reference(a)
    with np.load(a.board) as data:
        geometry = dict(panel=json.loads(str(data['panel'])), boundary=str(data['boundary']),
            nx=int(data['nx']), ny=int(data['ny']), vectors=data['ritz_vectors']@modal.basis)
    surfaces = [RealArithmeticSurface(geometry, [factor*n for n in approved['cells']])
                for factor in [2, 4, 8]]
    fft_points = int(np.prod(surfaces[-1].geometry.fft_size))
    mesh_estimate = 4*2**30+fft_points*(12*16+8*8)
    estimate = max(old['array_estimates']['estimated_bytes'], mesh_estimate)
    r['array_estimates'] = dict(estimated_bytes=int(estimate), maximum_bytes=12*2**30,
        mesh_phase_bytes=int(mesh_estimate), finest_fft_points=fft_points,
        angular_history_bytes=0, phases_sequential=True,
        explanation='Inherited conservative contact allowance; mesh adds twelve complex '
                    'and eight real FFT-sized arrays to a 4-GiB base. Native histories '
                    'deleted before mesh phase; not an OS memory cap.')
    save()
    if estimate > 12*2**30:
        raise MemoryError('declared array budget exceeded')
    _, wave, air = field.full.construct_air(a, approved, provider, surface, identity, 12*2**30, r, save)
    if r['cache_R_sha256'] != fine['cache_R_sha256']:
        raise ValueError('changed acoustic load')
    seed, _, proof = authenticated_seed(checkpoint_path, seed_path,
        '90cfd00a7363b12bb404f607c85c7955cab0876e',
        json.loads(checkpoint_path.read_text(encoding='utf-8')))
    r['seed_provenance'] = proof
    del seed
    system = field.BridgeHammer(field.MovingBridge(field.Wire(.62, .0081, 850),
        modal.support(Bsource[:, notes.index(60)]), transverse=96, axial=192),
        field.Felt(.002, 7.5e8, 2.6, .125, .02, .1))
    r['stage'] = 'coarse contact'; save()
    pilot = parent.ProgressContact(field.FastBoundedContact(system, air), r['rate'], r, save)
    state, row, t, B, V = capture_contact(pilot, 5., r['rate'], .003)
    previous = old['cases'][0]
    r['contact_reproduction_relative'] = {key:prior.parent.anchor.reference.difference(row[key], value)
        for key, value in previous.items() if key.endswith('_j') or key in ['final_wave_norm',
            'compression_final_m', 'peak_centered_contact_force_n', 'contact_impulse_n_s',
            'hammer_momentum_change_n_s']}
    r['contact'] = row
    r.pop('active_contact', None)
    del state, pilot
    r['stage'] = 'coarse Hann and original oracles'; save()
    f = np.r_[0., 60., parent.PROBES]
    r['oracle_frequency_hz'] = f.tolist()
    began = time.perf_counter()
    actual = local_hann_spectra(t, B, V, f)
    r['local_spectra_s'] = time.perf_counter()-began
    r['local_truncation_bounds'] = truncation_bounds(t, f)
    original = uniform_window_spectra(t, B, V, f)['hann']
    v_oracle = velocity_spectrum(t, B, V, f, True, 12)
    a_oracle = hann_acceleration_spectrum(t, B, V, f)
    r['local_oracle_relative'] = {name:[field.relative(x, y) for x, y in zip(values, oracle)]
        for name, values, oracle in [('velocity', actual[0], v_oracle), ('acceleration', actual[1], a_oracle)]}
    r['original_global_oracle_relative'] = {name:[field.relative(x, y) for x, y in zip(values, oracle)]
        for name, values, oracle in [('velocity', original[0], v_oracle), ('acceleration', original[1], a_oracle)]}
    v16 = velocity_spectrum(t, B, V, f, True, 16)
    r['original_velocity_oracle_order12_16_relative'] = [field.relative(x, y) for x, y in zip(v_oracle, v16)]
    local12 = local_hann_spectra(t, B, V, f, order=12)
    blocked = local_hann_spectra(t, B, V, f, segment_block=127, frequency_block=1)
    r['local_order8_12_relative'] = [[field.relative(x, y) for x, y in zip(a0, a1)] for a0, a1 in zip(actual, local12)]
    r['local_block_relative'] = [[field.relative(x, y) for x, y in zip(a0, a1)] for a0, a1 in zip(actual, blocked)]
    r['source_direction_temporal_relative'] = {label:[dict(frequency_hz=float(hz),
        relative=field.relative(source/np.linalg.norm(source),
            saved_source(fine, label, float(hz))/np.linalg.norm(saved_source(fine, label, float(hz)))))
        for hz, source in zip(parent.PROBES, values[2:])]
        for label, values in [('original_global_hann_velocity', original[0]), ('local_hann_velocity', actual[0])]}
    # Fine vectors already exist in the authenticated parent. No duplicates;
    # this coarse trajectory is discarded after its declared controls.
    del B, V, t, actual, original, v_oracle, a_oracle, v16, local12, blocked
    save()
    source = saved_source(fine, 'original_global_hann_velocity', 16000.)
    direction = source/np.linalg.norm(source)
    s = 2j*np.pi*16000.
    forces = []
    for surf in surfaces:
        r['stage'] = f'16-kHz cells {surf.nx}x{surf.ny}, order 12'; save()
        began = time.perf_counter()
        forces.append(CellAir(surf, s, 12).force(direction))
        print('mesh action', surf.nx, surf.ny, time.perf_counter()-began, flush=True)
    r['stage'] = '16-kHz finest quadrature order 16'; save()
    finest16 = CellAir(surfaces[-1], s, 16).force(direction)
    approximate = air.impedance(s)@direction
    measured = dict(frequency_hz=16000., source_velocity_transform_norm=float(np.linalg.norm(source)),
        cells=[[surf.nx, surf.ny] for surf in surfaces],
        inherited_refinement_relative=field.relative(forces[0], forces[1]),
        finest_refinement_relative=field.relative(forces[1], forces[2]),
        finest_quadrature_relative=field.relative(forces[2], finest16),
        causal_vs_1024_relative=field.relative(approximate, forces[1]),
        causal_vs_2048_relative=field.relative(approximate, forces[2]))
    inherited = next(p for p in fine['load_probes'] if p['frequency_hz'] == 16000.)
    measured['reproduction_relative'] = {
        'refinement':prior.parent.anchor.reference.difference(measured['inherited_refinement_relative'], inherited['load_fine_refinement_relative']),
        'causal_vs_1024':prior.parent.anchor.reference.difference(measured['causal_vs_1024_relative'], inherited['causal_vs_finest_cells_relative']),
        'source_norm':prior.parent.anchor.reference.difference(measured['source_velocity_transform_norm'], inherited['source_velocity_transform_norm'])}
    r['load_16k'] = measured
    for report in [old, fine, completed, r]:
        prior.verify(report)
    unchanged = (digest(a.board) == r['board_sha256'] and digest(a.cache/'R.npy') == r['cache_R_sha256']
        and digest(a.cache/'manifest.json') == r['cache_manifest_sha256']
        and digest(seed_path) == proof['payload_sha256']
        and digest(seed_path.with_suffix('.json')) == proof['manifest_sha256'])
    c = r['criteria']
    r['local_controls'] = dict(
        contact_reproduction=maximum_nested(r['contact_reproduction_relative']) < c['reproduction_relative'],
        mechanical_budget=row['maximum_mechanical_budget_relative'] < c['mechanical_budget_relative'],
        impulse=row['impulse_momentum_relative'] < c['impulse_relative'],
        nonnegative_heat=row['minimum_step_heat_j'] >= -1e-16,
        coarse_local_oracle=maximum_nested(r['local_oracle_relative']) < c['analytic_oracle_relative'],
        local_order=maximum_nested(r['local_order8_12_relative']) < c['analytic_oracle_relative'],
        local_blocks=maximum_nested(r['local_block_relative']) < c['analytic_oracle_relative'],
        inherited_fine_local_oracle=completed['local_controls']['fine_local_oracle'],
        mesh_reproduction=maximum_nested(measured['reproduction_relative']) < c['reproduction_relative'],
        mesh_16k=measured['finest_refinement_relative'] < c['load_cell_refinement_relative'],
        mesh_quadrature=measured['finest_quadrature_relative'] < c['load_cell_quadrature_relative'],
        source_and_cache_unchanged=unchanged)
    r['local_controls'] = {k:bool(v) for k, v in r['local_controls'].items()}
    r['local_controls_passed'] = all(r['local_controls'].values())
    r['two_rate_local_hann_oracle_passed'] = bool(r['local_controls']['coarse_local_oracle']
        and r['local_controls']['inherited_fine_local_oracle'])
    r['all_seven_directional_mesh_controls_passed'] = bool(r['local_controls']['mesh_16k']
        and all(p['load_fine_refinement_relative'] < c['load_cell_refinement_relative']
                for p in fine['load_probes'] if p['frequency_hz'] != 16000.))
    r['stage'] = 'coarse local Hann and saved 16-kHz mesh diagnostic completed'
    save()
    print('finished', json.dumps(r['local_controls']), r['seconds'], flush=True)


if __name__ == '__main__':
    main()
