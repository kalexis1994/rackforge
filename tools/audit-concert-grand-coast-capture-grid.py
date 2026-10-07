"""Separate native common-time coast refinement from interpolated-grid diagnostics.

Reproduce frozen dynamics and pressure diagnostics. Retain the failed
interpolated ratio; reuse the earlier common-native-time definition and
unchanged 0.35 criterion. Angular ledgers are linked, not measured again.
"""
import argparse
import importlib.util
import json
import time
from pathlib import Path
import numpy as np
from scipy.interpolate import CubicHermiteSpline
from concert_grand_coast_measurements import run_coast
from concert_grand_cell_observer import CellObserver,acceleration_spectrum,spectral_relative
from concert_grand_cell_audio import retarded_kernel,kernel_transfer,pressure_fft
from scipy.fft import irfft
from concert_grand_wave_cache import digest

spec=importlib.util.spec_from_file_location('frozen_coast_field',Path(__file__).with_name('audit-concert-grand-full-coast-field.py'))
field=importlib.util.module_from_spec(spec);spec.loader.exec_module(field)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['field-report','full-report','board','cache','report']:
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--direction-block',type=int,default=32)
    p.add_argument('--max-array-gib',type=float,default=12.)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    old=json.loads(a.field_report.read_text())
    if (old.get('stage')!='full coast field audit completed'
            or old.get('rates')!=[64000,128000,256000] or old.get('duration_s')!=.0005
            or old['board_sha256']!=digest(a.board) or digest(a.full_report) not in old['parent_sha256'].values()
            or not all(v for k,v in old['coast_gates'].items() if k!='second_order_board')):
        raise ValueError('completed matched field/pressure controls required')
    if not a.cache.resolve().is_relative_to(Path('local/concert-grand-physics').resolve()):
        raise ValueError('single local cache required')
    if not np.isfinite(a.max_array_gib) or a.max_array_gib<=0:raise ValueError('positive budget required')
    for name,sha in old['source_sha256'].items():
        if digest(Path(__file__).with_name(name))!=sha:raise ValueError('changed frozen source: '+name)
    start=time.perf_counter()
    r=dict(stage='construction',all_gates_passed=False,preflight_only=True,
        parent_sha256={str(a.field_report):digest(a.field_report),str(a.full_report):digest(a.full_report)},
        source_sha256={**old['source_sha256'],Path(__file__).name:digest(__file__)},
        criteria=dict(reproduction_relative=1e-10,refinement_ratio=old['criteria']['refinement_ratio']),
        original_coast_gates=old['coast_gates'],original_interpolated_ratio=old['board_refinement_ratio'],
        scope='Common native-time convergence of the already measured coast. Original interpolated failure '
              'retained; linked independent field/pressure gates only, no whole-note or timbre approval.')

    def checkpoint():
        r['seconds']=time.perf_counter()-start;r['peak_working_set_bytes']=field.full.reference.peak_memory()
        a.report.parent.mkdir(parents=True,exist_ok=True)
        a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')

    checkpoint()
    approved,modal,Bsource,notes,surface,provider,identity,_=field.full.prepare_reference(a)
    budget=provider.budget(real_gram=True)['estimated_array_peak_bytes']+256*2**20
    if budget>a.max_array_gib*2**30:raise MemoryError('array budget exceeded')
    _,_,air=field.full.construct_air(a,approved,provider,surface,identity,int(a.max_array_gib*2**30),r,checkpoint)
    if r['cache_R_sha256']!=old['cache_R_sha256']:raise ValueError('changed load')
    core=field.MovingBridge(field.Wire(.62,.0081,850),modal.support(Bsource[:,notes.index(60)]),transverse=3)
    system=field.BridgeHammer(core,field.Felt(.002,7.5e8,2.6,.125,.02,.1))
    contact=field.FastBoundedContact(system,air);seed=contact.initial_state(5.)
    r['stage']='reproducing contact';checkpoint()
    for i in range(2304):
        seed,_=contact.step(seed,1/768000,True)
        if (i+1)%576==0:print('seed',i+1,2304,flush=True)
    measured=dict(mechanical_final_j=system.energy(seed[0],seed[1]),
        relaxation_final_j=air.relaxation.energy(seed[3]),final_wave_norm=float(np.linalg.norm(seed[2])),
        compression_final_m=system.compression(seed[0]))
    r['seed_reproduction']={key:abs(value-old['seed'][key])/max(abs(old['seed'][key]),1e-14) for key,value in measured.items()}
    if max(r['seed_reproduction'].values())>=1e-10:raise AssertionError('different seed')
    traces=[];rows=[];pilot=field.ExponentialCoast(system,air)
    r['stage']='reproducing coast';checkpoint()
    for rate in old['rates']:
        _,row,_,t,B,V=run_coast(pilot,seed,rate,old['duration_s'])
        prior=next(c for c in old['cases'] if c['rate']==rate)
        errors={key:abs(row[key]-prior[key])/max(abs(prior[key]),1e-14) for key in [
            'mechanical_final_j','relaxation_final_j','final_wave_norm','compression_final_m',
            'felt_heat_j','material_heat_j','relaxation_loss_j','air_port_work_j']}
        rows.append(dict(rate=rate,diagnostic_reproduction=errors));traces.append((t,B,V))
    r['cases']=rows
    capture=old['rates'][0]
    native=[(B[::rate//capture],V[::rate//capture]) for rate,(_,B,V) in zip(old['rates'],traces)]
    errors=[max(field.relative(B,native[-1][0]),field.relative(V,native[-1][1])) for B,V in native[:-1]]
    r['native_common_times']=dict(capture_rate=capture,samples=len(native[0][0]),
        differences_relative=errors,ratio=errors[-1]/max(errors[-2],1e-30),
        interpolation_used=False)
    # Independently reproduce the diagnostic that failed, including its
    # source interpolation and pressure calculation; don't erase it.
    frequency=np.r_[0.,np.arange(60.,6001.,60.)]
    observer=CellObserver(provider.surface,old['microphones'],4);kernel=retarded_kernel(observer,old['pressure_rate'])
    H=kernel_transfer(kernel,old['pressure_rate'],frequency)
    common=np.arange(round(old['duration_s']*old['pressure_rate'])+1)/old['pressure_rate']
    boards=[];spectra=[];waveforms=[]
    for t,B,V in traces:
        spline=CubicHermiteSpline(t,B,V,axis=0);q,v=spline(common),spline(common,1);boards.append((q,v))
        spectra.append(np.einsum('fcm,fm->fc',H,acceleration_spectrum(t,B,V,frequency)))
        _,f,F,n=pressure_fft(common,q,v,kernel)
        waveforms.append(irfft(np.where((f<=6000)[:,None],F,0)*old['pressure_rate'],n=n,axis=0))
    checks=[]
    for i in range(2):
        current=dict(board_relative=max(field.relative(boards[i][0],boards[-1][0]),field.relative(boards[i][1],boards[-1][1])),
            pressure_spectral_relative=spectral_relative(frequency,spectra[i],spectra[-1],60,6000),
            pressure_time_relative=field.relative(waveforms[i],waveforms[-1]))
        checks.append({key:abs(value-old['comparisons'][i][key])/max(abs(old['comparisons'][i][key]),1e-14)
                       for key,value in current.items()})
    r['interpolated_pressure_diagnostic_reproduction']=checks
    largest=max([*r['seed_reproduction'].values(),
        *(v for row in rows for v in row['diagnostic_reproduction'].values()),
        *(v for row in checks for v in row.values())])
    r['reproduction_maximum_relative']=largest
    r['stage']='alignment diagnostics measured';checkpoint()
    r['alignment_gates']=dict(reproduced_field_dynamics_pressure=largest<1e-10,
        native_second_order=r['native_common_times']['ratio']<r['criteria']['refinement_ratio'],
        native_finest_board=errors[-1]<old['criteria']['board_relative'],
        source_unchanged=all(digest(Path(__file__).with_name(name))==sha for name,sha in r['source_sha256'].items()))
    r['alignment_gates']={name:bool(value) for name,value in r['alignment_gates'].items()}
    linked={key:value for key,value in old['coast_gates'].items() if key!='second_order_board'}
    linked.update({'aligned_'+key:bool(value) for key,value in r['alignment_gates'].items()})
    r['linked_coast_gates']=linked;r['linked_coast_gates_passed']=all(linked.values())
    r['changed_observable']='Order ratio on common native physical times, as in the prior cost preflight. '
    r['changed_observable']+='Interpolated-grid ratio and its original failed gate retained; threshold unchanged.'
    r['stage']='coast common-time alignment completed';checkpoint()
    print('finished',json.dumps(r['alignment_gates']),r['native_common_times'],r['seconds'],flush=True)


if __name__=='__main__':main()
