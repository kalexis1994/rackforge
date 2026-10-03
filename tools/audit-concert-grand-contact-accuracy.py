"""Independent time-integration checks of the resolved Euler contact kernel."""
import argparse,json,time
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from concert_grand_contact_study import highest_frequency,solve_contact

spec=spec_from_file_location('resolved_contact_audit',Path(__file__).with_name('audit-concert-grand-resolved-restrike.py'))
audit=module_from_spec(spec);spec.loader.exec_module(audit)


def difference(q,p,qr,pr,core):
    stiffness=np.r_[(np.pi*np.arange(1,core.nl+1)/core.wire.length)**2*core.wire.young/core.wire.density,
                     np.diag(core.kt),np.diag(core.kt),0.]
    error=np.dot(stiffness,(q-qr)**2)+np.dot(p-pr,p-pr)
    reference=np.dot(stiffness,qr*qr)+np.dot(pr,pr)
    return float(np.sqrt(error/reference))


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--input',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    system,speed=audit.load(args.input,60,124,12);q0,p0=system.initial_state(speed);n=system.size;duration=.003
    started=time.monotonic()
    coarse,coarse_metrics,coarse_trace=solve_contact(system,q0,p0,duration=duration,
        samples=int(np.ceil(8*highest_frequency(system)*duration))+1,rtol=1e-9,points_per_cycle=12)
    fine,fine_metrics,fine_trace=solve_contact(system,q0,p0,duration=duration,
        samples=int(np.ceil(8*highest_frequency(system)*duration))+1,rtol=1e-11,points_per_cycle=24)
    ref_q,ref_p=fine.y[:n,-1],fine.y[n:2*n,-1]
    error=difference(coarse.y[:n,-1],coarse.y[n:2*n,-1],ref_q,ref_p,system.string)
    report=dict(scope='C4 velocity124, Euler bending, 12 transverse/24 axial, 3ms; time accuracy, not audible fidelity',
        dop853_refinement_weighted_state_error=error,dop853_reference_balance_error=fine_metrics['maximum_balance_relative_error'],avf=[])
    assert error<1e-7
    for rate in (705600,1411200,2822400):
        steps=round(duration*rate);dt=duration/steps;q,p=q0.copy(),p0.copy();initial=system.energy(q,p);drift=0.;newton=0
        for _ in range(steps):
            q,p,stats=system.step(q,p,dt)
            drift=max(drift,abs(system.energy(q,p)-initial)/initial);newton=max(newton,stats['newton_iterations'])
        err=difference(q,p,ref_q,ref_p,system.string)
        report['avf'].append(dict(rate_hz=rate,dt_s=dt,weighted_state_error=err,maximum_balance_relative_error=drift,
            newton_iterations_max=newton,hammer_position_error_m=float(abs(q[-1]-ref_q[-1])/np.sqrt(system.felt.hammer_mass))))
        assert drift<1e-8
        print(rate,'state error',err,'balance',drift,flush=True)
        report['elapsed_s']=time.monotonic()-started
        (args.output/'accuracy.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    errors=[r['weighted_state_error'] for r in report['avf']]
    report['error_refinement_ratios']=[errors[i]/errors[i+1] for i in range(2)]
    assert all(3.5<x<4.5 for x in report['error_refinement_ratios'])
    (args.output/'accuracy.json').write_text(json.dumps(report,indent=2),encoding='utf-8')


if __name__=='__main__':main()
