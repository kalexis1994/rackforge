"""Measure the full 0-3 ms assumed C4 contact source through 24 kHz.

Reconstruct authenticated 1536/3072-kHz contact trajectories. Native Hermite
spectra use tested blocked analytic moments; original observers remain the
oracle. Retain endpoint identity, quadrature/refinement failures and scope.
No new R, endpoint capsule, angular history, physical tuning or audio export.
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
from concert_grand_window_spectra import velocity_spectrum,hann_acceleration_spectrum,band_norms
from concert_grand_cell_observer import CellObserver,acceleration_spectrum,spectral_relative
from concert_grand_matrixfree_air import CellAir
from concert_grand_real_surface import RealArithmeticSurface

spec = importlib.util.spec_from_file_location('contact_band_reference',
    Path(__file__).with_name('audit-concert-grand-source-band-refinement.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)
prior = reference.prior
field = prior.field
PROBES = prior.PROBES
BANDS = [(0.,6000.),(6000.,12000.),(12000.,24000.),(0.,24000.)]
RATES = [1536000,3072000]


def transfer_real(observer,frequency,progress=None):
    """Same cell Green integral with real BLAS, no large complex shape copy."""
    out = []
    for i,hz in enumerate(frequency):
        green = np.sum(observer.area[:,None,None]
            *np.exp(-2j*np.pi*hz*observer.distance/observer.surface.speed)/observer.distance,axis=0)
        value = green.real@observer.shapes+1j*(green.imag@observer.shapes)
        out.append(observer.surface.density/(2*np.pi)*value)
        if progress and (i+1)%100==0:
            progress(i+1,len(frequency))
    return np.asarray(out)


class ProgressContact:
    """Transparent progress wrapper; delegate every physical step unchanged."""
    def __init__(self,pilot,rate,report,save):
        self.pilot,self.rate,self.report,self.save = pilot,rate,report,save
        self.count = 0
        self.started = self.updated = time.perf_counter()
    def __getattr__(self,name):
        return getattr(self.pilot,name)
    def step(self,*args,**kwargs):
        result = self.pilot.step(*args,**kwargs)
        self.count += 1
        now = time.perf_counter()
        if now-self.updated>=30:
            self.report['active_contact'] = dict(rate=self.rate,steps_completed=self.count,
                steps_total=round(self.rate*.003),elapsed_s=now-self.started,
                latest_centered_force_n=float(result[1]['force_n']))
            self.save()
            print('contact progress',json.dumps(self.report['active_contact']),flush=True)
            self.updated = now
        return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report',type=Path,required=True)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
    old_path = prior.DOC/'medidas-P6-banda-superior/fuente-carga-24k-refinado.json'
    directed_path = prior.DOC/'medidas-P6-resolucion-cuerda/resolucion-dirigida.json'
    checkpoint_path = prior.DOC/'medidas-P6-coast-campo/coast-512k.json'
    seed_path = prior.LOCAL/'P6-contacto96-seed-2026-10-07/Contacto-96-3072k.npz'
    old = json.loads(old_path.read_text(encoding='utf-8'))
    directed = json.loads(directed_path.read_text(encoding='utf-8'))
    if (old['stage']!='refined finite-window high-band diagnostic completed'
            or not old['diagnostic_gates_passed'] or not all(old['diagnostic_gates'].values())
            or not directed['directed_gates_passed'] or not all(directed['directed_gates'].values())):
        raise ValueError('completed source-window and directed contact controls required')
    prior.verify(old);prior.verify(directed)
    expected = json.loads(checkpoint_path.read_text(encoding='utf-8'))
    a.full_report = prior.DOC/'medidas-P6-aire-bloques/carga128-pade2.json'
    a.board = prior.LOCAL/'P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    a.cache = prior.LOCAL/'P6-carga-causal-2026-10-06'
    a.direction_block = 32
    if not (a.cache/'R.npy').is_file() or not (a.cache/'manifest.json').is_file():
        raise ValueError('existing single acoustic cache required')
    criteria = old['criteria'].copy()
    criteria.update(analytic_oracle_relative=1e-10,mechanical_budget_relative=1e-9,
                    impulse_relative=1e-9,capture_decimation_relative=.001)
    start = time.perf_counter()
    sources = set(old['source_sha256']) | set(directed['source_sha256']) | {
        Path(__file__).name,'concert_grand_uniform_spectra.py'}
    frequency = np.unique(np.r_[np.arange(0.,24001.,31.25),60.,480.,1980.])
    grids = [np.unique(np.r_[np.arange(0.,24001.,step),60.,480.,1980.]) for step in [125.,62.5,31.25]]
    indices = [np.searchsorted(frequency,f) for f in grids]
    selected = np.searchsorted(frequency,PROBES)
    oracle_frequency = np.r_[0.,60.,PROBES]
    oracle_indices = np.searchsorted(frequency,oracle_frequency)
    r = dict(stage='construction',all_gates_passed=False,preflight_only=True,criteria=criteria,
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in sorted(sources)},
        parent_sha256={str(path):digest(path) for path in [old_path,directed_path,checkpoint_path]},
        board_sha256=digest(a.board),cache_manifest_sha256=digest(a.cache/'manifest.json'),
        geometry=old['geometry'],microphones=old['microphones'],rates=RATES,
        duration_s=.003,incident_speed_m_s=5.,transverse=96,axial=192,
        frequency_grid_hz=frequency.tolist(),frequency_steps_hz=[125.,62.5,31.25],
        scope='Full 0-3 ms source spectra of assumed C4, rectangular/Hann; '
              'finite-window microphone contribution and harmonic directional load probes. '
              'No angular ledger, causal live-memory replay, spatial bandwidth or timbre approval.',cases=[])
    def save():
        r['seconds'] = time.perf_counter()-start
        r['peak_working_set_bytes'] = field.full.reference.peak_memory()
        a.report.parent.mkdir(parents=True,exist_ok=True)
        a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')
    save()
    if r['board_sha256']!=old['board_sha256'] or r['board_sha256']!=directed['board_sha256']:
        raise ValueError('changed board')
    approved,modal,Bsource,notes,surface,provider,identity,_ = field.full.prepare_reference(a)
    trace_bytes = (round(.003*RATES[-1])+1)*provider.modes*8*2
    oracle_bytes = round(.003*RATES[-1])*provider.modes*12*64
    estimate = (a.cache/'R.npy').stat().st_size+provider.shapes.nbytes+5*trace_bytes+oracle_bytes+2560*2**20
    r['array_estimates'] = dict(estimated_bytes=int(estimate),maximum_bytes=12*2**30,
        largest_native_trace_bytes=int(trace_bytes),oracle_scratch_bytes=int(oracle_bytes),
        angular_history_bytes=0,explanation='Conservative arrays including old quadrature oracle; not an OS cap.')
    save()
    if estimate>12*2**30:
        raise MemoryError('declared array budget exceeded')
    _,wave,air = field.full.construct_air(a,approved,provider,surface,identity,12*2**30,r,save)
    if r['cache_R_sha256']!=old['cache_R_sha256']:
        raise ValueError('changed acoustic load')
    seed,checkpoint,proof = authenticated_seed(checkpoint_path,seed_path,
        '90cfd00a7363b12bb404f607c85c7955cab0876e',expected)
    r['seed_provenance'] = proof
    system = field.BridgeHammer(field.MovingBridge(field.Wire(.62,.0081,850),
        modal.support(Bsource[:,notes.index(60)]),transverse=96,axial=192),
        field.Felt(.002,7.5e8,2.6,.125,.02,.1))
    r['stage'] = 'continuous pressure transfer through 24 kHz';save()
    observer4 = CellObserver(provider.surface,r['microphones'],4)
    H = transfer_real(observer4,frequency,lambda i,n:print('pressure transfer',i,n,flush=True))
    observer8 = CellObserver(provider.surface,r['microphones'],8)
    H8 = transfer_real(observer8,PROBES)
    r['pressure_transfer_quadrature_relative'] = [field.relative(x,y) for x,y in zip(H[selected],H8)]
    # Separate implementation control against the untouched public observer.
    r['pressure_transfer_oracle_relative'] = field.relative(H[selected],observer4.acceleration_transfer(PROBES))
    del observer8
    spectra = []
    pressures = []
    for rate in RATES:
        r['stage'] = f'full contact at {rate}';save()
        pilot = ProgressContact(field.FastBoundedContact(system,air),rate,r,save)
        state,row,t,B,V = capture_contact(pilot,5.,rate,.003,
            lambda i,n:print('contact quarter',rate,i,n,flush=True))
        previous = next(c for c in directed['cases'] if c['case']==prior.parent.anchor.case_key(96,rate))['contact']
        row['reproduction_relative'] = {key:prior.parent.anchor.reference.difference(row[key],value)
            for key,value in previous.items() if key.endswith('_j') or key in ['final_wave_norm','compression_final_m',
                'peak_centered_contact_force_n','contact_impulse_n_s','hammer_momentum_change_n_s']}
        if rate==RATES[-1]:
            r['canonical_endpoint_exact'] = [bool(np.array_equal(x,y)) for x,y in zip(state,seed)]
            if not all(r['canonical_endpoint_exact']):
                raise ValueError('reconstructed canonical endpoint differs')
        r.pop('active_contact',None)
        del state,pilot
        r['stage'] = f'full contact spectra at {rate}';save()
        began = time.perf_counter()
        transforms = uniform_window_spectra(t,B,V,frequency)
        row['analytic_spectra_s'] = time.perf_counter()-began
        began = time.perf_counter()
        checks = {}
        for label,taper in [('rectangular',False),('hann',True)]:
            v_oracle = velocity_spectrum(t,B,V,oracle_frequency,taper,12)
            a_oracle = (hann_acceleration_spectrum if taper else acceleration_spectrum)(t,B,V,oracle_frequency)
            checks[label] = {name:[field.relative(x,y) for x,y in zip(values[oracle_indices],oracle)]
                for name,values,oracle in [('velocity',transforms[label][0],v_oracle),('acceleration',transforms[label][1],a_oracle)]}
        row['analytic_oracle_relative'] = checks
        row['oracle_spectra_s'] = time.perf_counter()-began
        P = {label:np.einsum('fcm,fm->fc',H,pair[1]) for label,pair in transforms.items()}
        decimated = uniform_window_spectra(t[::2],B[::2],V[::2],frequency)
        row['capture_decimation_relative'] = {label:{name:{f'{lo:g}-{hi:g}':spectral_relative(frequency,actual,expected,lo,hi)
            for lo,hi in BANDS} for name,actual,expected in [
                ('velocity',decimated[label][0],transforms[label][0]),
                ('acceleration',decimated[label][1],transforms[label][1]),
                ('pressure',np.einsum('fcm,fm->fc',H,decimated[label][1]),P[label])]} for label in transforms}
        result = {}
        for label,pair in transforms.items():
            values = dict(velocity=pair[0],acceleration=pair[1],pressure=P[label])
            norms = [{name:band_norms(f,values[name][idx],BANDS) for name in values} for f,idx in zip(grids,indices)]
            result[label] = dict(integrals_by_step_hz={str(step):n for step,n in zip(r['frequency_steps_hz'],norms)},
                frequency_refinements_relative=[{name:{key:abs(coarse[name][key]-fine[name][key])/max(fine[name][key],1e-40)
                    for key in fine[name]} for name in fine} for coarse,fine in zip(norms[:-1],norms[1:])],
                above_6k_fraction={name:(norms[-1][name]['6000-12000']+norms[-1][name]['12000-24000'])/norms[-1][name]['0-24000'] for name in values})
        row['source_and_pressure'] = result
        r['cases'].append(row)
        spectra.append(transforms);pressures.append(P)
        del B,V,t,decimated
        save()
    r['contact_temporal_relative'] = {label:{name:{f'{lo:g}-{hi:g}':spectral_relative(frequency,actual,expected,lo,hi)
        for lo,hi in BANDS} for name,actual,expected in [
            ('velocity',spectra[0][label][0],spectra[1][label][0]),
            ('acceleration',spectra[0][label][1],spectra[1][label][1]),
            ('pressure',pressures[0][label],pressures[1][label])]} for label in spectra[0]}
    r['pressure_probes'] = {label:[dict(frequency_hz=float(hz),transform_pa_s=[[float(z.real),float(z.imag)] for z in source])
        for hz,source in zip(PROBES,pressures[-1][label][selected])] for label in pressures[-1]}
    with np.load(a.board) as data:
        geometry = dict(panel=json.loads(str(data['panel'])),boundary=str(data['boundary']),
            nx=int(data['nx']),ny=int(data['ny']),vectors=data['ritz_vectors']@modal.basis)
    refined_surface = RealArithmeticSurface(geometry,[2*n for n in approved['cells']])
    r['load_cells'] = [approved['cells'],[2*n for n in approved['cells']]]
    r['load_probes'] = []
    for i,(hz,source) in enumerate(zip(PROBES,spectra[-1]['hann'][0][selected])):
        r['stage'] = f'full-contact directional load {hz:g} Hz';save()
        norm = float(np.linalg.norm(source))
        if norm<=0:
            raise ValueError('nonzero source direction required')
        direction = source/norm;s = 2j*np.pi*hz
        continuous8 = CellAir(surface,s,8).force(direction)
        continuous12 = CellAir(surface,s,12).force(direction)
        refined12 = CellAir(refined_surface,s,12).force(direction)
        approximate = air.impedance(s)@direction
        coarse = spectra[0]['hann'][0][selected][i]
        r['load_probes'].append(dict(frequency_hz=float(hz),source_velocity_transform_norm=norm,
            direction_contact_temporal_relative=field.relative(coarse/np.linalg.norm(coarse),direction),
            load_cell_quadrature_relative=field.relative(continuous8,continuous12),
            load_cell_refinement_relative=field.relative(continuous12,refined12),
            causal_vs_same_cells_relative=field.relative(approximate,continuous12),
            causal_vs_refined_cells_relative=field.relative(approximate,refined12),
            directional_load_within_inherited_2percent=bool(field.relative(approximate,continuous12)<criteria['directional_load_relative'])))
        print('load probe',hz,r['load_probes'][-1],flush=True);save()
    prior.verify(r)
    unchanged = (digest(a.board)==r['board_sha256'] and digest(a.cache/'R.npy')==r['cache_R_sha256']
        and digest(a.cache/'manifest.json')==r['cache_manifest_sha256']
        and digest(seed_path)==proof['payload_sha256'] and digest(seed_path.with_suffix('.json'))==proof['manifest_sha256'])
    nested = reference.maximum_nested
    r['diagnostic_gates'] = dict(
        contact_reproduction=max(nested(row['reproduction_relative']) for row in r['cases'])<criteria['reproduction_relative'],
        endpoint_exact=all(r['canonical_endpoint_exact']),
        mechanical_budget=max(row['maximum_mechanical_budget_relative'] for row in r['cases'])<criteria['mechanical_budget_relative'],
        impulse=max(row['impulse_momentum_relative'] for row in r['cases'])<criteria['impulse_relative'],
        nonnegative_heat=all(row['minimum_step_heat_j']>=-1e-16 for row in r['cases']),
        analytic_oracle=max(max(v for values in row['analytic_oracle_relative'].values() for vlist in values.values() for v in vlist)
            for row in r['cases'])<criteria['analytic_oracle_relative'] and r['pressure_transfer_oracle_relative']<criteria['analytic_oracle_relative'],
        contact_temporal=nested(r['contact_temporal_relative'])<criteria['source_relative'],
        capture_decimation=nested(r['cases'][-1]['capture_decimation_relative'])<criteria['capture_decimation_relative'],
        frequency_integration=max(nested(c['frequency_refinements_relative'][-1]) for row in r['cases'] for c in row['source_and_pressure'].values())<criteria['frequency_integral_relative'],
        pressure_quadrature=max(r['pressure_transfer_quadrature_relative'])<criteria['pressure_cell_quadrature_relative'],
        load_quadrature=max(row['load_cell_quadrature_relative'] for row in r['load_probes'])<criteria['load_cell_quadrature_relative'],
        load_cells=max(row['load_cell_refinement_relative'] for row in r['load_probes'])<criteria['load_cell_refinement_relative'],
        source_and_cache_unchanged=unchanged)
    r['diagnostic_gates'] = {k:bool(v) for k,v in r['diagnostic_gates'].items()}
    r['diagnostic_gates_passed'] = all(r['diagnostic_gates'].values())
    r['stage'] = 'full-contact high-band source and directional load diagnostic completed'
    save()
    print('finished',json.dumps(r['diagnostic_gates']),r['seconds'],flush=True)


if __name__=='__main__':
    main()
