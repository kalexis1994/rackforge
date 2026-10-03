"""Connect the frozen reciprocal SI bridge to a retarded surface observer.

Only the first 4 ms of C4/124 are observed. Frequency results are transforms
of that finite acceleration window, in Pa*s, not SPL or a sustained piano.
"""
import argparse,csv,hashlib,json,platform,zipfile
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
import scipy
from scipy.integrate import simpson
from concert_grand_si_observer import Surface,RayleighObserver

spec=spec_from_file_location('reciprocal_bridge',Path(__file__).with_name('audit-concert-grand-moving-bridge.py'))
bridge=module_from_spec(spec);spec.loader.exec_module(bridge)


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def acceleration_spectrum(t,B,V,frequency):
    # Integration by parts avoids differentiating sampled board velocity or
    # aliasing the high axial frequencies in a sampled acceleration trace.
    rows=[]
    for hz in frequency:
        omega=2*np.pi*hz;phase=np.exp(-1j*omega*t)
        integral=simpson(B*phase[:,None],x=t,axis=0)
        rows.append(phase[-1]*(V[-1]+1j*omega*B[-1])-V[0]-1j*omega*B[0]-omega**2*integral)
    return np.asarray(rows)


def capture(args):
    support,metadata=bridge.p2_support(args.asset,60)
    system,speed=bridge.load(args.input,60,124,support,args.transverse)
    result,metrics,_=bridge.solve(system,speed);core=system.string;n=system.size
    frozen=np.load(args.frozen)
    delta=np.r_[result.y[:n,-1]-frozen['Q'],result.y[n:2*n,-1]-frozen['P']]
    error=np.linalg.norm(delta)/np.linalg.norm(np.r_[frozen['Q'],frozen['P']]);assert error<1e-11
    B=np.array([core.physical(state[:n-1])[core.sb] for state in result.y.T])
    V=np.array([core.physical(state[n:2*n-1])[core.sb] for state in result.y.T])
    frequency=np.unique(np.r_[np.linspace(60,800,75),261.6255653,523.2511306,784.8766959])
    full=acceleration_spectrum(result.t,B,V,frequency)
    coarse=acceleration_spectrum(result.t[::2],B[::2],V[::2],frequency)
    quadrature=np.linalg.norm(coarse-full)/np.linalg.norm(full);assert quadrature<.001
    np.savez(args.output/'source.npz',time_s=result.t,board_displacement=B,board_velocity=V,
             frequency_hz=frequency,acceleration_spectrum=full)
    report=dict(transverse=args.transverse,axial=2*args.transverse,support=metadata,
        source_duration_s=.004,source_units='B sqrt(kg)*m; V sqrt(kg)*m/s; acceleration transform sqrt(kg)*m/s',
        source_temporal_quadrature_relative_error=float(quadrature),frozen_final_state_relative_error=float(error),
        input_sha256={p.name:digest(p) for p in sorted(args.input.glob('*.csv'))},
        frozen_state_sha256=digest(args.frozen),mechanical=metrics,source_sha256=digest(args.output/'source.npz'))
    (args.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps(dict(transverse=args.transverse,quadrature=quadrature,frozen_final_state=error)),flush=True)


def summarize(args):
    reports={n:json.loads((args.output/f'source{n}'/'report.json').read_text(encoding='utf-8')) for n in (48,72)}
    data={n:np.load(args.output/f'source{n}'/'source.npz') for n in (48,72)}
    frequency=data[72]['frequency_hz'];A=data[72]['acceleration_spectrum']
    np.testing.assert_array_equal(data[48]['frequency_hz'],frequency)
    with np.load(args.asset) as asset:
        selected=asset['frequency']<=800.
        model=dict(panel=json.loads(str(asset['panel'])),boundary=str(asset['boundary']),nx=int(asset['nx']),ny=int(asset['ny']),
            frequency=asset['frequency'][selected],vectors=asset['vectors'][:,selected])
    geometry=[(.55,.5,.66),(.85,.5,.66)]
    report=dict(scope=__doc__,mechanical_commit=args.commit,geometry_m=geometry,density_kg_m3=1.2,speed_m_s=343.,
        source_units=reports[72]['source_units'],pressure_transform_units='Pa*s, not SPL',
        declared_frequency_band_hz=[60,800],gain=1.,normalization='none; same SI input and geometry',
        assumptions=['Flat infinite rigid baffle; no lid/room/microphone pattern',
                     'Readout only: acoustic reaction and radiated energy are not in the mechanical budget',
                     'P2 low bank and geometry are assumed, not Steinway identification',
                     'Acceleration is windowed to 4 ms; no piano audition or tail validity claim'],
        sources=reports,spatial_quadrature={},sampled_delay={},asset_sha256=digest(args.asset),
        reference='https://acoustics.asn.au/conference_proceedings/INTERNOISE2014/papers/p898.pdf')
    transfers={};pressures={}
    for nx,ny in ((16,12),(32,24),(64,48),(96,72)):
        observer=RayleighObserver(Surface.from_plate(model,nx,ny),geometry)
        H=observer.acceleration_transfer(frequency);label=f'{nx}x{ny}'
        transfers[label]=H;pressures[label]=np.einsum('fcm,fm->fc',H,A)
    exact=transfers['96x72'];pressure=pressures['96x72']
    for label,H in transfers.items():
        report['spatial_quadrature'][label]=dict(
            transfer_relative_rms=float(np.linalg.norm(H-exact)/np.linalg.norm(exact)),
            source_pressure_relative_rms=float(np.linalg.norm(pressures[label]-pressure)/np.linalg.norm(pressure)))
    assert report['spatial_quadrature']['64x48']['transfer_relative_rms']<.001
    assert report['spatial_quadrature']['64x48']['source_pressure_relative_rms']<.001
    for rate in (48000,96000,192000):
        sampled=observer.sampled_transfer(frequency,rate);p=np.einsum('fcm,fm->fc',sampled,A)
        report['sampled_delay'][str(rate)]=dict(
            transfer_relative_rms=float(np.linalg.norm(sampled-exact)/np.linalg.norm(exact)),
            source_pressure_relative_rms=float(np.linalg.norm(p-pressure)/np.linalg.norm(pressure)))
    assert report['sampled_delay']['192000']['source_pressure_relative_rms']<.001
    low=np.einsum('fcm,fm->fc',exact,data[48]['acceleration_spectrum'])
    source_error=np.linalg.norm(low-pressure)/np.linalg.norm(pressure);assert source_error<.01
    report['mechanical_refinement_pressure_relative_rms']=float(source_error)
    no_delay=np.einsum('cm,fm->fc',observer.weight.sum(axis=1),A)
    modal=observer.acceleration_transfer(model['frequency']);frozen=modal[np.arange(len(model['frequency'])),:,np.arange(len(model['frequency']))].T
    frozen_pressure=np.einsum('cm,fm->fc',frozen,A)
    report['diagnostics']=dict(no_delay_relative_rms=float(np.linalg.norm(no_delay-pressure)/np.linalg.norm(pressure)),
        frozen_modal_phase_relative_rms=float(np.linalg.norm(frozen_pressure-pressure)/np.linalg.norm(pressure)),
        policy='Matched-input ablations of propagation, not fitted real-piano candidates')
    # Separate observer accuracy from whether the assumed mechanical losses
    # can represent this acoustic load. No pressure gain is changed here.
    load_frequency=np.array([80.,261.6255653,523.2511306,784.8766959])
    coarse_R=observer.radiation_resistance(load_frequency)
    fine_R=observer.radiation_resistance(load_frequency,polar=32,azimuth=64)
    load_error=np.linalg.norm(coarse_R-fine_R)/np.linalg.norm(fine_R);assert load_error<.001
    support=reports[72]['support'];omega0=2*np.pi*model['frequency'];c=.046*omega0
    projection=np.asarray(support['projection_kg_minus_half'])
    rows=[]
    for hz,R in zip(load_frequency,fine_R):
        omega=2*np.pi*hz;V=1j*omega*projection/(omega0**2-omega**2+1j*c*omega)
        radiated=.5*float(np.real(V.conj()@R@V));viscous=.5*float(np.dot(c,abs(V)**2))
        normalized=R/np.sqrt(c[:,None]*c[None,:])
        rows.append(dict(frequency_hz=float(hz),radiated_power_w_per_n_squared=radiated,
            modeled_viscous_power_w_per_n_squared=viscous,ratio=radiated/viscous,
            worst_state_resistance_to_loss=float(np.linalg.eigvalsh(normalized).max())))
    report['radiation_load_diagnostic']=dict(angular_refinement_relative_rms=float(load_error),rows=rows,
        policy='Bare P2 bank under harmonic unit force at the C4 point; not the transient or factory. '
               'The .046 loss may already include radiation. Identify its partition before adding acoustic damping; '
               'a dissipative resistance alone does not include acoustic reactive mass/stiffness.')
    with (args.output/'pressure-spectrum.csv').open('w',newline='') as f:
        writer=csv.writer(f);writer.writerow(['frequency_hz','left_real_pa_s','left_imag_pa_s','right_real_pa_s','right_imag_pa_s',
            'no_delay_left_real','no_delay_left_imag','frozen_phase_left_real','frozen_phase_left_imag'])
        writer.writerows((hz,p[0].real,p[0].imag,p[1].real,p[1].imag,d[0].real,d[0].imag,q[0].real,q[0].imag)
            for hz,p,d,q in zip(frequency,pressure,no_delay,frozen_pressure))
    tools=Path(__file__).resolve().parent
    files=[tools/name for name in ('concert_grand_si_observer.py','audit-concert-grand-si-observer.py','tests/test_concert_si_observer.py')]
    report['source_sha256']={str(p.relative_to(tools.parent)).replace('\\','/'):digest(p) for p in files}
    report['environment']=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__)
    report['factory_package_sha256']=digest(Path('C:/SHARED/RF-Concert-Grand-0.172.0.rfplugin'))
    assert report['factory_package_sha256']=='ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb'
    args.build.mkdir(parents=True,exist_ok=True);archive=args.build/'source-observer.zip'
    with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED) as out:
        for p in files:out.write(p,str(p.relative_to(tools.parent)))
    report['observer_source_archive_sha256']=digest(archive)
    (args.output/'resumen.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps({k:report[k] for k in ('spatial_quadrature','sampled_delay','mechanical_refinement_pressure_relative_rms','diagnostics')},indent=2))


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--phase',choices=('source','summary'),required=True)
    for name in ('input','asset','output'):parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--transverse',type=int,default=72);parser.add_argument('--frozen',type=Path)
    parser.add_argument('--build',type=Path,default=Path('local/concert-grand-si-observer/build'))
    parser.add_argument('--commit',default='346b6c8')
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    if args.phase=='source':
        if args.frozen is None:parser.error('--frozen required for source')
        capture(args)
    else:summarize(args)


if __name__=='__main__':main()
