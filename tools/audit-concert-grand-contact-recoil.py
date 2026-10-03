"""Distinguish loss of compression from separation of a recoiling hammer."""
import argparse,json
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from concert_grand_contact_study import from_motion,solve_contact,solve_free

spec=spec_from_file_location('resolved_contact_audit',Path(__file__).with_name('audit-concert-grand-resolved-restrike.py'))
audit=module_from_spec(spec);spec.loader.exec_module(audit)


def recoiling_separation(result,system):
    n=system.size
    for i,(t,state) in enumerate(zip(result.t_events[0],result.y_events[0])):
        if t>1e-8 and state[2*n-1]<0:return i,float(t),state
    raise RuntimeError('no separation with a returning hammer in the observation window')


def phase_controls(args):
    report=dict(scope='Diagnostic of forward-going first separation versus returning-hammer separation, no measured backcheck',cases={})
    for note in (60,72):
        system,speed=audit.load(args.input,note,124,12,loss=.1)
        q,p=system.initial_state(speed);first,_,_=solve_contact(system,q,p)
        n=system.size;qs,ps=first.y[:n-1,-1],first.y[n:2*n-1,-1]
        f0=np.sqrt(system.string.kt[0,0])/(2*np.pi);times=.010+np.arange(4)/(4*f0)
        free=solve_free(system.string,qs,ps,times)
        # Offset3 was selected because the first audit measured recontacts.
        next_system,next_speed=audit.load(args.input,note,60,12,loss=.1)
        q,p=from_motion(next_system,free.y[:n-1,3],free.y[n-1:,3],next_speed)
        result,metrics,trace=solve_contact(next_system,q,p)
        i,t,state=recoiling_separation(result,next_system);new_q,new_p=state[:n],state[n:2*n]
        initial=next_system.string.energy(q[:-1],p[:-1]);out=.5*new_p[-1]**2;loss=state[-1]
        changed=next_system.string.energy(new_q[:-1],new_p[:-1])-initial
        first_event=next(j for j,v in enumerate(result.t_events[0]) if v>1e-8)
        first_state=result.y_events[0][first_event]
        entry=dict(first_separation_time_s=metrics['contact_time_s'],returning_separation_time_s=t,
            first_separation_hammer_speed_m_s=float(first_state[2*n-1]/np.sqrt(next_system.felt.hammer_mass)),
            returning_separation_hammer_speed_m_s=float(new_p[-1]/np.sqrt(next_system.felt.hammer_mass)),
            separation_event_index=i,first_separation_delta_string_j=metrics['string_energy_at_separation_j']-initial,
            returning_separation_delta_string_j=changed,outgoing_hammer_energy_j=float(out),heat_j=float(loss),
            input_energy_j=metrics['incoming_hammer_energy_j'],balance_error_j=float(changed+out+loss-metrics['incoming_hammer_energy_j']))
        assert entry['first_separation_hammer_speed_m_s']>0 and entry['returning_separation_hammer_speed_m_s']<0
        assert abs(entry['balance_error_j'])<1e-10
        report['cases'][str(note)]=entry
        audit.save_trace(args.output/f'{note}-offset3.csv',trace)
        np.savez(args.output/f'{note}-returning-state.npz',q=new_q[:-1],p=new_p[:-1])
        (args.output/'recoil.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        print(note,entry,flush=True)


def sequence(args):
    first,_=audit.load(args.input,60,60,12,loss=.1)
    qs=np.zeros(first.string.size);ps=qs.copy();incoming=removed=heat=0.
    report=dict(scope='C4 8 relaunches every10ms; ideal removal at returning-hammer separation, not a calibrated action',strikes=[])
    for index,velocity in enumerate((60,124,60,124,124,60,124,60)):
        system,speed=audit.load(args.input,60,velocity,12,loss=.1);q,p=from_motion(system,qs,ps,speed)
        result,metrics,_=solve_contact(system,q,p)
        event_index,t,state=recoiling_separation(result,system);n=system.size
        qs,ps=state[:n-1].copy(),state[n:2*n-1].copy()
        outgoing=.5*state[2*n-1]**2
        incoming+=metrics['incoming_hammer_energy_j'];removed+=outgoing;heat+=state[-1]
        assert t<.010
        free=solve_free(system.string,qs,ps,[.010-t]);qs,ps=free.y[:n-1,-1],free.y[n-1:,-1]
        remaining=system.string.energy(qs,ps)
        entry=dict(index=index,velocity=velocity,returning_separation_time_s=t,
            returning_hammer_speed_m_s=float(state[2*n-1]/np.sqrt(system.felt.hammer_mass)),event_index=event_index,
            cumulative_input_j=incoming,cumulative_removed_hammer_j=float(removed),cumulative_heat_j=float(heat),
            remaining_string_energy_j=remaining,cumulative_balance_relative_error=float(abs(remaining-(incoming-removed-heat))/incoming))
        assert entry['cumulative_balance_relative_error']<1e-7 and remaining<=incoming*(1+1e-7)
        report['strikes'].append(entry)
        (args.output/'sequence-recoil.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        print(index,velocity,'energy',remaining,'return',entry['returning_hammer_speed_m_s'],flush=True)
    np.savez(args.output/'final-state.npz',q=qs,p=ps)


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--input',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--phase',choices=['controls','sequence'],required=True)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    (phase_controls if args.phase=='controls' else sequence)(args)


if __name__=='__main__':main()
