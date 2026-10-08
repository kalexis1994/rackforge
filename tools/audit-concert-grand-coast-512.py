"""Refine the failed 96-mode coast radiation gate at 128/256/512 kHz.

Reproduce old pressure/physics/field intervals and retain one living seed.
No new dynamics, force tuning, spectral default, plugin or audio output.
"""
import argparse
import importlib.util
import json
import time
from pathlib import Path
import numpy as np
from concert_grand_wave_cache import digest
from concert_grand_contact_seed import save_seed,overlapping_intervals
from concert_grand_seed_resume import authenticated_seed
from concert_grand_string_resolution import capture_contact,join_capture,pressure_observation,compare_observation
from concert_grand_coast_measurements import run_coast,field_balance
from concert_grand_bounded_observer import observation_basis,certificate_observer
from concert_grand_cell_observer import CellObserver
from concert_grand_cell_audio import retarded_kernel,kernel_transfer

spec=importlib.util.spec_from_file_location('refined_coast_parent',Path(__file__).with_name('audit-concert-grand-refined-coast-field.py'))
prior=importlib.util.module_from_spec(spec);spec.loader.exec_module(prior)
anchor=prior.anchor;field=prior.field;native_comparison=prior.native_comparison
RATES=(128000,256000,512000)


def prior_field_overlap(rows,parent):
    return {str(row['rate']):{key:bool(overlapping_intervals(row[key],
        next(c for c in parent['cases'] if c['rate']==row['rate'])[key]))
        for key in ['wave_initial_interval','wave_final_interval','wave_radiation_interval']}
        for row in rows}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['full-report','resolution-report','coast-reference-report','board','cache','seed','report']:
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--direction-block',type=int,default=32)
    p.add_argument('--max-array-gib',type=float,default=12.)
    p.add_argument('--seed-reference-report',type=Path)
    p.add_argument('--seed-producer-commit')
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    if not np.isfinite(a.max_array_gib) or a.max_array_gib<=0 or a.direction_block<1:
        raise ValueError('positive budget/block required')
    root=Path('local/concert-grand-physics').resolve()
    if not a.cache.exists() or not a.cache.resolve().is_relative_to(root):
        raise ValueError('existing single local cache required')
    if bool(a.seed_reference_report)!=bool(a.seed_producer_commit):raise ValueError('both seed provenance arguments required')
    if not a.seed.resolve().is_relative_to(root) or a.seed.suffix!='.npz':raise ValueError('unique local seed required')
    if not a.seed_reference_report and (a.seed.exists() or a.seed.with_suffix('.json').exists()):
        raise ValueError('existing seed requires authenticated resume')
    old=json.loads(a.resolution_report.read_text());parent=json.loads(a.coast_reference_report.read_text())
    if (old.get('stage')!='full board directed string resolution preflight completed'
            or not old.get('directed_gates_passed') or not all(old['directed_gates'].values())
            or parent.get('stage')!='refined full coast field audit completed'
            or parent.get('coast_gates_passed') is not False
            or {k for k,v in parent['coast_gates'].items() if not v}!={'refined_work'}
            or parent['rates']!=[64000,128000,256000] or parent['duration_s']!=.0005
            or parent['board_sha256']!=old['board_sha256'] or digest(a.board)!=parent['board_sha256']
            or digest(a.full_report) not in parent['parent_sha256'].values()
            or digest(a.resolution_report) not in parent['parent_sha256'].values()):
        raise ValueError('matched completed directed and failed coast reports required')
    for report in (old,parent):
        for name,sha in report['source_sha256'].items():
            if digest(Path(__file__).with_name(name))!=sha:raise ValueError('changed frozen source: '+name)
        for name,sha in report['parent_sha256'].items():
            if digest(name)!=sha:raise ValueError('changed frozen parent: '+name)
    sources=set(parent['source_sha256'])|{Path(__file__).name,'concert_grand_contact_seed.py','concert_grand_seed_resume.py'}
    criteria=parent['criteria'].copy();start=time.perf_counter()
    r=dict(stage='construction',all_gates_passed=False,preflight_only=True,criteria=criteria,
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in sorted(sources)},
        parent_sha256={str(path):digest(path) for path in [a.full_report,a.resolution_report,a.coast_reference_report]},
        board_sha256=digest(a.board),geometry=parent['geometry'],microphones=parent['microphones'],
        rates=list(RATES),duration_s=.0005,start_after_contact_s=.003,contact_rate=3072000,
        scope='Independent complete angular ledger of 0.5 ms coast, 128/256/512 kHz, '
              'same 96/192-mode contact. No full-contact ledger, wider-band or whole-note approval.',cases=[])
    def checkpoint():
        r['seconds']=time.perf_counter()-start;r['peak_working_set_bytes']=field.full.reference.peak_memory()
        a.report.parent.mkdir(parents=True,exist_ok=True)
        a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')
    checkpoint()
    approved,modal,Bsource,notes,surface,provider,identity,_=field.full.prepare_reference(a)
    history=sum(round(rate*.0005) for rate in RATES)*provider.taps*provider.modes*8
    scratch=4*provider.taps*provider.modes*16*provider.block
    estimate=(provider.budget(real_gram=True)['estimated_array_peak_bytes']+2*history+scratch
              +8*provider.taps*2*provider.block*provider.modes*8+512*2**20)
    r['array_estimates']=dict(maximum_bytes=int(a.max_array_gib*2**30),estimated_bytes=int(estimate),
        full_rank_input_history_bytes=int(history),maximum_observation_scratch_bytes=int(scratch),
        explanation='Conservative full-rank arrays; one small seed, no new R, histories in RAM, not an OS cap.')
    checkpoint()
    if estimate>a.max_array_gib*2**30:raise MemoryError('declared array budget exceeded')
    _,wave,air=field.full.construct_air(a,approved,provider,surface,identity,int(a.max_array_gib*2**30),r,checkpoint)
    if r['cache_R_sha256']!=parent['cache_R_sha256']:raise ValueError('changed acoustic load')
    core=field.MovingBridge(field.Wire(.62,.0081,850),modal.support(Bsource[:,notes.index(60)]),transverse=96,axial=192)
    system=field.BridgeHammer(core,field.Felt(.002,7.5e8,2.6,.125,.02,.1))
    if a.seed_reference_report:
        seed,seed_reference,seed_proof=authenticated_seed(a.seed_reference_report,a.seed,a.seed_producer_commit,r)
        contact=seed_reference['contact']
        checks=dict(mechanical=anchor.reference.difference(system.energy(seed[0],seed[1]),contact['mechanical_final_j']),
            relaxation=anchor.reference.difference(air.relaxation.energy(seed[3]),contact['relaxation_final_j']),
            wave_norm=anchor.reference.difference(float(np.linalg.norm(seed[2])),contact['final_wave_norm']),
            compression=anchor.reference.difference(system.compression(seed[0]),contact['compression_final_m']))
        if max(checks.values())>=criteria['seed_relative']:raise ValueError('seed inconsistent with current physical operator')
        r['seed_resume']=dict(**seed_proof,physical_consistency_relative=checks)
        r['parent_sha256'][str(a.seed_reference_report)]=seed_proof['checkpoint_sha256']
        r['baseline_reproduction']=seed_reference['baseline_reproduction']
        r['baseline_reproduction_scope']='Contact and whole-window pressure reproduction inherited from authenticated checkpoint; old coast pressures recomputed in this run.'
        print('authenticated living seed reused',flush=True)
    else:
        r['stage']='reconstructing measured 96-mode contact';checkpoint()
        seed,contact,t,B,V=capture_contact(field.FastBoundedContact(system,air),5.,3072000,.003,
            lambda i,n:print('contact',i,n,flush=True))
        contact_trace=(t,B,V)
    if contact['compression_final_m']>=0:raise AssertionError('separated contact endpoint required')
    r['contact']=contact
    states=[seed[2].copy()];segments=[];traces=[];pilot=field.ExponentialCoast(system,air)
    observer=CellObserver(provider.surface,r['microphones'],4)
    frequency=np.r_[0.,np.arange(60.,6001.,60.)]
    legacy_kernel=retarded_kernel(observer,256000);legacy_H=kernel_transfer(legacy_kernel,256000,frequency)
    kernel=retarded_kernel(observer,512000);H=kernel_transfer(kernel,512000,frequency)
    r['kernel_control_relative']=max(field.relative(x,y) for x,y in zip(H,observer.acceleration_transfer(frequency)))
    r['stage']='living refined coast capture';checkpoint()
    for rate in RATES:
        final,row,ports,tc,Bc,Vc=run_coast(pilot,seed,rate,.0005,
            lambda i,n:print('coast',rate,i,n,flush=True))
        states.append(final[2].copy());segments.append(dict(initial_index=0,final_index=len(states)-1,dt=1/rate,velocity=ports))
        r['cases'].append(row);traces.append((tc,Bc,Vc));checkpoint()
    if not a.seed_reference_report:
        _,whole=pressure_observation(join_capture(contact_trace,traces[1]),frequency,legacy_H,legacy_kernel,256000)
        directed=next(c for c in old['cases'] if c['case']==anchor.case_key(96,3072000))
        r['baseline_reproduction']=anchor.baseline_differences(dict(contact=contact,coast=r['cases'][1],pressure=whole),directed)
        del whole,contact_trace,t,B,V
    r['coast_reproduction']={}
    for i,rate in enumerate(RATES[:-1]):
        reference=next(c for c in parent['cases'] if c['rate']==rate)
        _,pressure=pressure_observation(traces[i],frequency,legacy_H,legacy_kernel,256000)
        physical={k:v for k,v in reference.items() if not k.startswith('wave_')}
        r['coast_reproduction'][str(rate)]=anchor.baseline_differences(
            dict(contact=contact,coast=r['cases'][i],pressure=pressure),
            dict(contact=parent['contact'],coast=physical,pressure=reference['pressure']))
    reproduction=[r['baseline_reproduction'],*r['coast_reproduction'].values()]
    if max(v for row in reproduction for group in row.values() for v in group.values())>=criteria['seed_relative']:
        raise AssertionError('changed measured contact/coast/pressure')
    if a.seed_reference_report:
        seed_identity={key:seed_proof[key] for key in ['payload_sha256','manifest_sha256','payload_bytes','manifest_bytes']}
    else:
        bindings=dict(source_sha256=r['source_sha256'],parent_sha256=r['parent_sha256'],
            board_sha256=r['board_sha256'],cache_R_sha256=r['cache_R_sha256'],geometry=r['geometry'],
            contact_rate=r['contact_rate'],contact_duration_s=.003,incident_speed_m_s=5.)
        loaded,seed_identity=save_seed(a.seed,seed,bindings)
        if not all(np.array_equal(x,y) for x,y in zip(seed,loaded)):raise AssertionError('changed seed round trip')
        del loaded
    r['contact_seed']=dict(path=str(a.seed),**seed_identity,roundtrip_exact=True)
    del legacy_H,legacy_kernel,pressure,final
    r['native_common_times']=native_comparison(traces,RATES);observations=[]
    for row,trace in zip(r['cases'],traces):
        observation,pressure=pressure_observation(trace,frequency,H,kernel,512000)
        observations.append(observation);row['pressure']=pressure
    r['pressure_comparisons']=[dict(rate=rate,reference_rate=RATES[-1],
        **compare_observation(o,observations[-1],frequency)) for rate,o in zip(RATES[:-1],observations[:-1])]
    r['pressure_scope']='Finite 3-3.5 ms acceleration contribution, common pressure grid 512 kHz, DC-6 kHz. '
    r['pressure_scope']+='Old pressure reproduced separately at 256 kHz; no endpoint impulses or note-off.'
    del observations
    r['stage']='independent complete angular certificate';checkpoint()
    memories=np.array(states);Q,basis=observation_basis(wave,memories,segments)
    r['observation_basis']=basis;print('observer rank',basis['rank'],flush=True);began=time.perf_counter()
    _,powers,bounds=certificate_observer(wave,memories,segments,Q,input_block=256,store_projected_inputs=True,
        progress=lambda i:print('field block',i,flush=True) if i%16==0 else None)
    r['certificate_seconds']=time.perf_counter()-began;r['bounded_observer']=bounds
    for i,row in enumerate(r['cases']):
        row.update(field_balance(row,bounds['storage_intervals'][0],bounds['storage_intervals'][i+1],bounds['radiation_intervals'][i]))
        row['wave_power_minimum_w']=float(powers[i].min())
        row['observation_error_relative']=max(interval['error_bound_j']/max(interval['estimate_j'],criteria['observation_floor_j'])
            for interval in [row['wave_initial_interval'],row['wave_final_interval'],row['wave_radiation_interval']])
    r['prior_field_reproduction']=prior_field_overlap(r['cases'][:-1],parent)
    r['work_refinement']=[]
    for row in r['cases'][:-1]:
        changes={key:field.work.energy_change(row,r['cases'][-1],key,
            dict(near_zero_j=criteria['work_floor_j'],energy_work_relative=criteria['work_relative']))
            for key in ['wave_final_j','wave_radiation_j','relaxation_final_j','relaxation_loss_j','material_heat_j','air_port_work_j']}
        r['work_refinement'].append(dict(rate=row['rate'],changes=changes,passed=all(c['passed'] for c in changes.values())))
    last=r['pressure_comparisons'][-1]
    r['coast_gates']=dict(
        reproduced_refined_baseline=True,
        reproduced_prior_field=all(v for row in r['prior_field_reproduction'].values() for v in row.values()),
        authenticated_seed=r['contact_seed']['roundtrip_exact'] and digest(a.seed)==seed_identity['payload_sha256']
            and digest(a.seed.with_suffix('.json'))==seed_identity['manifest_sha256'],
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
        source_unchanged=all(digest(Path(__file__).with_name(name))==sha for name,sha in r['source_sha256'].items()))
    r['coast_gates']={name:bool(v) for name,v in r['coast_gates'].items()}
    r['coast_gates_passed']=all(r['coast_gates'].values())
    r['stage']='refined 512 kHz coast field audit completed';checkpoint()
    print('finished',json.dumps(r['coast_gates']),r['seconds'],flush=True)


if __name__=='__main__':main()
