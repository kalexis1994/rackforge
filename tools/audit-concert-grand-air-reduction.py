"""Passive compression against frozen matrix and temporal references."""
import argparse,hashlib,json,time
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from concert_grand_acoustic_bath import AcousticBath,LoadedBoard
from concert_grand_bath_reduction import grouped,response,spectral_basis,fit,truncate_basis,realize,linear_drive,free_force
from concert_grand_orthotropic_plate import shape

spec=spec_from_file_location('contact_audit',Path(__file__).with_name('audit-concert-grand-bath-contact.py'))
contact=module_from_spec(spec);spec.loader.exec_module(contact)
CRITERIA=dict(complex_max=.02,resistance_max=.02,reactance_max=.02,laplace_max=.02,loaded_velocity_max=.05,
    static_mass_error=1e-10,state_ratio=.1,board_velocity_displacement_rms=.01,contact_energy_work=.002,
    contact_duration=.001,energy_budget=1e-9,memory_force_rms=.02,late_force_peak_normalized=.01)


def load_bath(path):
    with np.load(path) as d:return AcousticBath(d['omega_rad_s'],d['gamma_rad_s'],d['projection'],d['tail_mass'])


def save_bath(path,bath,**extra):
    np.savez_compressed(path,omega_rad_s=bath.omega,gamma_rad_s=bath.gamma,projection=bath.projection,tail_mass=bath.tail_mass,**extra)


def rel(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-30))


def compress(args):
    start=time.monotonic();reference=load_bath(args.reference/'bath-fine.npz');omega,gamma,residues=grouped(reference)
    static=reference.tail_mass+np.einsum('jmn,j->mn',residues,1/omega**2)
    f=np.r_[np.arange(35.7,1006.,1.),np.geomspace(1200.7,5700.7,24)]
    s=np.r_[2j*np.pi*f,30+2j*np.pi*75.3,80+2j*np.pi*333.7,150+2j*np.pi*750.3]
    target=response(omega,gamma,residues,reference.tail_mass,s)
    data=np.load(args.reference/'spectrum.npz');test_s=2j*np.pi*data['test_frequency_hz']
    full_test=response(omega,gamma,residues,reference.tail_mass,test_s)
    model=contact.bridge.p2_support(args.asset,60)[0]
    K=(2*np.pi*np.asarray(model.frequency))**2;D=.046*2*np.pi*np.asarray(model.frequency);phi=np.asarray(model.projection)
    previous=args.output/'compression.json'
    reports=json.loads(previous.read_text())['profiles'] if previous.exists() else {}
    for step in args.spacing:
        high=args.high_spacing if args.high_spacing else 5*step
        edges=np.r_[np.arange(0,1200+step/2,step),np.arange(1200+high,6000+high/2,high)]
        w,g,A=spectral_basis(data,edges,1.);before=time.monotonic()
        spatial_error=0.
        if args.rank_tolerance:A,spatial_error=truncate_basis(A,args.rank_tolerance)
        if args.gamma is not None:g[:]=2*np.pi*args.gamma
        if args.high_gamma is not None:g[w>2*np.pi*1200]=2*np.pi*args.high_gamma
        if args.no_fit:
            weights=np.ones(len(A));bath=realize(w,g,A,static,weights);optimization=dict(method='positive spectral quadrature, no weight fit')
        else:bath,weights,optimization=fit(w,g,A,static,s,target)
        rw,rg,rr=grouped(bath);actual=response(rw,rg,rr,bath.tail_mass,test_s)
        laplace=response(rw,rg,rr,bath.tail_mass,data['laplace_s']);physical=data['test_impedance']
        errors=[];velocity_error=[]
        for ss,a,b in zip(test_s,actual,physical):
            errors.append([rel(a,b),rel(a.real,b.real),rel(a.imag,b.imag)])
            mech=np.diag(K+ss*D+ss*ss)
            velocity_error.append(rel(ss*np.linalg.solve(mech+ss*a,phi),ss*np.linalg.solve(mech+ss*b,phi)))
        maxerr=np.max(errors,axis=0)
        metrics=dict(ports=bath.ports,state_ratio=bath.ports/reference.ports,spacing_hz=step,
            broadening='representation absorption, not measured radiation',gamma_range_hz=[float(g.min()/(2*np.pi)),float(g.max()/(2*np.pi))],
            training_points=len(s),test_points=len(test_s),optimization=optimization,
            rank_tolerance=args.rank_tolerance,maximum_spatial_projection_error=spatial_error,gamma_hz=args.gamma,
            high_spacing_hz=high,
            complex_max=float(maxerr[0]),resistance_max=float(maxerr[1]),reactance_max=float(maxerr[2]),
            full_bath_complex_max=max(rel(a,b) for a,b in zip(actual,full_test)),
            laplace_max=max(rel(a,b) for a,b in zip(laplace,data['laplace_reference'])),
            loaded_velocity_max=max(velocity_error),static_mass_error=rel(bath.static_mass(),static),
            tail_min_eigenvalue=float(np.linalg.eigvalsh(bath.tail_mass).min()),seconds=time.monotonic()-before)
        metrics['gates']={key:metrics[key]<CRITERIA[key] for key in ['complex_max','resistance_max','reactance_max','laplace_max','loaded_velocity_max','static_mass_error','state_ratio']}
        key=f'Grid{step:g}'
        if args.rank_tolerance:key+=f'-Rank{args.rank_tolerance:g}-High{high:g}'
        if args.gamma is not None:key+=f'-Gamma{args.gamma:g}'
        if args.high_gamma is not None:key+=f'-HighGamma{args.high_gamma:g}'
        if args.no_fit:key+='-Raw'
        else:key+='-Parts'
        reports[key]=metrics
        save_bath(args.output/f'{key}.npz',bath,weights=weights,basis_omega=w,basis_gamma=g,basis_static=A)
        np.savetxt(args.output/f'{key}-errors.csv',np.column_stack((data['test_frequency_hz'],errors,velocity_error)),
            delimiter=',',header='frequency_hz,complex_relative,resistance_relative,reactance_relative,loaded_velocity_relative',comments='')
        print(key,json.dumps(metrics),flush=True)
        (args.output/'compression.json').write_text(json.dumps(dict(criteria=CRITERIA,profiles=reports,
            training_frequency_hz=f.tolist(),elapsed_s=time.monotonic()-start),indent=2),encoding='utf-8')


def board_control(args):
    bath=load_bath(args.output/f'{args.profile}.npz');support,meta=contact.bridge.p2_support(args.asset,60)
    phi=np.asarray(support.projection);K=np.diag((2*np.pi*np.asarray(support.frequency))**2);D=np.diag(.046*2*np.pi*np.asarray(support.frequency))
    board=LoadedBoard(K,D,bath);rate=384000;state=board.zero_state();work=material=absorption=port=tail_jump=0.
    q=[];v=[];maximum=0.;start=time.monotonic()
    for i in range(round(.024*rate)+1):
        if i in (0,round(.008*rate)):
            old_air=board.energy(state)['air_j'];state,jump=board.impulse(state,phi*(.002 if i==0 else .0015));work+=jump
            tail_jump+=board.energy(state)['air_j']-old_air
        energy=board.energy(state);maximum=max(maximum,abs(energy['total_j']+material+absorption-work)/work)
        q.append(state[0].copy());v.append(state[1].copy())
        if i==round(.024*rate):break
        state,stats=board.step(state,1/rate);material+=stats['material_loss_j'];absorption+=stats['bath_absorption_j'];port+=stats['air_port_work_j']
    q=np.array(q);v=np.array(v)
    with np.load(args.reference/'Fine384.npz') as d:
        errors=dict(board_velocity_rms=rel(v,d['board_velocity']),board_displacement_rms=rel(q,d['board_displacement']),
            bridge_velocity_rms=rel(v@phi,d['board_velocity']@phi),bridge_displacement_rms=rel(q@phi,d['board_displacement']@phi))
    report=dict(profile=args.profile,rate_hz=rate,duration_s=.024,seconds=time.monotonic()-start,errors=errors,
        final=energy,work_j=work,material_j=material,numerical_absorption_j=absorption,total_air_port_j=port+tail_jump,
        maximum_budget_relative=maximum,air_budget_relative=abs(energy['air_j']+absorption-port-tail_jump)/work)
    report['passed']=bool(max(errors.values())<CRITERIA['board_velocity_displacement_rms'] and max(maximum,report['air_budget_relative'])<CRITERIA['energy_budget'])
    np.savez_compressed(args.output/f'{args.profile}-board.npz',board_position=q,board_velocity=v,air_position=state[2],air_velocity=state[3])
    (args.output/f'{args.profile}-board.json').write_text(json.dumps(report,indent=2),encoding='utf-8');print(json.dumps(report),flush=True)


def memory_control(args):
    with np.load(args.contact_reference/'T72-768000-Air.npz') as d:velocity=d['board_velocity']
    forces={};start=time.monotonic()
    for name,bath in [('Full',load_bath(args.reference/'bath-fine.npz')),('Reduced',load_bath(args.output/f'{args.profile}.npz'))]:
        a,p,driven=linear_drive(bath,velocity,1/128000,4)
        late=free_force(bath,a,p,round(.060*32000),1/32000)
        forces[name]=np.vstack((driven,late[1:]));print('memory',name,time.monotonic()-start,flush=True)
    full=forces['Full'];reduced=forces['Reduced'];time_axis=np.arange(len(full))/32000
    scale=float(np.linalg.norm(full[:129],axis=1).max())
    report=dict(profile=args.profile,duration_s=.064,input='Same frozen C4 board velocity for 4ms, then prescribed stationary port; not a coupled 64ms piano note',
        memory_force_rms=rel(reduced,full),late_force_peak_normalized=float(np.linalg.norm((reduced-full)[time_axis>=.010],axis=1).max()/scale),
        full_late_peak_normalized=float(np.linalg.norm(full[time_axis>=.010],axis=1).max()/scale),seconds=time.monotonic()-start)
    report['passed']=bool(report['memory_force_rms']<CRITERIA['memory_force_rms'] and report['late_force_peak_normalized']<CRITERIA['late_force_peak_normalized'])
    np.savez_compressed(args.output/f'{args.profile}-memory.npz',time_s=time_axis,full_force=full,reduced_force=reduced)
    (args.output/f'{args.profile}-memory.json').write_text(json.dumps(report,indent=2),encoding='utf-8');print(json.dumps(report),flush=True)


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--phase',choices=['compress','board','memory'],default='compress')
    parser.add_argument('--reference',type=Path,required=True);parser.add_argument('--asset',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True);parser.add_argument('--spacing',type=float,nargs='+',default=[20.,10.,5.]);parser.add_argument('--profile',default='Grid10')
    parser.add_argument('--rank-tolerance',type=float,default=0.);parser.add_argument('--high-spacing',type=float)
    parser.add_argument('--gamma',type=float);parser.add_argument('--no-fit',action='store_true')
    parser.add_argument('--high-gamma',type=float)
    parser.add_argument('--contact-reference',type=Path)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    if args.phase=='compress':compress(args)
    elif args.phase=='board':board_control(args)
    else:
        if args.contact_reference is None:parser.error('--contact-reference required for memory')
        memory_control(args)


if __name__=='__main__':main()
