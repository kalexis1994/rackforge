"""Independent geometric band sweep and polar/azimuth resolution controls."""
import argparse,hashlib,json,time
from pathlib import Path
import numpy as np
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_geometric_memory import GeometricMemory
from concert_grand_bounded_air import BoundedAir


def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda:f.read(1024*1024),b''):h.update(block)
    return h.hexdigest()
def rel(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-30))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['asset','coefficients','geometry','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--redo-probe',action='store_true')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    with np.load(args.asset) as d:
        mask=d['frequency']<=800
        model=dict(panel=json.loads(str(d['panel'])),nx=int(d['nx']),ny=int(d['ny']),boundary=str(d['boundary']),vectors=d['vectors'][:,mask])
    surface=RectangularLoad.from_plate(model,128,96)
    shapes=surface.shapes.reshape(-1,surface.modes)
    limit=surface.density*surface.speed*surface.hx*surface.hy*shapes.T@shapes
    files=[args.geometry/'Cells128x96-Rate128000-Q16.npz',args.geometry/'spatial192/Cells192x144-Rate128000-Q16.npz',args.geometry/'spatial256/Cells256x192-Rate128000-Q16.npz']
    with np.load(files[0]) as d:common_high=d['impedance'][-6:];probe_target=d['impedance'][-1].real
    # Use the frozen DC target exactly, rather than the quadrature's roundoff.
    target_path=Path('C:/SHARED/Fisica-pendiente-2026-10-03/P6-estados-acusticos/spectrum.npz')
    with np.load(target_path) as d:mass=d['static_mass']
    with np.load(args.coefficients) as d:air=BoundedAir(d['factor'],float(d['factor_rate']),mass,limit)
    probe_path=args.output/'angular-probe.json'
    if args.redo_probe or not probe_path.exists():
        rows=[]
        for polar,azimuth in [(48,96),(96,96),(48,192),(96,192)]:
            start=time.monotonic();res=surface.angular_resistance_streamed(6000,polar,azimuth)
            row=dict(polar=polar,azimuth=azimuth,resistance_relative=rel(res,probe_target),seconds=time.monotonic()-start)
            rows.append(row);print('angular',json.dumps(row),flush=True)
        probe_path.write_text(json.dumps(dict(frequency_hz=6000,reference='Direct Rayleigh Gauss16 128x96',profiles=rows),indent=2),encoding='utf-8')
    probe=json.loads(probe_path.read_text(encoding='utf-8'))
    if probe['frequency_hz']!=6000 or [(r['polar'],r['azimuth']) for r in probe['profiles']]!=[(48,96),(96,96),(48,192),(96,192)]:raise ValueError('different angular control')
    start=time.monotonic();memory=GeometricMemory(surface)
    nodes,weights,packed=memory.quadrature_kernel(1/128000,16)
    frequency=np.linspace(1000.,6000.,61);s=2j*np.pi*frequency
    weighted=np.broadcast_to(weights,nodes.shape).ravel()
    transform=(np.exp(-s[:,None]*nodes.ravel()[None,:])*weighted)@packed.reshape(-1,len(memory.upper[0]))
    reference=memory.unpack(transform)*s[:,None,None]
    actual=np.array([air.impedance(point) for point in s]);errors=np.array([[rel(a,b),rel(a.real,b.real),rel(a.imag,b.imag)] for a,b in zip(actual,reference)])
    controls={}
    for cells,path in zip(['128x96','192x144','256x192'],files):
        with np.load(path) as d:points=d['laplace_s'][-6:];target=d['impedance'][-6:]
        values=[dict(frequency_hz=float(point.imag/(2*np.pi)),complex_relative=rel(air.impedance(point),matrix)) for point,matrix in zip(points,target)]
        controls[cells]=dict(points=values,maximum_complex=max(v['complex_relative'] for v in values),
            geometry_change_relative_to_128=max(rel(a,b) for a,b in zip(target,common_high)))
    report=dict(baseline_commit='5834c39',coefficient_sha256=digest(args.coefficients),asset_sha256=digest(args.asset),target_sha256=digest(target_path),
        geometry_inputs={str(path):digest(path) for path in files},angular_probe_sha256=digest(probe_path),
        criteria=dict(complex_max=.02,reference_quadrature=16,reference_rate_hz=128000),
        sweep=dict(points=len(frequency),low_hz=1000,high_hz=6000,maximum_complex=float(errors[:,0].max()),
            maximum_complex_frequency_hz=float(frequency[np.argmax(errors[:,0])]),maximum_resistance=float(errors[:,1].max()),
            passed=bool(errors[:,0].max()<.02)),spatial_controls=controls,
        high_grid_change_common_128={cells:controls[cells]['geometry_change_relative_to_128'] for cells in ['192x144','256x192']},
        seconds=time.monotonic()-start,
        scope='Transfer of 35 frozen low P2 surface shapes through one baffled face; this does not add physical board modes above 800 Hz or certify the continuous full piano.',
        normalization='Transfer relative to geometric matrices; force RMS in separate audit retains its frozen bath denominator.')
    report['all_measured_high_controls_passed']=report['sweep']['passed'] and all(c['maximum_complex']<.02 for c in controls.values())
    np.savez_compressed(args.output/'band-sweep.npz',frequency_hz=frequency,actual_impedance=actual,geometric_impedance=reference,errors=errors)
    np.savetxt(args.output/'band-sweep.csv',np.column_stack((frequency,errors)),delimiter=',',header='frequency_hz,complex_relative,resistance_relative,reactance_relative',comments='')
    (args.output/'band.json').write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    print('band',json.dumps(report),flush=True)


if __name__=='__main__':main()
