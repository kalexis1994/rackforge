"""Finite-limit passive air: frozen geometry force and persistent board tests."""
import argparse,hashlib,json,platform,time
from pathlib import Path
import numpy as np
import scipy
from concert_grand_bounded_air import BoundedAir,LoadedBoundedBoard
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_orthotropic_plate import shape


def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda:stream.read(1024*1024),b''):h.update(chunk)
    return h.hexdigest()
def rel(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-30))
def save(path,data):path.write_text(json.dumps(data,indent=2,allow_nan=False),encoding='utf-8')


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['asset','bath','contact','rational','geometry','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--board-rates',type=int,nargs='+',default=[])
    p.add_argument('--profiles',nargs='+',help='specific coefficient filenames without .npz')
    p.add_argument('--board-profile',help='exact profile used for board tests')
    p.add_argument('--baseline-commit',default='5834c39')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    with np.load(args.asset) as d:
        mask=d['frequency']<=800
        model=dict(panel=json.loads(str(d['panel'])),nx=int(d['nx']),ny=int(d['ny']),boundary=str(d['boundary']),vectors=d['vectors'][:,mask],frequency=d['frequency'][mask])
    surface=RectangularLoad.from_plate(model,128,96);x=surface.shapes.reshape(-1,surface.modes)
    limit=surface.density*surface.speed*surface.hx*surface.hy*x.T@x
    spectrum=np.load(args.bath/'spectrum.npz')
    with np.load(args.contact) as d:velocity=d['board_velocity']
    geometry_files=[args.geometry/'Cells128x96-Rate128000-Q16.npz',args.geometry/'spatial192/Cells192x144-Rate128000-Q16.npz',args.geometry/'spatial256/Cells256x192-Rate128000-Q16.npz']
    geometries=[]
    for path in geometry_files:
        with np.load(path) as d:
            geometries.append(d['geometric_total_force']);norm=float(np.linalg.norm(d['bath_total_force']))
            peak=float(np.linalg.norm(d['bath_total_force'][:129],axis=1).max());axis=d['time_s']
            if len(geometries)==1:s=d['laplace_s'];z_geometry=d['impedance']
    report=dict(baseline_commit=args.baseline_commit,profiles={},board={},selected_profile=None,
        method='Adjacent-tap factor average, unchanged Cayley mapping, positive matrix mass/resistance relaxation. No coefficient or sample fit.',
        force_convention='Total regular force, left derivative at driven knots. Exact stationary stop. New bounded load has no inertial delta.',
        normalization='All prescribed-force errors use frozen bath RMS and initial peak, unchanged from c353719.',
        cells=[128,96],modes=surface.modes,
        criteria=dict(low_ZRX_max=.02,laplace_max=.02,loaded_velocity_max=.05,mass_relative=1e-10,limit_relative=1e-10,
            memory_rms_common=.02,late_peak_common=.01,energy_relative=1e-9,board_time_refinement=.001,
            board_velocity_vs_bath=.05,diagnostic_high_complex_max=.02,state_ratio=.1),
        inputs={str(path):digest(path) for path in [args.asset,args.contact,args.bath/'spectrum.npz',*geometry_files]},
        environment=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__),
        scope='One baffled face, 35 low P2 shapes, frozen C4 velocity; board impulses are not a nonlinear hammer or piano audio.')
    best=None
    names=args.profiles or ['Wave32000-Cells128x96-Angle24x48-Tail','Wave32000-Cells128x96-Angle48x96-Tail','Wave64000-Cells128x96-Angle24x48-Tail']
    board_profile=args.board_profile or 'Wave32000-Cells128x96-Angle48x96-Tail'
    if args.board_rates and board_profile not in names:p.error('board profile must be explicitly included in measured profiles')
    for label in names:
        start=time.monotonic();path=args.rational/(label+'.npz')
        with np.load(path) as d:air=BoundedAir(d['factor'],float(d['factor_rate']),spectrum['static_mass'],limit)
        z=np.array([air.impedance(value) for value in s])
        low=len(spectrum['test_frequency_hz']);end=low+len(spectrum['laplace_s'])
        errors=np.array([[rel(a,b),rel(a.real,b.real),rel(a.imag,b.imag)] for a,b in zip(z,z_geometry)])
        K=(2*np.pi*model['frequency'])**2;D=.046*2*np.pi*model['frequency']
        phi=shape(model,np.array([.642068965517241]),np.array([.5]))[0]
        loaded=[]
        for f,a,b in zip(spectrum['test_frequency_hz'],z[:low],spectrum['test_impedance']):
            point=2j*np.pi*f;mechanics=np.diag(K+point*D+point*point)
            loaded.append(rel(np.linalg.solve(mechanics+point*a,phi),np.linalg.solve(mechanics+point*b,phi)))
        force=air.linear_drive_force(velocity,1/128000,round(.060*128000),8)[::4]
        comparisons={}
        for cells,target in zip(['128x96','192x144','256x192'],geometries):
            error=force-target;value=dict(rms_common=float(np.linalg.norm(error)/norm),
                late_peak_common=float(np.linalg.norm(error[axis>=.010],axis=1).max()/peak))
            value['passed']=value['rms_common']<.02 and value['late_peak_common']<.01;comparisons[cells]=value
        rng=np.random.default_rng(243);storage=[]
        for dt in [1/48000,1/192000,1/768000]:
            state=(rng.normal(size=air.wave.zero_state().shape)*.001,rng.normal(size=air.modes)*.001);v=rng.normal(size=air.modes)*.01
            before=air.energy(state);updated,f,loss=air.midpoint(state,v,dt);after=air.energy(updated)
            storage.append(dict(dt_s=dt,balance_relative=abs(after-before-dt*(v@f-loss))/max(before,abs(dt*v@f),1e-30),radiation_power_w=loss))
        item=dict(input_sha256=digest(path),states=air.states,state_ratio=air.states/352026,
            relaxation_time_s=air.relaxation.tau[[0,-1]].tolist(),mass_relative=rel(air.static_mass(),spectrum['static_mass']),
            limit_relative=rel(air.high_limit(),limit),wave_limit_relative=float(np.linalg.norm(air.wave_limit)/np.linalg.norm(limit)),
            low_ZRX_max=errors[:low].max(axis=0).tolist(),laplace_max=float(errors[low:end,0].max()),loaded_velocity_max=max(loaded),
            additional_high=[dict(frequency_hz=float(point.imag/(2*np.pi)),complex_relative=float(error[0]),resistance_relative=float(error[1]),reactance_relative=float(error[2])) for point,error in zip(s[end:],errors[end:])],
            memory=comparisons,storage=storage,seconds=time.monotonic()-start)
        item['gates']=dict(low_frequency=max(item['low_ZRX_max'])<.02,laplace=item['laplace_max']<.02,loaded_velocity=item['loaded_velocity_max']<.05,
            mass=item['mass_relative']<1e-10,limit=item['limit_relative']<1e-10,memory=all(value['passed'] for value in comparisons.values()),
            energy=all(value['balance_relative']<1e-9 and value['radiation_power_w']>=0 for value in storage),states=item['state_ratio']<.1,
            diagnostic_high=max(value['complex_relative'] for value in item['additional_high'])<.02)
        report['profiles'][label]=item
        np.savez_compressed(args.output/(label+'-bounded.npz'),time_s=axis,total_force=force,
            mass_target=spectrum['static_mass'],limit_target=limit,relaxation_B=air.relaxation.B,relaxation_tau=air.relaxation.tau)
        save(args.output/'resumen.json',report);print(label,json.dumps(item),flush=True)
        if label==board_profile:best=air
    # Integrator refinement is separate from the accuracy of the surface load.
    trajectories={}
    if args.board_rates:report['inputs'][str(args.bath/'Fine384.npz')]=digest(args.bath/'Fine384.npz')
    for rate in args.board_rates:
        if rate<=0 or rate%8000 or (rate*.008)%1 or (rate*.024)%1:p.error('board rates must resolve 8/24ms impulses and common 8kHz observations')
        start=time.monotonic();board=LoadedBoundedBoard(np.diag(K),np.diag(D),best);state=board.zero_state()
        work=material=radiation=port=0.;rows=[];v_all=[];q_all=[];budgets=[];negative=0
        for i in range(round(.024*rate)+1):
            if i in [0,round(.008*rate)]:
                impulse=phi*(.002 if i==0 else .0015);old=state[1];new=old+impulse
                work+=float((new@new-old@old)/2);state=(state[0],new,state[2])
            if i in [0,round(.008*rate),round(.024*rate)]:
                acoustic=best.energy(state[2]);total=board.energy(state)
                budgets.append(dict(time_s=i/rate,total_balance_relative=abs(total+material+radiation-work)/work,
                    air_balance_relative=abs(acoustic+radiation-port)/work,stored_total_j=total,stored_air_j=acoustic))
            v_all.append(state[1].copy());q_all.append(state[0].copy())
            if i%(rate//8000)==0:rows.append([i/rate,phi@state[0],phi@state[1],material,radiation,port,work])
            if i==round(.024*rate):break
            state,stats=board.step(state,1/rate)
            material+=stats['material_loss_j'];radiation+=stats['radiation_loss_j'];port+=stats['air_port_work_j'];negative+=stats['air_port_work_j']<0
        v_all=np.array(v_all);q_all=np.array(q_all);trajectories[rate]=(v_all,q_all)
        item=dict(rate_hz=rate,seconds=time.monotonic()-start,energy_snapshots=budgets,
            material_loss_j=material,radiation_loss_j=radiation,air_port_work_j=port,external_impulse_work_j=work,
            negative_port_work_steps=negative,states_preserved_at_second_impulse=True)
        if 384000%rate==0:
            with np.load(args.bath/'Fine384.npz') as d:
                item['velocity_vs_bath']=rel(v_all,d['board_velocity'][::384000//rate]);item['displacement_vs_bath']=rel(q_all,d['board_displacement'][::384000//rate])
        np.savez_compressed(args.output/f'Board{rate}.npz',time_s=np.arange(len(v_all))/rate,board_velocity=v_all,board_displacement=q_all,
            wave_state_final=state[2][0],relaxation_state_final=state[2][1])
        np.savetxt(args.output/f'Board{rate}.csv',rows,delimiter=',',header='time_s,bridge_q_m,bridge_v_m_s,material_J,radiation_J,air_port_J,external_work_J',comments='')
        report['board'][str(rate)]=item;save(args.output/'resumen.json',report);print('board',rate,json.dumps(item),flush=True)
    if len(trajectories)>1:
        rates=sorted(trajectories);coarse,fine=rates[-2:]
        if fine%coarse:p.error('board refinement rates must be integer multiples')
        ratio=fine//coarse
        report['board_refinement']=dict(pair=[coarse,fine],velocity=rel(trajectories[coarse][0],trajectories[fine][0][::ratio]),displacement=rel(trajectories[coarse][1],trajectories[fine][1][::ratio]))
        r=report['board_refinement'];r['passed']=max(r['velocity'],r['displacement'])<.001
    if report['board']:
        report['board_gates']=dict(energy=all(s['total_balance_relative']<1e-9 for b in report['board'].values() for s in b['energy_snapshots']),
            air_budget=all(s['air_balance_relative']<1e-9 for b in report['board'].values() for s in b['energy_snapshots']),
            velocity_vs_bath=all(b.get('velocity_vs_bath',float('inf'))<.05 for b in report['board'].values()),
            time_refinement=report.get('board_refinement',{}).get('passed',False))
    report['decision']='No factory selection: broad-band transfer, nonlinear hammer contacts and wider note/velocity bank still required, regardless of low-band or board results.'
    save(args.output/'resumen.json',report)


if __name__=='__main__':main()
