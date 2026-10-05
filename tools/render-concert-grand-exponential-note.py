"""Extend measured separated-hammer C4 coasts and render their checked note.

Only new continuation segments are saved; onset, prefix and microphone data
stay in their unique existing files. Full-note pressure is compared on the
audio FFT grid without gain fitting. No factory or contact changes.
"""
import argparse,json,time
from importlib.util import spec_from_file_location,module_from_spec
from pathlib import Path
import numpy as np
from scipy.interpolate import CubicHermiteSpline
from scipy.fft import rfft
from scipy.signal import resample_poly
from scipy.io import wavfile
from concert_grand_polynomial_coast import PolynomialCoast
from concert_grand_cell_audio import pressure_fft,hermite_fft
from concert_grand_cell_observer import spectral_relative

spec=spec_from_file_location('physical_note_onset',Path(__file__).with_name('render-concert-grand-physical-note.py'))
onset=module_from_spec(spec);spec.loader.exec_module(onset)
digest=onset.digest;write=onset.write;relative=onset.relative
CRITERIA=dict(pressure_relative=.01,board_relative=.01,capture_pressure=.001,
              storage_loss_negative_relative=1e-9,join_relative=1e-12)
STATE_KEYS=['q','v','wave_state_final','relaxation_state_final']


def joined_trace(onset_B,onset_V,prefix,extension,rate):
    """Exact Hermite subdivision, preserving the coarser continuous source."""
    pB,pV=prefix['board_position'],prefix['board_velocity']
    eB,eV=extension['board_position'],extension['board_velocity']
    joins=dict(onset_position=relative(pB[0],onset_B[-1]),
               onset_velocity=relative(pV[0],onset_V[-1]),
               extension_position=relative(eB[0],pB[-1]),
               extension_velocity=relative(eV[0],pV[-1]))
    if max(joins.values())>CRITERIA['join_relative']:raise ValueError('discontinuous physical source join')
    B=np.vstack([pB,eB[1:]]);V=np.vstack([pV,eV[1:]])
    if rate not in [64000,128000]:raise ValueError('matched 64/128k capture required')
    if rate==64000:
        t=np.arange(len(B))/rate;h=CubicHermiteSpline(t,B,V,axis=0)
        u=np.arange(2*(len(B)-1)+1)/128000
        B,V=h(u),h(u,1)
    B=np.vstack([onset_B[:-1],B]);V=np.vstack([onset_V[:-1],V])
    return np.arange(len(B))/128000,B,V,joins


def extend(pilot,state,rate,duration):
    steps=round(rate*duration)
    if abs(steps/rate-duration)>1e-14:raise ValueError('continuation duration grid')
    B=[pilot.board_map@state[0]];V=[pilot.board_map@state[1]]
    pilot.oscillator_functions(.5/rate);pilot._air_prepare(1/rate)
    initial=pilot.energy(state)['total_j'];material=port=relax=0.;iterations=fallback=0
    start=last=time.monotonic()
    for i in range(steps):
        state,stats=pilot.step(state,1/rate,True)
        material+=stats['material_loss_j'];port+=stats['air_port_work_j'];relax+=stats['radiation_loss_j']
        iterations=max(iterations,stats['mechanical_iterations']);fallback+=stats['full_tangent_fallbacks']
        B.append(pilot.board_map@state[0]);V.append(pilot.board_map@state[1])
        now=time.monotonic()
        if now-last>=20 or i+1==steps:
            print(f'{rate}Hz extension {(i+1)/steps:.1%}, source t={.25+(i+1)/rate:.6f}s, dynamics={now-start:.1f}s',flush=True);last=now
    elapsed=time.monotonic()-start
    final=pilot.energy(state)['total_j']
    metrics=dict(rate_hz=rate,extension_duration_s=duration,dynamics_s=elapsed,
        initial_energy_j=initial,final_energy_j=final,material_heat_j=material,
        air_work_j=port,relaxation_heat_j=relax,
        radiation_inferred_from_storage_j=initial-final-material,
        maximum_mechanical_iterations=iterations,full_tangent_fallbacks=fallback,
        energy_scope='Full storage evaluated at segment endpoints; long angular radiation inferred, not independently integrated.')
    return state,dict(time_s=.25+np.arange(len(B))/rate,board_position=np.array(B),board_velocity=np.array(V)),metrics


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['input','asset','coefficients','contact-report','contact-data','prefix-report','prefix-data',
                 'kernel','kernel-report','output','audio','report']:
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--duration',type=float,default=1.)
    p.add_argument('--gain',type=float,default=.25)
    args=p.parse_args();args.rate=768000
    if not np.isfinite(args.duration) or args.duration<=.25 or not np.isfinite(args.gain) or args.gain<=0:
        p.error('duration >250ms and positive gain required')
    if any(path.exists() for path in [args.output,args.audio,args.report]):raise ValueError('preserve measured outputs')
    started=time.monotonic();parent=json.loads(args.prefix_report.read_text());mic=json.loads(args.kernel_report.read_text())
    if parent.get('solver')!='polynomial' or not parent['gates'] or not all(parent['gates'].values()):raise ValueError('validated polynomial prefix required')
    if parent['duration_s']!=.246 or parent['start_after_onset_s']!=.004:raise ValueError('expected 250ms prefix')
    for name in ['concert_grand_exponential_coast.py','concert_grand_polynomial_coast.py']:
        if digest(Path(__file__).with_name(name))!=parent['source_sha256'][name]:raise ValueError('prefix solver changed')
    if digest(args.contact_report)!=mic['contact_report_sha256'] or not mic['all_gates_passed'] or digest(args.kernel)!=mic['kernel_sha256']:
        raise ValueError('matched validated readout required')
    for path in [args.asset,args.coefficients,args.contact_report,args.kernel,args.kernel_report]:
        if digest(path)!=parent['inputs_sha256'][str(path)]:raise ValueError('prefix physical inputs changed')
    frozen,old,_,_,trace,onset_B,onset_V=onset.initialize(args)
    if len(onset_B)!=513 or len(onset_V)!=513:raise ValueError('128k frozen onset capture')
    pilot=PolynomialCoast(old.system,old.air)
    with np.load(args.kernel) as d:kernel=d['kernel']
    args.output.mkdir(parents=True)
    result=dict(duration_s=args.duration,criteria=CRITERIA,rates_hz=[64000,128000],cases={},gates={},
        initialization_s=time.monotonic()-started,continuation_from_s=.25,
        prefix_report_sha256=digest(args.prefix_report),source_provenance=frozen['provenance'],
        inputs_sha256={str(path):digest(path) for path in [args.contact_report,args.kernel,args.kernel_report,args.prefix_report]},
        paths=dict(prefix_data=str(args.prefix_data),extension_data=str(args.output),audio=str(args.audio)),
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in ['render-concert-grand-exponential-note.py',
            'concert_grand_exponential_coast.py','concert_grand_polynomial_coast.py','concert_grand_cell_audio.py']},
        scope='One vertical C4/124 string; same nonlinear parameters and full live states. Full-note rate comparison, no identification or factory change.')
    write(args.report,result);notes={};pressures={}
    for rate in result['rates_hz']:
        prefix_path=args.prefix_data/f'Coast-{rate}.npz'
        if digest(prefix_path)!=parent['cases'][str(rate)]['trajectory_sha256']:raise ValueError('prefix trajectory changed')
        with np.load(prefix_path) as d:prefix={k:d[k] for k in ['time_s','board_position','board_velocity']+STATE_KEYS}
        state=tuple(prefix[k] for k in STATE_KEYS)
        final,extension,metrics=extend(pilot,state,rate,args.duration-.25)
        if abs(metrics['initial_energy_j']-parent['cases'][str(rate)]['final_energy_j'])>1e-12:raise ValueError('stored energy join changed')
        path=args.output/f'Extension-{rate}.npz'
        np.savez_compressed(path,**extension,**dict(zip(STATE_KEYS,final)))
        metrics.update(prefix_sha256=digest(prefix_path),extension_sha256=digest(path))
        t,B,V,joins=joined_trace(onset_B,onset_V,prefix,extension,rate)
        if abs(t[-1]-args.duration)>1e-14:raise ValueError('source duration changed')
        pressure,f,P,size=pressure_fft(t,B,V,kernel)
        metrics['join_relative']=joins
        metrics['physical_pressure_peak_pa']=float(np.max(abs(pressure)))
        metrics['total_since_4ms_material_heat_j']=parent['cases'][str(rate)]['material_heat_j']+metrics['material_heat_j']
        metrics['total_since_4ms_radiation_inferred_j']=parent['cases'][str(rate)]['initial_energy_j']-metrics['final_energy_j']-metrics['total_since_4ms_material_heat_j']
        result['cases'][str(rate)]=metrics;notes[rate]=(t,B,V);pressures[rate]=P
        if rate==128000:selected=(pressure,f,size)
        write(args.report,result);print('case',rate,json.dumps(metrics),flush=True)
    t,B,V=notes[128000];_,coarse_B,coarse_V=notes[64000]
    pressure,f,size=selected
    comparisons=dict(pressure_relative=spectral_relative(f,pressures[64000],pressures[128000],60,6000),
        board_position_relative=relative(coarse_B,B),board_velocity_relative=relative(coarse_V,V))
    cf,CA=hermite_fft(t[::2],B[::2],V[::2],size//2)
    if not np.allclose(f[:len(cf)],cf,rtol=1e-12,atol=1e-10):raise ValueError('matched capture FFT duration')
    H=rfft(kernel,n=size,axis=0);CP=np.einsum('fcm,fm->fc',H[:len(CA)],CA)
    capture_error=spectral_relative(cf,CP,pressures[128000][:len(cf)],60,6000)
    result.update(comparisons=comparisons,capture_pressure_relative=capture_error,selected_render_rate_hz=128000)
    result['gates']=dict(mechanical_rate_pressure=comparisons['pressure_relative']<CRITERIA['pressure_relative'],
        mechanical_rate_board=max(comparisons['board_position_relative'],comparisons['board_velocity_relative'])<CRITERIA['board_relative'],
        capture_pressure=capture_error<CRITERIA['capture_pressure'],
        positive_loss_residual=all(c['radiation_inferred_from_storage_j']>=-CRITERIA['storage_loss_negative_relative']*c['initial_energy_j'] for c in result['cases'].values()))
    result['all_gates_passed']=all(result['gates'].values())
    write(args.report,result)
    if not result['all_gates_passed']:raise AssertionError('long-note measurement failed; audio not promoted')
    pressure=pressure[:len(t)+len(kernel)-1].copy()
    fade=min(round(.008*128000),len(pressure))
    pressure[-fade:]*=(.5+.5*np.cos(np.linspace(0,np.pi,fade)))[:,None]
    audio=resample_poly(pressure*args.gain,3,8,axis=0);peak=float(np.max(abs(audio)))
    if peak>=.98:raise ValueError('common listening gain clips')
    audio=np.vstack([np.zeros((4800,2)),audio,np.zeros((14400,2))]).astype(np.float32)
    args.audio.parent.mkdir(parents=True,exist_ok=True);wavfile.write(args.audio,48000,audio)
    result.update(audio_rate_hz=48000,audio_channels=2,audio_peak=peak,audio_seconds=len(audio)/48000,
        audition_gain_pa_to_full_scale=args.gain,terminal_edit_fade_s=.008,
        audio_sha256=digest(args.audio),total_new_work_s=time.monotonic()-started,
        unchecked=['59->90 structural convergence','independent long angular radiation integral','identified geometry/losses',
                   'unison','release/action/pedal','other notes/velocities','runtime integration'])
    write(args.report,result);print('saved',str(args.audio),json.dumps(dict(gates=result['gates'],comparisons=comparisons,audio_peak=peak)),flush=True)


if __name__=='__main__':main()
