"""Held-out transfer and finite-horizon diagnostics of paired-wave rational air."""
import argparse,hashlib,json,platform,time
from pathlib import Path
import numpy as np
import scipy
from concert_grand_rational_air import RationalAir,surface_factor
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_orthotropic_plate import shape

CRITERIA=dict(complex_max=.02,resistance_max=.02,reactance_max=.02,laplace_max=.02,
    loaded_velocity_max=.05,static_mass_error=1e-10,state_ratio=.1,
    memory_force_rms=.02,late_force_peak_normalized=.01,energy_budget=1e-9)


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def rel(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-30))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--asset',type=Path,required=True);parser.add_argument('--reference',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True);parser.add_argument('--rates',type=int,nargs='+',default=[16000,32000])
    parser.add_argument('--cells',type=int,nargs=2,default=[64,48]);parser.add_argument('--angular',type=int,nargs=2,default=[12,24])
    parser.add_argument('--contact-reference',type=Path);parser.add_argument('--memory-reference',type=Path)
    parser.add_argument('--profile');parser.add_argument('--memory-rates',type=int,nargs='+',default=[128000,256000])
    parser.add_argument('--reuse-factors',action='store_true',help='reuse the frozen Raw factor with exactly these grid parameters')
    parser.add_argument('--exact-quadrature',type=int,nargs='+');parser.add_argument('--exact-only',action='store_true')
    parser.add_argument('--wide-band',action='store_true',help='diagnose load above the accepted low-bank band, without fitting it')
    parser.add_argument('--coefficients',type=Path,help='read a frozen coefficient profile; write corrected measurements to a different output')
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    data=np.load(args.reference/'spectrum.npz')
    with np.load(args.asset) as d:
        select=d['frequency']<=800
        model=dict(panel=json.loads(str(d['panel'])),boundary=str(d['boundary']),nx=int(d['nx']),ny=int(d['ny']),
                   vectors=d['vectors'][:,select],frequency=d['frequency'][select])
    surface=RectangularLoad.from_plate(model,*args.cells)
    phi=shape(model,np.array([.642068965517241]),np.array([.5]))[0]
    K=(2*np.pi*model['frequency'])**2;D=.046*2*np.pi*model['frequency']
    report_path=args.output/'resumen.json'
    report=json.loads(report_path.read_text(encoding='utf-8')) if report_path.exists() else dict(
        criteria=CRITERIA,profiles={},baseline_commit='de2cb2e',asset_sha256=digest(args.asset),
        spectrum_sha256=digest(args.reference/'spectrum.npz'),panel=model['panel'],modes=surface.modes,
        environment=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__),
        method='Paired surface wave FIR, positive-real completion, Cayley continuous rational realization; no frequency fit',
        references=['https://www.dsprelated.com/freebooks/pasp/Digitizing_Bridge_Reflectance.html',
                    'https://arxiv.org/pdf/1904.13326'])
    report['criteria']=CRITERIA
    if report.get('force_convention_version')!='total-left-derivative-v2' and any('memory' in item for item in report['profiles'].values()):
        raise ValueError('Frozen v1 memory report: use a fresh output and --coefficients to preserve historical results')
    report['force_convention_version']='total-left-derivative-v2'
    if not args.profile:
        for rate in args.rates:
            start=time.monotonic()
            key=f'Wave{rate}-Cells{args.cells[0]}x{args.cells[1]}-Angle{args.angular[0]}x{args.angular[1]}-Raw'
            if args.reuse_factors:
                if report['asset_sha256']!=digest(args.asset) or report['spectrum_sha256']!=digest(args.reference/'spectrum.npz'):raise ValueError('frozen input changed')
                with np.load(args.output/f'{key}.npz') as frozen:raw=RationalAir(frozen['factor'],float(frozen['factor_rate']))
            else:raw=RationalAir(surface_factor(surface,rate,*args.angular),rate)
            calibrated,transform=raw.mass_congruent(data['static_mass'])
            variants=[('Raw',raw),('Mass',calibrated)]
            tail=data['static_mass']-raw.static_mass()
            try:variants.append(('Tail',raw.with_static_tail(data['static_mass'])))
            except ValueError:
                report.setdefault('rejected_tails',{})[f'{rate}-{args.cells}-{args.angular}']=np.linalg.eigvalsh(tail)[[0,-1]].tolist()
            for name,air in variants:
                key=f'Wave{rate}-Cells{args.cells[0]}x{args.cells[1]}-Angle{args.angular[0]}x{args.angular[1]}-{name}'
                z=np.array([air.impedance(2j*np.pi*f) for f in data['test_frequency_hz']])
                reference=data['test_impedance'];errors=[]
                for f,a,b in zip(data['test_frequency_hz'],z,reference):
                    s=2j*np.pi*f;mech=np.diag(K+s*D+s*s)
                    errors.append([rel(a,b),rel(a.real,b.real),rel(a.imag,b.imag),
                                   rel(np.linalg.solve(mech+s*a,phi),np.linalg.solve(mech+s*b,phi))])
                maximum=np.max(errors,axis=0)
                item=dict(states=air.states,state_ratio=air.states/352026,order=air.order,
                    wave_outputs=air.outputs,rate=rate,cells=args.cells,angular=args.angular,
                    stable_pole_rad_s=-air.a,reciprocity_error=air.reciprocity_error,dc_error=air.dc_error,
                    complex_max=float(maximum[0]),resistance_max=float(maximum[1]),reactance_max=float(maximum[2]),
                    loaded_velocity_max=float(maximum[3]),static_mass_error=rel(air.static_mass(),data['static_mass']),
                    raw_static_mass_error=rel(raw.static_mass(),data['static_mass']),
                    laplace_max=max(rel(air.impedance(s),b) for s,b in zip(data['laplace_s'],data['laplace_reference'])),
                    mass_eigenvalue_range=np.linalg.eigvalsh(air.static_mass())[[0,-1]].tolist(),
                    tail_eigenvalue_range=np.linalg.eigvalsh(air.tail_mass)[[0,-1]].tolist(),
                    congruence_change=rel(transform,np.eye(air.modes)) if name=='Mass' else 0.,
                    seconds=time.monotonic()-start)
                item['gates']={k:item[k]<v for k,v in CRITERIA.items() if k in item}
                report['profiles'][key]=item
                np.savez_compressed(args.output/f'{key}.npz',factor=air.H,factor_rate=air.rate,tail_mass=air.tail_mass,
                                    congruence=transform if name=='Mass' else np.eye(air.modes))
                np.savetxt(args.output/f'{key}-errors.csv',np.column_stack((data['test_frequency_hz'],errors)),delimiter=',',
                           header='frequency_hz,complex_relative,resistance_relative,reactance_relative,loaded_velocity_relative',comments='')
                report_path.write_text(json.dumps(report,indent=2),encoding='utf-8');print(key,json.dumps(item),flush=True)
    else:
        if args.contact_reference is None or args.memory_reference is None:parser.error('memory needs both frozen references')
        coefficients=args.coefficients or args.output
        if args.profile not in report['profiles']:
            if args.coefficients is None:parser.error('missing coefficient profile')
            frozen=json.loads((coefficients/'resumen.json').read_text(encoding='utf-8'))
            item=frozen['profiles'][args.profile].copy()
            item.pop('memory',None)
            report['profiles'][args.profile]=item
            report['inherited_frequency_manifest_sha256']=digest(coefficients/'resumen.json')
        report['profiles'][args.profile]['coefficient_input_sha256']=digest(coefficients/f'{args.profile}.npz')
        with np.load(coefficients/f'{args.profile}.npz') as d:air=RationalAir(d['factor'],float(d['factor_rate']),d['tail_mass'] if 'tail_mass' in d else None)
        if args.wide_band:
            geometric=RectangularLoad.from_plate(model,*report['profiles'][args.profile]['cells'])
            high=[]
            for f in [1000.,1200.,2000.,3000.,4500.,6000.]:
                physical=geometric.impedance(2j*np.pi*f,8);actual=air.impedance(2j*np.pi*f)
                high.append(dict(frequency_hz=f,complex_relative=rel(actual,physical),
                                 resistance_relative=rel(actual.real,physical.real),reactance_relative=rel(actual.imag,physical.imag)))
            report['profiles'][args.profile]['wide_band_diagnostic']=high
            shapes=geometric.shapes.reshape(-1,air.modes)
            physical_limit=geometric.density*geometric.speed*geometric.hx*geometric.hy*(shapes.T@shapes)
            wave_limit=air.D+np.einsum('k,kmn->mn',(-1.)**np.arange(1,air.order+1),air.R[1:])
            remaining=physical_limit-wave_limit
            report['profiles'][args.profile]['high_frequency_limit']=dict(
                physical_resistance_eigenvalue_range=np.linalg.eigvalsh(physical_limit)[[0,-1]].tolist(),
                missing_resistance_eigenvalue_range=np.linalg.eigvalsh(remaining)[[0,-1]].tolist(),
                passive_additive_resistance_possible=bool(np.linalg.eigvalsh(remaining).min()>=0),
                inertial_tail_is_unbounded_at_infinity=bool(np.any(air.tail_mass)))
            print('wide',json.dumps(high),flush=True)
        with np.load(args.contact_reference) as d:velocity=d['board_velocity']
        with np.load(args.memory_reference) as d:time_axis=d['time_s'];full=d['full_force']
        original_t=np.arange(len(velocity))/128000
        previous=None;profiles=report['profiles'][args.profile].get('memory',{}).get('profiles',{})
        scale=float(np.linalg.norm(full[:129],axis=1).max())
        certificate=[];rng=np.random.default_rng(373)
        for dt in [1/48000,1/192000,1/768000]:
            state=rng.normal(scale=1e-4,size=air.zero_state().shape)
            v0=rng.normal(scale=1e-4,size=air.modes);v1=rng.normal(scale=1e-4,size=air.modes);vm=(v0+v1)/2
            updated,force,loss=air.midpoint(state,vm,dt)
            e0=air.energy(state,v0);e1=air.energy(updated,v1)
            work=dt*(vm@force+vm@air.tail_mass@((v1-v0)/dt))
            certificate.append(dict(dt_s=dt,storage_j=e0,radiated_j=dt*loss,
                                    balance_relative=abs(e1-e0-work+dt*loss)/max(e0,abs(work),dt*loss,1e-30)))
        report['profiles'][args.profile]['physical_factor_certificate']=certificate
        report['profiles'][args.profile]['certificate_passed']=bool(max(x['balance_relative'] for x in certificate)<CRITERIA['energy_budget'])
        for rate in ([] if args.exact_only else args.memory_rates):
            start=time.monotonic();t=np.arange(round(.064*rate)+1)/rate
            drive=np.column_stack([np.interp(t,original_t,col,left=0,right=0) for col in velocity.T])
            # Post-stop values are zero; the shrinking last interval resolves
            # the prescribed stop. The cutoff bath's delta tail is separate.
            slope_full=np.zeros_like(drive)
            inside=(t>0)&(t<=.004)
            interval=np.clip(np.ceil(t[inside]*128000).astype(int)-1,0,len(velocity)-2)
            slope_full[inside]=np.diff(velocity,axis=0)[interval]*128000
            actual=air.sampled_force(drive,1/rate,slope_full)
            at=np.rint(time_axis*rate).astype(int);actual=actual[at]
            # linear_drive's frozen reference ALREADY includes its own tail
            # on the drive, with LEFT derivatives. Only stop deltas are absent.
            compared=actual
            error=compared-full;normalization=max(np.linalg.norm(full),1e-30)
            item=dict(rate_hz=rate,memory_force_rms=rel(compared,full),
                late_force_peak_normalized=float(np.linalg.norm((compared-full)[time_axis>=.010],axis=1).max()/scale),
                full_late_peak_normalized=float(np.linalg.norm(full[time_axis>=.010],axis=1).max()/scale),
                temporal_refinement=None if previous is None else rel(compared,previous),seconds=time.monotonic()-start,
                error_partition={name:float(np.linalg.norm(error[mask])/normalization) for name,mask in
                    [('driven',time_axis<.004),('stop_to_10ms',(time_axis>=.004)&(time_axis<.010)),('after_10ms',time_axis>=.010)]},
                maximum_error_time_s=float(time_axis[np.argmax(np.linalg.norm(error,axis=1))]),
                scope='Total regular force, own tail included with LEFT derivative at drive knots; both stop delta impulses excluded. Prescribed port, not a coupled piano note.',
                rational_direct_term_included=True)
            item['gates']={k:item[k]<CRITERIA[k] for k in ['memory_force_rms','late_force_peak_normalized']}
            profiles[str(rate)]=item;previous=compared
            np.savez_compressed(args.output/f'{args.profile}-Memory{rate}.npz',time_s=time_axis,
                                full_force=full,rational_total_force=actual,compared_force=compared)
            print(json.dumps(item),flush=True)
        memory=report['profiles'][args.profile].get('memory',{}).copy()
        memory.update(profiles=profiles,contact_input_sha256=digest(args.contact_reference),memory_reference_sha256=digest(args.memory_reference))
        if args.exact_quadrature:
            previous=None;exact={}
            for quadrature in args.exact_quadrature:
                start=time.monotonic()
                actual=air.linear_drive_force(velocity,1/128000,round(.060*128000),quadrature)[::4]
                compared=actual;error=compared-full
                item=dict(quadrature=quadrature,memory_force_rms=rel(compared,full),
                    late_force_peak_normalized=float(np.linalg.norm(error[time_axis>=.010],axis=1).max()/scale),
                    quadrature_refinement=None if previous is None else rel(compared,previous),
                    error_partition={name:float(np.linalg.norm(error[mask])/np.linalg.norm(full)) for name,mask in
                        [('driven',time_axis<.004),('stop_to_10ms',(time_axis>=.004)&(time_axis<.010)),('after_10ms',time_axis>=.010)]},
                    maximum_error_time_s=float(time_axis[np.argmax(np.linalg.norm(error,axis=1))]),
                    scope='Continuous rational step kernel and exact velocity jump; both stop tail delta impulses excluded, left wave force at stop knot',
                    seconds=time.monotonic()-start)
                item['gates']={k:item[k]<CRITERIA[k] for k in ['memory_force_rms','late_force_peak_normalized']}
                exact[str(quadrature)]=item;previous=compared
                np.savez_compressed(args.output/f'{args.profile}-Exact{quadrature}.npz',time_s=time_axis,
                                    full_force=full,rational_total_force=actual,compared_force=compared)
                print('exact',json.dumps(item),flush=True)
            memory['exact_profiles']=exact
        report['profiles'][args.profile]['memory']=memory
        report_path.write_text(json.dumps(report,indent=2),encoding='utf-8')


if __name__=='__main__':main()
