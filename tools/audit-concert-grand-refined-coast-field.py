"""Independent full angular coast ledger after directed wire convergence.

Reconstruct the authenticated 96/192-mode contact at 3072 kHz. Continue its
living endpoint at 64/128/256 kHz, with native common-time convergence and
finite pressure. Spectral observer probes do not replace the certificate.
"""
import argparse
import importlib.util
import json
import time
from pathlib import Path
import numpy as np
from concert_grand_wave_cache import digest
from concert_grand_string_resolution import capture_contact,join_capture,pressure_observation,compare_observation
from concert_grand_coast_measurements import run_coast,field_balance
from concert_grand_bounded_observer import observation_basis,certificate_observer
from concert_grand_cell_observer import CellObserver
from concert_grand_cell_audio import retarded_kernel,kernel_transfer
from concert_grand_window_field_probe import ProbeProvider,observation_wave_view

spec=importlib.util.spec_from_file_location('refined_coast_reference',Path(__file__).with_name('audit-concert-grand-string-resolution-directed.py'))
anchor=importlib.util.module_from_spec(spec);spec.loader.exec_module(anchor)
field=anchor.field
RATES=(64000,128000,256000)


def native_comparison(traces,rates):
    if (len(traces)!=3 or len(rates)!=3 or sorted(set(rates))!=list(rates)
            or any(rate%rates[0] for rate in rates)):
        raise ValueError('three nested native traces required')
    sampled=[(t[::rate//rates[0]],B[::rate//rates[0]],V[::rate//rates[0]])
             for rate,(t,B,V) in zip(rates,traces)]
    if any(not np.array_equal(t,sampled[-1][0]) for t,_,_ in sampled):
        raise ValueError('identical native physical times required')
    errors=[max(field.relative(B,sampled[-1][1]),field.relative(V,sampled[-1][2])) for _,B,V in sampled[:-1]]
    return dict(capture_rate=rates[0],samples=len(sampled[0][0]),differences_relative=errors,
                ratio=errors[-1]/max(errors[-2],1e-30),interpolation_used=False)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['full-report','resolution-report','coast-reference-report','board','cache','report']:
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--direction-block',type=int,default=32)
    p.add_argument('--max-array-gib',type=float,default=12.)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    if not np.isfinite(a.max_array_gib) or a.max_array_gib<=0 or a.direction_block<1:raise ValueError('positive budget/block required')
    if not a.cache.exists() or not a.cache.resolve().is_relative_to(Path('local/concert-grand-physics').resolve()):
        raise ValueError('existing single local cache required')
    old=json.loads(a.resolution_report.read_text());prior_coast=json.loads(a.coast_reference_report.read_text())
    if (old.get('stage')!='full board directed string resolution preflight completed'
            or not old.get('directed_gates_passed') or not all(old['directed_gates'].values())
            or digest(a.full_report) not in old['parent_sha256'].values() or digest(a.board)!=old['board_sha256']
            or prior_coast.get('stage')!='full coast field audit completed'
            or prior_coast.get('duration_s')!=.0005 or prior_coast['board_sha256']!=old['board_sha256']
            or digest(a.full_report) not in prior_coast['parent_sha256'].values()):
        raise ValueError('matched completed directed/reference coast reports required')
    for name,sha in old['source_sha256'].items():
        if digest(Path(__file__).with_name(name))!=sha:raise ValueError('changed frozen source: '+name)
    for name,sha in old['parent_sha256'].items():
        if digest(name)!=sha:raise ValueError('changed frozen parent: '+name)
    sources=set(old['source_sha256'])|{Path(__file__).name,'concert_grand_window_field_probe.py','concert_grand_pade_window_spectrum.py'}
    criteria=prior_coast['criteria'].copy();criteria['spectral_probe_relative']=1e-10
    start=time.perf_counter()
    r=dict(stage='construction',all_gates_passed=False,preflight_only=True,criteria=criteria,
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in sorted(sources)},
        parent_sha256={str(path):digest(path) for path in [a.full_report,a.resolution_report,a.coast_reference_report]},
        board_sha256=digest(a.board),geometry=old['geometry'],microphones=old['microphones'],
        rates=list(RATES),duration_s=.0005,start_after_contact_s=.003,contact_rate=3072000,
        scope='Independent complete angular ledger of 0.5 ms coast after the checked 96/192-mode contact. '
              'Pressure contribution only, no full-contact angular ledger, wider-band load, whole-note or timbre approval.',cases=[])
    def checkpoint():
        r['seconds']=time.perf_counter()-start;r['peak_working_set_bytes']=field.full.reference.peak_memory()
        a.report.parent.mkdir(parents=True,exist_ok=True)
        a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')
    checkpoint()
    approved,modal,Bsource,notes,surface,provider,identity,_=field.full.prepare_reference(a)
    history=sum(round(rate*.0005) for rate in RATES)*provider.taps*provider.modes*8
    probe_scratch=(128+2*16)*provider.taps*provider.modes*16
    estimate=(provider.budget(real_gram=True)['estimated_array_peak_bytes']+history+probe_scratch
              +8*provider.taps*2*provider.block*provider.modes*8+512*2**20)
    r['array_estimates']=dict(maximum_bytes=int(a.max_array_gib*2**30),estimated_bytes=int(estimate),
        full_rank_input_history_bytes=int(history),maximum_probe_scratch_bytes=int(probe_scratch),
        explanation='Conservative array estimate; no additional R/cache, histories in RAM, not an OS cap.')
    checkpoint()
    if estimate>a.max_array_gib*2**30:raise MemoryError('declared complete array budget exceeded')
    _,air_wave,air=field.full.construct_air(a,approved,provider,surface,identity,int(a.max_array_gib*2**30),r,checkpoint)
    if r['cache_R_sha256']!=old['cache_R_sha256']:raise ValueError('changed acoustic load')
    core=field.MovingBridge(field.Wire(.62,.0081,850),modal.support(Bsource[:,notes.index(60)]),transverse=96,axial=192)
    system=field.BridgeHammer(core,field.Felt(.002,7.5e8,2.6,.125,.02,.1))
    r['stage']='reconstructing measured 96-mode contact';checkpoint()
    seed,contact,t,B,V=capture_contact(field.FastBoundedContact(system,air),5.,3072000,.003,
        lambda i,n:print('contact',i,n,flush=True))
    if contact['compression_final_m']>=0:raise AssertionError('reference contact must end separated')
    r['contact']=contact;contact_trace=(t,B,V)
    parent=next(c for c in old['cases'] if c['case']==anchor.case_key(96,3072000))
    r['contact_reproduction']={name:anchor.reference.difference(contact[name],parent['contact'][name])
        for name in parent['contact'] if name.endswith('_j') or name in ['final_wave_norm','compression_final_m',
            'peak_centered_contact_force_n','contact_impulse_n_s','hammer_momentum_change_n_s']}
    if max(r['contact_reproduction'].values())>=criteria['seed_relative']:raise AssertionError('changed measured contact')
    states=[seed[2].copy()];segments=[];traces=[]
    pilot=field.ExponentialCoast(system,air)
    observer=CellObserver(provider.surface,r['microphones'],4);kernel=retarded_kernel(observer,256000)
    frequency=np.r_[0.,np.arange(60.,6001.,60.)];H=kernel_transfer(kernel,256000,frequency)
    r['kernel_control_relative']=max(field.relative(x,y) for x,y in zip(H,observer.acceleration_transfer(frequency)))
    r['stage']='living refined coast capture';checkpoint()
    for rate in RATES:
        final,row,ports,tc,Bc,Vc=run_coast(pilot,seed,rate,.0005,
            lambda i,n:print('coast',rate,i,n,flush=True))
        states.append(final[2].copy());segments.append(dict(initial_index=0,final_index=len(states)-1,dt=1/rate,velocity=ports))
        r['cases'].append(row);traces.append((tc,Bc,Vc));checkpoint()
    _,whole_pressure=pressure_observation(join_capture(contact_trace,traces[-1]),frequency,H,kernel,256000)
    baseline=anchor.baseline_differences(dict(contact=contact,coast=r['cases'][-1],pressure=whole_pressure),parent)
    r['baseline_reproduction']=baseline
    if max(v for group in baseline.values() for v in group.values())>=criteria['seed_relative']:
        raise AssertionError('changed measured refined contact/coast pressure')
    r['native_common_times']=native_comparison(traces,RATES);observations=[]
    for row,trace in zip(r['cases'],traces):
        observation,pressure=pressure_observation(trace,frequency,H,kernel,256000)
        observations.append(observation);row['pressure']=pressure
    r['pressure_comparisons']=[dict(rate=rate,reference_rate=RATES[-1],
        **compare_observation(o,observations[-1],frequency)) for rate,o in zip(RATES[:-1],observations[:-1])]
    errors=[max(c['physical_board_position_relative'],c['physical_board_velocity_relative']) for c in r['pressure_comparisons']]
    r['interpolated_board_ratio_diagnostic']=errors[-1]/max(errors[-2],1e-30)
    r['pressure_scope']='Finite acceleration contribution at 3–3.5 ms. Same continuous-cell kernel and '
    r['pressure_scope']+='256 kHz common pressure grid, DC–6 kHz, no endpoint impulses or note-off.'
    del contact_trace,t,B,V,final,observations
    r['stage']='independent complete angular certificate';checkpoint()
    memories=np.array(states);Q,basis=observation_basis(air_wave,memories,segments)
    r['observation_basis']=basis;print('observer rank',basis['rank'],flush=True)
    count=(len(provider.directions)+provider.block-1)//provider.block
    selected=(0,count//2)
    probe=ProbeProvider(provider,air_wave,memories,segments,Q,selected)
    view=observation_wave_view(air_wave,probe)
    began=time.perf_counter()
    energies,powers,bounds=certificate_observer(view,memories,segments,Q,input_block=256,
        store_projected_inputs=True,progress=lambda i:print('field block',i,flush=True) if i%16==0 else None)
    r['certificate_seconds_including_probes']=time.perf_counter()-began
    r['bounded_observer']=bounds;r['spectral_field_probes']=probe.results
    for i,row in enumerate(r['cases']):
        row.update(field_balance(row,bounds['storage_intervals'][0],bounds['storage_intervals'][i+1],bounds['radiation_intervals'][i]))
        row['wave_power_minimum_w']=float(powers[i].min())
        row['observation_error_relative']=max(interval['error_bound_j']/max(interval['estimate_j'],criteria['observation_floor_j'])
            for interval in [row['wave_initial_interval'],row['wave_final_interval'],row['wave_radiation_interval']])
    r['work_refinement']=[]
    for row in r['cases'][:-1]:
        changes={key:field.work.energy_change(row,r['cases'][-1],key,
            dict(near_zero_j=criteria['work_floor_j'],energy_work_relative=criteria['work_relative']))
            for key in ['wave_final_j','wave_radiation_j','relaxation_final_j','relaxation_loss_j','material_heat_j','air_port_work_j']}
        r['work_refinement'].append(dict(rate=row['rate'],changes=changes,passed=all(c['passed'] for c in changes.values())))
    last=r['pressure_comparisons'][-1]
    trials=[trial for block in probe.results for segment in block['segments'] for trial in segment['trials']]
    r['coast_gates']=dict(
        reproduced_refined_baseline=True,
        complete_memory_replay=max(bounds['full_endpoint_replay_relative'])<1e-11,
        bounded_energy=max(max(c['total_budget_bound_relative'],c['air_budget_bound_relative']) for c in r['cases'])<criteria['energy_relative'],
        observation_precision=max(c['observation_error_relative'] for c in r['cases'])<criteria['observation_relative'],
        nonnegative_loss=all(c['minimum_step_heat_j']>=-1e-16 and c['wave_power_minimum_w']>=0 for c in r['cases']),
        nonzero_wave_flux=all(c['wave_radiation_interval']['lower_j']>criteria['work_floor_j'] for c in r['cases']),
        refined_work=r['work_refinement'][-1]['passed'],
        native_refined_board=r['native_common_times']['differences_relative'][-1]<criteria['board_relative'],
        native_second_order=r['native_common_times']['ratio']<criteria['refinement_ratio'],
        refined_pressure=max([*last['complex_pressure_relative'].values(),last['temporal_pressure_relative']])<criteria['pressure_relative'],
        refined_capture=r['cases'][-1]['pressure']['capture_decimation_relative']<criteria['capture_relative'],
        kernel=r['kernel_control_relative']<criteria['kernel_relative'],
        probe_complete=len(probe.results)==len(selected) and len(trials)==12,
        probe_field=max(t['complex_field_relative'] for t in trials)<criteria['spectral_probe_relative'],
        probe_flux=max(t['radiation_relative'] for t in trials)<criteria['spectral_probe_relative'],
        source_unchanged=all(digest(Path(__file__).with_name(name))==sha for name,sha in r['source_sha256'].items()))
    r['coast_gates']={name:bool(v) for name,v in r['coast_gates'].items()}
    r['coast_gates_passed']=all(r['coast_gates'].values())
    r['stage']='refined full coast field audit completed';checkpoint()
    print('finished',json.dumps(r['coast_gates']),r['seconds'],flush=True)


if __name__=='__main__':main()
