"""Physical single-note bank entry for the first separated-note phrase.

New pitch-specific wire/contact and bridge projection; unchanged SI physics,
full modes and air. Cold contact is checked independently before coasting.
This is not a shared-board polyphonic engine or a note-off/action model.
"""
import argparse,json,time
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from scipy.fft import rfft
from scipy.signal import resample_poly
from scipy.io import wavfile
from concert_grand_air_coefficients import load_prefix
from concert_grand_bounded_air import BoundedAir
from concert_grand_fast_contact import FastBoundedContact
from concert_grand_polynomial_coast import PolynomialCoast
from concert_grand_cell_audio import pressure_fft,hermite_fft
from concert_grand_cell_observer import spectral_relative

def module(name,path):
    spec=spec_from_file_location(name,Path(__file__).with_name(path));m=module_from_spec(spec);spec.loader.exec_module(m);return m
bridge=module('bank_bridge','audit-concert-grand-moving-bridge.py')
note_tool=module('bank_note_join','render-concert-grand-exponential-note.py')
sha=note_tool.digest;write=note_tool.write;rel=note_tool.relative
CRITERIA=dict(energy_budget=1e-9,contact_time=.001,contact_force=.002,contact_board=.01,
              pressure=.01,board=.01,capture=.001,storage_negative=1e-9)
KEYS=['q','v','wave_state_final','relaxation_state_final']


def contact(solver,speed,rate):
    state=solver.initial_state(speed);initial=solver.energy(state)['total_j']
    loss=heat=material=port=impulse=peak=0.;separation=None;B=[];V=[];pending=[]
    started=time.monotonic()
    for i in range(round(rate*.004)+1):
        if i%(rate//128000)==0:B.append(solver.board_map@state[0]);V.append(solver.board_map@state[1])
        if i==round(rate*.004):break
        before=solver.system.compression(state[0]);state,stats=solver.step(state,1/rate,True)
        after=solver.system.compression(state[0])
        if separation is None and before>0 and after<=0 and state[1][-1]<0:separation=(i+before/(before-after))/rate
        loss+=stats['loss_j'];heat+=stats['felt_loss_j'];material+=stats['material_loss_j'];port+=stats['air_port_work_j']
        peak=max(peak,stats['force_n']);impulse+=stats['force_n']/rate;pending.append(stats['wave_inputs'])
        if len(pending)==64:
            loss+=float(solver.air.wave.radiation_batch(np.array(pending)).sum()/rate);pending.clear()
    if pending:loss+=float(solver.air.wave.radiation_batch(np.array(pending)).sum()/rate)
    final=solver.energy(state)['total_j']
    if separation is None or solver.system.compression(state[0])>=0 or state[1][-1]>=0:raise ValueError('returning hammer must separate by 4ms')
    metrics=dict(rate_hz=rate,initial_energy_j=initial,final_energy_j=final,
        felt_heat_j=heat,material_heat_j=material,air_work_j=port,independent_radiation_j=loss-heat-material,
        energy_budget_relative=abs(final+loss-initial)/initial,contact_time_s=separation,
        maximum_force_n=peak,impulse_n_s=impulse,seconds=time.monotonic()-started)
    return state,np.array(B),np.array(V),metrics


def coast(solver,state,rate,duration):
    B=[solver.board_map@state[0]];V=[solver.board_map@state[1]];steps=round(duration*rate)
    if abs(steps/rate-duration)>1e-14:raise ValueError('exact capture duration')
    solver.oscillator_functions(.5/rate);solver._air_prepare(1/rate)
    E0=solver.energy(state)['total_j'];material=port=relax=0.;iterations=0
    started=last=time.monotonic()
    for i in range(steps):
        state,stats=solver.step(state,1/rate,True)
        material+=stats['material_loss_j'];port+=stats['air_port_work_j'];relax+=stats['radiation_loss_j']
        iterations=max(iterations,stats['mechanical_iterations'])
        B.append(solver.board_map@state[0]);V.append(solver.board_map@state[1])
        now=time.monotonic()
        if now-last>=30 or i+1==steps:
            print(f'coast {rate}Hz {(i+1)/steps:.1%}, source {.004+(i+1)/rate:.6f}s, dynamics {now-started:.1f}s',flush=True);last=now
    elapsed=time.monotonic()-started;final=solver.energy(state)['total_j']
    metrics=dict(rate_hz=rate,dynamics_s=elapsed,initial_energy_j=E0,final_energy_j=final,material_heat_j=material,
        air_work_j=port,relaxation_heat_j=relax,radiation_inferred_j=E0-final-material,maximum_iterations=iterations)
    return state,dict(time_s=.004+np.arange(len(B))/rate,board_position=np.array(B),board_velocity=np.array(V)),metrics


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['input','asset','coefficients','baseline-report','kernel','kernel-report','output','audio','report']:
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--note',type=int,required=True);p.add_argument('--velocity',type=int,default=124)
    p.add_argument('--duration',type=float,default=1.);p.add_argument('--gain',type=float,default=.25)
    args=p.parse_args()
    if any(x.exists() for x in [args.output,args.audio,args.report]):raise ValueError('preserve measurements')
    if not 21<=args.note<=108 or args.velocity!=124 or args.duration!=1. or args.gain!=.25:p.error('this bank is 1s/124 with fixed .25 gain')
    started=time.monotonic();frozen=json.loads(args.baseline_report.read_text());mic=json.loads(args.kernel_report.read_text());prov=frozen['provenance']
    if not all(frozen['gates'].values()) or sha(args.asset)!=prov['asset_sha256'] or sha(args.coefficients)!=prov['coefficient_sha256']:raise ValueError('frozen physical load changed')
    if not mic['all_gates_passed'] or sha(args.kernel)!=mic['kernel_sha256'] or sha(args.baseline_report)!=mic['contact_report_sha256']:raise ValueError('readout mismatch')
    support,projection=bridge.p2_support(args.asset,args.note,prov['support']['cutoff_hz'])
    if list(support.frequency)!=prov['support']['frequencies_hz']:raise ValueError('common physical board basis')
    system,speed=bridge.load(args.input,args.note,args.velocity,support,prov['transverse'])
    factor,rate,mass,limit,parent=load_prefix(args.coefficients,support.frequency)
    air=BoundedAir(factor,rate,mass,limit);del factor
    fast=FastBoundedContact(system,air);pilot=PolynomialCoast(system,air)
    with np.load(args.kernel) as d:kernel=d['kernel']
    args.output.mkdir(parents=True)
    result=dict(note=args.note,velocity=args.velocity,duration_s=args.duration,gain=args.gain,criteria=CRITERIA,
        physical=dict(wire=vars(system.string.wire),felt=vars(system.felt),incident_speed_m_s=speed,support=projection,
                      transverse=system.string.nt,axial=system.string.nl,acoustic_states=air.states),
        inputs_sha256={str(path):sha(path) for path in [args.input/'transfer.csv',args.input/'contact-end.csv',args.asset,args.coefficients,args.baseline_report,args.kernel,args.kernel_report]},
        source_sha256={n:sha(Path(__file__).with_name(n)) for n in ['render-concert-grand-physical-bank-note.py',
            'concert_grand_fast_contact.py','concert_grand_exponential_coast.py','concert_grand_polynomial_coast.py','concert_grand_cell_audio.py']},
        contacts={},coasts={},gates={},scope='Independent cold one-string notes for separated-note musical audition; no pitch-shift, shared-board polyphony, action or identified piano.')
    write(args.report,result);previous=None
    for rate in [384000,768000,1536000,3072000]:
        print('contact',args.note,rate,flush=True)
        state,B,V,metrics=contact(fast,speed,rate)
        path=args.output/f'Contact-{rate}.npz';np.savez_compressed(path,board_position=B,board_velocity=V,**dict(zip(KEYS,state)))
        metrics['sha256']=sha(path);result['contacts'][str(rate)]=metrics
        if metrics['energy_budget_relative']>=CRITERIA['energy_budget']:raise AssertionError('independent contact energy ledger')
        if previous is not None:
            old,a,b=previous
            changes=dict(contact_time=abs(old['contact_time_s']-metrics['contact_time_s'])/metrics['contact_time_s'],
                force=abs(old['maximum_force_n']-metrics['maximum_force_n'])/metrics['maximum_force_n'],
                impulse=abs(old['impulse_n_s']-metrics['impulse_n_s'])/metrics['impulse_n_s'],
                board_position=rel(a,B),board_velocity=rel(b,V))
            passed=changes['contact_time']<CRITERIA['contact_time'] and max(changes['force'],changes['impulse'])<CRITERIA['contact_force'] and max(changes['board_position'],changes['board_velocity'])<CRITERIA['contact_board']
            result.setdefault('contact_comparisons',[]).append(dict(fine_rate_hz=rate,changes=changes,passed=passed))
            if passed:result['selected_contact_rate_hz']=rate;result['gates']['contact']=True;break
        previous=(metrics,B,V);write(args.report,result)
    else:raise AssertionError('contact rate did not converge')
    write(args.report,result);onset_B,onset_V=B,V;frozen_state=state;notes={};pressures={}
    for rate in [64000,128000]:
        print('sustain',args.note,rate,flush=True)
        state,trace,metrics=coast(pilot,tuple(a.copy() for a in frozen_state),rate,args.duration-.004)
        path=args.output/f'Coast-{rate}.npz';np.savez_compressed(path,**trace,**dict(zip(KEYS,state)));metrics['sha256']=sha(path)
        end=dict(board_position=trace['board_position'][-1:],board_velocity=trace['board_velocity'][-1:])
        t,B,V,joins=note_tool.joined_trace(onset_B,onset_V,trace,end,rate)
        pressure,f,P,size=pressure_fft(t,B,V,kernel);metrics['join_relative']=joins
        result['coasts'][str(rate)]=metrics;notes[rate]=(t,B,V);pressures[rate]=P
        if rate==128000:selected=(pressure,f,size)
        write(args.report,result)
    t,B,V=notes[128000];_,b,v=notes[64000];pressure,f,size=selected
    comparison=dict(pressure=spectral_relative(f,pressures[64000],pressures[128000],60,6000),position=rel(b,B),velocity=rel(v,V))
    cf,CA=hermite_fft(t[::2],B[::2],V[::2],size//2);H=rfft(kernel,n=size,axis=0)
    CP=np.einsum('fcm,fm->fc',H[:len(CA)],CA);capture=spectral_relative(cf,CP,pressures[128000][:len(cf)],60,6000)
    result.update(coast_comparison=comparison,capture_pressure_relative=capture)
    result['gates'].update(coast_pressure=comparison['pressure']<CRITERIA['pressure'],coast_board=max(comparison['position'],comparison['velocity'])<CRITERIA['board'],
        capture=capture<CRITERIA['capture'],positive_storage_residual=all(c['radiation_inferred_j']>=-CRITERIA['storage_negative']*c['initial_energy_j'] for c in result['coasts'].values()))
    result['all_gates_passed']=all(result['gates'].values());write(args.report,result)
    if not result['all_gates_passed']:raise AssertionError('bank-note waveform refinement failed')
    pressure=pressure[:len(t)+len(kernel)-1].copy();result['physical_pressure_peak_pa']=float(np.max(abs(pressure)))
    fade=round(.008*128000);pressure[-fade:]*=(.5+.5*np.cos(np.linspace(0,np.pi,fade)))[:,None]
    audio=resample_poly(pressure*args.gain,3,8,axis=0)
    result['audio_peak']=float(np.max(abs(audio)))
    if result['audio_peak']>=.98:raise ValueError('common gain clips')
    audio=np.vstack([np.zeros((4800,2)),audio,np.zeros((14400,2))]).astype(np.float32)
    args.audio.parent.mkdir(parents=True,exist_ok=True);wavfile.write(args.audio,48000,audio)
    result.update(audio_path=str(args.audio),audio_sha256=sha(args.audio),audio_rate_hz=48000,audio_seconds=len(audio)/48000,seconds=time.monotonic()-started)
    write(args.report,result);print('saved',args.note,str(args.audio),json.dumps(result['gates']),flush=True)


if __name__=='__main__':main()
