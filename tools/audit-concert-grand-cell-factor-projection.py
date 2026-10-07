"""Measure observation-only cell projection against the frozen full coast.

Authenticate unchanged physical sources, single R, board and baseline observer
from Git. Reconstruct the same living trajectories; compare complete angular
storage/flux, pressure diagnostics and native-time convergence. New projection
is optional; old factor projection remains the default. No timbre approval.
"""
import argparse
import hashlib
import json
import subprocess
import time
from pathlib import Path
import numpy as np
from scipy.interpolate import CubicHermiteSpline
from scipy.fft import irfft
from concert_grand_wave_cache import digest
from concert_grand_bounded_observer import observation_basis,certificate_observer
from concert_grand_coast_measurements import run_coast,field_balance
from concert_grand_cell_observer import CellObserver,acceleration_spectrum,spectral_relative
from concert_grand_cell_audio import retarded_kernel,kernel_transfer,pressure_fft
import importlib.util

spec=importlib.util.spec_from_file_location('field_reference',Path(__file__).with_name('audit-concert-grand-full-coast-field.py'))
field=importlib.util.module_from_spec(spec);spec.loader.exec_module(field)


def delta(value,prior):return float(abs(value-prior)/max(abs(prior),1e-14))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['field-report','alignment-report','full-report','board','cache','report']:
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--baseline-revision',default='e6c3a93')
    p.add_argument('--direction-block',type=int,default=32)
    p.add_argument('--max-array-gib',type=float,default=12.)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    old=json.loads(a.field_report.read_text());aligned=json.loads(a.alignment_report.read_text())
    if (old.get('stage')!='full coast field audit completed' or old.get('rates')!=[64000,128000,256000]
            or old.get('duration_s')!=.0005 or digest(a.board)!=old['board_sha256']
            or digest(a.full_report) not in old['parent_sha256'].values()
            or not aligned.get('linked_coast_gates_passed')
            or digest(a.field_report) not in aligned['parent_sha256'].values()):
        raise ValueError('matched completed field and alignment controls required')
    if not a.cache.exists() or not a.cache.resolve().is_relative_to(Path('local/concert-grand-physics').resolve()):
        raise ValueError('existing single local cache required')
    if not np.isfinite(a.max_array_gib) or a.max_array_gib<=0 or a.direction_block<1:
        raise ValueError('positive memory/block budget required')
    changed='concert_grand_bounded_observer.py'
    for name,sha in old['source_sha256'].items():
        if name!=changed and digest(Path(__file__).with_name(name))!=sha:
            raise ValueError('changed frozen physical/control source: '+name)
    frozen=subprocess.run(['git','show',a.baseline_revision+':tools/'+changed],check=True,capture_output=True).stdout
    if hashlib.sha256(frozen).hexdigest()!=old['source_sha256'][changed]:
        raise ValueError('baseline Git observer differs from frozen field report')
    source=set(old['source_sha256'])|{Path(__file__).name,'concert_grand_cell_factor_projection.py'}
    start=time.perf_counter();criteria=old['criteria']
    r=dict(stage='construction',all_gates_passed=False,preflight_only=True,
        parent_sha256={str(path):digest(path) for path in [a.field_report,a.alignment_report,a.full_report]},
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in sorted(source)},
        baseline_observer=dict(revision=a.baseline_revision,sha256=old['source_sha256'][changed]),
        criteria={**criteria,'equivalence_relative':1e-10,'factor_probe_relative':1e-10},
        board_sha256=digest(a.board),rates=old['rates'],duration_s=old['duration_s'],cases=[],
        scope='Same assumed C4, complete board/air and all angular channels/taps. Observation-only '
              'projection before histograms; unchanged numerical interval safeguards. No audible/plugin approval.')

    def checkpoint():
        r['seconds']=time.perf_counter()-start;r['peak_working_set_bytes']=field.full.reference.peak_memory()
        a.report.parent.mkdir(parents=True,exist_ok=True)
        a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')

    checkpoint()
    approved,modal,Bsource,notes,surface,provider,identity,_=field.full.prepare_reference(a)
    # Include simultaneous full/projected factors, projected cell shapes, live
    # endpoint copies, FFT products and full-rank histories. No additional R.
    n=provider.modes;taps=provider.taps;channels=min(provider.outputs,2*provider.block)
    max_history=sum(round(rate*old['duration_s']) for rate in old['rates'])*taps*n*8
    estimate=(provider.budget(real_gram=True)['estimated_array_peak_bytes']+max_history
              +provider.shapes.nbytes+8*taps*channels*n*8+256*2**20)
    r['array_estimates']=dict(maximum_bytes=int(a.max_array_gib*2**30),estimated_bytes=estimate,
        full_rank_history_bytes=max_history,maximum_added_projected_shapes_bytes=provider.shapes.nbytes)
    if estimate>a.max_array_gib*2**30:raise MemoryError('complete declared array budget exceeded')
    _,wave,air=field.full.construct_air(a,approved,provider,surface,identity,int(a.max_array_gib*2**30),r,checkpoint)
    if r['cache_R_sha256']!=old['cache_R_sha256']:raise ValueError('changed full load')
    system=field.BridgeHammer(field.MovingBridge(field.Wire(.62,.0081,850),
        modal.support(Bsource[:,notes.index(60)]),transverse=3),field.Felt(.002,7.5e8,2.6,.125,.02,.1))
    contact=field.FastBoundedContact(system,air);seed=contact.initial_state(5.)
    r['stage']='reconstructing frozen contact';checkpoint()
    for i in range(2304):
        seed,_=contact.step(seed,1/768000,True)
        if (i+1)%576==0:print('seed',i+1,2304,flush=True)
    measured=dict(mechanical_final_j=system.energy(seed[0],seed[1]),
        relaxation_final_j=air.relaxation.energy(seed[3]),final_wave_norm=float(np.linalg.norm(seed[2])),
        compression_final_m=system.compression(seed[0]))
    r['seed_reproduction']={key:delta(value,old['seed'][key]) for key,value in measured.items()}
    if max(r['seed_reproduction'].values())>=1e-10:raise AssertionError('changed seed')
    states=[seed[2].copy()];segments=[];traces=[];pilot=field.ExponentialCoast(system,air)
    r['stage']='reconstructing frozen coast';checkpoint()
    for rate,prior in zip(old['rates'],old['cases'],strict=True):
        final,row,ports,t,B,V=run_coast(pilot,seed,rate,old['duration_s'])
        row['diagnostic_reproduction']={key:delta(row[key],prior[key]) for key in [
            'mechanical_final_j','relaxation_final_j','final_wave_norm','compression_final_m',
            'felt_heat_j','material_heat_j','relaxation_loss_j','air_port_work_j']}
        if max(row['diagnostic_reproduction'].values())>=1e-10:raise AssertionError('changed coast')
        states.append(final[2].copy());segments.append(dict(initial_index=0,final_index=len(states)-1,dt=1/rate,velocity=ports))
        traces.append((t,B,V));r['cases'].append(row);checkpoint()
    memories=np.array(states);Q,basis=observation_basis(wave,memories,segments)
    r['observation_basis']=basis
    if basis['rank']!=old['observation_basis']['rank']:raise AssertionError('changed observation rank')
    count=(len(provider.directions)+provider.block-1)//provider.block
    probes=tuple(sorted(set([0,count//2,count-1])))
    r['stage']='complete angular observation with cell projection';checkpoint()
    begin=time.perf_counter()
    _,powers,bounds=certificate_observer(wave,memories,segments,Q,input_block=256,store_projected_inputs=True,
        factor_projection='cells',projection_probe_blocks=probes,
        progress=lambda i:print('field block',i,count,flush=True) if i%16==0 else None)
    r['certificate_seconds']=time.perf_counter()-begin;r['bounded_observer']=bounds
    for i,(row,prior) in enumerate(zip(r['cases'],old['cases'],strict=True)):
        row.update(field_balance(row,bounds['storage_intervals'][0],bounds['storage_intervals'][i+1],bounds['radiation_intervals'][i]))
        row['wave_power_minimum_w']=float(powers[i].min())
        row['field_reproduction']={key:delta(row[key],prior[key]) for key in ['wave_initial_j','wave_final_j','wave_radiation_j','wave_power_minimum_w']}
        row['observation_error_relative']=max(item['error_bound_j']/max(item['estimate_j'],criteria['observation_floor_j'])
            for item in [row['wave_initial_interval'],row['wave_final_interval'],row['wave_radiation_interval']])
        row['previous_estimates_in_new_intervals']=all(row[key]['lower_j']<=prior[key]['estimate_j']<=row[key]['upper_j']
            for key in ['wave_initial_interval','wave_final_interval','wave_radiation_interval'])
    r['stage']='complete field measured';checkpoint()
    r['work_refinement']=[]
    for row in r['cases'][:-1]:
        changes={key:field.work.energy_change(row,r['cases'][-1],key,
            dict(near_zero_j=criteria['work_floor_j'],energy_work_relative=criteria['work_relative']))
            for key in ['wave_final_j','wave_radiation_j','relaxation_final_j','relaxation_loss_j','material_heat_j','air_port_work_j']}
        r['work_refinement'].append(dict(rate=row['rate'],changes=changes,passed=all(c['passed'] for c in changes.values())))
    native=[(B[::rate//old['rates'][0]],V[::rate//old['rates'][0]]) for rate,(_,B,V) in zip(old['rates'],traces)]
    errors=[max(field.relative(B,native[-1][0]),field.relative(V,native[-1][1])) for B,V in native[:-1]]
    r['native_common_times']=dict(differences_relative=errors,ratio=errors[-1]/max(errors[-2],1e-30),interpolation_used=False)
    r['native_reproduction']=[delta(x,y) for x,y in zip(errors,aligned['native_common_times']['differences_relative'])]
    frequency=np.r_[0.,np.arange(60.,6001.,60.)]
    observer=CellObserver(provider.surface,old['microphones'],4);kernel=retarded_kernel(observer,old['pressure_rate'])
    H=kernel_transfer(kernel,old['pressure_rate'],frequency);green=observer.acceleration_transfer(frequency)
    r['kernel_control_relative']=max(field.relative(x,y) for x,y in zip(H,green))
    common=np.arange(round(old['duration_s']*old['pressure_rate'])+1)/old['pressure_rate']
    boards=[];spectra=[];waveforms=[];captures=[]
    for t,B,V in traces:
        spline=CubicHermiteSpline(t,B,V,axis=0);q,v=spline(common),spline(common,1);boards.append((q,v))
        P=np.einsum('fcm,fm->fc',H,acceleration_spectrum(t,B,V,frequency));spectra.append(P)
        coarse=np.einsum('fcm,fm->fc',H,acceleration_spectrum(t[::2],B[::2],V[::2],frequency))
        captures.append(spectral_relative(frequency,coarse,P,60,6000))
        _,f,F,n=pressure_fft(common,q,v,kernel)
        waveforms.append(irfft(np.where((f<=6000)[:,None],F,0)*old['pressure_rate'],n=n,axis=0))
    r['comparisons']=[dict(board_relative=max(field.relative(q,boards[-1][0]),field.relative(v,boards[-1][1])),
        pressure_spectral_relative=spectral_relative(frequency,P,spectra[-1],60,6000),
        pressure_time_relative=field.relative(wav,waveforms[-1])) for (q,v),P,wav in zip(boards[:-1],spectra[:-1],waveforms[:-1])]
    r['pressure_reproduction']=[{key:delta(value,old['comparisons'][i][key]) for key,value in c.items()} for i,c in enumerate(r['comparisons'])]
    r['capture_errors']=captures
    maximum=max([*r['seed_reproduction'].values(),*r['native_reproduction'],
        *(v for row in r['cases'] for v in row['diagnostic_reproduction'].values()),
        *(v for c in r['pressure_reproduction'] for v in c.values())])
    r['reproduction_maximum_relative']=maximum
    projection=bounds['factor_projection']
    r['cost_comparison']=dict(frozen_certificate_s=old['certificate_seconds'],current_certificate_s=r['certificate_seconds'],
        historical_speedup=old['certificate_seconds']/r['certificate_seconds'],
        scope='Historical full certificate timing, not an isolated paired whole-run benchmark. '
              'Three isolated full-factor/cell projection timings on the same regenerated blocks are recorded separately; '
              'current certificate includes these oracle probes and both full/projected factor construction.')
    r['projection_gates']=dict(
        dynamics_pressure_reproduced=maximum<1e-10,
        field_equivalence=max(v for row in r['cases'] for v in row['field_reproduction'].values())<1e-10,
        previous_estimates_contained=all(row['previous_estimates_in_new_intervals'] for row in r['cases']),
        complete_factors=bounds['factor_norm_relative_error']<1e-10 and bounds['projected_factor_norm_relative_error']<1e-10 and projection['blocks']==count,
        factor_probes=max(probe['relative'] for probe in projection['probe_results'])<1e-10,
        complete_memory_replay=max(bounds['full_endpoint_replay_relative'])<1e-11,
        bounded_energy=max(max(c['total_budget_bound_relative'],c['air_budget_bound_relative']) for c in r['cases'])<criteria['energy_relative'],
        observation_precision=max(c['observation_error_relative'] for c in r['cases'])<criteria['observation_relative'],
        nonnegative_loss=all(c['minimum_step_heat_j']>=-1e-16 and c['wave_power_minimum_w']>=0 for c in r['cases']),
        nonzero_wave_flux=all(c['wave_radiation_interval']['lower_j']>criteria['work_floor_j'] for c in r['cases']),
        refined_work=r['work_refinement'][-1]['passed'],
        native_second_order=r['native_common_times']['ratio']<criteria['refinement_ratio'],
        refined_board=errors[-1]<criteria['board_relative'],
        refined_pressure=max(r['comparisons'][-1]['pressure_spectral_relative'],r['comparisons'][-1]['pressure_time_relative'])<criteria['pressure_relative'],
        refined_capture=captures[-1]<criteria['capture_relative'],kernel=r['kernel_control_relative']<criteria['kernel_relative'],
        source_unchanged=all(digest(Path(__file__).with_name(name))==sha for name,sha in r['source_sha256'].items()))
    r['projection_gates']={key:bool(value) for key,value in r['projection_gates'].items()}
    r['projection_gates_passed']=all(r['projection_gates'].values())
    r['original_interpolated_failure']=old['coast_gates']['second_order_board']
    r['stage']='full cell projection observation audit completed';checkpoint()
    print('finished',json.dumps(r['projection_gates']),r['certificate_seconds'],r['seconds'],flush=True)


if __name__=='__main__':main()
