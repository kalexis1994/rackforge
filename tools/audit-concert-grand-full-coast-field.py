"""Independent full-board coast energy/flux and finite-window microphone pressure.

All 606 physical coordinates and live air states remain. RAM-only captures;
one authenticated R. The 0.5 ms pressure window is not a held-note release.
"""
import argparse
import importlib.util
import json
import time
from pathlib import Path

import numpy as np
from scipy.interpolate import CubicHermiteSpline
from scipy.fft import irfft

from concert_grand_wave_cache import digest
from concert_grand_moving_bridge import MovingBridge,BridgeHammer
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
from concert_grand_fast_contact import FastBoundedContact
from concert_grand_exponential_coast import ExponentialCoast
from concert_grand_coast_measurements import run_coast,field_balance
from concert_grand_bounded_observer import observation_basis,certificate_observer
from concert_grand_cell_observer import CellObserver,acceleration_spectrum,spectral_relative
from concert_grand_cell_audio import retarded_kernel,kernel_transfer,pressure_fft


def module(name,file):
    spec=importlib.util.spec_from_file_location(name,Path(__file__).with_name(file))
    result=importlib.util.module_from_spec(spec);spec.loader.exec_module(result)
    return result

full=module('full_coast_reference','audit-concert-grand-pade-contact-full.py')
work=module('coast_work_reference','audit-concert-grand-prefix-work-refinement.py')


def relative(a,b):return float(np.linalg.norm(np.asarray(a)-b)/max(np.linalg.norm(b),1e-30))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['full-report','board','cache','cost-report','microphone-report','report']:
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--rates',type=int,nargs='+',default=[64000,128000,256000])
    p.add_argument('--duration',type=float,default=.0005)
    p.add_argument('--direction-block',type=int,default=32)
    p.add_argument('--max-array-gib',type=float,default=12.)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    if (len(a.rates)<3 or sorted(set(a.rates))!=a.rates or min(a.rates)<=0
        or any(rate%min(a.rates) for rate in a.rates)
        or not np.isfinite(a.duration) or a.duration<=0 or any(abs(rate*a.duration-round(rate*a.duration))>1e-9 for rate in a.rates)
        or any(round(rate*a.duration)%2 for rate in a.rates)
        or not np.isfinite(a.max_array_gib) or a.max_array_gib<=0):
        raise ValueError('nested positive rates, exact duration and positive memory budget required')
    if not a.cache.exists() or not a.cache.resolve().is_relative_to(Path('local/concert-grand-physics').resolve()):
        raise ValueError('existing single local cache required')
    old=json.loads(a.cost_report.read_text());mic=json.loads(a.microphone_report.read_text())
    for data in [old,mic]:
        if (digest(a.full_report) not in data.get('parent_sha256',{}).values()
                or data.get('board_sha256')!=digest(a.board)):
            raise ValueError('matching frozen board/load reports required')
    if (old.get('stage')!='full propagation cost preflight completed' or not old.get('preflight_gates_passed')
            or mic.get('stage')!='full microphone kernel audit completed' or not mic.get('kernel_gates_passed')
            or old['contact']['rate']!=768000 or old['contact']['duration_s']!=.003):
        raise ValueError('approved cost/kernel controls required')
    sources=set(old['source_sha256'])|set(mic['source_sha256'])|{
        Path(__file__).name,'concert_grand_coast_measurements.py','concert_grand_bounded_observer.py',
        'concert_grand_pade_port_certificate.py','audit-concert-grand-prefix-work-refinement.py'}
    # A new audit adds observers, not a changed physical/coast construction.
    for data in [old,mic]:
        for name,sha in data['source_sha256'].items():
            if digest(Path(__file__).with_name(name))!=sha:raise ValueError('changed frozen cost/kernel source: '+name)
    start=time.perf_counter()
    criteria=dict(energy_relative=1e-9,observation_relative=1e-4,observation_floor_j=1e-14,
        work_relative=.002,work_floor_j=1e-14,pressure_relative=.001,capture_relative=.001,
        board_relative=.01,refinement_ratio=.35,seed_relative=1e-10,kernel_relative=.0002)
    r=dict(stage='construction',all_gates_passed=False,preflight_only=True,criteria=criteria,
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in sorted(sources)},
        parent_sha256={str(path):digest(path) for path in [a.full_report,a.cost_report,a.microphone_report]},
        board_sha256=digest(a.board),cache=str(a.cache),rates=a.rates,duration_s=a.duration,
        start_after_contact_s=.003,cases=[],
        scope='Assumed single C4 wire and board. Independent complete-angle coast ledger and '
              'finite-window two-capsule pressure; no full sustain/release, timbre or plugin approval.')

    def checkpoint():
        r['seconds']=time.perf_counter()-start;r['peak_working_set_bytes']=full.reference.peak_memory()
        a.report.parent.mkdir(parents=True,exist_ok=True)
        a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')

    checkpoint()
    approved,modal,source_B,notes,surface,provider,identity,_=full.prepare_reference(a)
    r['physical_source_sha256']=approved['source_sha256'];r['modes']=provider.modes;r['states']=approved['states']
    max_history=sum(round(rate*a.duration) for rate in a.rates)*provider.taps*provider.modes*8
    estimate=(provider.budget(real_gram=True)['estimated_array_peak_bytes']+max_history
              +8*provider.taps*2*provider.block*provider.modes*8+256*2**20)
    r['array_estimates']=dict(maximum_bytes=int(a.max_array_gib*2**30),estimated_bytes=estimate,
                              full_rank_input_history_bytes=max_history)
    if estimate>a.max_array_gib*2**30:raise MemoryError('declared complete array budget exceeded')
    wave,air_wave,air=full.construct_air(a,approved,provider,surface,
                                      identity,int(a.max_array_gib*2**30),r,checkpoint)
    if old['cache_R_sha256']!=r['cache_R_sha256']:raise ValueError('changed acoustic coefficients')
    core=MovingBridge(Wire(.62,.0081,850),modal.support(source_B[:,notes.index(60)]),transverse=3)
    system=BridgeHammer(core,Felt(.002,7.5e8,2.6,.125,.02,.1))
    contact=FastBoundedContact(system,air);state=contact.initial_state(5.)
    r['stage']='reconstructing measured contact';checkpoint()
    for i in range(2304):
        state,_=contact.step(state,1/768000,True)
        if (i+1)%576==0:print('seed',i+1,2304,flush=True)
    prior=next(row for row in old['contact']['cases'] if row['kernel']=='batched')
    seed_metrics=dict(mechanical_final_j=system.energy(state[0],state[1]),
        relaxation_final_j=air.relaxation.energy(state[3]),final_wave_norm=float(np.linalg.norm(state[2])),
        compression_final_m=system.compression(state[0]))
    r['seed']=seed_metrics;r['seed_comparison']={key:abs(value-prior[key])/max(abs(prior[key]),1e-30)
                                              for key,value in seed_metrics.items()}
    if max(r['seed_comparison'].values())>=criteria['seed_relative'] or system.compression(state[0])>=0:
        raise AssertionError('seed differs from measured separated endpoint')
    states=[state[2].copy()];segments=[];traces=[];finals=[]
    pilot=ExponentialCoast(system,air)
    r['stage']='full living coast capture';checkpoint()
    for rate in a.rates:
        final,row,ports,t,B,V=run_coast(pilot,state,rate,a.duration,
                                      lambda i,n:print('coast',rate,i,n,flush=True))
        states.append(final[2].copy());segments.append(dict(initial_index=0,final_index=len(states)-1,dt=1/rate,velocity=ports))
        r['cases'].append(row);traces.append((t,B,V));finals.append(final);checkpoint()
    r['stage']='independent angular storage and flux';checkpoint()
    memories=np.array(states)
    Q,basis=observation_basis(air_wave,memories,segments)
    r['observation_basis']=basis;checkpoint();print('observer rank',basis['rank'],flush=True)
    t0=time.perf_counter()
    energies,powers,bounds=certificate_observer(air_wave,memories,segments,Q,input_block=256,
        store_projected_inputs=True,progress=lambda i:print('field block',i,flush=True) if i%16==0 else None)
    r['certificate_seconds']=time.perf_counter()-t0;r['bounded_observer']=bounds
    for i,row in enumerate(r['cases']):
        row.update(field_balance(row,bounds['storage_intervals'][0],bounds['storage_intervals'][i+1],bounds['radiation_intervals'][i]))
        row['wave_power_minimum_w']=float(powers[i].min())
        row['observation_error_relative']=max(item['error_bound_j']/max(item['estimate_j'],criteria['observation_floor_j'])
            for item in [row['wave_initial_interval'],row['wave_final_interval'],row['wave_radiation_interval']])
    r['stage']='independent angular certificate measured';checkpoint()
    r['work_refinement']=[]
    for row in r['cases'][:-1]:
        changes={key:work.energy_change(row,r['cases'][-1],key,
            dict(near_zero_j=criteria['work_floor_j'],energy_work_relative=criteria['work_relative']))
            for key in ['wave_final_j','wave_radiation_j','relaxation_final_j','relaxation_loss_j','material_heat_j','air_port_work_j']}
        r['work_refinement'].append(dict(rate=row['rate'],changes=changes,passed=all(c['passed'] for c in changes.values())))
    r['stage']='finite-window microphone pressure';checkpoint()
    frequency=np.r_[0.,np.arange(60.,6001.,60.)]
    observer=CellObserver(provider.surface,mic['microphones'],4)
    pressure_rate=256000
    if abs(a.duration*pressure_rate-round(a.duration*pressure_rate))>1e-9:raise ValueError('exact common pressure grid required')
    kernel=retarded_kernel(observer,pressure_rate)
    H=kernel_transfer(kernel,pressure_rate,frequency);green=observer.acceleration_transfer(frequency)
    r['microphones']=mic['microphones'];r['pressure_rate']=pressure_rate
    r['kernel_control_relative']=max(relative(x,y) for x,y in zip(H,green))
    spectra=[];audio=[];boards=[];capture_errors=[]
    common=np.arange(round(a.duration*pressure_rate)+1)/pressure_rate
    for row,(t,B,V) in zip(r['cases'],traces):
        A=acceleration_spectrum(t,B,V,frequency);P=np.einsum('fcm,fm->fc',H,A)
        spectra.append(P)
        coarse=acceleration_spectrum(t[::2],B[::2],V[::2],frequency)
        capture_errors.append(spectral_relative(frequency,np.einsum('fcm,fm->fc',H,coarse),P,60,6000))
        spline=CubicHermiteSpline(t,B,V,axis=0);q,v=spline(common),spline(common,1)
        boards.append((q,v));_,f,F,size=pressure_fft(common,q,v,kernel)
        band=irfft(np.where((f<=6000)[:,None],F,0)*pressure_rate,n=size,axis=0)
        audio.append(band)
        row['pressure_peak_pa']=np.max(abs(band),axis=0).tolist()
        row['pressure_rms_pa']=np.sqrt(np.mean(band*band,axis=0)).tolist()
    r['pressure_window']=dict(source_start_s=.003,source_duration_s=a.duration,frequency_band_hz=[60,6000],
        temporal_band_hz=[0,6000],capture_comparison='Every second native endpoint, identical Green kernel; no endpoint delta impulses.',
        waveform_scope='Padded band-limited contribution of this finite acceleration window; not a physical note-off or release tail.',
        fft_size=len(audio[0]),pressure_sample_count=len(audio[0]),kernel_bytes=kernel.nbytes)
    r['comparisons']=[dict(rate=row['rate'],reference_rate=a.rates[-1],
        board_relative=max(relative(q,boards[-1][0]),relative(v,boards[-1][1])),
        pressure_spectral_relative=spectral_relative(frequency,P,spectra[-1],60,6000),
        pressure_time_relative=relative(wav,audio[-1]))
        for row,(q,v),P,wav in zip(r['cases'][:-1],boards[:-1],spectra[:-1],audio[:-1])]
    r['capture_errors']=[dict(rate=rate,coarsened_rate=rate//2,pressure_relative=error) for rate,error in zip(a.rates,capture_errors)]
    errors=[row['board_relative'] for row in r['comparisons']]
    r['board_refinement_ratio']=errors[-1]/max(errors[-2],1e-30)
    r['coast_gates']=dict(
        seed=max(r['seed_comparison'].values())<criteria['seed_relative'],
        complete_memory_replay=max(bounds['full_endpoint_replay_relative'])<1e-11,
        bounded_energy=max(max(c['total_budget_bound_relative'],c['air_budget_bound_relative']) for c in r['cases'])<criteria['energy_relative'],
        observation_precision=max(c['observation_error_relative'] for c in r['cases'])<criteria['observation_relative'],
        nonnegative_loss=all(c['minimum_step_heat_j']>=-1e-16 and c['wave_power_minimum_w']>=0 for c in r['cases']),
        nonzero_wave_flux=all(c['wave_radiation_interval']['lower_j']>criteria['work_floor_j'] for c in r['cases']),
        refined_work=r['work_refinement'][-1]['passed'],
        refined_board=errors[-1]<criteria['board_relative'],
        second_order_board=r['board_refinement_ratio']<criteria['refinement_ratio'],
        refined_pressure=max(r['comparisons'][-1]['pressure_spectral_relative'],r['comparisons'][-1]['pressure_time_relative'])<criteria['pressure_relative'],
        refined_capture=capture_errors[-1]<criteria['capture_relative'],
        kernel=r['kernel_control_relative']<criteria['kernel_relative'],
        source_unchanged=all(digest(Path(__file__).with_name(name))==sha for name,sha in r['source_sha256'].items()))
    r['coast_gates']={name:bool(value) for name,value in r['coast_gates'].items()}
    r['coast_gates_passed']=all(r['coast_gates'].values());r['stage']='full coast field audit completed'
    r['next']='Extend the checked source window before listening; natural restrike/release and real-piano identification remain.'
    checkpoint();print('finished',json.dumps(r['coast_gates']),r['seconds'],flush=True)


if __name__=='__main__':main()
