"""Recompute and verify a wider physical P2 basis, without fitting samples.

One canonical repository report; SHARED holds only numerical coefficients
and measurements. The 35-mode factor is an unchanged-subspace control, not
the radiation load for the additional board modes.
"""
import argparse,hashlib,json,platform,time
from pathlib import Path
import numpy as np
import scipy
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_geometric_memory import GeometricMemory
from concert_grand_rational_air import surface_factor_sparse
from concert_grand_bounded_air import BoundedAir
from concert_grand_orthotropic_plate import shape
from concert_grand_air_coefficients import load_prefix

CRITERIA=dict(factor_subspace=1e-10,mass_subspace=1e-10,mass_limit=1e-10,
    geometric_reference=1e-5,low_ZRX=.02,laplace=.02,high_Z=.02,
    loaded_velocity=.05,reference_spatial_change=.01,memory_rms=.02,
    memory_late_peak=.01,energy=1e-9)


def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda:stream.read(1024*1024),b''):h.update(block)
    return h.hexdigest()
def rel(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-30))
def write(path,data):
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(json.dumps(data,indent=2,allow_nan=False),encoding='utf-8')


def model_from_asset(path,cutoff):
    with np.load(path) as data:
        frequency=data['frequency'];selected=frequency<=cutoff
        if not np.isfinite(cutoff) or cutoff<=800 or cutoff>frequency.max() or not selected.any():raise ValueError('wider cutoff within exported structural spectrum')
        if np.any(np.diff(frequency)<0):raise ValueError('ordered physical modal basis required')
        return dict(panel=json.loads(str(data['panel'])),nx=int(data['nx']),ny=int(data['ny']),boundary=str(data['boundary']),
            vectors=data['vectors'][:,selected],frequency=frequency[selected])


def transform(memory,points,rate=128000,quadrature=16):
    nodes,weights,packed=memory.quadrature_kernel(1/rate,quadrature)
    flattened=packed.reshape(-1,len(memory.upper[0]));weight=np.broadcast_to(weights,nodes.shape).ravel()
    value=(np.exp(-np.asarray(points)[:,None]*nodes.ravel()[None,:])*weight)@flattened
    impedance=memory.unpack(value)*np.asarray(points)[:,None,None]
    mass=memory.unpack((weight@flattened)[None,:])[0]
    integration=memory.unpack(np.einsum('q,tqm->tm',weights,packed))
    return impedance,mass,integration


def build(args):
    if (not args.parent_coefficients and args.coefficients.exists()) or args.report.exists():raise ValueError('preserve existing experiment; choose new destinations')
    model=model_from_asset(args.asset,args.cutoff);surface=RectangularLoad.from_plate(model,*args.cells)
    started=time.monotonic();reuse=None
    if args.parent_coefficients:
        parent=json.loads(args.parent_report.read_text())
        if not parent['all_load_gates_passed'] or not all(parent['gates'].values()) or parent['cells']!=args.cells or parent['factor_rate_hz']!=args.factor_rate or parent['inputs'][str(args.asset)]!=digest(args.asset) or parent['coefficient_sha256']!=digest(args.parent_coefficients):
            raise ValueError('verified parent with matched asset and discretization required')
        H,rate,parent_mass,parent_limit,parent_modes=load_prefix(args.parent_coefficients,model['frequency'])
        if rate!=args.factor_rate or parent_modes<=surface.modes:raise ValueError('strictly larger parent at matched factor rate required')
        reuse=dict(path=str(args.parent_coefficients),parent_report_sha256=digest(args.parent_report),parent_modes=parent_modes,selected_modes=surface.modes,
            selection='Physical ordered prefix of factor/static mass/high limit; passive completion rebuilt independently; no factor copy saved.')
    else:H=surface_factor_sparse(surface,args.factor_rate,96,96)
    with np.load(args.control) as data:old=data['factor'];old_rate=float(data['factor_rate'])
    if old_rate!=64000 or old.shape[2]!=35:raise ValueError('frozen 64k/96x96 35-mode control required')
    refined=args.cells!=[128,96] or args.factor_rate!=old_rate
    if refined:
        del old
        small=RectangularLoad(surface.length,surface.width,surface.shapes[:,:,:35])
        old=surface_factor_sparse(small,args.factor_rate,96,96)
    equivalence=rel(H[:,:,:35],old);del old
    mass=surface.added_mass_static(8);x=surface.shapes.reshape(-1,surface.modes)
    limit=surface.density*surface.speed*surface.hx*surface.hy*x.T@x
    parent_error=dict(mass_relative=rel(parent_mass,mass),limit_relative=rel(parent_limit,limit)) if reuse else None
    with np.load(args.mass_control) as data:archived_mass=data['static_mass']
    archived_mass_change=rel(mass[:35,:35],archived_mass)
    mass_error=rel(mass[:35,:35],small.added_mass_static(8) if refined else archived_mass)
    report=dict(baseline_commit=args.baseline_commit,scope='Expanded intrinsic P2 modes, one baffled face; not a complete piano or listening result.',
        cutoff_hz=args.cutoff,modes=surface.modes,frequency_hz=model['frequency'].tolist(),cells=args.cells,factor_rate_hz=args.factor_rate,angular=[96,96],
        subspace_control='Independent 35-coordinate construction at the candidate spatial/temporal resolution' if refined else 'Archived unchanged 35-mode factor and mass',
        coefficient_reuse=reuse,parent_target_checks=parent_error,
        inputs={str(path):digest(path) for path in [args.asset,args.control,args.mass_control,args.contact,args.memory_control]},criteria=CRITERIA,
        environment=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__,blas_threads=1),
        build=dict(taps=len(H),outputs=H.shape[1],factor_subspace_relative=equivalence,mass_subspace_relative=mass_error,
            archived_control_mass_relative_change=archived_mass_change,
            dc_relative=float(np.linalg.norm(H.sum(axis=0))/np.linalg.norm(H)),seconds=time.monotonic()-started),gates={})
    report['gates']['subspace']=equivalence<CRITERIA['factor_subspace'] and mass_error<CRITERIA['mass_subspace']
    if reuse:report['gates']['subspace'] &= max(parent_error.values())<CRITERIA['mass_limit']
    if not report['gates']['subspace']:
        write(args.report,report);raise AssertionError('wider construction changes frozen subspace')
    args.coefficients.parent.mkdir(parents=True,exist_ok=True)
    if not reuse:
        np.savez_compressed(args.coefficients,factor=H,factor_rate=args.factor_rate,static_mass=mass,high_limit=limit,
            frequency_hz=model['frequency'],cutoff_hz=args.cutoff)
    report['coefficient_sha256']=digest(args.coefficients);write(args.report,report)
    print('build',json.dumps(report['build']),flush=True)


def verify(args):
    report=json.loads(args.report.read_text());started=time.monotonic()
    if report['criteria']!=CRITERIA or report['coefficient_sha256']!=digest(args.coefficients):raise ValueError('frozen coefficients/criteria changed')
    supplied={str(path):digest(path) for path in [args.asset,args.control,args.mass_control,args.contact,args.memory_control]}
    if report['cutoff_hz']!=args.cutoff or supplied!=report['inputs'] or report['cells']!=args.cells or report['factor_rate_hz']!=args.factor_rate:raise ValueError('frozen physical/control inputs changed')
    report['gates']={'subspace':report['gates']['subspace']}
    for key in ['completion','reference_checks','reference_probe_s','transfer','memory','spatial','storage','verification_s','all_load_gates_passed','decision','source_sha256','measurement_sha256']:
        report.pop(key,None)
    write(args.report,report)
    model=model_from_asset(args.asset,args.cutoff);surface=RectangularLoad.from_plate(model,*args.cells)
    factor,rate,mass,limit,parent_modes=load_prefix(args.coefficients,model['frequency'])
    if args.parent_coefficients:
        if report['coefficient_reuse']['parent_report_sha256']!=digest(args.parent_report) or report['coefficient_reuse']['parent_modes']!=parent_modes:raise ValueError('frozen parent report/order changed')
    report['build']['dc_relative']=float(np.linalg.norm(factor.sum(axis=0))/np.linalg.norm(factor))
    air=BoundedAir(factor,rate,mass,limit)
    del factor
    report['completion']=dict(states=air.states,relaxation_time_s=air.relaxation.tau[[0,-1]].tolist(),
        mass_relative=rel(air.static_mass(),mass),limit_relative=rel(air.high_limit(),limit))
    report['gates']['limits']=max(report['completion'][k] for k in ['mass_relative','limit_relative'])<CRITERIA['mass_limit']
    write(args.report,report);print('completion',json.dumps(report['completion']),flush=True)
    # Every structural resonance in the declared low band is an evaluation
    # point. The new modes are excited in an additional prescribed-port test.
    low=np.unique(np.r_[np.linspace(60,args.cutoff,149),model['frequency'][model['frequency']>=60],261.6255653,523.2511306])
    high=np.linspace(1000.,6000.,61);laplace=np.array([20+2j*np.pi*60,50+2j*np.pi*261.6255653,100+2j*np.pi*args.cutoff])
    points=np.r_[2j*np.pi*low,laplace,2j*np.pi*high];n=len(low);end=n+len(laplace)
    direct=GeometricMemory(surface);reference,geometric_mass,integration=transform(direct,points)
    probes=np.r_[2j*np.pi*np.array([60.,261.6255653,args.cutoff,2000.,4500.,6000.]),laplace]
    duffy=np.array([surface.impedance(s,8) for s in probes]);geometric_probes,_,_=transform(direct,probes,256000,8)
    probe_reference,_,_=transform(direct,probes)
    reference_check=dict(duffy_relative=rel(probe_reference,duffy),time_refinement_relative=rel(probe_reference,geometric_probes),mass_relative=rel(geometric_mass,mass))
    reference_check['duffy_max_point_relative']=max(rel(a,b) for a,b in zip(probe_reference,duffy))
    reference_check['time_refinement_max_point_relative']=max(rel(a,b) for a,b in zip(probe_reference,geometric_probes))
    report['reference_checks']=reference_check
    report['reference_probe_s']=[[float(s.real),float(s.imag)] for s in probes]
    report['gates']['reference']=max(reference_check.values())<CRITERIA['geometric_reference']
    actual=np.array([air.impedance(s) for s in points]);errors=np.array([[rel(a,b),rel(a.real,b.real),rel(a.imag,b.imag)] for a,b in zip(actual,reference)])
    phi=shape(model,np.array([model['panel']['length']*(.1+.8*(60-21)/87)]),np.array([model['panel']['width']/2]))[0]
    omega=2*np.pi*model['frequency'];loaded=[]
    for point,a,b in zip(points[:n],actual[:n],reference[:n]):
        mechanics=np.diag(omega**2+point*.046*omega+point*point)
        loaded.append(rel(np.linalg.solve(mechanics+point*a,phi),np.linalg.solve(mechanics+point*b,phi)))
    report['transfer']=dict(low_points=n,low_ZRX_max=errors[:n].max(axis=0).tolist(),laplace_max=float(errors[n:end,0].max()),
        high_points=len(high),high_complex_max=float(errors[end:,0].max()),high_max_frequency_hz=float(high[np.argmax(errors[end:,0])]),loaded_velocity_max=max(loaded))
    report['gates'].update(low_transfer=max(report['transfer']['low_ZRX_max'])<CRITERIA['low_ZRX'],
        laplace=report['transfer']['laplace_max']<CRITERIA['laplace'],high_transfer=report['transfer']['high_complex_max']<CRITERIA['high_Z'],
        loaded_velocity=report['transfer']['loaded_velocity_max']<CRITERIA['loaded_velocity'])
    report['transfer']['subspace_impedance_change']=None
    with np.load(args.control) as data:
        old_factor=data['factor'];old_rate=float(data['factor_rate'])
    old_model={**model,'frequency':model['frequency'][:35],'vectors':model['vectors'][:,:35]}
    old_surface=RectangularLoad.from_plate(old_model,128,96)
    with np.load(args.mass_control) as data:old_mass=data['static_mass']
    old_x=old_surface.shapes.reshape(-1,35);old_limit=old_surface.density*old_surface.speed*old_surface.hx*old_surface.hy*old_x.T@old_x
    old_air=BoundedAir(old_factor,old_rate,old_mass,old_limit);del old_factor
    report['transfer']['subspace_impedance_change']=max(rel(a[:35,:35],old_air.impedance(s)) for s,a in zip(points,actual));del old_air
    np.savez_compressed(args.output/'transfer.npz',laplace_s=points,geometric_impedance=reference,actual_impedance=actual,errors=errors,loaded_velocity_error=loaded)
    write(args.report,report);print('transfer',json.dumps(report['transfer']),flush=True)
    with np.load(args.contact) as data:old_velocity=data['board_velocity']
    if old_velocity.shape!=(513,35):raise ValueError('frozen 4ms C4 control at 128k required')
    old_drive=np.zeros((513,air.modes));old_drive[:,:35]=old_velocity
    t=np.arange(513)/128000;wd=omega*np.sqrt(1-(.046/2)**2)
    wider_drive=.002*phi[None,:]*np.exp(-.046*omega[None,:]*t[:,None]/2)*np.cos(wd[None,:]*t[:,None])*np.sin(np.pi*t[:,None]/.004)**2
    wider_drive[[0,-1]]=0.  # Exact specified zero-ended envelope, not output clipping.
    # The central bridge point has nodal modes. Use an additional prescribed
    # modal port, with equal amplitudes and the same total amplitude norm,
    # to exercise every coordinate. This is not a nonlinear piano contact.
    amplitude=.002*np.linalg.norm(phi)/np.sqrt(air.modes)
    distributed_drive=amplitude*np.exp(-.046*omega[None,:]*t[:,None]/2)*np.cos(wd[None,:]*t[:,None])*np.sin(np.pi*t[:,None]/.004)**2
    distributed_drive[[0,-1]]=0.
    with np.load(args.memory_control) as data:normalizer=float(np.linalg.norm(data['bath_total_force']));old_peak=float(np.linalg.norm(data['bath_total_force'][:129],axis=1).max())
    drives=dict(FrozenC4=old_drive,C4Projected=wider_drive,Distributed=distributed_drive);targets={};forces={};normalizers={};peaks={};rows={}
    for name,velocity in drives.items():
        target=direct.linear_drive_force(velocity,1/128000,round(.060*128000),16,integration)[::4]
        value=air.linear_drive_force(velocity,1/128000,round(.060*128000),8)[::4]
        targets[name]=target;forces[name]=value;normalizers[name]=normalizer if name=='FrozenC4' else float(np.linalg.norm(target));peaks[name]=old_peak if name=='FrozenC4' else float(np.linalg.norm(target[:129],axis=1).max())
        delta=value-target;axis=np.arange(len(target))/32000
        rows[name]=dict(rms_relative=float(np.linalg.norm(delta)/normalizers[name]),late_peak_relative=float(np.linalg.norm(delta[axis>=.010],axis=1).max()/peaks[name]),
            normalizer='Frozen original bath total-force RMS and initial peak' if name=='FrozenC4' else 'Independent geometric reference RMS and initial peak; fixed before comparing candidate',
            input_new_mode_velocity_l2=float(np.linalg.norm(velocity[:,35:])),
            excited_modes=int(np.count_nonzero(np.linalg.norm(velocity,axis=0)>np.linalg.norm(velocity)*1e-8)),
            excited_new_modes=int(np.count_nonzero(np.linalg.norm(velocity[:,35:],axis=0)>np.linalg.norm(velocity)*1e-8)))
        print('memory',name,json.dumps(rows[name]),flush=True)
    spatial_surface=RectangularLoad.from_plate(model,*args.spatial_cells);spatial=GeometricMemory(spatial_surface)
    high_control=2j*np.pi*np.unique([1000.,1200.,args.cutoff,2000.,2416.6666666666665,3000.,4500.,6000.]);spatial_z,_,spatial_integral=transform(spatial,high_control)
    control_z,_,_=transform(direct,high_control)
    spatial_changes=[rel(a,b) for a,b in zip(spatial_z,control_z)];spatial_error=[rel(air.impedance(s),b) for s,b in zip(high_control,spatial_z)]
    for name,velocity in drives.items():
        target=spatial.linear_drive_force(velocity,1/128000,round(.060*128000),16,spatial_integral)[::4]
        rows[name]['spatial_fine_rms_relative']=float(np.linalg.norm(forces[name]-target)/normalizers[name])
        rows[name]['spatial_fine_late_peak_relative']=float(np.linalg.norm((forces[name]-target)[axis>=.010],axis=1).max()/peaks[name])
        rows[name]['reference_grid_rms_change']=float(np.linalg.norm(target-targets[name])/normalizers[name])
    report['memory']=rows;report['spatial']=dict(cells=args.spatial_cells,frequency_hz=(high_control.imag/(2*np.pi)).tolist(),reference_complex_max_change=max(spatial_changes),candidate_high_complex_max=max(spatial_error))
    report['gates']['all_mode_drive']=rows['Distributed']['excited_modes']==air.modes and rows['Distributed']['excited_new_modes']==air.modes-35
    report['gates']['memory']=all(max(v['rms_relative'],v['spatial_fine_rms_relative'])<CRITERIA['memory_rms'] and max(v['late_peak_relative'],v['spatial_fine_late_peak_relative'])<CRITERIA['memory_late_peak'] for v in rows.values())
    report['gates']['spatial']=max(spatial_changes)<CRITERIA['reference_spatial_change'] and max(spatial_error)<CRITERIA['high_Z'] and all(v['reference_grid_rms_change']<CRITERIA['reference_spatial_change'] for v in rows.values())
    np.savez_compressed(args.output/'memory.npz',time_s=axis,drive_time_s=t,
        **{f'drive_{name}':value for name,value in drives.items()},
        **{f'actual_{name}':value for name,value in forces.items()},
        **{f'reference_{name}':value for name,value in targets.items()})
    rng=np.random.default_rng(1200);storage=[]
    for dt in [1/48000,1/192000,1/768000]:
        state=(rng.normal(size=air.wave.zero_state().shape)*.001,rng.normal(size=air.modes)*.001);velocity=rng.normal(size=air.modes)*.01
        before=air.energy(state);updated,force,power=air.midpoint(state,velocity,dt);after=air.energy(updated)
        storage.append(dict(dt_s=dt,budget_relative=abs(after-before-dt*(velocity@force-power))/max(before,abs(dt*velocity@force),1e-30),radiation_power_w=power))
    report['storage']=storage;report['gates']['energy']=all(row['budget_relative']<CRITERIA['energy'] and row['radiation_power_w']>=0 for row in storage)
    report['verification_s']=time.monotonic()-started;report['all_load_gates_passed']=all(report['gates'].values());report['decision']='Load verification only: wider coupled nonlinear contacts, observation and listening still required; no factory selection.'
    report['source_sha256']={name:digest(Path(__file__).with_name(name)) for name in ['audit-concert-grand-extended-board.py','concert_grand_air_coefficients.py','concert_grand_surface_impedance.py','concert_grand_geometric_memory.py','concert_grand_rational_air.py','concert_grand_bounded_air.py']}
    report['measurement_sha256']={name:digest(args.output/name) for name in ['transfer.npz','memory.npz']}
    write(args.report,report);print('gates',json.dumps(report['gates']),flush=True)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['asset','control','mass-control','contact','memory-control','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--cutoff',type=float,default=1200.);p.add_argument('--phase',choices=['build','verify'],required=True)
    p.add_argument('--baseline-commit',default='d35cb4a')
    p.add_argument('--cells',type=int,nargs=2,default=[128,96])
    p.add_argument('--spatial-cells',type=int,nargs=2,default=[192,144])
    p.add_argument('--factor-rate',type=int,default=64000)
    p.add_argument('--parent-coefficients',type=Path)
    p.add_argument('--parent-report',type=Path)
    p.add_argument('--report',type=Path,default=Path(__file__).resolve().parents[1]/'docs'/'concert-grand-physics-roadmap-2026-10-02'/'resumen-P6-tabla-1200.json')
    args=p.parse_args()
    if bool(args.parent_coefficients)!=bool(args.parent_report):p.error('parent coefficients and verified parent report required together')
    if min(args.cells+args.spatial_cells)<2 or args.factor_rate<=0 or any(b<=a for a,b in zip(args.cells,args.spatial_cells)):p.error('positive factor rate and strictly finer spatial control required')
    args.output.mkdir(parents=True,exist_ok=True)
    modes=len(model_from_asset(args.asset,args.cutoff)['frequency'])
    args.coefficients=args.parent_coefficients or args.output/f'Wave{args.factor_rate}-Cells{args.cells[0]}x{args.cells[1]}-Angle96x96-Modes{modes}.npz'
    if args.phase=='build':build(args)
    else:verify(args)


if __name__=='__main__':main()
