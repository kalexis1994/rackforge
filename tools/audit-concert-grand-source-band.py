"""Diagnose finite-window source and air-load directions above 6 kHz.

Reuses the authenticated full contact endpoint, never tunes the dynamics.
Probe directions are spectra of the observed board velocity. Harmonic load
actions on them are diagnostic, not a Fourier replay of the live air memory.
"""
import argparse
import importlib.util
import json
import time
from pathlib import Path
import numpy as np
from concert_grand_wave_cache import digest
from concert_grand_seed_resume import authenticated_seed
from concert_grand_coast_measurements import run_coast
from concert_grand_cell_observer import CellObserver, acceleration_spectrum, spectral_relative
from concert_grand_window_spectra import (velocity_spectrum,
    hann_acceleration_spectrum, band_norms, projected_pressure)
from concert_grand_matrixfree_air import CellAir
from concert_grand_real_surface import RealArithmeticSurface

spec = importlib.util.spec_from_file_location('coast512_parent',
    Path(__file__).with_name('audit-concert-grand-coast-512.py'))
parent = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parent)
field = parent.field
ROOT = Path(__file__).resolve().parents[1]
DOC = ROOT/'docs/concert-grand-physics-roadmap-2026-10-02'
LOCAL = ROOT/'local/concert-grand-physics'
PROBES = np.array([6000.,8000.,10000.,12000.,16000.,20000.,24000.])
BANDS = [(0,6000),(6000,12000),(12000,24000),(0,24000)]
CRITERIA = dict(reproduction_relative=1e-10, source_relative=.001,
    frequency_integral_relative=.001, time_quadrature_relative=1e-10,
    pressure_capture_relative=.001, pressure_cell_quadrature_relative=.0002,
    load_cell_quadrature_relative=.0002, load_cell_refinement_relative=.02,
    directional_load_relative=.02)


def verify(report):
    for name, sha in report['source_sha256'].items():
        if digest(Path(__file__).with_name(name)) != sha:
            raise ValueError('changed source: '+name)
    for name, sha in report['parent_sha256'].items():
        if digest(name) != sha:
            raise ValueError('changed parent: '+name)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report', type=Path, required=True)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
    a.full_report = DOC/'medidas-P6-aire-bloques/carga128-pade2.json'
    a.board = LOCAL/'P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    a.cache = LOCAL/'P6-carga-causal-2026-10-06'
    a.direction_block = 32
    if not (a.cache/'R.npy').is_file() or not (a.cache/'manifest.json').is_file():
        raise ValueError('existing single acoustic cache required')
    seed_path = LOCAL/'P6-contacto96-seed-2026-10-07/Contacto-96-3072k.npz'
    checkpoint_path = DOC/'medidas-P6-coast-campo/coast-512k.json'
    completed_path = DOC/'medidas-P6-coast-campo/coast-512k-validado.json'
    old = json.loads(completed_path.read_text(encoding='utf-8'))
    expected = json.loads(checkpoint_path.read_text(encoding='utf-8'))
    if (old['stage'] != 'refined 512 kHz coast field audit completed'
            or not old['coast_gates_passed'] or not all(old['coast_gates'].values())
            or old['duration_s'] != .0005 or old['rates'] != [128000,256000,512000]):
        raise ValueError('completed matched coast required')
    verify(old)
    if digest(a.board) != old['board_sha256']:
        raise ValueError('changed board')
    start = time.perf_counter()
    sources = set(old['source_sha256']) | {Path(__file__).name,'concert_grand_window_spectra.py'}
    r = dict(stage='construction',all_gates_passed=False,preflight_only=True,
        criteria=CRITERIA,board_sha256=digest(a.board),cache_manifest_sha256=digest(a.cache/'manifest.json'),
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in sorted(sources)},
        parent_sha256={str(completed_path):digest(completed_path),str(checkpoint_path):digest(checkpoint_path)},
        geometry=old['geometry'],microphones=old['microphones'],rates=[256000,512000],
        duration_s=.0005,start_after_contact_s=.003,probe_frequencies_hz=PROBES.tolist(),
        scope='Finite 3-3.5 ms board source window, rectangular and Hann observation; '
              'directional harmonic load probes, not live-memory replay, '
              'full-operator bandwidth certificate, radiation ledger, whole note or timbre.',cases=[])
    def save():
        r['seconds'] = time.perf_counter()-start
        r['peak_working_set_bytes'] = field.full.reference.peak_memory()
        a.report.parent.mkdir(parents=True,exist_ok=True)
        a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')
    save()
    approved,modal,Bsource,notes,surface,provider,identity,_ = field.full.prepare_reference(a)
    _,wave,air = field.full.construct_air(a,approved,provider,surface,identity,12*2**30,r,save)
    if r['cache_R_sha256'] != old['cache_R_sha256']:
        raise ValueError('changed air')
    seed,checkpoint,proof = authenticated_seed(checkpoint_path,seed_path,
        '90cfd00a7363b12bb404f607c85c7955cab0876e',expected)
    r['seed_resume'] = proof
    core = field.MovingBridge(field.Wire(.62,.0081,850),
        modal.support(Bsource[:,notes.index(60)]),transverse=96,axial=192)
    system = field.BridgeHammer(core,field.Felt(.002,7.5e8,2.6,.125,.02,.1))
    contact = checkpoint['contact']
    r['seed_consistency_relative'] = dict(
        mechanical=parent.anchor.reference.difference(system.energy(seed[0],seed[1]),contact['mechanical_final_j']),
        relaxation=parent.anchor.reference.difference(air.relaxation.energy(seed[3]),contact['relaxation_final_j']),
        wave_norm=parent.anchor.reference.difference(float(np.linalg.norm(seed[2])),contact['final_wave_norm']),
        compression=parent.anchor.reference.difference(system.compression(seed[0]),contact['compression_final_m']))
    if max(r['seed_consistency_relative'].values()) >= CRITERIA['reproduction_relative']:
        raise ValueError('seed inconsistent with operator')
    print('authenticated live endpoint; no contact reconstruction',flush=True)
    pilot = field.ExponentialCoast(system,air)
    frequency = np.unique(np.r_[np.arange(0.,24001.,125.),60.,480.,1980.])
    coarse_frequency = np.unique(np.r_[np.arange(0.,24001.,250.),60.,480.,1980.])
    coarse_indices = np.searchsorted(frequency,coarse_frequency)
    selected = np.searchsorted(frequency,PROBES)
    r['frequency_grid_hz'] = frequency.tolist()
    spectra = []
    pressure_sources = []
    for rate in r['rates']:
        r['stage'] = f'capture and source spectra at {rate}';save()
        final,row,ports,t,B,V = run_coast(pilot,seed,rate,.0005,
            lambda i,n:print('coast',rate,i,n,flush=True))
        previous = next(c for c in old['cases'] if c['rate']==rate)
        keys = ['mechanical_final_j','relaxation_final_j','final_wave_norm','compression_final_m',
                'felt_heat_j','material_heat_j','relaxation_loss_j','air_port_work_j']
        row['reproduction_relative'] = {key:parent.anchor.reference.difference(row[key],previous[key]) for key in keys}
        result = {}
        transforms = {}
        for taper in [False,True]:
            label = 'hann' if taper else 'rectangular'
            vel = velocity_spectrum(t,B,V,frequency,taper,12)
            acc = (hann_acceleration_spectrum if taper else acceleration_spectrum)(t,B,V,frequency)
            transforms[label] = (vel,acc)
            vel8 = velocity_spectrum(t,B,V,PROBES,taper,8)
            velocity_bands = band_norms(frequency,vel,BANDS)
            acceleration_bands = band_norms(frequency,acc,BANDS)
            coarse_v = band_norms(coarse_frequency,vel[coarse_indices],BANDS)
            coarse_a = band_norms(coarse_frequency,acc[coarse_indices],BANDS)
            result[label] = dict(velocity_squared_transform_integrals=velocity_bands,
                acceleration_squared_transform_integrals=acceleration_bands,
                velocity_above_6k_fraction=(velocity_bands['6000-12000']+velocity_bands['12000-24000'])/velocity_bands['0-24000'],
                acceleration_above_6k_fraction=(acceleration_bands['6000-12000']+acceleration_bands['12000-24000'])/acceleration_bands['0-24000'],
                velocity_time_quadrature_relative=field.relative(vel8,vel[selected]),
                frequency_integral_relative={name:{key:abs(coarse[key]-fine[key])/max(fine[key],1e-40)
                    for key in fine} for name,fine,coarse in [('velocity',velocity_bands,coarse_v),('acceleration',acceleration_bands,coarse_a)]})
        row['source'] = result
        r['cases'].append(row)
        spectra.append(transforms)
        pressure_sources.append(transforms['hann'][1][selected])
        del final,ports,B,V
        save()
    r['source_capture_relative'] = {taper:{name:{f'{lo:g}-{hi:g}':spectral_relative(frequency,
        spectra[0][taper][i],spectra[1][taper][i],lo,hi) for lo,hi in BANDS}
        for i,name in enumerate(['velocity','acceleration'])} for taper in ['rectangular','hann']}
    r['stage'] = 'microphone quadrature probes';save()
    observer4 = CellObserver(provider.surface,r['microphones'],4)
    observer8 = CellObserver(provider.surface,r['microphones'],8)
    p4 = projected_pressure(observer4,PROBES,pressure_sources[-1])
    p8 = projected_pressure(observer8,PROBES,pressure_sources[-1])
    p256 = projected_pressure(observer8,PROBES,pressure_sources[0])
    r['pressure_probes'] = [dict(frequency_hz=float(hz),hann_transform_pa_s=[
        [float(z.real),float(z.imag)] for z in source],
        cell_quadrature_relative=field.relative(approx,source),
        source_capture_relative=field.relative(capture,source))
        for hz,approx,source,capture in zip(PROBES,p4,p8,p256)]
    del observer4,observer8
    with np.load(a.board) as data:
        geometry = dict(panel=json.loads(str(data['panel'])),boundary=str(data['boundary']),
            nx=int(data['nx']),ny=int(data['ny']),vectors=data['ritz_vectors']@modal.basis)
    refined_surface = RealArithmeticSurface(geometry,[2*n for n in approved['cells']])
    r['load_cells'] = [approved['cells'],[2*n for n in approved['cells']]]
    r['load_probes'] = []
    for hz,source in zip(PROBES,spectra[-1]['hann'][0][selected]):
        r['stage'] = f'directional load probe {hz:g} Hz';save()
        s = 2j*np.pi*hz
        norm = float(np.linalg.norm(source))
        if norm <= 0:
            raise ValueError('nonzero observed source direction required')
        direction = source/norm
        continuous8 = CellAir(surface,s,8).force(direction)
        continuous12 = CellAir(surface,s,12).force(direction)
        refined12 = CellAir(refined_surface,s,12).force(direction)
        approximate = air.impedance(s)@direction
        r['load_probes'].append(dict(frequency_hz=float(hz),source_velocity_transform_norm=norm,
            load_action_norm=float(np.linalg.norm(continuous12)),
            load_cell_quadrature_relative=field.relative(continuous8,continuous12),
            load_cell_refinement_relative=field.relative(continuous12,refined12),
            causal_vs_same_cells_relative=field.relative(approximate,continuous12),
            causal_vs_refined_cells_relative=field.relative(approximate,refined12),
            directional_load_within_inherited_2percent=bool(field.relative(approximate,continuous12)<CRITERIA['directional_load_relative'])))
        print('load probe',hz,r['load_probes'][-1],flush=True)
        save()
    verify(r)
    unchanged = (digest(a.board)==r['board_sha256'] and digest(a.cache/'R.npy')==r['cache_R_sha256']
        and digest(a.cache/'manifest.json')==r['cache_manifest_sha256']
        and digest(seed_path)==proof['payload_sha256'] and digest(seed_path.with_suffix('.json'))==proof['manifest_sha256'])
    r['diagnostic_gates'] = dict(
        reproduction=max([*r['seed_consistency_relative'].values(),
            *(v for row in r['cases'] for v in row['reproduction_relative'].values())])<CRITERIA['reproduction_relative'],
        source_capture=max(v for taper in r['source_capture_relative'].values() for kind in taper.values() for v in kind.values())<CRITERIA['source_relative'],
        time_quadrature=max(c['velocity_time_quadrature_relative'] for row in r['cases'] for c in row['source'].values())<CRITERIA['time_quadrature_relative'],
        frequency_integration=max(v for row in r['cases'] for c in row['source'].values() for kind in c['frequency_integral_relative'].values() for v in kind.values())<CRITERIA['frequency_integral_relative'],
        pressure_capture=max(row['source_capture_relative'] for row in r['pressure_probes'])<CRITERIA['pressure_capture_relative'],
        pressure_quadrature=max(row['cell_quadrature_relative'] for row in r['pressure_probes'])<CRITERIA['pressure_cell_quadrature_relative'],
        load_quadrature=max(row['load_cell_quadrature_relative'] for row in r['load_probes'])<CRITERIA['load_cell_quadrature_relative'],
        load_cells=max(row['load_cell_refinement_relative'] for row in r['load_probes'])<CRITERIA['load_cell_refinement_relative'],
        source_and_cache_unchanged=unchanged)
    r['diagnostic_gates'] = {k:bool(v) for k,v in r['diagnostic_gates'].items()}
    r['diagnostic_gates_passed'] = all(r['diagnostic_gates'].values())
    r['stage'] = 'finite-window high-band source and directional load diagnostic completed'
    save()
    print('finished',json.dumps(r['diagnostic_gates']),r['seconds'],flush=True)


if __name__=='__main__':
    main()
