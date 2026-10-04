"""P6: complex load of the frozen P2 low bank, without factory changes."""
import argparse,csv,hashlib,json,platform,time,zipfile
from pathlib import Path
import numpy as np
import scipy
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_si_observer import Surface,RayleighObserver
from concert_grand_orthotropic_plate import shape


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def relative(a,b):return float(np.linalg.norm(a-b)/np.linalg.norm(b))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--asset',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--commit',default='3bbb170')
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    with np.load(args.asset) as data:
        mask=data['frequency']<=800.
        model=dict(panel=json.loads(str(data['panel'])),boundary=str(data['boundary']),
            nx=int(data['nx']),ny=int(data['ny']),vectors=data['vectors'][:,mask],frequency=data['frequency'][mask])
    frequency=np.array([60.,80.,261.6255653,523.2511306,784.8766959,800.])
    criteria=dict(grid_complex_relative=.01,grid_real_relative=.005,grid_imag_relative=.005,
        grid_static_mass_relative=.005,quadrature_relative=1e-5,
        same_surface_angular_relative=1e-8,continuous_angular_relative=.005,
        reciprocity_relative=1e-12,negative_eigenvalue_relative=1e-8,power_balance_relative=1e-10)
    report=dict(scope=__doc__,baseline_commit=args.commit,asset_sha256=digest(args.asset),
        panel=model['panel'],modes=len(model['frequency']),modal_cutoff_hz=800.,
        frequency_hz=frequency.tolist(),units='modal Z s^-1; static added mass dimensionless; intrinsic modal mass is identity',
        convention='exp(+i omega t), outgoing exp(-s*r/c); force on wood is -Z*V',
        density_kg_m3=1.2,speed_m_s=343.,criteria=criteria,
        assumptions=['Same frozen mass-normalized P2 shapes; uniform-cell averages approximate the continuous surface',
            'One face in an infinite rigid baffle; no second face, edge, lid or room',
            'P2 geometry is an assumption, not an identified Steinway',
            'Frequency samples are not a reduced causal passive time-domain realization',
            'Diagnostic harmonic mechanical loss .046 is assumed material; factory loss partition remains unknown'],
        environment=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__),
        grids={},quadrature={},references=['https://arxiv.org/pdf/2407.00265',
            'https://doi.org/10.1121/1.1430684','https://physics.byu.edu/docs/publication/3436'],sources={})
    grids={};masses={}
    for nx,ny in ((32,24),(64,48),(128,96),(192,144),(256,192)):
        start=time.monotonic();load=RectangularLoad.from_plate(model,nx,ny)
        z=np.array([load.impedance(2j*np.pi*f,8) for f in frequency]);mass=load.added_mass_static(8)
        label=f'{nx}x{ny}';grids[label]=z;masses[label]=mass
        np.savez(args.output/f'impedance-{label}.npz',frequency_hz=frequency,
            impedance_s_inv=z,static_added_mass=mass,mode_frequency_hz=model['frequency'])
        report['grids'][label]=dict(seconds=time.monotonic()-start,
            reciprocity_relative=relative(z,np.swapaxes(z,1,2)),
            resistance_min_eigenvalue_relative=float(min(np.linalg.eigvalsh(a.real).min()/np.linalg.norm(a.real,2) for a in z)),
            reactance_min_eigenvalue=float(min(np.linalg.eigvalsh(a.imag).min() for a in z)),
            static_mass_eigenvalue_range=np.linalg.eigvalsh(mass)[[0,-1]].tolist())
        print(json.dumps(dict(grid=label,seconds=report['grids'][label]['seconds'])),flush=True)
    reference=grids['256x192'];mass_reference=masses['256x192']
    for label,z in grids.items():
        report['grids'][label].update(complex_relative=relative(z,reference),
            real_relative=relative(z.real,reference.real),imag_relative=relative(z.imag,reference.imag),
            static_mass_relative=relative(masses[label],mass_reference),
            real_max_per_frequency_relative=max(relative(a.real,b.real) for a,b in zip(z,reference)),
            imag_max_per_frequency_relative=max(relative(a.imag,b.imag) for a,b in zip(z,reference)))
    load=RectangularLoad.from_plate(model,64,48)
    refined=np.array([load.impedance(2j*np.pi*f,16) for f in frequency])
    refined_mass=load.added_mass_static(16)
    report['quadrature']=dict(order8_to16_complex_relative=relative(grids['64x48'],refined),
        order8_to16_static_relative=relative(masses['64x48'],refined_mass))
    angular=np.array([load.angular_resistance(f,32,64) for f in frequency])
    report['same_surface_angular_relative']=relative(refined.real,angular)
    observer=RayleighObserver(Surface.from_plate(model,64,48),[(.55,.5,.66),(.85,.5,.66)])
    continuous=observer.radiation_resistance(frequency,32,64)
    report['continuous_angular_relative']=relative(reference.real,continuous)
    report['continuous_angular_per_frequency_relative']=[relative(a.real,b) for a,b in zip(reference,continuous)]
    # Samples in the open right half-plane: diagnostic, not a global
    # positive-real proof or justification to truncate a delayed kernel.
    s_points=[10+2j*np.pi*60,300+2j*np.pi*261.6255653,1000+2j*np.pi*800]
    report['right_half_plane']=[]
    for s in s_points:
        z=load.impedance(s,16);h=(z+z.conj().T)/2
        report['right_half_plane'].append(dict(s_real=s.real,s_imag=s.imag,
            hermitian_min_eigenvalue_relative=float(np.linalg.eigvalsh(h).min()/np.linalg.norm(h,2))))
    phi=shape(model,np.array([.642068965517241]),np.array([.5]))[0]
    stiffness=(2*np.pi*model['frequency'])**2;damping=.046*2*np.pi*model['frequency']
    report['harmonic_diagnostics']=[];rows=[]
    for f,z in zip(frequency,reference):
        omega=2*np.pi*f;mechanical=np.diag(stiffness-omega**2+1j*omega*damping)
        responses={}
        for name,air in (('Dry',np.zeros_like(z)),('Resistance-only',z.real),('Full',z),('Diagonal',np.diag(np.diag(z)))):
            q=np.linalg.solve(mechanical+1j*omega*air,phi);v=1j*omega*q
            responses[name]=v
            source=.5*np.real(np.vdot(phi,v));material=.5*np.sum(damping*abs(v)**2)
            wave=.5*np.real(np.vdot(v,air.real@v));balance=abs(source-material-wave)/source
            rows.append([f,name,abs(phi@v),source,material,wave,balance])
        r=z.real;x=z.imag
        report['harmonic_diagnostics'].append(dict(frequency_hz=float(f),
            resistance_off_diagonal_fraction=relative(r-np.diag(np.diag(r)),r),
            reactance_off_diagonal_fraction=relative(x-np.diag(np.diag(x)),x),
            full_vs_resistance_velocity_relative=relative(responses['Full'],responses['Resistance-only']),
            full_vs_diagonal_velocity_relative=relative(responses['Full'],responses['Diagonal'])))
    report['harmonic_power_balance_max_relative']=max(row[-1] for row in rows)
    with (args.output/'harmonic.csv').open('w',newline='',encoding='utf-8') as out:
        writer=csv.writer(out);writer.writerow(['frequency_hz','profile','bridge_mobility_m_s_N','input_power_W','material_heat_W','radiation_W','power_balance_relative']);writer.writerows(rows)
    grid=report['grids']['192x144']
    report['gates']=dict(grid_complex=grid['complex_relative']<criteria['grid_complex_relative'],
        grid_real=grid['real_max_per_frequency_relative']<criteria['grid_real_relative'],
        grid_imag=grid['imag_max_per_frequency_relative']<criteria['grid_imag_relative'],
        grid_mass=grid['static_mass_relative']<criteria['grid_static_mass_relative'],
        quadrature=max(report['quadrature'].values())<criteria['quadrature_relative'],
        angular_same=report['same_surface_angular_relative']<criteria['same_surface_angular_relative'],
        angular_continuous=max(report['continuous_angular_per_frequency_relative'])<criteria['continuous_angular_relative'],
        reciprocal=all(g['reciprocity_relative']<criteria['reciprocity_relative'] for g in report['grids'].values()),
        dissipative=all(g['resistance_min_eigenvalue_relative']>-criteria['negative_eigenvalue_relative'] for g in report['grids'].values()),
        reactive=all(g['reactance_min_eigenvalue']>0 for g in report['grids'].values()),
        static_mass=all(g['static_mass_eigenvalue_range'][0]>0 for g in report['grids'].values()),
        right_half_plane=all(g['hermitian_min_eigenvalue_relative']>-criteria['negative_eigenvalue_relative'] for g in report['right_half_plane']),
        harmonic_power=report['harmonic_power_balance_max_relative']<criteria['power_balance_relative'])
    report['gates']={key:bool(value) for key,value in report['gates'].items()}
    source_files=[Path(__file__),Path(__file__).with_name('concert_grand_surface_impedance.py'),Path(__file__).parent/'tests/test_concert_surface_impedance.py']
    report['sources']={str(p.relative_to(Path(__file__).resolve().parents[1])).replace('\\','/'):digest(p) for p in source_files}
    report['frozen_dependencies_sha256']={name:digest(Path(__file__).with_name(name)) for name in
        ('concert_grand_si_observer.py','concert_grand_orthotropic_plate.py')}
    with zipfile.ZipFile(args.output/'source-surface-impedance.zip','w',compression=zipfile.ZIP_DEFLATED) as archive:
        for p in source_files:archive.write(p,p.relative_to(Path(__file__).resolve().parents[1]))
    report['source_archive_sha256']=digest(args.output/'source-surface-impedance.zip')
    report['factory_package_sha256']=digest(Path('C:/SHARED/RF-Concert-Grand-0.172.0.rfplugin'))
    (args.output/'resumen.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps(dict(gates=report['gates'],angular=report['continuous_angular_relative'],grid=grid)),flush=True)
    if not all(report['gates'].values()):raise SystemExit('Unpassed P6 gate; do not promote')


if __name__=='__main__':main()
