"""Closed hammer/string/support work versus a forced-support ablation."""
import argparse,csv,hashlib,json,time
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from scipy.integrate import solve_ivp
from concert_grand_moving_bridge import MovingBridge,Support,BridgeHammer
import concert_grand_orthotropic_plate as plate

spec=spec_from_file_location('contact_parameters',Path(__file__).with_name('audit-concert-grand-coupled-contact.py'))
previous=module_from_spec(spec);spec.loader.exec_module(previous)


def load(folder,note,velocity,support,nt=12):
    old,speed,_=previous.load_case(folder,note,velocity)
    return BridgeHammer(MovingBridge(old.string.wire,support,transverse=nt),old.felt),speed


def solve(system,speed,duration=.004,rtol=1e-9,points=8):
    core=system.string;n=system.size;q,p=system.initial_state(speed);initial=system.energy(q,p)
    def derivative(t,state):
        Q,P=state[:n],state[n:2*n];g,heat_f=system.conservative_force(Q,P)
        acceleration=-g-system.damping@P
        z=core.physical(Q[:-1]);zd=core.physical(P[:-1]);za=core.physical(acceleration[:-1])
        heat_b=float(np.dot(core.viscous,zd[core.sb]**2))
        work_b=float(zd[core.sb]@(za[core.sb]+core.omega**2*z[core.sb]+core.viscous*zd[core.sb]))
        return np.r_[P,acceleration,heat_f,heat_b,work_b]
    def separation(t,state):return system.compression(state[:n])
    separation.direction=-1
    linear=core.gradient_hessian(np.zeros(core.size))[1]
    highest=np.sqrt(np.linalg.eigvalsh(linear).max())/(2*np.pi)
    result=solve_ivp(derivative,(0,duration),np.r_[q,p,0.,0.,0.],method='DOP853',rtol=rtol,atol=rtol*1e-4,
        max_step=1/(points*highest),t_eval=np.linspace(0,duration,401),events=separation)
    assert result.success
    trace=[];balance=0.;partition=0.;power_error=0.;force_error=0.
    maximum_support=maximum_support_before_separation=0.
    valid=[(t,s) for t,s in zip(result.t_events[0],result.y_events[0]) if t>1e-8 and s[2*n-1]<0]
    assert valid,'no separation of a returning hammer'
    t_sep,sep=valid[0]
    for t,state in zip(result.t,result.y.T):
        Q,P=state[:n],state[n:2*n];g,_=system.continuous_force(Q,P);parts=core.components(Q[:-1],P[:-1])
        interface=system.interface(Q,P,-g);energy=system.energy(Q,P)
        balance=max(balance,abs(energy+state[2*n]+state[2*n+1]-initial)/initial)
        partition=max(partition,abs(parts['support_energy_j']+state[2*n+1]-state[2*n+2])/initial)
        power_error=max(power_error,abs(interface['power_residual_w']))
        force_error=max(force_error,interface['force_equation_residual'])
        maximum_support=max(maximum_support,parts['support_energy_j'])
        if t<=t_sep:maximum_support_before_separation=max(maximum_support_before_separation,parts['support_energy_j'])
        assert parts['string_energy_j']>=-1e-12 and parts['support_energy_j']>=-1e-12
        trace.append((t,parts['bridge_displacement_m'],parts['bridge_velocity_m_s'],interface['reaction_n'],
            interface['power_into_support_w'],parts['string_energy_j'],parts['support_energy_j'],state[2*n],state[2*n+1],state[2*n+2]))
    final=result.y[:,-1];parts=core.components(final[:n-1],final[n:2*n-1])
    sep_parts=core.components(sep[:n-1],sep[n:2*n-1])
    report=dict(initial_energy_j=initial,maximum_balance_relative_error=balance,support_work_balance_relative_error=partition,
        maximum_interface_power_residual_w=power_error,maximum_interface_force_residual=force_error,
        final_string_energy_j=parts['string_energy_j'],final_support_energy_j=parts['support_energy_j'],
        final_hammer_energy_j=float(.5*final[2*n-1]**2),final_felt_heat_j=float(final[2*n]),final_support_heat_j=float(final[2*n+1]),
        final_work_into_support_j=float(final[2*n+2]),maximum_support_energy_j=maximum_support,
        maximum_support_energy_during_contact_j=maximum_support_before_separation,contact_time_s=float(t_sep),
        support_energy_at_separation_j=sep_parts['support_energy_j'],string_energy_at_separation_j=sep_parts['string_energy_j'],
        maximum_bridge_displacement_m=max(abs(r[1]) for r in trace),nfev=result.nfev,
        minimum_mass_eigenvalue=float(np.linalg.eigvalsh(core.mass).min()))
    assert balance<1e-7 and partition<1e-7 and force_error<1e-9 and power_error<1e-9
    return result,report,np.asarray(trace)


def forced_support(system,speed,support,duration=.004):
    """A fixed string drives an extra body without paying its work. Diagnostic."""
    core=system.string;n=system.size;q,p=system.initial_state(speed)
    phi=support.projection[0];omega=2*np.pi*support.frequency[0];c=support.loss[0]*omega
    def derivative(t,state):
        Q,P=state[:n],state[n:2*n];g,heat=system.continuous_force(Q,P)
        # end_force is external force on the fixed string. The string acts
        # on the support with its negative, not the same force direction.
        reaction=-core.base.end_force(Q[:core.ns])[0]
        B,V=state[2*n],state[2*n+1]
        return np.r_[P,-g,V,-omega*omega*B-c*V+phi*reaction,heat,c*V*V,phi*reaction*V]
    highest=core.nl*np.sqrt(core.wire.young/core.wire.density)/(2*core.wire.length)
    result=solve_ivp(derivative,(0,duration),np.r_[q,p,0.,0.,0.,0.,0.],method='DOP853',rtol=1e-9,atol=1e-13,
        max_step=1/(8*highest),t_eval=[duration])
    assert result.success
    final=result.y[:,-1];initial=system.energy(q,p)
    body=.5*(final[2*n+1]**2+(omega*final[2*n])**2);body_heat=final[2*n+3];work=final[2*n+4]
    extra=system.energy(final[:n],final[n:2*n])+final[2*n+2]+body+body_heat-initial
    assert abs(extra-work)/initial<1e-7
    return dict(initial_hammer_energy_j=initial,support_energy_j=float(body),support_heat_j=float(body_heat),
        work_from_unaccounted_driver_j=float(work),extra_total_energy_j=float(extra),extra_fraction_of_hammer_energy=float(extra/initial))


def p2_support(path,note,cutoff=800.):
    with np.load(path) as data:
        model=dict(panel=json.loads(str(data['panel'])),boundary=str(data['boundary']),nx=int(data['nx']),ny=int(data['ny']),
                   frequency=data['frequency'],vectors=data['vectors'])
    selected=model['frequency']<=cutoff
    model['frequency']=model['frequency'][selected];model['vectors']=model['vectors'][:,selected]
    x=model['panel']['length']*(.1+.8*(note-21)/87);y=.5*model['panel']['width']
    phi=plate.shape(model,np.array([x]),np.array([y]))[0]
    return Support(tuple(model['frequency']),tuple(phi),tuple(np.full(len(phi),.046))),dict(
        asset_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),modes=len(phi),x_m=x,y_m=y,cutoff_hz=cutoff,
        frequencies_hz=model['frequency'].tolist(),projection_kg_minus_half=phi.tolist())


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--input',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--p2-asset',type=Path);parser.add_argument('--phase',choices=['scalar','resolution','p2'],default='scalar')
    parser.add_argument('--transverse',type=int,default=12,help='string modes for the P2 support run')
    parser.add_argument('--resolution-modes',type=int,nargs='+',default=[12,24,48])
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    report=dict(scope='Moving vertical endpoint with reciprocal mixed inertia; no acoustic observer or piano identification',cases={})
    start=time.monotonic()
    if args.phase=='scalar':
        jobs=[(n,v,tag,Support.oscillator(loss=loss,coupling=coupling),12) for n,v in [(60,60),(60,124),(72,124)]
              for tag,loss,coupling in [('Rigid',0.,0.),('Coupled',0.,1.),('Coupled-loss',.05,1.)]]
    elif args.phase=='resolution':jobs=[(60,124,'Coupled-loss',Support.oscillator(),nt) for nt in args.resolution_modes]
    else:
        if args.p2_asset is None:parser.error('--p2-asset required')
        support,metadata=p2_support(args.p2_asset,60);report['support']=metadata
        jobs=[(60,124,'P2-low-band',support,args.transverse)]
    for note,velocity,tag,support,nt in jobs:
        system,speed=load(args.input,note,velocity,support,nt)
        result,metrics,trace=solve(system,speed)
        key=f'{note}-{velocity}-{tag}-T{nt}'
        metrics.update(transverse=nt,axial=2*nt,support_modes=len(support.frequency),support_frequency=list(support.frequency),
            support_projection=list(support.projection),support_loss=list(support.loss),lift_contact=system.lift_contact)
        report['cases'][key]=metrics
        with (args.output/f'{key}.csv').open('w',newline='') as f:
            w=csv.writer(f);w.writerow(['time_s','bridge_displacement_m','bridge_velocity_m_s','force_on_support_n','power_into_support_w',
                'string_energy_j','support_energy_j','felt_heat_j','support_heat_j','work_into_support_j']);w.writerows(trace)
        n=system.size;np.savez(args.output/f'{key}.npz',Q=result.y[:n,-1],P=result.y[n:2*n,-1])
        print(key,'support energy',metrics['final_support_energy_j'],'work balance',metrics['support_work_balance_relative_error'],flush=True)
        report['elapsed_s']=time.monotonic()-start
        (args.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    if args.phase=='scalar':
        fixed,speed=load(args.input,60,124,Support.oscillator(coupling=0.,loss=0.))
        report['forced_support_ablation']=forced_support(fixed,speed,Support.oscillator())
        (args.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')


if __name__=='__main__':main()
