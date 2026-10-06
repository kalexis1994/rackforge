"""Observe contact force at common physical timestamps of frozen prefixes.

Retain the original interval-averaged AVF peak and its failed refinement.
Only observation changes: full states, equations, load and impulses stay.
No acoustic ledger, full separation, repetition or timbral approval here.
"""
import argparse
import importlib.util
import json
import time
from pathlib import Path
from types import SimpleNamespace

import numpy as np
from concert_grand_projected_support import ModalSupportMap, projected_matrices
from concert_grand_real_surface import RealArithmeticSurface
from concert_grand_streaming_air import SurfaceFactorBlocks
from concert_grand_pade_air import FourthFactorBlocks, PadeAir, PadeBoundedAir
from concert_grand_wave_cache import digest, load_wave_cache
from concert_grand_moving_bridge import MovingBridge, BridgeHammer
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
from concert_grand_fast_contact import FastBoundedContact
import concert_grand_orthotropic_plate as plate

spec = importlib.util.spec_from_file_location('force_reference',Path(__file__).with_name('audit-concert-grand-streaming-air.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)


def physical_force(system,state):
    q,v = state[:2]
    delta = max(system.compression(q),0.)
    f = system.felt
    return float(f.stiffness*delta**f.exponent*np.exp(
        np.clip(f.rate_damping_s_per_m*float(system.direction@v),-40,40)))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['prefix-report','full-report','board','cache','report']:
        p.add_argument('--'+name,type=Path,required=True)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
    prefix = json.loads(a.prefix_report.read_text())
    full = json.loads(a.full_report.read_text())
    mechanical_stages = {'independent storage/radiation certificate',
                         'independent certificate measured','contact prefix audit completed'}
    expected_cases = {(phase,rate) for phase in ['Cold','Continued'] for rate in prefix['rates']}
    if (prefix['stage'] not in mechanical_stages or not full['harmonic_gates_passed']
            or len(prefix['cases'])!=len(expected_cases)
            or {(row['phase'],row['rate']) for row in prefix['cases']}!=expected_cases
            or digest(a.full_report) not in prefix['parent_sha256'].values()
            or digest(a.board)!=full['board_sha256']
            or a.cache.resolve()!=Path(prefix['cache']).resolve()):
        raise ValueError('matching completed mechanical prefixes/full load/cache required')
    physical = set(full['source_sha256']) | {
        'concert_grand_surface_impedance.py','concert_grand_pade_midpoint.py',
        'concert_grand_fast_contact.py','concert_grand_bounded_contact.py',
        'concert_grand_moving_bridge.py','concert_grand_nonlinear_string.py',
        'concert_grand_coupled_contact.py'}
    for name in physical:
        sha = prefix['source_sha256'].get(name,full['source_sha256'].get(name))
        # The legacy prefix omitted the constants module; its provider signature
        # and explicit source-commit attestation bind those realized constants.
        if sha is not None and digest(Path(__file__).with_name(name))!=sha:
            raise ValueError('changed measured physical source: '+name)
    with np.load(a.board) as d:
        mechanical = {name:d[name] for name in [
            'frequency','low_count','tail_mass','tail_stiffness','tail_damping','low_damping']}
        geometry = dict(panel=json.loads(str(d['panel'])),boundary=str(d['boundary']),
                        nx=int(d['nx']),ny=int(d['ny']),vectors=d['ritz_vectors'])
        notes = d['test_notes'].tolist()
    modal = ModalSupportMap.from_matrices(*projected_matrices(mechanical))
    xs = geometry['panel']['length']*(.1+.8*(np.array(notes)-21)/87)
    source_B = plate.shape(geometry,xs,np.full(len(xs),geometry['panel']['width']/2)).T
    B = source_B[:,notes.index(60)]
    geometry['vectors'] = geometry['vectors']@modal.basis
    surface = RealArithmeticSurface(geometry,full['cells'])
    shapes = surface.forward(np.eye(surface.modes))
    plain = SimpleNamespace(shapes=shapes,modes=surface.modes,nx=surface.nx,ny=surface.ny,
                            hx=surface.hx,hy=surface.hy,length=surface.length,width=surface.width,
                            density=surface.density,speed=surface.speed)
    provider = FourthFactorBlocks(SurfaceFactorBlocks(plain,full['rate'],*full['angular'],32,fold_y=True))
    identity = dict(format=1,board_sha256=digest(a.board),full_report_sha256=digest(a.full_report),
                    physical_source_sha256=full['source_sha256'],cells=full['cells'],rate=full['rate'],
                    angular=full['angular'],derivative='fourth',fold_y=True)
    wave,metadata = load_wave_cache(a.cache,provider,identity)
    if metadata['R_sha256']!=prefix['cache_R_sha256']:
        raise ValueError('changed measured correlations')
    target = reference.direct_matrix(surface,0).real
    target = (target+target.T)/2
    limit = surface.density*surface.speed*surface.hx*surface.hy*(provider.shapes.T@provider.shapes)
    air = PadeBoundedAir(PadeAir(wave),target,limit)
    w,f = prefix['wire'],prefix['felt']
    core = MovingBridge(Wire(w['length'],w['linear_density'],w['tension']),
                        modal.support(B),transverse=w['transverse'])
    system = BridgeHammer(core,Felt(f['hammer_mass'],f['stiffness'],f['exponent'],
                                   f['strike'],f['width'],f['rate_loss']))
    coupled = FastBoundedContact(system,air)
    rates = prefix['rates']
    capture = min(rates)
    if 60 not in notes or surface.modes!=prefix['modes']:
        raise ValueError('unchanged full board and C4 port required')
    initial = coupled.initial_state(prefix['incident_speed_m_s'])
    rows,finals = [],[]
    for phase in ['Cold','Continued']:
        live = initial if phase=='Cold' else finals[len(rates)-1]
        for rate in rates:
            state = tuple(item.copy() for item in live)
            forces = [physical_force(system,state)]
            impulse = peak = 0.
            start = time.perf_counter()
            for i in range(round(rate*prefix['duration_s'])):
                state,stats = coupled.step(state,1/rate,True)
                impulse += stats['force_n']/rate
                peak = max(peak,stats['force_n'])
                if (i+1)%(rate//capture)==0:
                    forces.append(physical_force(system,state))
            observed = dict(impulse_n_s=impulse,maximum_force_n=peak,
                mechanical_final_j=system.energy(*state[:2]),compression_final_m=system.compression(state[0]),
                final_wave_norm=float(np.linalg.norm(state[2])),relaxation_final_j=air.relaxation.energy(state[3]))
            original = next(row for row in prefix['cases'] if row['phase']==phase and row['rate']==rate)
            reproduction = {key:abs(value-original[key])/max(abs(original[key]),1e-30)
                            for key,value in observed.items()}
            if max(reproduction.values())>=1e-10:
                raise ValueError('changed frozen contact trajectory')
            rows.append(dict(phase=phase,rate=rate,common_force_n=forces,
                common_peak_force_n=max(forces),interval_average_peak_force_n=peak,
                impulse_n_s=impulse,reproduction_relative=reproduction,
                elapsed_s=time.perf_counter()-start))
            finals.append(state)
            print('force observer',phase,rate,'common peak',max(forces),flush=True)
    comparisons = []
    ratios = {}
    for phase in ['Cold','Continued']:
        group = [row for row in rows if row['phase']==phase]
        fine = group[-1]
        errors = []
        for row in group[:-1]:
            error = reference.relative(np.array(row['common_force_n']),np.array(fine['common_force_n']))
            errors.append(error)
            comparisons.append(dict(phase=phase,rate=row['rate'],reference_rate=fine['rate'],
                common_trace_relative=error,
                common_peak_relative=abs(row['common_peak_force_n']-fine['common_peak_force_n'])/fine['common_peak_force_n'],
                interval_average_peak_relative=abs(row['interval_average_peak_force_n']-fine['interval_average_peak_force_n'])/fine['interval_average_peak_force_n']))
        ratios[phase] = errors[-1]/errors[-2] if len(errors)>=2 else None
    finest = [row for row in comparisons if row['rate']==rates[-2]]
    gates = dict(common_force_trace=max(row['common_trace_relative'] for row in finest)<.002,
                 common_peak=max(row['common_peak_relative'] for row in finest)<.002,
                 second_order=all(value is not None and value<.35 for value in ratios.values()))
    gates = {name:bool(value) for name,value in gates.items()}
    report = dict(parent_sha256={str(a.prefix_report):digest(a.prefix_report),str(a.full_report):digest(a.full_report)},
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in physical | {
            Path(__file__).name,'concert_grand_wave_cache.py'}},
        cache_R_sha256=metadata['R_sha256'],cache_manifest_sha256=digest(a.cache/'manifest.json'),
        capture_rate=capture,common_time_s=(np.arange(len(rows[0]['common_force_n']))/capture).tolist(),
        rows=rows,comparisons=comparisons,refinement_ratios=ratios,criteria=dict(force_relative=.002,refinement_ratio=.35),
        common_force_gates=gates,common_force_passed=all(gates.values()),
        preflight_only=True,all_gates_passed=False,
        scope='Same measured physical trajectories, continuous constitutive force at common timestamps. '
              'Original interval-average peak retained. No independent ledger/full-contact/timbral approval.')
    a.report.parent.mkdir(parents=True,exist_ok=True)
    a.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    print('force observation gates',gates,flush=True)


if __name__=='__main__':
    main()
