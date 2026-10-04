"""Exact spherical air-port benchmark and SI contact integration.

The sphere is an analytical test geometry, not a calibrated piano panel.
The support's .05 loss is explicitly material loss in this benchmark. Nothing
is subtracted from or added to the factory's phenomenological damping.
"""
import argparse,csv,hashlib,json,time,zipfile,platform
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
import scipy
from scipy.integrate import solve_ivp
from scipy.linalg import expm
from concert_grand_air_load import SphereLoad,contact_sphere,AirCoupledContact,linear_generator
from concert_grand_moving_bridge import Support

spec=spec_from_file_location('bridge_benchmark',Path(__file__).with_name('audit-concert-grand-moving-bridge.py'))
bridge=module_from_spec(spec);spec.loader.exec_module(bridge)


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def linear_benchmark(output):
    mass=.25;f0=180.;radius=.18;omega0=2*np.pi*f0;material=.05*omega0
    sphere=SphereLoad([radius],[[1/np.sqrt(mass)]]);R=sphere.resistance[0];M=sphere.mass[0]
    frequency=np.linspace(40,400,3601);omega=2*np.pi*frequency
    physical_Z=sphere.impedance(frequency)[:,0,0]*mass
    resistive=float(sphere.impedance([f0])[0,0,0].real*mass)
    velocity={label:1j*omega/(mass*omega0**2-mass*omega**2+1j*omega*(mass*material+Z))
        for label,Z in [('Dry',0.),('Resistance-only',resistive),('Sphere',physical_Z)]}
    rows={label:dict(velocity_resonance_hz=float(frequency[np.abs(v).argmax()]),
        maximum_velocity_m_s_per_n=float(np.abs(v).max())) for label,v in velocity.items()}
    pressure=sphere.pressure_transfer(frequency,[.66])[:,0]*velocity['Sphere']
    with (output/'linear-frequency.csv').open('w',newline='') as f:
        writer=csv.writer(f);writer.writerow(['frequency_hz','dry_velocity_m_s_per_n','resistance_velocity_m_s_per_n',
            'sphere_velocity_m_s_per_n','sphere_pressure_pa_per_n','air_impedance_real_kg_s','air_impedance_imag_kg_s'])
        writer.writerows(zip(frequency,abs(velocity['Dry']),abs(velocity['Resistance-only']),abs(velocity['Sphere']),
            abs(pressure),physical_Z.real,physical_Z.imag))
    A,H,L=linear_generator([[omega0**2]],[[material]],sphere)
    initial=np.array([0.,np.sqrt(mass)*.02,0.]);initial_energy=.5*float(initial@H@initial)
    def derivative(t,state):
        port=sphere.response(state[1:2],state[2:3])
        return np.r_[A@state[:3],material*state[1]**2,port['escaped_power_w'],port['port_power_w']]
    result=solve_ivp(derivative,(0,.09),np.r_[initial,0.,0.,0.],method='DOP853',rtol=1e-11,atol=1e-15,
        max_step=.00002,t_eval=np.linspace(0,.09,1801))
    assert result.success
    balance=port_error=0.;negative_power=0.;trace=[]
    for t,state in zip(result.t,result.y.T):
        port=sphere.response(state[1:2],state[2:3]);total=.5*float(state[:3]@H@state[:3])
        balance=max(balance,abs(total+state[3]+state[4]-initial_energy)/initial_energy)
        port_error=max(port_error,abs(.5*state[2]**2+state[4]-state[5])/initial_energy)
        negative_power+=int(port['port_power_w']<0)
        trace.append((t,state[0]/np.sqrt(mass),state[1]/np.sqrt(mass),.5*state[2]**2,state[4],
            port['port_power_w'],port['escaped_power_w']))
    reference=expm(A*.09)@initial
    state_error=np.sqrt((result.y[:3,-1]-reference)@H@(result.y[:3,-1]-reference)/initial_energy)
    assert balance<1e-8 and port_error<1e-8 and state_error<1e-8 and negative_power>0
    with (output/'linear-transient.csv').open('w',newline='') as f:
        writer=csv.writer(f);writer.writerow(['time_s','displacement_m','velocity_m_s','stored_air_energy_j',
            'escaped_wave_energy_j','power_to_air_w','escaped_power_w']);writer.writerows(trace)
    final=result.y[:,-1]
    return dict(structural_mass_kg=mass,structural_frequency_hz=f0,material_loss=.05,radius_m=radius,
        radiation_resistance_high_limit_kg_s=float(R),air_mass_low_limit_kg=float(M),air_memory_s=float(M/R),
        resistance_only_kg_s=resistive,frequency_profiles=rows,
        initial_energy_j=initial_energy,final_stored_air_energy_j=float(.5*final[2]**2),
        final_escaped_wave_energy_j=float(final[4]),final_material_heat_j=float(final[3]),
        maximum_energy_balance_relative_error=float(balance),maximum_air_port_balance_relative_error=float(port_error),
        final_state_error_against_exponential=float(state_error),negative_port_power_samples=negative_power)


def contact_run(args,nt,profile):
    system,speed=bridge.load(args.input,60,124,Support.oscillator(),nt)
    load=contact_sphere(system,coupling=0. if profile=='Dry' else 1.)
    coupled=AirCoupledContact(system,load);n=coupled.size;b=system.size;q,p=coupled.initial_state(speed)
    initial=coupled.energy(q,p);fixed_R=float(load.resistance[0]*(2*np.pi*180*load.mass[0]/load.resistance[0])**2
        /(1+(2*np.pi*180*load.mass[0]/load.resistance[0])**2))
    def force_and_power(Q,P):
        if profile in ('Dry','Sphere'):
            return coupled.continuous_force(Q,P)
        g,heat=system.continuous_force(Q[:b],P[:b]);port=load.response(P[:b],P[b:])
        if profile=='One-way':return np.r_[g,-port['air_derivative']],heat,port
        v=load.projection@P[:b];force=fixed_R*v;power=float(force@v)
        return np.r_[g+load.projection.T@force,np.zeros(load.ports)],heat,dict(port_power_w=power,escaped_power_w=power)
    def derivative(t,state):
        g,heat,port=force_and_power(state[:n],state[n:2*n])
        return np.r_[state[n:2*n],-g,heat,port['escaped_power_w'],port['port_power_w']]
    def separation(t,state):return system.compression(state[:b])
    separation.direction=-1
    core=system.string;linear=core.gradient_hessian(np.zeros(core.size))[1]
    highest=np.sqrt(np.linalg.eigvalsh(linear).max())/(2*np.pi)
    result=solve_ivp(derivative,(0,.004),np.r_[q,p,0.,0.,0.],method='DOP853',rtol=1e-9,atol=1e-13,
        max_step=1/(8*highest),t_eval=np.linspace(0,.004,401),events=separation)
    assert result.success
    returning=[float(t) for t,state in zip(result.t_events[0],result.y_events[0]) if t>1e-8 and state[n+b-1]<0]
    assert returning
    balance=partition=0.;negative_power=0.;trace=[]
    for t,state in zip(result.t,result.y.T):
        Q,P=state[:n],state[n:2*n];_,_,port=force_and_power(Q,P)
        parts=core.components(Q[:b-1],P[:b-1]);air=.5*float(P[b:]@P[b:])
        external=state[2*n+2] if profile=='One-way' else 0.
        balance=max(balance,abs(coupled.energy(Q,P)+state[2*n]+state[2*n+1]-initial-external)/initial)
        partition=max(partition,abs(air+state[2*n+1]-state[2*n+2])/initial)
        negative_power+=int(port['port_power_w']<0)
        trace.append((t,parts['bridge_displacement_m'],parts['bridge_velocity_m_s'],parts['support_energy_j'],
            air,state[2*n+1],state[2*n],state[2*n+2],port['port_power_w']))
    assert balance<1e-7 and partition<1e-7
    final=result.y[:,-1];parts=core.components(final[:b-1],final[n:n+b-1])
    report=dict(profile=profile,transverse=nt,axial=2*nt,initial_hammer_energy_j=initial,
        final_string_energy_j=parts['string_energy_j'],final_support_energy_j=parts['support_energy_j'],
        final_hammer_energy_j=float(.5*final[n+b-1]**2),final_stored_air_energy_j=float(.5*final[n+b:2*n]@final[n+b:2*n]),
        final_escaped_wave_energy_j=float(final[2*n+1]),final_material_and_felt_heat_j=float(final[2*n]),
        final_port_work_j=float(final[2*n+2]),external_driver_work_j=float(final[2*n+2]) if profile=='One-way' else 0.,
        maximum_energy_balance_relative_error=float(balance),maximum_air_port_balance_relative_error=float(partition),
        contact_time_s=returning[0],negative_port_power_samples=negative_power,nfev=result.nfev)
    if profile=='Dry':
        frozen=np.load(args.frozen_root/'resolution'/f'60-124-Coupled-loss-T{nt}.npz')
        delta=np.r_[final[:b]-frozen['Q'],final[n:n+b]-frozen['P']]
        error=float(np.linalg.norm(delta)/np.linalg.norm(np.r_[frozen['Q'],frozen['P']]))
        assert error<1e-9;report['frozen_mechanical_state_relative_error']=error
    key=f'{profile}-T{nt}'
    with (args.output/f'{key}.csv').open('w',newline='') as f:
        writer=csv.writer(f);writer.writerow(['time_s','bridge_displacement_m','bridge_velocity_m_s','support_energy_j',
            'stored_air_energy_j','escaped_wave_energy_j','material_and_felt_heat_j','air_port_work_j','air_port_power_w']);writer.writerows(trace)
    np.savez(args.output/f'{key}.npz',Q=final[:n],P=final[n:2*n])
    return key,report


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--phase',choices=('linear','contact','summary'),required=True)
    for name in ('input','output','frozen-root'):parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--transverse',type=int,default=48)
    parser.add_argument('--profiles',nargs='+',choices=('Dry','Sphere','Resistance-only','One-way'),default=['Dry','Sphere','Resistance-only','One-way'])
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    if args.phase=='linear':
        report=linear_benchmark(args.output);(args.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        print(json.dumps(report,indent=2));return
    if args.phase=='contact':
        report=dict(scope=__doc__,cases={});start=time.monotonic()
        for profile in args.profiles:
            key,metrics=contact_run(args,args.transverse,profile);report['cases'][key]=metrics;report['elapsed_s']=time.monotonic()-start
            (args.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
            print(key,'escaped',metrics['final_escaped_wave_energy_j'],'stored air',metrics['final_stored_air_energy_j'],flush=True)
        return
    linear=json.loads((args.output/'linear'/'report.json').read_text(encoding='utf-8'))
    reports={n:json.loads((args.output/f'contact{n}'/'report.json').read_text(encoding='utf-8')) for n in (48,72)}
    left=reports[48]['cases']['Sphere-T48'];right=reports[72]['cases']['Sphere-T72']
    fields=('final_string_energy_j','final_support_energy_j','final_stored_air_energy_j','final_escaped_wave_energy_j','final_port_work_j','contact_time_s')
    relative={k:abs(left[k]-right[k])/abs(right[k]) for k in fields}
    a=np.loadtxt(args.output/'contact48'/'Sphere-T48.csv',delimiter=',',skiprows=1)
    b=np.loadtxt(args.output/'contact72'/'Sphere-T72.csv',delimiter=',',skiprows=1)
    for column,name in ((1,'bridge_displacement'),(2,'bridge_velocity')):relative[name+'_rms']=float(np.linalg.norm(a[:,column]-b[:,column])/np.linalg.norm(b[:,column]))
    passed=all(relative[k]<.002 for k in fields[:-1]) and relative['contact_time_s']<.001 and relative['bridge_velocity_rms']<.01 and relative['bridge_displacement_rms']<.01
    assert passed
    tools=Path(__file__).resolve().parent;files=[tools/p for p in ('concert_grand_air_load.py','audit-concert-grand-air-load.py','tests/test_concert_air_load.py')]
    report=dict(scope=__doc__,baseline_commit='aaa520d',linear=linear,contacts=reports,modal_refinement_relative_errors=relative,
        modal_gate_passed=passed,source_sha256={str(p.relative_to(tools.parent)).replace('\\','/'):digest(p) for p in files},
        environment=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__),
        contact_solver=dict(method='DOP853',duration_s=.004,rtol=1e-9,atol=1e-13,max_step='1/(8*highest linear structural frequency)'),
        input_sha256={p.name:digest(p) for p in sorted(args.input.glob('*.csv'))},
        references=['https://www.mathworks.com/help/audio/ref/sphericalsourceradiationimpedance.html'],
        limits=['One exact spherical load; not the P2 plate radiation matrix','Material loss fixed by benchmark definition, not factory loss partition',
                'One-string SI contact, 4 ms, C4/124 only; no audible piano candidate'])
    package=Path('C:/SHARED/RF-Concert-Grand-0.172.0.rfplugin');report['factory_package_sha256']=digest(package)
    assert report['factory_package_sha256']=='ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb'
    build=Path('local/concert-grand-air-load/build');build.mkdir(parents=True,exist_ok=True);archive=build/'source-air-load.zip'
    with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED) as out:
        for p in files:out.write(p,str(p.relative_to(tools.parent)))
    report['source_archive_sha256']=digest(archive)
    (args.output/'resumen.json').write_text(json.dumps(report,indent=2),encoding='utf-8');print(json.dumps(relative,indent=2))


if __name__=='__main__':main()
