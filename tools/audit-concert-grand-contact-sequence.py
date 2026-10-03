"""Rapid SI relaunches with explicitly paid action input/output energy."""
import argparse,json
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from concert_grand_contact_study import from_motion,solve_contact,solve_free

spec=spec_from_file_location('resolved_contact_audit',Path(__file__).with_name('audit-concert-grand-resolved-restrike.py'))
audit=module_from_spec(spec);spec.loader.exec_module(audit)


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--input',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    first,_=audit.load(args.input,60,60,12,loss=.1)
    qs=np.zeros(first.string.size);ps=qs.copy();incoming=0.;removed=0.;heat=0.
    report=dict(scope='C4 8 relaunches every10ms, first separation then ideal action removal; a stress test, not a measured action',
                transverse=12,axial=24,rate_damping_s_per_m=.1,strikes=[])
    for index,velocity in enumerate((60,124,60,124,124,60,124,60)):
        system,speed=audit.load(args.input,60,velocity,12,loss=.1);q,p=from_motion(system,qs,ps,speed)
        result,metrics,_=solve_contact(system,q,p,rtol=1e-10,samples=1201)
        valid=next(i for i,t in enumerate(result.t_events[0]) if t>1e-8)
        event=float(result.t_events[0][valid]);state=result.y_events[0][valid];n=system.size
        qs,ps=state[:n-1].copy(),state[n:2*n-1].copy()
        incoming+=metrics['incoming_hammer_energy_j'];removed+=metrics['outgoing_hammer_energy_j'];heat+=metrics['heat_at_separation_j']
        remaining=system.string.energy(qs,ps);residual=remaining-(incoming-removed-heat)
        assert abs(residual)/incoming<1e-7 and remaining<=incoming*(1+1e-7)
        assert event<.010,'stress interval overlaps the preceding contact'
        free=solve_free(system.string,qs,ps,[.010-event]);qs,ps=free.y[:n-1,-1],free.y[n-1:,-1]
        remaining=system.string.energy(qs,ps)
        metrics.update(index=index,velocity=velocity,cumulative_input_j=incoming,cumulative_removed_hammer_j=removed,
            cumulative_heat_j=heat,remaining_string_energy_j=remaining,
            cumulative_balance_relative_error=abs(remaining-(incoming-removed-heat))/incoming)
        assert metrics['cumulative_balance_relative_error']<1e-7
        report['strikes'].append(metrics)
        (args.output/'sequence.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        print(index,velocity,'energy',remaining,'budget error',metrics['cumulative_balance_relative_error'],flush=True)
    np.savez(args.output/'final-state.npz',q=qs,p=ps)


if __name__=='__main__':main()
