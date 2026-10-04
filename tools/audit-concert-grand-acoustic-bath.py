"""Offline P2 spectral acoustic states: transfer, energy and finite horizon."""
import argparse,csv,hashlib,json,platform,time,zipfile
from pathlib import Path
import numpy as np
import scipy
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_acoustic_bath import AcousticBath,LoadedBoard
from concert_grand_orthotropic_plate import shape


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def relative(a,b):return float(np.linalg.norm(a-b)/np.linalg.norm(b))
def save_report(path,report):path.write_text(json.dumps(report,indent=2),encoding='utf-8')
def read_report(path):return json.loads(path.read_text(encoding='utf-8'))
def model_from_asset(path):
    with np.load(path) as data:
        select=data['frequency']<=800.
        return dict(panel=json.loads(str(data['panel'])),boundary=str(data['boundary']),nx=int(data['nx']),ny=int(data['ny']),
            vectors=data['vectors'][:,select],frequency=data['frequency'][select])


def spectrum(args):
    model=model_from_asset(args.asset);load=RectangularLoad.from_plate(model,128,96)
    frequency=np.r_[.01,np.arange(5.,1200.1,5.),np.arange(1220.,6000.1,20.)]
    rows=[];start=time.monotonic()
    for i,f in enumerate(frequency):
        rows.append(load.impedance(2j*np.pi*f,8).real)
        if i%100==0:print(json.dumps(dict(spectrum=i,total=len(frequency),seconds=time.monotonic()-start)),flush=True)
    # Independent evaluation points: not the spectral interpolation samples.
    test=np.unique(np.r_[np.linspace(60,800,75),80.,261.6255653,523.2511306,784.8766959])
    reference=np.array([load.impedance(2j*np.pi*f,8) for f in test])
    s=np.array([20+2j*np.pi*60,50+2j*np.pi*261.6255653,100+2j*np.pi*800])
    laplace=np.array([load.impedance(value,8) for value in s])
    np.savez_compressed(args.output/'spectrum.npz',frequency_hz=frequency,resistance_s_inv=np.array(rows),static_mass=load.added_mass_static(8),
        test_frequency_hz=test,test_impedance=reference,laplace_s=s,laplace_reference=laplace)
    save_report(args.output/'spectrum.json',dict(asset_sha256=digest(args.asset),source_sha256=digest(args.output/'spectrum.npz'),
        cells=[128,96],modes=len(model['frequency']),source_frequency_count=len(frequency),test_frequency_count=len(test),
        cutoff_hz=6000.,source_spacing_hz=[5.,20.],seconds=time.monotonic()-start))


def construct(data,gamma_hz):
    spacing=gamma_hz/2
    edges=np.r_[np.arange(0.,1200.+spacing/2,spacing),np.arange(1210.,6000.1,10.)]
    return AcousticBath.from_spectrum(data['frequency_hz'],data['resistance_s_inv'],data['static_mass'],edges,gamma_hz)


def refine_reference(args):
    # Resolve narrow board resonances explicitly, without regenerating the
    # separately cached spectral source curve.
    model=model_from_asset(args.asset);load=RectangularLoad.from_plate(model,128,96)
    with np.load(args.output/'spectrum.npz') as data:values={name:data[name] for name in data.files}
    test=np.unique(np.r_[np.linspace(60,800,149),model['frequency'][model['frequency']>=60],80.,261.6255653,523.2511306,784.8766959])
    values['test_frequency_hz']=test;values['test_impedance']=np.array([load.impedance(2j*np.pi*f,8) for f in test])
    np.savez_compressed(args.output/'spectrum.npz',**values)
    report=read_report(args.output/'spectrum.json');report.update(test_frequency_count=len(test),source_sha256=digest(args.output/'spectrum.npz'))
    save_report(args.output/'spectrum.json',report)


def realize(args):
    data=np.load(args.output/'spectrum.npz');reference=data['test_impedance'];frequencies=data['test_frequency_hz']
    criteria=dict(complex_max_relative=.02,resistance_max_relative=.02,reactance_max_relative=.02,
        laplace_max_relative=.02,static_mass_relative=1e-10,spectral_roundoff_relative=1e-10,loaded_velocity_max_relative=.05,
        time_energy_balance_relative=1e-9,time_velocity_refinement_relative=.001,
        time_displacement_refinement_relative=.001,broadening_velocity_relative=.01,port_energy_balance_relative=1e-9)
    report=dict(scope=__doc__,baseline_commit='ec03076',criteria=criteria,profiles={},
        density_kg_m3=1.2,speed_m_s=343.,frequency_band_hz=[60.,800.],test_frequency_count=len(frequencies),
        spectrum=read_report(args.output/'spectrum.json'),
        environment=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__),
        assumptions=['35 intrinsic modes of assumed P2; one face in an infinite rigid baffle',
            'Positive oscillator spectral quadrature, not a fitted Steinway or compact real-time model',
            'Numerical bath absorption is not identified radiative/material/air attenuation',
            'Static tail represents frequencies above 6000 Hz only to first inertial order',
            'Finite frequency grid must be assessed over the declared time horizon; distant recurrence is not piano physics'],
        references=['https://doc.comsol.com/6.3/doc/com.comsol.help.models.aco.wave_based_room/wave_based_room.html',
            'https://arxiv.org/pdf/0811.3625'])
    model=model_from_asset(args.asset);phi=shape(model,np.array([.642068965517241]),np.array([.5]))[0]
    stiffness=(2*np.pi*model['frequency'])**2;damping=.046*2*np.pi*model['frequency']
    csvrows=[]
    for gamma in (2.,1.,.5):
        start=time.monotonic();bath=construct(data,gamma)
        z=np.array([bath.impedance(2j*np.pi*f) for f in frequencies])
        laplace=np.array([bath.impedance(s) for s in data['laplace_s']])
        errors=[dict(frequency_hz=float(f),complex_relative=relative(a,b),resistance_relative=relative(a.real,b.real),
            reactance_relative=relative(a.imag,b.imag)) for f,a,b in zip(frequencies,z,reference)]
        for f,a,b,row in zip(frequencies,z,reference,errors):
            omega=2*np.pi*f;mechanical=np.diag(stiffness-omega**2+1j*omega*damping)
            exact_v=1j*omega*np.linalg.solve(mechanical+1j*omega*b,phi)
            actual_v=1j*omega*np.linalg.solve(mechanical+1j*omega*a,phi)
            row['loaded_velocity_relative']=relative(actual_v,exact_v)
        profile=dict(construction=bath.construction,seconds=time.monotonic()-start,
            complex_rms_relative=relative(z,reference),complex_max_relative=max(e['complex_relative'] for e in errors),
            resistance_rms_relative=relative(z.real,reference.real),resistance_max_relative=max(e['resistance_relative'] for e in errors),
            reactance_rms_relative=relative(z.imag,reference.imag),reactance_max_relative=max(e['reactance_relative'] for e in errors),
            laplace_max_relative=max(relative(a,b) for a,b in zip(laplace,data['laplace_reference'])),
            static_mass_relative=relative(bath.static_mass(),data['static_mass']),
            loaded_velocity_max_relative=max(e['loaded_velocity_relative'] for e in errors),
            positive_real_min_relative=float(min(np.linalg.eigvalsh(a.real).min()/np.linalg.norm(a.real,2) for a in z)))
        report['profiles'][str(gamma)]=profile
        for row in errors:csvrows.append([gamma,row['frequency_hz'],row['complex_relative'],row['resistance_relative'],row['reactance_relative'],row['loaded_velocity_relative']])
        if gamma==.5:
            np.savez_compressed(args.output/'bath-fine.npz',omega_rad_s=bath.omega,gamma_rad_s=bath.gamma,projection=bath.projection,tail_mass=bath.tail_mass)
        print(json.dumps(dict(gamma=gamma,profile=profile)),flush=True)
    with (args.output/'frequency-errors.csv').open('w',newline='',encoding='utf-8') as stream:
        writer=csv.writer(stream);writer.writerow(['gamma_hz','frequency_hz','complex_relative','resistance_relative','reactance_relative','loaded_velocity_relative']);writer.writerows(csvrows)
    fine=report['profiles']['0.5']
    report['frequency_gates']={key:bool(fine[key]<criteria[key]) for key in
        ('complex_max_relative','resistance_max_relative','reactance_max_relative','laplace_max_relative','static_mass_relative','loaded_velocity_max_relative')}
    report['frequency_gates']['positive_spectral_construction']=bool(max(fine['construction'][k] for k in
        ('source_roundoff_relative','spectral_roundoff_relative','tail_roundoff_relative'))<criteria['spectral_roundoff_relative'])
    save_report(args.output/'realization.json',report)
    if not all(report['frequency_gates'].values()):raise SystemExit('Frequency gate failed; keep as diagnostic')


def transient(args):
    report=read_report(args.output/'realization.json')
    if not all(report['frequency_gates'].values()):raise SystemExit('Frequency gates must pass before time candidate')
    model=model_from_asset(args.asset);data=np.load(args.output/'bath-fine.npz')
    bath=AcousticBath(data['omega_rad_s'],data['gamma_rad_s'],data['projection'],data['tail_mass'])
    K=np.diag((2*np.pi*model['frequency'])**2);D=np.diag(.046*2*np.pi*model['frequency'])
    phi=shape(model,np.array([.642068965517241]),np.array([.5]))[0]
    duration=.024;profiles={};velocities={};displacements={}
    configuration=[('Fine48',48000,.5),('Fine96',96000,.5),('Fine192',192000,.5),('Fine384',384000,.5),('No-absorption96',96000,0.),('Double-absorption96',96000,1.)]
    if args.profiles:
        names=args.profiles.split(',')
        if set(names)-{name for name,_,_ in configuration}:raise ValueError('unknown time profile')
        existing=read_report(args.output/'transient.json')
        if existing['bath_parameters_sha256']!=digest(args.output/'bath-fine.npz'):raise ValueError('changed bath parameters; cached time data invalid')
        profiles=existing['profiles']
        for name in profiles:
            with np.load(args.output/f'{name}.npz') as trajectory:
                velocities[name]=trajectory['board_velocity'];displacements[name]=trajectory['board_displacement']
        configuration=[entry for entry in configuration if entry[0] in names]
    for name,rate,gamma in configuration:
        current=AcousticBath(bath.omega,np.full(bath.ports,2*np.pi*gamma),bath.projection,bath.tail_mass)
        board=LoadedBoard(K,D,current);state=board.zero_state();initial=0.;work=material=absorption=port=0.;balance_max=port_max=0.;negative=0
        displacement=[];velocity=[];rows=[];start=time.monotonic()
        for i in range(round(duration*rate)+1):
            if i in (0,round(.008*rate)):
                old_air=(state[2].copy(),state[3].copy())
                state,jump=board.impulse(state,phi*(.002 if i==0 else .0015));work+=jump
                for before,after in zip(old_air,state[2:]):np.testing.assert_array_equal(before,after)
            energy=board.energy(state);balance_max=max(balance_max,abs(energy['total_j']+material+absorption-work)/work)
            displacement.append(state[0].copy());velocity.append(state[1].copy())
            if i%(rate//8000)==0:rows.append([i/rate,phi@state[0],phi@state[1],energy['wood_j'],energy['air_j'],material,absorption,work,port])
            if i==round(duration*rate):break
            old_air=energy['air_j'];state,stats=board.step(state,1/rate)
            material+=stats['material_loss_j'];absorption+=stats['bath_absorption_j'];port+=stats['air_port_work_j']
            negative+=stats['air_port_work_j']<0
            port_max=max(port_max,abs(stats['air_port_work_j']-(board.energy(state)['air_j']-old_air+stats['bath_absorption_j']))/work)
        profiles[name]=dict(rate_hz=rate,gamma_hz=gamma,duration_s=duration,seconds=time.monotonic()-start,
            final_energy=board.energy(state),external_impulse_work_j=work,material_loss_j=material,
            numerical_bath_absorption_j=absorption,air_port_work_j=port,negative_port_work_steps=negative,
            max_energy_balance_relative=balance_max,max_port_balance_relative=port_max)
        velocities[name]=np.array(velocity);displacements[name]=np.array(displacement)
        np.savez_compressed(args.output/f'{name}.npz',time_s=np.arange(len(velocity))/rate,board_displacement=displacements[name],board_velocity=velocities[name],
            air_displacement_final=state[2],air_velocity_final=state[3])
        with (args.output/f'{name}.csv').open('w',newline='',encoding='utf-8') as stream:
            writer=csv.writer(stream);writer.writerow(['time_s','bridge_displacement_m','bridge_velocity_m_s','wood_J','air_and_tail_J','material_heat_J','numerical_absorption_J','external_work_J','air_port_work_J']);writer.writerows(rows)
        print(json.dumps(dict(time=name,metrics=profiles[name])),flush=True)
    errors={}
    for a,b in (('Fine48','Fine96'),('Fine96','Fine192'),('Fine192','Fine384')):
        if a not in profiles or b not in profiles:continue
        errors[a+'-'+b]=dict(velocity_relative=relative(velocities[a],velocities[b][::2]),
            displacement_relative=relative(displacements[a],displacements[b][::2]))
    broadening={name:relative(velocities[name],velocities['No-absorption96']) for name in ('Fine96','Double-absorption96')}
    gate_pair='Fine192-Fine384' if 'Fine384' in profiles else 'Fine96-Fine192'
    fine=errors[gate_pair];c=report['criteria']
    gates=dict(energy=all(p['max_energy_balance_relative']<c['time_energy_balance_relative'] for p in profiles.values()),
        port=all(p['max_port_balance_relative']<c['port_energy_balance_relative'] for p in profiles.values()),
        velocity=fine['velocity_relative']<c['time_velocity_refinement_relative'],
        displacement=fine['displacement_relative']<c['time_displacement_refinement_relative'],
        broadening=broadening['Fine96']<c['broadening_velocity_relative'])
    save_report(args.output/'transient.json',dict(profiles=profiles,refinement=errors,refinement_gate_pair=gate_pair,broadening=broadening,gates=gates,
        source='two prescribed point-force impulses at 0/8 ms, not the nonlinear hammer or a factory note',
        damping='.046*omega assumed material for this diagnostic only',
        air_energy='includes near field, tail inertance and waves in the finite bath; absorption is numerical, not separately identified radiation',
        bath_parameters_sha256=digest(args.output/'bath-fine.npz'),
        horizon_note='24 ms; no validation of long tails or distant finite-bath recurrence'))
    if not all(gates.values()):raise SystemExit('Time gate failed; do not promote')


def summarize(args):
    report=read_report(args.output/'realization.json');report['transient']=read_report(args.output/'transient.json')
    # The inertial tail accepts instantaneous work at a prescribed impulse.
    # Account for that separately from the continuous port's timestep work.
    model=model_from_asset(args.asset)
    phi=shape(model,np.array([.642068965517241]),np.array([.5]))[0]
    with np.load(args.output/'bath-fine.npz') as parameters:tail=parameters['tail_mass']
    M=np.eye(len(tail))+tail
    for name,metrics in report['transient']['profiles'].items():
        with np.load(args.output/f'{name}.npz') as trajectory:velocity=trajectory['board_velocity']
        jumps=[]
        for index,amplitude in ((0,.002),(round(.008*metrics['rate_hz']),.0015)):
            after=velocity[index];before=after-np.linalg.solve(M,phi*amplitude)
            jumps.append(.5*float(after@tail@after-before@tail@before))
        impulsive=sum(jumps);total=metrics['air_port_work_j']+impulsive;work=metrics['external_impulse_work_j']
        metrics.update(impulsive_tail_work_j=impulsive,total_air_port_work_j=total,
            final_air_budget_relative=abs(metrics['final_energy']['air_j']+metrics['numerical_bath_absorption_j']-total)/work,
            final_wood_budget_relative=abs(metrics['final_energy']['wood_j']+metrics['material_loss_j']+total-work)/work)
        with (args.output/f'{name}.csv').open(encoding='utf-8',newline='') as stream:rows=list(csv.reader(stream))
        # A summary rerun is idempotent; leave the original continuous column.
        if len(rows[0])==9:
            rows[0][-1]='continuous_air_port_work_J';rows[0].extend(['impulsive_tail_work_J','total_air_port_work_J'])
            for row in rows[1:]:
                impulse_work=jumps[0]+(jumps[1] if float(row[0])>=.008 else 0.)
                row.extend([impulse_work,float(row[8])+impulse_work])
            with (args.output/f'{name}.csv').open('w',encoding='utf-8',newline='') as stream:csv.writer(stream).writerows(rows)
    report['transient']['gates']['global_wood_air_budgets']=all(max(p['final_air_budget_relative'],p['final_wood_budget_relative'])<report['criteria']['port_energy_balance_relative'] for p in report['transient']['profiles'].values())
    save_report(args.output/'transient.json',report['transient'])
    source_files=[Path(__file__),Path(__file__).with_name('concert_grand_acoustic_bath.py'),Path(__file__).parent/'tests/test_concert_acoustic_bath.py']
    report['sources']={str(p.relative_to(Path(__file__).resolve().parents[1])).replace('\\','/'):digest(p) for p in source_files}
    report['frozen_dependencies_sha256']={name:digest(Path(__file__).with_name(name)) for name in
        ('concert_grand_si_observer.py','concert_grand_surface_impedance.py','concert_grand_orthotropic_plate.py')}
    with zipfile.ZipFile(args.output/'source-acoustic-bath.zip','w',compression=zipfile.ZIP_DEFLATED) as archive:
        for p in source_files:archive.write(p,p.relative_to(Path(__file__).resolve().parents[1]))
    report['source_archive_sha256']=digest(args.output/'source-acoustic-bath.zip')
    report['bath_parameters_sha256']=digest(args.output/'bath-fine.npz')
    report['factory_package_sha256']=digest(Path('C:/SHARED/RF-Concert-Grand-0.172.0.rfplugin'))
    save_report(args.output/'resumen.json',report)
    print(json.dumps(dict(frequency=report['frequency_gates'],time=report['transient']['gates'])),flush=True)
    if not all(report['frequency_gates'].values()) or not all(report['transient']['gates'].values()):raise SystemExit('Unpassed gate; do not promote')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--phase',choices=['spectrum','reference','realize','time','summary'],required=True)
    parser.add_argument('--asset',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--profiles',help='Only rerun selected comma-separated time profiles against frozen cached parameters')
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    {'spectrum':spectrum,'reference':refine_reference,'realize':realize,'time':transient,'summary':summarize}[args.phase](args)
