"""Resolve the three failed high-band source-window controls without tuning.

The previous 256/512-kHz diagnostic is immutable. Refine native integration
and capture to 512/1024/2048 kHz and frequency integrals to 31.25/62.5 Hz.
No angular certificate, new acoustic cache, contact rerun or audio export.
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

spec = importlib.util.spec_from_file_location('source_band_parent',
    Path(__file__).with_name('audit-concert-grand-source-band.py'))
prior = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prior)
field = prior.field
PROBES = prior.PROBES
BANDS = prior.BANDS


def maximum_nested(value):
    if isinstance(value,dict):
        return max(maximum_nested(v) for v in value.values())
    return float(value)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report',type=Path,required=True)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
    old_path = prior.DOC/'medidas-P6-banda-superior/fuente-carga-24k.json'
    checkpoint_path = prior.DOC/'medidas-P6-coast-campo/coast-512k.json'
    seed_path = prior.LOCAL/'P6-contacto96-seed-2026-10-07/Contacto-96-3072k.npz'
    old = json.loads(old_path.read_text(encoding='utf-8'))
    if (old['stage']!='finite-window high-band source and directional load diagnostic completed'
            or {k for k,v in old['diagnostic_gates'].items() if not v}
            != {'source_capture','frequency_integration','pressure_capture'}):
        raise ValueError('completed parent with the three measured failures required')
    prior.verify(old)
    expected = json.loads(checkpoint_path.read_text(encoding='utf-8'))
    a.full_report = prior.DOC/'medidas-P6-aire-bloques/carga128-pade2.json'
    a.board = prior.LOCAL/'P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    a.cache = prior.LOCAL/'P6-carga-causal-2026-10-06'
    a.direction_block = 32
    if not (a.cache/'R.npy').is_file() or not (a.cache/'manifest.json').is_file():
        raise ValueError('existing single cache required')
    criteria = old['criteria'].copy()
    sources = set(old['source_sha256']) | {Path(__file__).name}
    r = dict(stage='construction',all_gates_passed=False,preflight_only=True,criteria=criteria,
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in sorted(sources)},
        parent_sha256={str(old_path):digest(old_path)},board_sha256=digest(a.board),
        cache_manifest_sha256=digest(a.cache/'manifest.json'),geometry=old['geometry'],microphones=old['microphones'],
        duration_s=.0005,start_after_contact_s=.003,rates=[512000,1024000,2048000],
        frequency_steps_hz=[125.,62.5,31.25],probe_frequencies_hz=PROBES.tolist(),
        scope='Refined finite-window source and harmonic directional load diagnostics only; '
              'no new angular balance, full-operator bandwidth, whole-note or timbre approval.',cases=[])
    if r['board_sha256']!=old['board_sha256']:
        raise ValueError('changed board')
    start = time.perf_counter()
    def save():
        r['seconds'] = time.perf_counter()-start
        r['peak_working_set_bytes'] = field.full.reference.peak_memory()
        a.report.parent.mkdir(parents=True,exist_ok=True)
        a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')
    save()
    approved,modal,Bsource,notes,surface,provider,identity,_ = field.full.prepare_reference(a)
    _,wave,air = field.full.construct_air(a,approved,provider,surface,identity,12*2**30,r,save)
    if r['cache_R_sha256']!=old['cache_R_sha256']:
        raise ValueError('changed load')
    seed,checkpoint,proof = authenticated_seed(checkpoint_path,seed_path,
        '90cfd00a7363b12bb404f607c85c7955cab0876e',expected)
    r['seed_resume'] = proof
    core = field.MovingBridge(field.Wire(.62,.0081,850),
        modal.support(Bsource[:,notes.index(60)]),transverse=96,axial=192)
    system = field.BridgeHammer(core,field.Felt(.002,7.5e8,2.6,.125,.02,.1))
    contact = checkpoint['contact']
    r['seed_consistency_relative'] = dict(
        mechanical=prior.parent.anchor.reference.difference(system.energy(seed[0],seed[1]),contact['mechanical_final_j']),
        relaxation=prior.parent.anchor.reference.difference(air.relaxation.energy(seed[3]),contact['relaxation_final_j']),
        wave_norm=prior.parent.anchor.reference.difference(float(np.linalg.norm(seed[2])),contact['final_wave_norm']),
        compression=prior.parent.anchor.reference.difference(system.compression(seed[0]),contact['compression_final_m']))
    if maximum_nested(r['seed_consistency_relative'])>=criteria['reproduction_relative']:
        raise ValueError('seed inconsistent with operator')
    print('authenticated endpoint, refining source only',flush=True)
    pilot = field.ExponentialCoast(system,air)
    frequency = np.unique(np.r_[np.arange(0.,24001.,31.25),60.,480.,1980.])
    grids = [np.unique(np.r_[np.arange(0.,24001.,step),60.,480.,1980.]) for step in r['frequency_steps_hz']]
    indices = [np.searchsorted(frequency,f) for f in grids]
    selected = np.searchsorted(frequency,PROBES)
    r['frequency_grid_hz'] = frequency.tolist()
    spectra = []
    pressures = []
    observer8 = CellObserver(provider.surface,r['microphones'],8)
    observer4 = CellObserver(provider.surface,r['microphones'],4)
    native = []
    for rate in r['rates']:
        r['stage'] = f'capture and refined spectra at {rate}';save()
        final,row,ports,t,B,V = run_coast(pilot,seed,rate,.0005,
            lambda i,n:print('coast',rate,i,n,flush=True))
        native.append((t,B,V))
        result = {}
        transforms = {}
        for taper in [False,True]:
            label = 'hann' if taper else 'rectangular'
            vel = velocity_spectrum(t,B,V,frequency,taper,12)
            acc = (hann_acceleration_spectrum if taper else acceleration_spectrum)(t,B,V,frequency)
            transforms[label] = (vel,acc)
            norms = [{name:band_norms(f,values[i],BANDS) for name,values in [('velocity',vel),('acceleration',acc)]}
                     for f,i in zip(grids,indices)]
            result[label] = dict(integrals_by_step_hz={str(step):n for step,n in zip(r['frequency_steps_hz'],norms)},
                frequency_refinements_relative=[{name:{key:abs(coarse[name][key]-fine[name][key])/max(fine[name][key],1e-40)
                    for key in fine[name]} for name in fine} for coarse,fine in zip(norms[:-1],norms[1:])],
                velocity_above_6k_fraction=(norms[-1]['velocity']['6000-12000']+norms[-1]['velocity']['12000-24000'])/norms[-1]['velocity']['0-24000'],
                acceleration_above_6k_fraction=(norms[-1]['acceleration']['6000-12000']+norms[-1]['acceleration']['12000-24000'])/norms[-1]['acceleration']['0-24000'],
                velocity_time_quadrature_relative=field.relative(velocity_spectrum(t,B,V,PROBES,taper,8),vel[selected]))
        row['source'] = result
        if rate==512000:
            previous = old['cases'][-1]
            keys = ['mechanical_final_j','relaxation_final_j','final_wave_norm','compression_final_m',
                    'felt_heat_j','material_heat_j','relaxation_loss_j','air_port_work_j']
            r['parent_reproduction_relative'] = {'physics':{k:prior.parent.anchor.reference.difference(row[k],previous[k]) for k in keys},
                'spectral_integrals':{label:{name:{key:prior.parent.anchor.reference.difference(
                    result[label]['integrals_by_step_hz']['125.0'][kind][key],previous['source'][label][name][key])
                    for key in previous['source'][label][name]} for kind,name in [('velocity','velocity_squared_transform_integrals'),
                    ('acceleration','acceleration_squared_transform_integrals')]} for label in result}}
        pressure = projected_pressure(observer8,PROBES,transforms['hann'][1][selected])
        pressures.append(pressure)
        spectra.append(transforms)
        r['cases'].append(row)
        del final,ports
        save()
    r['native_common_times'] = prior.parent.native_comparison(native,r['rates'])
    del native,t,B,V
    r['source_capture_comparisons'] = [dict(rate=rate,reference_rate=r['rates'][-1],
        bands_relative={label:{name:{f'{lo:g}-{hi:g}':spectral_relative(frequency,actual[label][i],spectra[-1][label][i],lo,hi)
        for lo,hi in BANDS} for i,name in enumerate(['velocity','acceleration'])} for label in ['rectangular','hann']})
        for rate,actual in zip(r['rates'][:-1],spectra[:-1])]
    old_pressure = np.array([np.array(row['hann_transform_pa_s'])[:,0]+1j*np.array(row['hann_transform_pa_s'])[:,1] for row in old['pressure_probes']])
    r['parent_reproduction_relative']['pressure'] = field.relative(pressures[0],old_pressure)
    p4 = projected_pressure(observer4,PROBES,spectra[-1]['hann'][1][selected])
    r['pressure_probes'] = [dict(frequency_hz=float(hz),hann_transform_pa_s=[[float(z.real),float(z.imag)] for z in source],
        capture_relative_by_rate={str(rate):field.relative(p[i],source) for rate,p in zip(r['rates'][:-1],pressures[:-1])},
        cell_quadrature_relative=field.relative(p4[i],source)) for i,(hz,source) in enumerate(zip(PROBES,pressures[-1]))]
    with np.load(a.board) as data:
        geometry = dict(panel=json.loads(str(data['panel'])),boundary=str(data['boundary']),
            nx=int(data['nx']),ny=int(data['ny']),vectors=data['ritz_vectors']@modal.basis)
    refined_surface = RealArithmeticSurface(geometry,[2*n for n in approved['cells']])
    r['load_cells'] = old['load_cells']
    r['load_probes'] = []
    for i,(hz,source) in enumerate(zip(PROBES,spectra[-1]['hann'][0][selected])):
        r['stage'] = f'refined source load probe {hz:g} Hz';save()
        norm = float(np.linalg.norm(source))
        if norm<=0:
            raise ValueError('nonzero source required')
        direction = source/norm
        s = 2j*np.pi*hz
        continuous8 = CellAir(surface,s,8).force(direction)
        continuous12 = CellAir(surface,s,12).force(direction)
        refined12 = CellAir(refined_surface,s,12).force(direction)
        approx = air.impedance(s)@direction
        r['load_probes'].append(dict(frequency_hz=float(hz),source_velocity_transform_norm=norm,
            direction_capture_relative=field.relative(spectra[-2]['hann'][0][selected][i]/np.linalg.norm(spectra[-2]['hann'][0][selected][i]),direction),
            load_cell_quadrature_relative=field.relative(continuous8,continuous12),
            load_cell_refinement_relative=field.relative(continuous12,refined12),
            causal_vs_same_cells_relative=field.relative(approx,continuous12),
            causal_vs_refined_cells_relative=field.relative(approx,refined12),
            directional_load_within_inherited_2percent=bool(field.relative(approx,continuous12)<criteria['directional_load_relative'])))
        print('load probe',hz,r['load_probes'][-1],flush=True);save()
    prior.verify(r)
    unchanged = (digest(a.board)==r['board_sha256'] and digest(a.cache/'R.npy')==r['cache_R_sha256']
        and digest(a.cache/'manifest.json')==r['cache_manifest_sha256']
        and digest(seed_path)==proof['payload_sha256'] and digest(seed_path.with_suffix('.json'))==proof['manifest_sha256'])
    r['diagnostic_gates'] = dict(
        reproduction=max(maximum_nested(r['seed_consistency_relative']),maximum_nested(r['parent_reproduction_relative']))<criteria['reproduction_relative'],
        source_capture=maximum_nested(r['source_capture_comparisons'][-1]['bands_relative'])<criteria['source_relative'],
        frequency_integration=max(maximum_nested(c['frequency_refinements_relative'][-1]) for row in r['cases'] for c in row['source'].values())<criteria['frequency_integral_relative'],
        time_quadrature=max(c['velocity_time_quadrature_relative'] for row in r['cases'] for c in row['source'].values())<criteria['time_quadrature_relative'],
        pressure_capture=max(row['capture_relative_by_rate']['1024000'] for row in r['pressure_probes'])<criteria['pressure_capture_relative'],
        pressure_quadrature=max(row['cell_quadrature_relative'] for row in r['pressure_probes'])<criteria['pressure_cell_quadrature_relative'],
        load_quadrature=max(row['load_cell_quadrature_relative'] for row in r['load_probes'])<criteria['load_cell_quadrature_relative'],
        load_cells=max(row['load_cell_refinement_relative'] for row in r['load_probes'])<criteria['load_cell_refinement_relative'],
        source_and_cache_unchanged=unchanged)
    r['diagnostic_gates'] = {k:bool(v) for k,v in r['diagnostic_gates'].items()}
    r['diagnostic_gates_passed'] = all(r['diagnostic_gates'].values())
    r['stage'] = 'refined finite-window high-band diagnostic completed'
    save()
    print('finished',json.dumps(r['diagnostic_gates']),r['seconds'],flush=True)


if __name__=='__main__':
    main()
