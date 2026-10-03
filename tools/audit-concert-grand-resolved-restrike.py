"""Modal resolution and energy-accounted restrikes of a fixed-end SI wire.

No per-note output gain, factory changes, acoustic fit or real action model.
"""
import argparse,csv,hashlib,json,time
from fractions import Fraction
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from scipy.signal import resample_poly
from concert_grand_coupled_contact import HammerString
from concert_grand_contact_study import CosineString,from_motion,highest_frequency,solve_contact,solve_free

spec=spec_from_file_location('previous_contact_audit',Path(__file__).with_name('audit-concert-grand-coupled-contact.py'))
previous=module_from_spec(spec);spec.loader.exec_module(previous)


def load(folder,note,velocity,nt,nl=None,bending=True,loss=0.):
    old,speed,_=previous.load_case(folder,note,velocity,rate_damping=loss)
    core=CosineString(old.string.wire,transverse=nt,axial=2*nt if nl is None else nl,bending=bending)
    return HammerString(core,old.felt),speed


def bands(trace):
    # The diagnostic reaction is sampled above 2*(f_axial+f_transverse)
    # and 6*f_transverse. Filter before reducing to the audible band.
    fs=1/(trace[1,0]-trace[0,0]);ratio=Fraction(44100/fs).limit_denominator(100000)
    reduced=resample_poly(trace[:,[2,3]],ratio.numerator,ratio.denominator,axis=0)
    return reduced


def final_weighted_error(coarse_state,coarse,fine_state,fine):
    n=fine.size;a=np.zeros(n);b=np.zeros(n)
    for cs,fs in ((coarse.su,fine.su),(coarse.sv,fine.sv),(coarse.sw,fine.sw)):
        length=len(coarse_state[0][cs]);a[fs.start:fs.start+length]=coarse_state[0][cs];b[fs.start:fs.start+length]=coarse_state[1][cs]
    diag=np.r_[(np.pi*np.arange(1,fine.nl+1)/fine.wire.length)**2*fine.wire.young/fine.wire.density,
               np.diag(fine.kt),np.diag(fine.kt)]
    dq=a-fine_state[0];dp=b-fine_state[1]
    numerator=np.dot(diag,dq*dq)+np.dot(dp,dp)
    denominator=np.dot(diag,fine_state[0]**2)+np.dot(fine_state[1],fine_state[1])
    return float(np.sqrt(numerator/denominator))


def save_trace(path,trace):
    # Report a compact, filtered force diagnostic, not an aliased raw force.
    reduced=bands(trace)
    with path.open('w',newline='') as f:
        writer=csv.writer(f);writer.writerow(['time_s','contact_force_44100_band_n','end_reaction_44100_band_n'])
        writer.writerows(np.column_stack([np.arange(len(reduced))/44100,reduced]))


def resolution(args):
    report=dict(scope='Fixed-end steel SI contact with Euler bending; modal convergence, not piano identification',
        thresholds=dict(impulse=.002,string_energy=.002,contact_time=.001,contact_force_band=.01,end_reaction_band=.01,final_weighted_state=.01),
        cases={});started=time.monotonic()
    cases=[(60,124)] if args.quick else [(n,v) for n in (60,72,84,90) for v in (60,124)]
    for note,velocity in cases:
        key=f'{note}-{velocity}';runs={};comparison=[];prior=None
        for nt in args.modes:
            system,speed=load(args.input,note,velocity,nt);q,p=system.initial_state(speed)
            samples=max(1201,int(np.ceil(8*highest_frequency(system)*.004))+1)
            result,metrics,trace=solve_contact(system,q,p,duration=.004,samples=samples,rtol=1e-9)
            assert metrics['maximum_balance_relative_error']<1e-7
            n=system.size;state=(result.y[:n-1,-1],result.y[n:2*n-1,-1]);filtered=bands(trace)
            runs[str(nt)]=metrics
            save_trace(args.output/f'{key}-T{nt}-L{2*nt}.csv',trace)
            if prior:
                pc,pm,ps,pband=prior
                ratios={label:abs(pm[field]/metrics[field]-1) for label,field in
                        [('impulse','contact_impulse_n_s'),('string_energy','string_energy_at_separation_j'),('contact_time','contact_time_s')]}
                for index,label in enumerate(['contact_force_band','end_reaction_band']):
                    count=min(len(pband),len(filtered))
                    ratios[label]=float(np.linalg.norm(pband[:count,index]-filtered[:count,index])/np.linalg.norm(filtered[:count,index]))
                ratios['final_weighted_state']=final_weighted_error(ps,pc,state,system.string)
                ratios['from_transverse']=pc.nt;ratios['to_transverse']=nt
                ratios['passes']=all(ratios[k]<limit for k,limit in report['thresholds'].items())
                comparison.append(ratios)
            prior=(system.string,metrics,state,filtered)
            print(key,'T',nt,'L',2*nt,'E',metrics['string_energy_at_separation_j'],'balance',metrics['maximum_balance_relative_error'],flush=True)
            np.savez(args.output/f'{key}-T{nt}-L{2*nt}.npz',q=state[0],p=state[1])
            report['cases'][key]=dict(runs=runs,refinements=comparison)
            report['elapsed_s']=time.monotonic()-started
            (args.output/'resolution.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
            if nt>=48 and comparison and comparison[-1]['passes']:break
    return report


def restrikes(args):
    report=dict(scope='First touch of a relaunched SI hammer on preserved string motion, no damper/bridge/backcheck trajectory',cases={})
    for note in ((60,) if args.quick else (60,72)):
        nt=args.restrike_modes;system,speed=load(args.input,note,124,nt,loss=.1)
        q,p=system.initial_state(speed);first,first_metrics,_=solve_contact(system,q,p,rtol=1e-10)
        n=system.size;qs,ps=first.y[:n-1,-1],first.y[n:2*n-1,-1]
        f0=np.sqrt(system.string.kt[0,0])/(2*np.pi)
        # These are time offsets of a quarter fundamental period, not a claim
        # that the complete nonlinear state has a single phase.
        times=.010+np.arange(4)/(4*f0)
        free=solve_free(system.string,qs,ps,times)
        free_drift=max(abs(system.string.energy(free.y[:n-1,i],free.y[n-1:,i])-system.string.energy(qs,ps)) for i in range(4))
        assert free_drift/system.string.energy(qs,ps)<1e-7
        for velocity in (60,124):
            next_system,next_speed=load(args.input,note,velocity,nt,loss=.1)
            assert system.string.wire==next_system.string.wire
            for phase in range(4):
                prior_q,prior_p=free.y[:n-1,phase],free.y[n-1:,phase]
                q,p=from_motion(next_system,prior_q,prior_p,next_speed)
                initial_string=next_system.string.energy(prior_q,prior_p)
                initialization_error=abs(next_system.energy(q,p)-initial_string-.5*next_system.felt.hammer_mass*next_speed**2)
                result,metrics,trace=solve_contact(next_system,q,p,rtol=1e-10,
                    samples=int(np.ceil(8*highest_frequency(next_system)*.006))+1)
                metrics.update(initialization_energy_error_j=initialization_error,free_balance_error_j=float(free_drift),
                    delta_string_energy_j=metrics['string_energy_at_separation_j']-initial_string,
                    first_contact=first_metrics,quarter_period_offset=phase,elapsed_after_first_contact_s=.006+float(times[phase]))
                assert metrics['maximum_balance_relative_error']<1e-7 and initialization_error<1e-12
                key=f'{note}-{velocity}-offset{phase}';report['cases'][key]=metrics
                save_trace(args.output/f'{key}.csv',trace)
                print(key,'string change',metrics['delta_string_energy_j'],'contacts',metrics['actual_contact_segments'],flush=True)
                (args.output/'restrikes.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    return report


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--input',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--phase',choices=['resolution','restrikes'],required=True);parser.add_argument('--quick',action='store_true')
    parser.add_argument('--modes',type=int,nargs='+',default=[12,24,48,72]);parser.add_argument('--restrike-modes',type=int,default=12)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    (resolution if args.phase=='resolution' else restrikes)(args)


if __name__=='__main__':main()
