"""Audit a joint SI contact, not a production voice or a Steinway fit."""
import argparse,csv,json,time
from pathlib import Path
import numpy as np
from scipy.integrate import solve_ivp
from concert_grand_nonlinear_string import Wire,String
from concert_grand_coupled_contact import Felt,HammerString,LinearString,felt_divided_force


def gates():
    errors=[]
    for p in (1.,2.46896553,2.6137931347,3.5):
        for a,b in ((-.002,-.001),(-.001,.002),(.002,-.001),(.001,.002),(.002,.001),(.001,.001*(1+1e-9))):
            k=7.5e8;force,jac=felt_divided_force(a,b,k,p)
            work=k/(p+1)*(max(b,0)**(p+1)-max(a,0)**(p+1))
            errors.append(abs(force*(b-a)-work)/max(abs(work),k*max(a,b,0)**(p+1)*1e-6,1e-20))
            assert jac>=0
    # Finite differences of the same shared compression potential.
    core=String(Wire(.6,.008,850),transverse=4,axial=2)
    system=HammerString(core,Felt(.002,7.5e8,2.6,.125,.02))
    q,p=system.initial_state(3.);q[-1]=np.sqrt(.002)*.0003
    gradient,_=system.continuous_force(q,np.zeros_like(p));direction=np.arange(1,system.size+1,dtype=float)
    direction/=np.linalg.norm(direction);eps=1e-10
    measured=(system.potential(q+eps*direction)-system.potential(q-eps*direction))/(2*eps)
    gradient_error=abs(measured-direction@gradient)/max(abs(measured),1e-12)
    # Centered velocity losses and contact-rate heat must pay for every joule.
    damped=HammerString(core,Felt(.002,7.5e8,2.6,.125,.02,.1))
    q,p=damped.initial_state(3.);initial=damped.energy(q,p);heat=0.;drift=0.;minimum=0.
    for _ in range(1600):
        q,p,stats=damped.step(q,p,1/800000,damping=3.)
        heat+=stats['loss_j'];minimum=min(minimum,stats['loss_j'])
        drift=max(drift,abs(damped.energy(q,p)+heat-initial)/initial)
    assert max(errors)<1e-6 and gradient_error<1e-6 and drift<1e-9 and minimum>=-1e-18
    return dict(felt_work_relative_error=max(errors),shared_potential_gradient_relative_error=gradient_error,
                damped_balance_relative_error=drift,minimum_loss_j=minimum)


def load_case(folder,note,velocity,nt=12,rate_damping=0.,linear=False):
    with (folder/'transfer.csv').open() as f:
        rows=[r for r in csv.DictReader(f) if int(r['note'])==note and int(r['velocity'])==velocity and int(r['wire'])==0]
    row=rows[0]
    with (folder/'contact-end.csv').open() as f:
        contact=next(r for r in csv.DictReader(f) if int(r['note'])==note and int(r['velocity'])==velocity)
    value=lambda k:float(row[k])
    wire=Wire(value('length_m'),2*value('modal_mass_kg')/value('length_m'),value('tension_n'))
    core=(LinearString if linear else String)(wire,transverse=nt,axial=2,quadrature=max(96,nt*8),bending=False)
    felt=Felt(value('hammer_mass_per_string_kg'),float(contact['stiffness_n_per_m_p']),float(contact['exponent']),
              float(contact['strike_fraction']),float(contact['width_fraction']),rate_damping)
    return HammerString(core,felt),value('hammer_speed_m_s'),rows


def reference(system,speed,duration):
    n=system.size;q,p=system.initial_state(speed)
    def derivative(t,state):
        q,p=state[:n],state[n:2*n];gradient,heat=system.continuous_force(q,p)
        return np.r_[p,-gradient,heat]
    def separation(t,state):return system.compression(state[:n])
    separation.direction=-1;separation.terminal=False
    axial_hz=system.string.nl*np.sqrt(system.string.wire.young/system.string.wire.density)/(2*system.string.wire.length)
    result=solve_ivp(derivative,(0,duration),np.r_[q,p,0.],method='DOP853',rtol=1e-11,atol=1e-14,
                     max_step=1/(40*axial_hz),events=separation,dense_output=True)
    assert result.success
    state=result.y[:,-1]
    return result,dict(contact_time_s=float(result.t_events[0][0]),nfev=result.nfev,
        final_balance_relative_error=abs(system.energy(state[:n],state[n:2*n])+state[-1]-system.energy(q,p))/system.energy(q,p))


def run(system,speed,rate,duration,ref=None,trace_path=None):
    steps=round(rate*duration);dt=duration/steps;q,p=system.initial_state(speed)
    initial=system.energy(q,p);heat=0.;drift=0.;force_max=0.;compression_max=0.;newton=0
    separated=None;trace=[];previous_delta=0.;free_check=None
    for i in range(steps):
        q0,p0=q,p;q,p,stats=system.step(q,p,dt);heat+=stats['loss_j']
        drift=max(drift,abs(system.energy(q,p)+heat-initial)/initial)
        force_max=max(force_max,stats['force_n']);delta=system.compression(q)
        compression_max=max(compression_max,delta);newton=max(newton,stats['newton_iterations'])
        if separated is None and previous_delta>0 and delta<=0:
            string_e=system.string.energy(q[:-1],p[:-1]);head_e=.5*p[-1]**2
            # Copy the same q and p to the same free potential; no normalization.
            mismatch=abs(system.energy(q,p)-string_e-head_e)
            free_q,free_p,_,_=system.string.step(q[:-1].copy(),p[:-1].copy(),dt)
            joint_q,joint_p,_=system.step(q,p,dt)
            free_check=float(max(np.max(abs(free_q-joint_q[:-1])),np.max(abs(free_p-joint_p[:-1]))))
            separated=dict(contact_time_s=(i+previous_delta/(previous_delta-delta))*dt,
                first_separated_step_s=(i+1)*dt,string_energy_j=string_e,hammer_energy_j=float(head_e),
                heat_j=float(heat),handoff_energy_jump_j=float(mismatch),free_step_state_difference=free_check,
                components=system.string.components(q[:-1],p[:-1]))
        if trace_path and (i%max(1,steps//600)==0 or i==steps-1):
            trace.append(((i+1)*dt,delta,stats['force_n'],system.energy(q,p),heat,
                          system.string.components(q[:-1],p[:-1])['axial_linear_j']))
        previous_delta=delta
    assert separated is not None,'no separation in audit interval'
    result=dict(rate_hz=rate,dt_s=dt,initial_energy_j=initial,max_balance_relative_error=drift,
        maximum_force_n=force_max,maximum_compression_m=compression_max,newton_iterations_max=newton,
        separation=separated,final_components=system.string.components(q[:-1],p[:-1]),final_heat_j=heat)
    if ref is not None:
        n=system.size;state=ref.y[:,-1];dq=q[:-1]-state[:n-1];dp=p-state[n:2*n]
        linear=system.string.gradient_hessian(np.zeros(n-1))[1]
        error_energy=float(dq@linear@dq+dp@dp)
        reference_energy=float(state[:n-1]@linear@state[:n-1]+state[n:2*n]@state[n:2*n])
        result['energy_weighted_state_error']=float(np.sqrt(error_energy/reference_energy))
        result['hammer_position_error_relative_to_max_compression']=float(abs(q[-1]-state[n-1])/np.sqrt(system.felt.hammer_mass)/compression_max)
    if trace_path:
        with trace_path.open('w',newline='') as f:
            writer=csv.writer(f);writer.writerow(['time_s','compression_m','average_contact_force_n','energy_j','heat_j','axial_linear_j']);writer.writerows(trace)
        np.savez(trace_path.with_suffix('.npz'),q=q,p=p,dt=dt)
    assert drift<1e-8 and separated['handoff_energy_jump_j']<1e-12 and free_check<1e-10
    return result


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--input',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--quick',action='store_true');args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    started=time.monotonic();report=dict(scope='Isolated fixed-end SI contact; no bridge, microphone or real-felt identification',
        gates=gates(),assumptions=dict(young_pa=200e9,density_kg_m3=7850,transverse_per_direction=12,axial=2,
                                     bending=False,rate_damping_s_per_m=.1,output_normalization=False),cases={})
    rates=(176400,352800,705600);duration=.006
    for note,velocity in ([(60,124)] if args.quick else [(60,60),(60,124),(72,60),(72,124)]):
        name=f'{note}-{velocity}';system,speed,rows=load_case(args.input,note,velocity)
        ref,ref_report=reference(system,speed,duration);case=dict(reference=ref_report,conservative=[])
        for rate in rates:
            result=run(system,speed,rate,duration,ref,args.output/f'{name}-{rate}.csv' if rate==rates[-1] else None)
            case['conservative'].append(result);print(name,rate,'state error',result['energy_weighted_state_error'],flush=True)
        errors=[r['energy_weighted_state_error'] for r in case['conservative']]
        case['refinement_error_ratios']=[errors[i]/errors[i+1] for i in range(len(errors)-1)]
        linear,_,_=load_case(args.input,note,velocity,linear=True)
        case['linear_control']=run(linear,speed,rates[-1],duration,trace_path=args.output/f'{name}-linear.csv')
        damped,_,_=load_case(args.input,note,velocity,rate_damping=.1)
        damp_ref,damp_reference=reference(damped,speed,duration)
        case['rate_damped']=run(damped,speed,rates[-1],duration,damp_ref,args.output/f'{name}-rate-damped.csv')
        case['rate_damped_reference']=damp_reference
        # Same material/contact, more transverse modes: disclose truncation sensitivity.
        expanded,_,_=load_case(args.input,note,velocity,nt=24)
        case['modal_24_control']=run(expanded,speed,rates[-1],duration)
        base=case['conservative'][-1]['separation'];expanded_e=case['modal_24_control']['separation']
        case['modal_string_energy_change_fraction']=expanded_e['string_energy_j']/base['string_energy_j']-1
        case['modal_contact_time_change_fraction']=expanded_e['contact_time_s']/base['contact_time_s']-1
        report['cases'][name]=case
        report['elapsed_s']=time.monotonic()-started
        (args.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        print(name,'complete',round(report['elapsed_s'],1),'s',flush=True)
    print('audit complete',flush=True)


if __name__=='__main__':main()
