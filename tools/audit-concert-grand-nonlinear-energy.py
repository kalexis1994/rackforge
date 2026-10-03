"""Independent energy, reciprocity and time-convergence gates for P3.

Controlled plain wire and deliberate 17:1 axial/transverse resonance; no
Steinway identification, microphone fit or factory integration. All energies
are SI joules. A feed-forward axial ablation demonstrates unaccounted work.
"""
import argparse
from dataclasses import asdict
import hashlib
import json
from pathlib import Path

import numpy as np
from scipy.integrate import solve_ivp

from concert_grand_nonlinear_string import Wire,String


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    a=parser.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    young,density,f0=200e9,7850.,261.6255653
    length=np.sqrt(young/density)/(2*f0*17)
    wire=Wire(length,700/(2*length*f0)**2)
    model=String(wire,transverse=12,axial=2)
    root=np.sqrt(wire.modal_mass)
    q=np.zeros(model.size);p=np.zeros_like(q)
    p[model.nl+7]=1.2*root;p[model.nl+8]=1.2*root
    p[model.nl]=.3*root;p[model.nl+model.nt+1]=.4*root
    initial=model.energy(q,p)
    report=dict(policy=__doc__,wire=asdict(wire),modal_mass_kg=wire.modal_mass,
                transverse_modes=12,axial_modes=2,initial_energy_j=initial,checks={},runs={})
    rng=np.random.default_rng(431)
    qa=rng.normal(0,2e-6,model.size);qb=rng.normal(0,2e-6,model.size)
    g,jac=model.avf(qa,qb)
    delta=model.potential(qb)-model.potential(qa)
    report['checks']['discrete_gradient_work_relative_error']=float(abs(g@(qb-qa)-delta)/max(abs(delta),1e-30))
    assert report['checks']['discrete_gradient_work_relative_error']<1e-10
    # Directional finite differences verify a gradient derived independently
    # from the potential and reciprocal mixed derivatives of T/L motion.
    direction=rng.normal(size=model.size);direction/=np.linalg.norm(direction)
    epsilon=1e-9
    analytic,hessian=model.gradient_hessian(qa)
    finite=(model.potential(qa+epsilon*direction)-model.potential(qa-epsilon*direction))/(2*epsilon)
    gradient_error=abs(finite-analytic@direction)/max(abs(finite),1e-30)
    finite_h=(model.gradient_hessian(qa+epsilon*direction,False)-model.gradient_hessian(qa-epsilon*direction,False))/(2*epsilon)
    hessian_error=np.linalg.norm(finite_h-hessian@direction)/np.linalg.norm(finite_h)
    report['checks'].update(gradient_relative_error=float(gradient_error),hessian_relative_error=float(hessian_error),
                            reciprocity_hessian_error=float(abs(hessian-hessian.T).max()))
    assert gradient_error<1e-7 and hessian_error<1e-7
    # Independent fine spatial energy by direct physical sine derivatives.
    xx=np.linspace(0,wire.length,4097)
    du=np.cos(xx[:,None]*np.pi*np.arange(1,3)/wire.length)*np.pi*np.arange(1,3)/wire.length/root
    dt=np.cos(xx[:,None]*np.pi*np.arange(1,13)/wire.length)*np.pi*np.arange(1,13)/wire.length/root
    ux,vx,wx=du@qa[model.su],dt@qa[model.sv],dt@qa[model.sw]
    density_energy=.5*wire.tension*(vx*vx+wx*wx)+.5*wire.axial_rigidity*(ux+.5*(vx*vx+wx*wx))**2
    spatial=float(np.trapezoid(density_energy,xx))
    report['checks']['independent_spatial_energy_relative_error']=abs(spatial-model.potential(qa))/spatial
    assert report['checks']['independent_spatial_energy_relative_error']<1e-6
    def rhs(t,state):
        return np.r_[state[model.size:],-model.gradient_hessian(state[:model.size],False)]
    duration=.012
    reference_step=1/(40*34*f0)
    independent=solve_ivp(rhs,(0,duration),np.r_[q,p],method='DOP853',rtol=2e-11,atol=1e-13,max_step=reference_step)
    assert independent.success
    ref=independent.y[:,-1]
    errors=[]
    for rate in (44100,88200,176400,352800,705600,1411200):
        steps=int(round(duration*rate));dt=duration/steps
        current_q=q.copy();current_p=p.copy();trace=[];maximum_drift=0.;max_iterations=0
        maximum_axial=0.
        for i in range(steps):
            current_q,current_p,loss,iterations=model.step(current_q,current_p,dt)
            energy=model.energy(current_q,current_p)
            maximum_drift=max(maximum_drift,abs(energy-initial)/initial)
            components=model.components(current_q,current_p)
            maximum_axial=max(maximum_axial,components['axial_linear_j'])
            max_iterations=max(max_iterations,iterations)
            if i%max(1,steps//300)==0:trace.append(dict(time_s=(i+1)*dt,**components))
        final=np.r_[current_q,current_p]
        # Independent reference norm in energy units; no raw q/p unit mixing.
        dq=final[:model.size]-ref[:model.size];dp=final[model.size:]-ref[model.size:]
        _,linear=model.gradient_hessian(np.zeros(model.size))
        error=np.sqrt((dp@dp+dq@linear@dq)/(2*initial))
        errors.append(float(error))
        assert maximum_drift<1e-8 and maximum_axial>1e-7*initial
        report['runs'][str(rate)]=dict(actual_dt_s=dt,maximum_relative_energy_drift=maximum_drift,
            final_reference_energy_norm_error=float(error),maximum_axial_energy_fraction=float(maximum_axial/initial),
            maximum_newton_iterations=max_iterations,trace=trace)
        print(rate,{k:v for k,v in report['runs'][str(rate)].items() if k!='trace'},flush=True)
    assert all(a>b for a,b in zip(errors,errors[1:])) and errors[-1]<.01
    report['checks']['last_time_error_reduction_ratio']=errors[-2]/errors[-1]
    current_q=q.copy();current_p=p.copy();paid=0.;max_balance=0.
    damp=np.full(model.size,2.);damp[model.su]=20.
    for i in range(441):
        before=model.energy(current_q,current_p)
        current_q,current_p,loss,_=model.step(current_q,current_p,1/44100,damp)
        paid+=loss;after=model.energy(current_q,current_p)
        assert after<=before+1e-12
        max_balance=max(max_balance,abs(after+paid-initial)/initial)
    assert max_balance<1e-8
    report['checks']['damped_energy_work_max_relative_error']=max_balance
    # Feed-forward ablation holds the transverse motion on its uncoupled
    # solution while forcing u from the full potential, omitting reaction.
    _,linear=model.gradient_hessian(np.zeros(model.size))
    wt=np.sqrt(np.diag(linear)[model.nl:])
    def ff_rhs(t,state):
        transverse=p[model.nl:]*np.sin(wt*t)/wt
        state_q=np.r_[state[:model.nl],transverse]
        return np.r_[state[model.nl:],-model.gradient_hessian(state_q,False)[model.su]]
    times=np.linspace(0,duration,1001)
    ff=solve_ivp(ff_rhs,(0,duration),np.zeros(2*model.nl),method='DOP853',t_eval=times,rtol=2e-11,atol=1e-13,max_step=reference_step)
    assert ff.success
    imbalance=[]
    for t,state in zip(times,ff.y.T):
        fq=np.r_[state[:model.nl],p[model.nl:]*np.sin(wt*t)/wt]
        fp=np.r_[state[model.nl:],p[model.nl:]*np.cos(wt*t)]
        imbalance.append((model.energy(fq,fp)-initial)/initial)
    report['checks']['feedforward_max_energy_excess_fraction']=float(max(imbalance))
    report['checks']['feedforward_max_absolute_energy_error_fraction']=float(max(abs(np.array(imbalance))))
    report['script_sha256']=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    report['core_sha256']=hashlib.sha256(Path(__file__).with_name('concert_grand_nonlinear_string.py').read_bytes()).hexdigest()
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(report['checks'],flush=True)


if __name__=='__main__':main()
