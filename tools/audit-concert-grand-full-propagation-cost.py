"""Full-coordinate contact/coast cost and unchanged-kernel equivalence.

Reconstruct the already measured 3 ms C4 contact, then start separated coast
cases from the same living endpoint. This is a cost/refinement preflight:
no new angular energy certificate, pressure waveform or timbral approval.
The single authenticated R stays read-only; all trajectories stay in RAM.
"""
import argparse
import importlib.util
import json
import os
import time
from pathlib import Path

import numpy as np

from concert_grand_exponential_coast import ExponentialCoast
from concert_grand_fast_contact import FastBoundedContact
from concert_grand_moving_bridge import MovingBridge, BridgeHammer
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
from concert_grand_wave_cache import digest

spec = importlib.util.spec_from_file_location(
    'full_contact_reference', Path(__file__).with_name('audit-concert-grand-pade-contact-full.py'))
full = importlib.util.module_from_spec(spec)
spec.loader.exec_module(full)


def relative(a, b):
    return float(np.linalg.norm(np.asarray(a)-b)/max(np.linalg.norm(b), 1e-30))


def run_pair(system, air, initial, rate, duration, coast=False, capture=64000):
    """Alternate equally prepared kernels; compare every live endpoint."""
    pilots = [(ExponentialCoast if coast else FastBoundedContact)(system, air)
              for _ in range(2)]
    for pilot, kernel in zip(pilots, ['reference', 'batched']):
        if coast:
            pilot.oscillator_functions(1/(2*rate))
            cache = pilot._air_prepare(1/rate)
        else:
            cache = pilot._prepare(1/rate)
        cache['wave_plan'].residue_kernel = kernel
    states = [tuple(x.copy() for x in initial) for _ in pilots]
    histories = [[np.r_[state[0], state[1]]] for state in states]
    times = [[], []]
    sums = [dict(felt_loss_j=0., material_loss_j=0., radiation_loss_j=0., air_port_work_j=0.)
            for _ in pilots]
    max_state = np.zeros(4)
    max_ports = 0.
    separation = [None, None]
    contact_seen = [system.compression(initial[0])>0]*2
    iteration_max = [0, 0]
    fallback = [0, 0]
    steps = round(rate*duration)
    for i in range(steps):
        stats = [None, None]
        for j in ([0, 1] if i%2 == 0 else [1, 0]):
            begin = time.perf_counter()
            states[j], stats[j] = pilots[j].step(states[j], 1/rate, True)
            times[j].append(time.perf_counter()-begin)
            for name in sums[j]:
                sums[j][name] += stats[j][name]
            iteration_max[j] = max(iteration_max[j], stats[j].get(
                'mechanical_iterations' if coast else 'newton_iterations', 0))
            fallback[j] += stats[j]['full_tangent_fallbacks']
            if system.compression(states[j][0])>0:
                contact_seen[j] = True
            elif contact_seen[j] and separation[j] is None:
                separation[j] = [i/rate, (i+1)/rate]
            if (i+1)%(rate//capture) == 0:
                histories[j].append(np.r_[states[j][0], states[j][1]])
        max_state = np.maximum(max_state, [relative(b, a) for a, b in zip(states[0], states[1])])
        max_ports = max(max_ports, relative(stats[1]['wave_inputs'], stats[0]['wave_inputs']))
        if (i+1)%max(1,steps//4) == 0:
            print('coast' if coast else 'contact', rate, i+1, steps, flush=True)
    rows = []
    for j, kernel in enumerate(['reference', 'batched']):
        state = states[j]
        rows.append(dict(kernel=kernel, dynamics_s=sum(times[j]), step_median_s=float(np.median(times[j])),
            mechanical_final_j=system.energy(state[0], state[1]),
            relaxation_final_j=air.relaxation.energy(state[3]), final_wave_norm=float(np.linalg.norm(state[2])),
            compression_final_m=system.compression(state[0]), separation_bracket_s=separation[j],
            iterations_max=iteration_max[j], full_tangent_fallbacks=fallback[j], **sums[j]))
    comparison = dict(maximum_live_state_relative=max_state.tolist(), maximum_section_input_relative=max_ports,
        cumulative_work_difference_j={name:abs(sums[1][name]-sums[0][name]) for name in sums[0]},
        dynamics_ratio_batched_over_reference=rows[1]['dynamics_s']/rows[0]['dynamics_s'])
    return states, rows, comparison, np.asarray(histories)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['full-report', 'board', 'cache', 'contact-report', 'report']:
        p.add_argument('--'+name, type=Path, required=True)
    p.add_argument('--coast-rates', type=int, nargs='+', default=[64000, 128000, 256000])
    p.add_argument('--coast-duration', type=float, default=.0005)
    p.add_argument('--direction-block', type=int, default=32)
    p.add_argument('--max-array-gib', type=float, default=8.)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
    if (len(a.coast_rates)<3 or sorted(set(a.coast_rates))!=a.coast_rates
            or min(a.coast_rates)<=0 or any(rate%min(a.coast_rates) for rate in a.coast_rates)
            or not np.isfinite(a.coast_duration) or a.coast_duration<=0
            or any(abs(round(rate*a.coast_duration)-rate*a.coast_duration)>1e-9 for rate in a.coast_rates)
            or not np.isfinite(a.max_array_gib) or a.max_array_gib<=0):
        raise ValueError('nested positive coast rates, exact duration and positive array budget required')
    cache_root = Path('local/concert-grand-physics').resolve()
    if not a.cache.resolve().is_relative_to(cache_root) or not a.cache.exists():
        raise ValueError('existing single local cache required')
    old = json.loads(a.contact_report.read_text())
    if (old.get('stage')!='contact prefix audit completed' or not old.get('contact_prefix_gates_passed')
            or old.get('phases')!=['Cold', 'Continued'] or old.get('duration_s')!=.0015
            or old.get('rates')!=[192000,384000,768000]
            or digest(a.full_report) not in old.get('parent_sha256',{}).values()
            or digest(a.board)!=old.get('board_sha256')):
        raise ValueError('matched measured 3 ms full-board contact required')
    rate = old['rates'][-1]
    if rate%min(a.coast_rates):
        raise ValueError('common nested contact/coast capture rate required')
    start = time.perf_counter()
    sources = {name:digest(Path(__file__).with_name(name)) for name in [
        Path(__file__).name, 'audit-concert-grand-pade-contact-full.py', 'concert_grand_pade_midpoint.py',
        'concert_grand_fast_contact.py', 'concert_grand_exponential_coast.py',
        'concert_grand_wave_cache.py', 'concert_grand_bounded_contact.py',
        'concert_grand_moving_bridge.py', 'concert_grand_coupled_contact.py', 'concert_grand_nonlinear_string.py']}
    r = dict(stage='construction', all_gates_passed=False, preflight_only=True,
        criteria=dict(kernel_state_relative=1e-10, kernel_work_relative=1e-10,
                      frozen_endpoint_relative=1e-10, board_refinement_relative=.01,
                      refinement_ratio=.35), source_sha256=sources,
        parent_sha256={str(path):digest(path) for path in [a.full_report, a.contact_report]},
        board_sha256=digest(a.board), cache=str(a.cache),
        blas_environment={key:os.environ.get(key) for key in ['OPENBLAS_NUM_THREADS','OMP_NUM_THREADS']},
        scope='606 board modes and all living acoustic states. Kernel equivalence and short separated '
              'coast refinement/cost only; no independent angular coast ledger, pressure, timbre or runtime approval.')

    def checkpoint():
        r['seconds'] = time.perf_counter()-start
        r['peak_working_set_bytes'] = full.reference.peak_memory()
        a.report.parent.mkdir(parents=True,exist_ok=True)
        a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')

    checkpoint()
    approved,modal,source_B,notes,surface,provider,identity,shapes = full.prepare_reference(a)
    r['physical_source_sha256'] = approved['source_sha256']
    r['modes'] = provider.modes
    r['states'] = approved['states']
    budget = provider.budget(real_gram=True)
    # Two complete live trajectories; just common mechanical captures in RAM.
    estimate = budget['estimated_array_peak_bytes']+64*provider.taps*provider.modes*16+128*2**20
    if estimate>a.max_array_gib*2**30:
        raise MemoryError('declared array budget exceeded')
    r['array_estimate_bytes'] = estimate
    wave,air_wave,air = full.construct_air(a,approved,provider,surface,identity,int(a.max_array_gib*2**30),r,checkpoint)
    if r['cache_R_sha256']!=old['cache_R_sha256']:
        raise ValueError('contact belongs to different acoustic coefficients')
    core = MovingBridge(Wire(.62,.0081,850),modal.support(source_B[:,notes.index(60)]),transverse=3)
    system = BridgeHammer(core,Felt(.002,7.5e8,2.6,.125,.02,.1))
    initial = FastBoundedContact(system,air).initial_state(5.)
    energy0 = system.energy(initial[0],initial[1])
    r['stage'] = 'paired full contact'
    checkpoint()
    states,rows,comparison,_ = run_pair(system,air,initial,rate,2*old['duration_s'],capture=min(a.coast_rates))
    r['contact'] = dict(rate=rate,duration_s=2*old['duration_s'],initial_mechanical_j=energy0,
                        cases=rows,comparison=comparison)
    finest = next(row for row in old['cases'] if row['phase']=='Continued' and row['rate']==rate)
    cold = next(row for row in old['cases'] if row['phase']=='Cold' and row['rate']==rate)
    r['frozen_contact_comparison'] = {
        name:abs(rows[0][name]-finest[name])/max(abs(finest[name]),1e-30)
        for name in ['mechanical_final_j','relaxation_final_j','final_wave_norm','compression_final_m']}
    r['frozen_contact_comparison'].update({
        name:abs(rows[0][name]-cold[name]-finest[name])/energy0
        for name in ['material_heat_j','felt_heat_j','relaxation_loss_j','air_port_work_j']
        if name in rows[0]})
    # Work names differ between the paired integrator stats and the old report.
    for new, prior in [('felt_loss_j','felt_heat_j'), ('material_loss_j','material_heat_j'),
                       ('radiation_loss_j','relaxation_loss_j')]:
        r['frozen_contact_comparison'][prior] = abs(rows[0][new]-cold[prior]-finest[prior])/energy0
    if max(r['frozen_contact_comparison'].values())>=r['criteria']['frozen_endpoint_relative']:
        r['stage'] = 'rejected frozen contact'
        checkpoint()
        raise AssertionError('shared fixture/contact no longer reproduces frozen reference')
    seed = states[0]
    if system.compression(seed[0])>=0:
        raise AssertionError('contact endpoint is not separated')
    r['stage'] = 'paired separated coast'
    r['coast'] = []
    trajectories = []
    board_map = FastBoundedContact(system,air).board_map
    checkpoint()
    for coast_rate in a.coast_rates:
        finals,rows,comparison,history = run_pair(system,air,seed,coast_rate,a.coast_duration,
                                                 coast=True,capture=min(a.coast_rates))
        r['coast'].append(dict(rate=coast_rate,duration_s=a.coast_duration,cases=rows,comparison=comparison,
                               final_interface=FastBoundedContact(system,air).interface(finals[0])))
        # Physical B and Bdot, without modal energy weighting or a hidden mode cutoff.
        trajectories.append(np.stack((history[0,:,:system.size]@board_map.T,
                                       history[0,:,system.size:]@board_map.T)))
        checkpoint()
    best = trajectories[-1]
    errors = [max(relative(trace[0],best[0]),relative(trace[1],best[1])) for trace in trajectories[:-1]]
    r['coast_refinement'] = dict(capture_rate=min(a.coast_rates), rates=a.coast_rates,
        board_position_velocity_relative_to_finest=errors,
        ratio=errors[-1]/max(errors[-2],1e-30))
    pairs = [r['contact']]+r['coast']
    r['preflight_gates'] = dict(
        frozen_contact=max(r['frozen_contact_comparison'].values())<r['criteria']['frozen_endpoint_relative'],
        live_kernel_equivalence=max(max(pair['comparison']['maximum_live_state_relative']) for pair in pairs)<r['criteria']['kernel_state_relative'],
        section_input_equivalence=max(pair['comparison']['maximum_section_input_relative'] for pair in pairs)<r['criteria']['kernel_state_relative'],
        work_equivalence=max(max(pair['comparison']['cumulative_work_difference_j'].values())/energy0 for pair in pairs)<r['criteria']['kernel_work_relative'],
        finest_coast_board=errors[-1]<r['criteria']['board_refinement_relative'],
        second_order_coast=r['coast_refinement']['ratio']<r['criteria']['refinement_ratio'],
        zero_coast_felt=all(row['felt_loss_j']==0 for pair in r['coast'] for row in pair['cases']),
        passive_material_relaxation=all(min(row['material_loss_j'],row['radiation_loss_j'])>=0 for pair in pairs for row in pair['cases']),
        source_unchanged=all(digest(Path(__file__).with_name(name))==sha for name,sha in sources.items()))
    r['preflight_gates'] = {name:bool(value) for name,value in r['preflight_gates'].items()}
    r['preflight_gates_passed'] = all(r['preflight_gates'].values())
    r['stage'] = 'full propagation cost preflight completed'
    r['next'] = 'Independent coast angular storage/flux and two-microphone temporal pressure before rate selection; '
    r['next'] += 'short board refinement is not audible-note or whole-piano approval.'
    checkpoint()
    print('finished',json.dumps(r['preflight_gates']),r['seconds'],flush=True)


if __name__ == '__main__':
    main()
