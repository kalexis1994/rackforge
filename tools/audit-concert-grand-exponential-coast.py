"""Matched nonlinear C4 coast rate/pressure/cost study for AAVF + passive air."""
import argparse,json,time,hashlib
from importlib.util import spec_from_file_location,module_from_spec
from pathlib import Path
import numpy as np
from scipy.interpolate import CubicHermiteSpline
from concert_grand_exponential_coast import ExponentialCoast
from concert_grand_polynomial_coast import PolynomialCoast
from concert_grand_cell_audio import kernel_transfer
from concert_grand_cell_observer import acceleration_spectrum,spectral_relative

spec=spec_from_file_location('frozen_physical_onset',Path(__file__).with_name('render-concert-grand-physical-note.py'))
onset=module_from_spec(spec);spec.loader.exec_module(onset)
CRITERIA=dict(pressure_relative=.01,board_relative=.01,storage_loss_negative_relative=1e-9)


def sha(path):
    with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def write(path,data):
    path.parent.mkdir(parents=True,exist_ok=True);path.write_text(json.dumps(data,indent=2,allow_nan=False),encoding='utf-8')
def relative(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-30))


def run(pilot,state,rate,duration):
    state=tuple(a.copy() for a in state);steps=round(rate*duration);capture=min(rate,128000)
    if rate%capture or abs(steps/rate-duration)>1e-14:raise ValueError('duration/capture grid')
    stride=rate//capture;core=pilot.system.string;B=[pilot.board_map@state[0]];V=[pilot.board_map@state[1]]
    pilot.oscillator_functions(1/(2*rate));pilot._air_prepare(1/rate)
    E0=pilot.energy(state)['total_j'];material=port=relax=0.;maximum=fallback=0
    start=time.perf_counter()
    for i in range(steps):
        state,stats=pilot.step(state,1/rate,True)
        material+=stats['material_loss_j'];port+=stats['air_port_work_j'];relax+=stats['radiation_loss_j']
        maximum=max(maximum,stats['mechanical_iterations']);fallback+=stats['full_tangent_fallbacks']
        if (i+1)%stride==0:B.append(pilot.board_map@state[0]);V.append(pilot.board_map@state[1])
    dynamics=time.perf_counter()-start
    final=pilot.energy(state)
    result=dict(rate_hz=rate,duration_s=duration,capture_hz=capture,dynamics_s=dynamics,
        dynamics_s_per_audio_s=dynamics/duration,initial_energy_j=E0,final_energy_j=final['total_j'],
        material_heat_j=material,air_work_j=port,relaxation_heat_j=relax,
        radiation_inferred_from_storage_j=E0-final['total_j']-material,
        maximum_mechanical_iterations=maximum,full_tangent_fallbacks=fallback,
        storage_scope='Long wave power not independently integrated here. Six small-fixture tests separately close the full radiation/energy ledger.')
    return state,result,np.arange(len(B))/capture,np.array(B),np.array(V)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['input','asset','coefficients','contact-report','contact-data','kernel','kernel-report','reference-trajectory','reference-report','output','report']:
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--rates',type=int,nargs='+',default=[64000,128000,256000,512000])
    p.add_argument('--duration',type=float,default=.012)
    p.add_argument('--solver',choices=['gauss','polynomial'],default='gauss')
    args=p.parse_args();args.rate=768000
    if len(args.rates)<2 or any(r<=0 for r in args.rates) or sorted(set(args.rates))!=args.rates or not np.isfinite(args.duration) or args.duration<=0:p.error('ordered distinct positive rates and duration required')
    if args.report.exists() or args.output.exists():raise ValueError('preserve measured experiment')
    frozen,original,_,state,*_=onset.initialize(args)
    reference=json.loads(args.reference_report.read_text());readout=json.loads(args.kernel_report.read_text())
    if sha(args.reference_trajectory)!=reference['raw_sha256'] or sha(args.kernel)!=readout['kernel_sha256'] or not readout['all_gates_passed']:
        raise ValueError('validated frozen trajectory/kernel required')
    if reference['onset_report_sha256']!=sha(args.contact_report) or readout['contact_report_sha256']!=sha(args.contact_report):raise ValueError('matched onset required')
    pilot=(PolynomialCoast if args.solver=='polynomial' else ExponentialCoast)(original.system,original.air)
    with np.load(args.kernel) as data:kernel=data['kernel']
    frequency=np.arange(60.,6001.,20.);H=kernel_transfer(kernel,128000,frequency)
    with np.load(args.reference_trajectory) as data:
        mask=(data['time_s']>=.004-1e-16)&(data['time_s']<=.004+args.duration+1e-15)
        old_t=data['time_s'][mask];old_B=data['board_position'][mask];old_V=data['board_velocity'][mask]
    old_t=old_t-old_t[0]
    if abs(old_t[-1]-args.duration)>1e-12:raise ValueError('reference duration not covered')
    project=lambda A:np.einsum('fcm,fm->fc',H,A)
    old_P=project(acceleration_spectrum(old_t,old_B,old_V,frequency))
    args.output.mkdir(parents=True)
    result=dict(criteria=CRITERIA,source_provenance=frozen['provenance'],duration_s=args.duration,
        start_after_onset_s=.004,rates_hz=args.rates,solver=args.solver,cases={},comparisons={},gates={},
        inputs_sha256={str(path):sha(path) for path in [args.asset,args.coefficients,args.contact_report,args.kernel,args.kernel_report,args.reference_trajectory,args.reference_report]},
        method='AAVF mechanical half steps, exact quadratic phases, passive kinetic/air midpoint full step; no modal or acoustic-state reduction.',
        source_sha256={name:sha(Path(__file__).with_name(name)) for name in ['audit-concert-grand-exponential-coast.py','concert_grand_exponential_coast.py','concert_grand_polynomial_coast.py']},
        scope='Single C4/124 prefix after the validated cold onset. Conserved energy does not imply waveform convergence. Longer hold and held-out notes pending.')
    trajectories={};pressures={}
    write(args.report,result)
    for rate in args.rates:
        print('case',rate,flush=True)
        final,metrics,t,B,V=run(pilot,state,rate,args.duration)
        path=args.output/f'Coast-{rate}.npz'
        np.savez_compressed(path,time_s=t,board_position=B,board_velocity=V,q=final[0],v=final[1],wave_state_final=final[2],relaxation_state_final=final[3])
        metrics['trajectory_sha256']=sha(path)
        P=project(acceleration_spectrum(t,B,V,frequency))
        metrics['pressure_difference_from_midpoint_768k']=spectral_relative(frequency,P,old_P,60,6000)
        result['cases'][str(rate)]=metrics;trajectories[rate]=(t,B,V);pressures[rate]=P
        write(args.report,result);print(json.dumps(metrics),flush=True)
    finest=args.rates[-1];t,B,V=trajectories[finest];interpolator=CubicHermiteSpline(t,B,V,axis=0)
    for rate in args.rates[:-1]:
        u,Q,W=trajectories[rate]
        comparison=dict(pressure_relative=spectral_relative(frequency,pressures[rate],pressures[finest],60,6000),
            board_position_relative=relative(Q,interpolator(u)),board_velocity_relative=relative(W,interpolator(u,1)))
        comparison['passed']=comparison['pressure_relative']<CRITERIA['pressure_relative'] and max(comparison['board_position_relative'],comparison['board_velocity_relative'])<CRITERIA['board_relative']
        result['comparisons'][str(rate)]=comparison
    next_fine=args.rates[-2]
    result['gates']['finest_pair_prefix']=result['comparisons'][str(next_fine)]['passed']
    result['gates']['positive_loss_residual']=all(c['radiation_inferred_from_storage_j']>=-CRITERIA['storage_loss_negative_relative']*c['initial_energy_j'] for c in result['cases'].values())
    passed=[r for r in args.rates[:-1] if result['comparisons'][str(r)]['passed']]
    result['candidate_rate_hz']=min(passed) if passed and all(result['gates'].values()) else None
    result['decision']='Rate sensitivity on the measured interval only. Any longer duration, other note or contact must be checked independently.'
    write(args.report,result);print('summary',json.dumps(dict(gates=result['gates'],comparisons=result['comparisons'],candidate=result['candidate_rate_hz'])),flush=True)


if __name__=='__main__':main()
