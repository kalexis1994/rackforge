"""Short full-board nonlinear contact prefixes and independent air ledgers.

All 606 board coordinates and living acoustic states stay. A continuation
prefix starts from the finest cold prefix, without relaunch/reset. This is
not full hammer separation, natural repetition, release or timbral approval.
Keep one local R cache; collect all certificate observables in one factor pass.
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
from concert_grand_streaming_air import SurfaceFactorBlocks, StreamingRationalAir
from concert_grand_pade_air import FourthFactorBlocks, PadeAir, PadeBoundedAir
from concert_grand_wave_cache import digest, save_wave_cache, load_wave_cache
from concert_grand_pade_certificate import certificate_batch
from concert_grand_pade_port_certificate import certificate_ports,replay_endpoints
from concert_grand_bounded_observer import observation_basis,certificate_observer
from concert_grand_moving_bridge import MovingBridge, BridgeHammer
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
from concert_grand_fast_contact import FastBoundedContact
import concert_grand_orthotropic_plate as plate

spec = importlib.util.spec_from_file_location(
    'contact_full_reference', Path(__file__).with_name('audit-concert-grand-streaming-air.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)


def physical_force(system,state):
    q,v = state[:2]
    f = system.felt
    return float(f.stiffness*max(system.compression(q),0.)**f.exponent*np.exp(
        np.clip(f.rate_damping_s_per_m*float(system.direction@v),-40,40)))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['full-report', 'board', 'cache', 'report']:
        p.add_argument('--'+name, type=Path, required=True)
    p.add_argument('--rates', type=int, nargs='+', default=[384000, 768000, 1536000])
    p.add_argument('--duration', type=float, default=.000125)
    p.add_argument('--direction-block', type=int, default=32)
    p.add_argument('--max-array-gib', type=float, default=8.)
    p.add_argument('--compact-certificate', action='store_true')
    p.add_argument('--certificate-input-block', type=int, default=16)
    p.add_argument('--replay-preflight-only', action='store_true')
    p.add_argument('--bounded-observer', action='store_true')
    p.add_argument('--store-observer-inputs', action='store_true')
    p.add_argument('--cold-only', action='store_true')
    p.add_argument('--observer-svd-relative', type=float, default=1e-12)
    p.add_argument('--compare-prefix-report', type=Path)
    a = p.parse_args()
    phases = ['Cold'] if a.cold_only else ['Cold','Continued']
    if a.report.exists():
        raise ValueError('new report required')
    if a.replay_preflight_only and not a.compact_certificate:
        raise ValueError('replay preflight requires compact port capture')
    if a.bounded_observer and not a.compact_certificate:
        raise ValueError('bounded observation requires full compact port capture')
    if a.store_observer_inputs and not a.bounded_observer:
        raise ValueError('stored observer inputs require bounded observation')
    if not 0<a.observer_svd_relative<1:
        raise ValueError('positive relative observation-basis tolerance required')
    if (len(a.rates) < 2 or sorted(set(a.rates)) != a.rates or min(a.rates) <= 0
            or any(rate % min(a.rates) for rate in a.rates)
            or not np.isfinite(a.duration) or a.duration <= 0
            or any(abs(round(rate*a.duration)-rate*a.duration) > 1e-9 for rate in a.rates)):
        raise ValueError('nested increasing rates and exact positive duration required')
    if not np.isfinite(a.max_array_gib) or a.max_array_gib <= 0 or a.certificate_input_block < 1:
        raise ValueError('positive memory budget')
    cache_root = Path('local/concert-grand-physics').resolve()
    if not a.cache.resolve().is_relative_to(cache_root):
        raise ValueError('one cache under local/concert-grand-physics required')
    clock = time.get_clock_info('perf_counter')
    start = time.perf_counter()
    approved = json.loads(a.full_report.read_text())
    if not approved['harmonic_gates_passed'] or not all(approved['harmonic_gates'].values()):
        raise ValueError('approved full-coordinate harmonic load required')
    if digest(a.board) != approved['board_sha256']:
        raise ValueError('changed projected board')
    for name, sha in approved['source_sha256'].items():
        if digest(Path(__file__).with_name(name)) != sha:
            raise ValueError('changed physical construction source: '+name)
    for path, sha in approved['parent_sha256'].items():
        if digest(path) != sha:
            raise ValueError('changed frozen parent: '+path)
    with np.load(a.board) as d:
        mechanical = {name: d[name] for name in [
            'frequency', 'low_count', 'tail_mass', 'tail_stiffness', 'tail_damping', 'low_damping']}
        geometry = dict(panel=json.loads(str(d['panel'])), boundary=str(d['boundary']),
                        nx=int(d['nx']), ny=int(d['ny']), vectors=d['ritz_vectors'])
        notes = d['test_notes'].tolist()
    modal = ModalSupportMap.from_matrices(*projected_matrices(mechanical))
    xs = geometry['panel']['length']*(.1+.8*(np.array(notes)-21)/87)
    source_B = plate.shape(geometry, xs, np.full(len(xs), geometry['panel']['width']/2)).T
    geometry['vectors'] = geometry['vectors'] @ modal.basis
    surface = RealArithmeticSurface(geometry, approved['cells'])
    n = surface.modes
    if n != approved['modes'] or notes != approved['notes']:
        raise ValueError('unchanged full board and ports required')
    del geometry, mechanical
    shapes = surface.forward(np.eye(n))
    plain = SimpleNamespace(shapes=shapes, modes=n, nx=surface.nx, ny=surface.ny,
                            hx=surface.hx, hy=surface.hy, length=surface.length,
                            width=surface.width, density=surface.density, speed=surface.speed)
    provider = FourthFactorBlocks(SurfaceFactorBlocks(
        plain, approved['rate'], *approved['angular'], a.direction_block, fold_y=True))
    identity = dict(format=1, board_sha256=digest(a.board), full_report_sha256=digest(a.full_report),
                    physical_source_sha256=approved['source_sha256'], cells=approved['cells'],
                    rate=approved['rate'], angular=approved['angular'], derivative='fourth', fold_y=True)
    budget = provider.budget(real_gram=True)
    max_bytes = int(a.max_array_gib*2**30)
    port_bytes = len(phases)*sum(round(rate*a.duration) for rate in a.rates)*n*8
    section_bytes = port_bytes*provider.taps
    inputs_bytes = port_bytes if a.compact_certificate else section_bytes
    replay_chunk_bytes = a.certificate_input_block*provider.taps*n*8 if a.compact_certificate else 0
    # Conservative separate construction/certificate estimates, not an OS cap.
    certificate_bytes = (budget['correlation_bytes']+inputs_bytes+replay_chunk_bytes+2*shapes.nbytes
                         +5*budget['factor_block_bytes']+32*provider.taps*n*16
                         +128*2**20)
    # Before selecting Q, budget its worst possible rank: all full coordinates.
    observation_history_max = section_bytes if a.store_observer_inputs else 0
    certificate_bytes += observation_history_max
    if max(budget['estimated_array_peak_bytes'], certificate_bytes) > max_bytes:
        raise MemoryError('declared construction/certificate budget exceeded')
    sources = sorted(set(approved['source_sha256']) | {
        Path(__file__).name, 'concert_grand_wave_cache.py', 'concert_grand_pade_certificate.py',
        'concert_grand_pade_midpoint.py', 'concert_grand_fast_contact.py',
        'concert_grand_bounded_contact.py', 'concert_grand_moving_bridge.py',
        'concert_grand_nonlinear_string.py', 'concert_grand_coupled_contact.py',
        'concert_grand_surface_impedance.py','concert_grand_pade_port_certificate.py',
        'concert_grand_bounded_observer.py'})
    r = dict(parent_sha256={str(a.full_report): digest(a.full_report)}, board_sha256=digest(a.board),
             source_sha256={name: digest(Path(__file__).with_name(name)) for name in sources},
             cache=str(a.cache), rate_factor=provider.rate, angular=approved['angular'], cells=approved['cells'],
             modes=n, states=approved['states'], rates=a.rates, duration_s=a.duration,phases=phases,
             array_estimates=dict(construction=budget['estimated_array_peak_bytes'],
                                  certificate=certificate_bytes, histories=inputs_bytes,
                                  full_section_inputs=section_bytes, compact_port_inputs=port_bytes,
                                  replay_chunk=replay_chunk_bytes,
                                  observation_history_maximum=observation_history_max,
                                  maximum=max_bytes),
             criteria=dict(energy_relative=1e-9, interface_force_n=1e-9, interface_power_w=1e-9,
                           mechanical_trace_relative=.01, force_impulse_relative=.002,
                           finest_pair_state_relative=.01, refinement_ratio=.35,
                           observation_relative=1e-4,observation_floor_j=1e-14,
                           cache_control_absolute=1e-10),
             certificate_method='port replay' if a.compact_certificate else 'stored section inputs',
             force_observation='Constitutive force on common captured timestamps; raw AVF peak retained',
             cases=[], stage='construction', preflight_only=True, all_gates_passed=False,
             contact_prefix_gates_passed=False,
             wire=dict(length=.62,linear_density=.0081,tension=850,transverse=3),
             felt=dict(hammer_mass=.002,stiffness=7.5e8,exponent=2.6,strike=.125,width=.02,rate_loss=.1),
             incident_speed_m_s=5.,
             timing_clock=dict(implementation=clock.implementation,resolution_s=clock.resolution),
             scope='Short C4 assumed-wire prefixes on all 606 board modes. Continued live contact, '
                   'not full separation/repetition, release, timbre or runtime approval.')

    def checkpoint():
        r['seconds'] = time.perf_counter()-start
        r['peak_working_set_bytes'] = reference.peak_memory()
        a.report.parent.mkdir(parents=True, exist_ok=True)
        a.report.write_text(json.dumps(r, indent=2, allow_nan=False), encoding='utf-8')

    checkpoint()
    print('array estimates', json.dumps(r['array_estimates']), flush=True)
    if a.cache.exists():
        wave, metadata = load_wave_cache(a.cache, provider, identity)
        r['cache_reused'] = True
    else:
        wave = StreamingRationalAir(provider, max_bytes, real_gram=True,
                                   progress=lambda i: print('wave block', i, flush=True) if i%16 == 0 else None)
        r['cache_reused'] = False
        metadata = None
    air_wave = PadeAir(wave)
    target = reference.direct_matrix(surface, 0).real
    target = (target+target.T)/2
    limit = surface.density*surface.speed*surface.hx*surface.hy*(provider.shapes.T @ provider.shapes)
    air = PadeBoundedAir(air_wave, target, limit)
    residue = target-air_wave.static_mass()
    eig = np.linalg.eigvalsh((residue+residue.T)/2)
    mass_error = max(abs(eig.min()-approved['mass_residual_minimum']),
                     abs(eig.max()-approved['mass_residual_maximum']))/approved['mass_residual_maximum']
    r['cache_mass_control_relative'] = float(mass_error)
    r['cache_transfer_controls'] = []
    for old in approved['rows']:
        hz = old['frequency_hz']
        if hz not in [60.,1881.9607122235952,6000.]:
            continue
        s = 2j*np.pi*hz
        z = air.impedance(s)
        exact = reference.direct_matrix(surface, s)
        errors = [reference.relative(z, exact), reference.relative(z.real, exact.real),
                  reference.relative(z.imag, exact.imag)]
        delta = max(abs(new-prior) for new, prior in zip(errors, old['impedance_ZRX_relative']))
        r['cache_transfer_controls'].append(dict(hz=hz, impedance_ZRX_relative=errors,
                                                 frozen_error_absolute=float(delta)))
    if mass_error >= 1e-10 or max(row['frozen_error_absolute'] for row in r['cache_transfer_controls']) >= 1e-10:
        r['stage'] = 'rejected construction equivalence'
        checkpoint()
        raise AssertionError('cache candidate differs from approved full load')
    if metadata is None:
        metadata = save_wave_cache(a.cache, wave, identity)
    r['cache_R_sha256'] = metadata['R_sha256']
    r['cache_R_bytes'] = (a.cache/'R.npy').stat().st_size
    print('cache verified', r['cache_R_sha256'], flush=True)
    core = MovingBridge(Wire(.62,.0081,850), modal.support(source_B[:,notes.index(60)]), transverse=3)
    system = BridgeHammer(core, Felt(.002,7.5e8,2.6,.125,.02,.1))
    coupled = FastBoundedContact(system, air)
    initial = coupled.initial_state(5.)
    certificates = []
    inputs = []
    traces = []
    final_states = []
    capture_rate = min(a.rates)
    r['capture_rate'] = capture_rate
    r['stage'] = 'contact prefixes'
    checkpoint()
    for phase in phases:
        live = initial if phase == 'Cold' else final_states[len(a.rates)-1]
        for rate in a.rates:
            state = tuple(x.copy() for x in live)
            q0, v0 = state[:2]
            mechanical0 = system.energy(q0,v0)
            certificates.append(state[2].copy())
            before_index = len(certificates)-1
            relaxation0 = air.relaxation.energy(state[3])
            parts0 = core.components(q0[:-1],v0[:-1])
            steps = round(rate*a.duration)
            history = np.empty((steps,n) if a.compact_certificate else (steps,provider.taps,n))
            trace = [np.r_[q0,v0]]
            common_forces = [physical_force(system,state)]
            contact_seen = system.compression(state[0])>0
            separation = None
            felt = material = relax = port = impulse = peak_force = 0.
            max_newton = 0
            begin = time.perf_counter()
            step_seconds = []
            min_heat = 0.
            for i in range(steps):
                step_start = time.perf_counter()
                state, stats = coupled.step(state,1/rate,True)
                step_seconds.append(time.perf_counter()-step_start)
                history[i] = stats['wave_inputs'][0] if a.compact_certificate else stats['wave_inputs']
                if system.compression(state[0])>0:
                    contact_seen = True
                elif contact_seen and separation is None:
                    separation = [i/rate,(i+1)/rate]
                felt += stats['felt_loss_j']
                material += stats['material_loss_j']
                relax += stats['radiation_loss_j']
                port += stats['air_port_work_j']
                impulse += stats['force_n']/rate
                peak_force = max(peak_force,stats['force_n'])
                max_newton = max(max_newton,stats['newton_iterations'])
                min_heat = min(min_heat,stats['felt_loss_j'],stats['material_loss_j'],stats['radiation_loss_j'])
                if (i+1)%(rate//capture_rate) == 0:
                    trace.append(np.r_[state[0],state[1]])
                    common_forces.append(physical_force(system,state))
                if (i+1)%max(1,steps//4) == 0:
                    print('contact',phase,rate,i+1,steps,'seconds',round(time.perf_counter()-begin,3),flush=True)
            interface = coupled.interface(state)
            certificates.append(state[2].copy())
            after_index = len(certificates)-1
            parts = core.components(state[0][:-1],state[1][:-1])
            row = dict(phase=phase,rate=rate,steps=steps,elapsed_s=time.perf_counter()-begin,
                       first_step_s=step_seconds[0],steady_step_median_s=float(np.median(step_seconds[1:])),
                       mechanical_initial_j=mechanical0,mechanical_final_j=system.energy(state[0],state[1]),
                       relaxation_initial_j=relaxation0,relaxation_final_j=air.relaxation.energy(state[3]),
                       initial_support_j=parts0['support_energy_j'],final_support_j=parts['support_energy_j'],
                       initial_wave_norm=float(np.linalg.norm(live[2])), final_wave_norm=float(np.linalg.norm(state[2])),
                       felt_heat_j=felt,material_heat_j=material,relaxation_loss_j=relax,air_port_work_j=port,
                       impulse_n_s=impulse,maximum_force_n=peak_force,minimum_step_heat_j=min_heat,
                       common_force_n=common_forces,maximum_common_force_n=max(common_forces),
                       separation_bracket_s=separation,initial_compression_m=system.compression(q0),
                       newton_iterations_max=max_newton,compression_final_m=system.compression(state[0]),
                       interface=interface,certificate_before_index=before_index,certificate_after_index=after_index)
            r['cases'].append(row)
            inputs.append(history)
            traces.append(np.array(trace))
            final_states.append(state)
            checkpoint()
    r['stage'] = 'independent storage/radiation certificate'
    checkpoint()
    segments = None
    if a.compact_certificate:
        segments = [dict(initial_index=row['certificate_before_index'],final_index=row['certificate_after_index'],
                         dt=1/row['rate'],velocity=history) for row,history in zip(r['cases'],inputs)]
        replay = replay_endpoints(air_wave,np.array(certificates),segments)
        r['certificate_replay_relative'] = replay.tolist()
        r['compact_replay_passed'] = bool(max(replay)<1e-11)
        r['captured_port_history_bytes'] = sum(history.nbytes for history in inputs)
        if not r['compact_replay_passed']:
            r['stage'] = 'rejected compact endpoint replay'
            checkpoint()
            raise AssertionError('compact histories differ from contact memories')
        if a.replay_preflight_only:
            r['stage'] = 'compact capture/replay preflight completed'
            r['scope'] += ' This run stops before independent storage/radiation: no energy gate approval.'
            checkpoint()
            print('compact capture/replay passed',r['certificate_replay_relative'],r['seconds'],flush=True)
            return
    certificate_start = time.perf_counter()
    progress = lambda i:print('certificate block',i,flush=True) if i%16==0 else None
    observer = None
    if a.bounded_observer:
        Q,basis = observation_basis(air_wave,np.array(certificates),segments,a.observer_svd_relative)
        r['observation_basis'] = basis
        r['observation_input_history_bytes'] = section_bytes*basis['rank']//n if a.store_observer_inputs else 0
        r['certificate_method'] = 'field projection with full residual bounds'
        checkpoint()
        print('observation rank',basis['rank'],'of',basis['full_modes'],flush=True)
        energies,losses,observer = certificate_observer(air_wave,np.array(certificates),segments,Q,
            progress=progress,input_block=a.certificate_input_block,
            store_projected_inputs=a.store_observer_inputs)
        r['bounded_observer'] = observer
    elif a.compact_certificate:
        energies,losses,replay = certificate_ports(air_wave,np.array(certificates),segments,
                                                 progress=progress,input_block=a.certificate_input_block)
        r['certificate_replay_relative'] = replay.tolist()
    else:
        energies, losses = certificate_batch(provider,np.array(certificates),inputs,
                                            progress=progress,input_block=a.certificate_input_block)
    r['certificate_seconds'] = time.perf_counter()-certificate_start
    for row, power in zip(r['cases'],losses):
        row['wave_initial_j'] = float(energies[row['certificate_before_index']])
        row['wave_final_j'] = float(energies[row['certificate_after_index']])
        row['wave_radiation_j'] = float(power.sum()/row['rate'])
        row['wave_power_minimum_w'] = float(power.min())
        e0 = row['mechanical_initial_j']+row['relaxation_initial_j']+row['wave_initial_j']
        e1 = row['mechanical_final_j']+row['relaxation_final_j']+row['wave_final_j']
        radiation = row['wave_radiation_j']+row['relaxation_loss_j']
        row['total_budget_relative'] = abs(e1+row['felt_heat_j']+row['material_heat_j']+radiation-e0)/e0
        row['air_budget_relative'] = abs(row['wave_final_j']+row['relaxation_final_j']
                                        -row['wave_initial_j']-row['relaxation_initial_j']
                                        +radiation-row['air_port_work_j'])/e0
    if observer is not None:
        for row,rad in zip(r['cases'],observer['radiation_intervals']):
            before = observer['storage_intervals'][row['certificate_before_index']]
            after = observer['storage_intervals'][row['certificate_after_index']]
            row['wave_initial_interval'] = before
            row['wave_final_interval'] = after
            row['wave_radiation_interval'] = rad
            total_lo = row['mechanical_final_j']+row['relaxation_final_j']+after['lower_j']
            total_hi = row['mechanical_final_j']+row['relaxation_final_j']+after['upper_j']
            initial_lo = row['mechanical_initial_j']+row['relaxation_initial_j']+before['lower_j']
            initial_hi = row['mechanical_initial_j']+row['relaxation_initial_j']+before['upper_j']
            heat = row['felt_heat_j']+row['material_heat_j']+row['relaxation_loss_j']
            row['total_budget_bound_relative'] = max(abs(total_lo+heat+rad['lower_j']-initial_hi),
                abs(total_hi+heat+rad['upper_j']-initial_lo))/initial_lo
            constant = row['relaxation_final_j']-row['relaxation_initial_j']+row['relaxation_loss_j']-row['air_port_work_j']
            row['air_budget_bound_relative'] = max(abs(after['lower_j']-before['upper_j']+rad['lower_j']+constant),
                abs(after['upper_j']-before['lower_j']+rad['upper_j']+constant))/initial_lo
            row['observation_error_relative'] = max(item['error_bound_j']/max(
                item['estimate_j'],r['criteria']['observation_floor_j']) for item in [before,after,rad])
    # Preserve the expensive independent observations before gate assembly.
    r['stage'] = 'independent certificate measured'
    checkpoint()
    r['comparisons'] = []
    for phase_index, phase in enumerate(phases):
        first = phase_index*len(a.rates)
        finest = first+len(a.rates)-1
        errors = []
        force_errors = []
        for index in range(first,finest):
            row, best = r['cases'][index],r['cases'][finest]
            mechanical_error = reference.relative(traces[index],traces[finest])
            bq = core.physical(final_states[index][0][:-1])[core.sb]
            bv = core.physical(final_states[index][1][:-1])[core.sb]
            eq = core.physical(final_states[finest][0][:-1])[core.sb]
            ev = core.physical(final_states[finest][1][:-1])[core.sb]
            end_error = max(reference.relative(bq,eq),reference.relative(bv,ev))
            impulse_error = abs(row['impulse_n_s']-best['impulse_n_s'])/max(abs(best['impulse_n_s']),1e-30)
            force_error = abs(row['maximum_force_n']-best['maximum_force_n'])/max(abs(best['maximum_force_n']),1e-30)
            common_peak_error = abs(row['maximum_common_force_n']-best['maximum_common_force_n'])/max(
                abs(best['maximum_common_force_n']),1e-30)
            common_force_error = reference.relative(np.array(row['common_force_n']),np.array(best['common_force_n']))
            errors.append(mechanical_error)
            force_errors.append(common_force_error)
            r['comparisons'].append(dict(phase=phase,rate=row['rate'],reference_rate=best['rate'],
                                        mechanical_trace_relative=mechanical_error,board_endpoint_relative=end_error,
                                        impulse_relative=impulse_error,peak_force_relative=force_error))
            r['comparisons'][-1].update(common_peak_force_relative=common_peak_error,
                                       common_force_trace_relative=common_force_error)
        # A finite finest reference gives approximately 0.2 for second-order error.
        r.setdefault('refinement_ratios',{})[phase] = float(errors[-1]/max(errors[-2],1e-30)) if len(errors)>=2 else None
        r.setdefault('force_refinement_ratios',{})[phase] = float(force_errors[-1]/max(force_errors[-2],1e-30)) if len(force_errors)>=2 else None
    fine = [row for row in r['comparisons'] if row['rate'] == a.rates[-2]]
    r['prefix_gates'] = dict(
        independent_energy=max(max(row['total_budget_relative'],row['air_budget_relative']) for row in r['cases']) < 1e-9,
        nonnegative_loss=all(row['minimum_step_heat_j']>=-1e-16 and row['wave_power_minimum_w']>=0 for row in r['cases']),
        interface=max(row['interface']['force_residual'] for row in r['cases'])<1e-9
                  and max(abs(row['interface']['power_residual_w']) for row in r['cases'])<1e-9,
        finest_mechanics=max(row['mechanical_trace_relative'] for row in fine)<.01,
        finest_board=max(row['board_endpoint_relative'] for row in fine)<.01,
        finest_force=max(max(row['impulse_relative'],row['common_peak_force_relative'],
                             row['common_force_trace_relative']) for row in fine)<.002,
        second_order=len(a.rates)>=3 and all(value is not None and value<.35 for value in r['refinement_ratios'].values()))
    r['prefix_gates']['second_order_force'] = len(a.rates)>=3 and all(
        value is not None and value<.35 for value in r['force_refinement_ratios'].values())
    if a.compact_certificate:
        r['prefix_gates']['replayed_memories'] = max(r['certificate_replay_relative'])<1e-11
    if observer is not None:
        r['prefix_gates']['bounded_energy'] = max(max(row['total_budget_bound_relative'],
            row['air_budget_bound_relative']) for row in r['cases'])<r['criteria']['energy_relative']
        r['prefix_gates']['observation_precision'] = max(row['observation_error_relative'] for row in r['cases'])<r['criteria']['observation_relative']
        r['observation_comparison'] = []
        if a.compare_prefix_report is not None:
            old = json.loads(a.compare_prefix_report.read_text())
            if (old['stage']!='contact prefix audit completed' or old['cache_R_sha256']!=r['cache_R_sha256']
                    or old['rates']!=r['rates'] or old['duration_s']!=r['duration_s']):
                raise ValueError('matching independent full-observation report required')
            r['parent_sha256'][str(a.compare_prefix_report)] = digest(a.compare_prefix_report)
            for row in r['cases']:
                exact = next(case for case in old['cases'] if case['phase']==row['phase'] and case['rate']==row['rate'])
                for field,interval in [('wave_initial_j','wave_initial_interval'),('wave_final_j','wave_final_interval'),
                                       ('wave_radiation_j','wave_radiation_interval')]:
                    item = row[interval]
                    r['observation_comparison'].append(dict(phase=row['phase'],rate=row['rate'],field=field,
                        exact_j=exact[field],absolute_error_j=abs(row[field]-exact[field]),
                        contained=bool(item['lower_j']<=exact[field]<=item['upper_j'])))
            r['prefix_gates']['independent_observer_comparison'] = all(item['contained'] for item in r['observation_comparison'])
    r['duration_checks'] = dict(cold_hammer_separated=all(row['separation_bracket_s'] is not None for row in r['cases'] if row['phase']=='Cold'),
        nonzero_wave_radiation=any(row.get('wave_radiation_interval',{}).get('lower_j',row['wave_radiation_j'])>1e-14 for row in r['cases']))
    r['prefix_gates'] = {name:bool(value) for name,value in r['prefix_gates'].items()}
    r['contact_prefix_gates_passed'] = all(r['prefix_gates'].values())
    r['stage'] = 'contact prefix audit completed'
    r['next'] = 'Full hammer separation/repetition, release, broader strings and time pressure observation remain. '
    r['next'] += 'Prefix ledger accuracy is not whole-note, timbre or runtime approval.'
    checkpoint()
    print('prefix audit finished',json.dumps(r['prefix_gates']),r['seconds'],flush=True)


if __name__ == '__main__':
    main()
