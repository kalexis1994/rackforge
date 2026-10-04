"""Direct Rayleigh transient reference, with frozen air/rational comparisons."""
import argparse,hashlib,json,platform,time
from pathlib import Path
import numpy as np
import scipy
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_geometric_memory import GeometricMemory
from concert_grand_rational_air import RationalAir


def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for b in iter(lambda:f.read(1024*1024),b''):h.update(b)
    return h.hexdigest()


def rel(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-30))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--asset',type=Path,required=True);p.add_argument('--bath',type=Path,required=True)
    p.add_argument('--contact',type=Path,required=True);p.add_argument('--memory',type=Path,required=True)
    p.add_argument('--rational',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--cells',type=int,nargs=2,default=[128,96])
    p.add_argument('--rates',type=int,nargs='+',default=[128000,128000,256000]);p.add_argument('--quadratures',type=int,nargs='+',default=[8,16,8])
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    if len(args.rates)!=len(args.quadratures) or any(r<=0 or r%128000 for r in args.rates) or any(q<2 for q in args.quadratures):p.error('matching positive rates/quadratures; rates multiples of frozen input rate')
    with np.load(args.asset) as d:
        mask=d['frequency']<=800
        model=dict(panel=json.loads(str(d['panel'])),nx=int(d['nx']),ny=int(d['ny']),boundary=str(d['boundary']),vectors=d['vectors'][:,mask])
    surface=RectangularLoad.from_plate(model,*args.cells);start=time.monotonic();direct=GeometricMemory(surface)
    with np.load(args.contact) as d:velocity=d['board_velocity']
    with np.load(args.memory) as d:time_axis=d['time_s'];full=d['full_force']
    np.testing.assert_allclose(time_axis,np.arange(len(full))/32000,rtol=0,atol=1e-15)
    spectrum=np.load(args.bath/'spectrum.npz')
    rational={}
    for label in ['Wave32000-Cells128x96-Angle24x48-Tail','Wave64000-Cells128x96-Angle24x48-Tail','Wave32000-Cells128x96-Angle48x96-Tail']:
        with np.load(args.rational/f'{label}.npz') as d:air=RationalAir(d['factor'],float(d['factor_rate']),d['tail_mass'])
        rational[label]=air.linear_drive_force(velocity,1/128000,round(.060*128000),16)[::4]
    # The frozen bath helper ALREADY adds M_tail * LEFT segment acceleration
    # during the drive, including the last knot. It excludes only the stop
    # delta impulse. Never add or subtract the regular tail a second time.
    full_total=full
    scale=float(np.linalg.norm(full[:129],axis=1).max());norm=float(np.linalg.norm(full))
    high_f=np.array([1000.,1200.,2000.,3000.,4500.,6000.])
    high_z=np.array([surface.impedance(2j*np.pi*f,8) for f in high_f])
    # Low-band/Laplace targets remain the frozen 128x96 matrices. Additional
    # high-band targets independently evaluate the CURRENT cell grid.
    s=np.r_[2j*np.pi*spectrum['test_frequency_hz'],spectrum['laplace_s'],2j*np.pi*high_f]
    reference=np.concatenate((spectrum['test_impedance'],spectrum['laplace_reference'],high_z))
    low_count=len(spectrum['test_frequency_hz']);laplace_end=low_count+len(spectrum['laplace_s'])
    report=dict(baseline_commit='de2cb2e',method=__doc__,profiles={},
        panel=model['panel'],modes=direct.modes,cells=args.cells,max_delay_s=direct.max_delay,
        frozen_low_reference_cells=[128,96],additional_high_reference_cells=args.cells,
        setup_s=time.monotonic()-start,frequency_count=len(s),
        inputs={str(path):digest(path) for path in [args.asset,args.contact,args.memory,args.bath/'spectrum.npz',args.bath/'bath-fine.npz']},
        rational_inputs={name:digest(args.rational/f'{name}.npz') for name in rational},
        criteria=dict(reference_quadrature_relative=1e-5,time_refinement_relative=.001,
                      legacy_memory_force_rms=.02,legacy_late_peak_normalized=.01),
        force_convention='v2: total regular force, including each model own M_tail * LEFT segment acceleration at drive knots. Initial force zero; both inertial stop deltas excluded, geometric model has none. Wave force at stop knot is left limit.',
        normalization='All pairwise force errors use the SAME frozen bath total regular RMS and initial peak; no new normalizer to improve a score',
        scope='One lossless baffled face and frozen low P2 shapes, prescribed velocity; not a coupled integrator or piano audio',
        environment=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__),
        references=['https://doi.org/10.1121/1.1912541','https://doi.org/10.1121/1.1912424'])
    previous=None
    for rate,q in zip(args.rates,args.quadratures):
        begin=time.monotonic();key=f'Cells{args.cells[0]}x{args.cells[1]}-Rate{rate}-Q{q}'
        nodes,weights,packed=direct.quadrature_kernel(1/rate,q)
        mass_packed=np.einsum('q,tqm->m',weights,packed)
        mass=direct.unpack(mass_packed[None,:])[0]
        quadrature_weight=np.broadcast_to(weights,nodes.shape).ravel()
        zpacked=(np.exp(-s[:,None]*nodes.ravel()[None,:])*quadrature_weight)@packed.reshape(-1,len(direct.upper[0]))
        z=direct.unpack(zpacked)*s[:,None,None]
        errors=np.array([[rel(a,b),rel(a.real,b.real),rel(a.imag,b.imag)] for a,b in zip(z,reference)])
        integration=direct.unpack(np.einsum('q,tqm->tm',weights,packed));del packed
        input_t=np.arange(len(velocity))/128000
        drive_t=np.arange(round(.004*rate)+1)/rate
        drive=np.column_stack([np.interp(drive_t,input_t,v) for v in velocity.T])
        force=direct.linear_drive_force(drive,1/rate,round(.060*rate),q,integration)[::rate//32000]
        np.testing.assert_equal(force.shape,full.shape)
        def comparison(actual,target):
            error=actual-target
            return dict(force_rms_common=float(np.linalg.norm(error)/norm),
                        late_peak_common=float(np.linalg.norm(error[time_axis>=.010],axis=1).max()/scale),
                        error_partition={name:float(np.linalg.norm(error[mask])/norm) for name,mask in
                            [('driven',time_axis<.004),('stop_to_10ms',(time_axis>=.004)&(time_axis<.010)),('after_10ms',time_axis>=.010)]},
                        max_error_time_s=float(time_axis[np.argmax(np.linalg.norm(error,axis=1))]))
        old=comparison(full_total,force)
        item=dict(rate_hz=rate,quadrature=q,seconds=time.monotonic()-begin,
            static_mass_relative=rel(mass,spectrum['static_mass']),
            low_frequency_max=errors[:low_count].max(axis=0).tolist(),laplace_max=float(errors[low_count:laplace_end,0].max()),
            high_frequency_errors=[dict(frequency_hz=float(f),complex_relative=float(e[0]),resistance_relative=float(e[1]),reactance_relative=float(e[2])) for f,e in zip(high_f,errors[laplace_end:])],
            high_frequency_limit_relative=rel(direct.kernel([0])[0],surface.density*surface.speed*surface.hx*surface.hy*(surface.shapes.reshape(-1,direct.modes).T@surface.shapes.reshape(-1,direct.modes))),
            physical_force_after_support_peak_common=float(np.linalg.norm(force[time_axis>.004+direct.max_delay],axis=1).max()/scale),
            bath_vs_geometric=old,rational_vs_geometric={name:comparison(f,force) for name,f in rational.items()},
            rational_vs_bath={name:comparison(f,full_total) for name,f in rational.items()},
            refinement=None if previous is None else dict(force_rms_common=float(np.linalg.norm(force-previous[0])/norm),
                impedance_relative=rel(z,previous[1]),mass_relative=rel(mass,previous[2])))
        item['legacy_bath_consistency_passed']=bool(old['force_rms_common']<.02 and old['late_peak_common']<.01)
        item['geometric_reference_checks_passed']=bool(max(item['low_frequency_max'])<1e-5 and item['laplace_max']<1e-5
            and max(row['complex_relative'] for row in item['high_frequency_errors'])<1e-5
            and item['static_mass_relative']<1e-5 and item['high_frequency_limit_relative']<1e-10) if args.cells==[128,96] else None
        for comparison_set in ['rational_vs_bath','rational_vs_geometric']:
            for value in item[comparison_set].values():value['memory_passed']=bool(value['force_rms_common']<.02 and value['late_peak_common']<.01)
        report['profiles'][key]=item
        np.savez_compressed(args.output/f'{key}.npz',time_s=time_axis,geometric_total_force=force,
            bath_total_force=full_total,
            integration=integration,mode_mass=mass,laplace_s=s,impedance=z,
            rational_names=np.array(list(rational)),rational_total_force=np.array(list(rational.values())))
        (args.output/'resumen.json').write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
        print(key,json.dumps(item),flush=True);previous=force,z,mass


if __name__=='__main__':main()
