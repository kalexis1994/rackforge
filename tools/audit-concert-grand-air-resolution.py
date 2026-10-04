"""Equivalent sparse cell quadrature and fine-angle temporal refinement.

Generate one passive surface factor without changing the frozen physical
targets, mass, normalizers or acceptance criteria. A 32k control matches
the archived dense factor before generating the higher-rate coefficient.
"""
import argparse,hashlib,json,platform,time
from pathlib import Path
import numpy as np
import scipy
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_rational_air import surface_factor_sparse


def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for chunk in iter(lambda:f.read(1024*1024),b''):h.update(chunk)
    return h.hexdigest()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['asset','control','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--rate',type=int,default=64000)
    p.add_argument('--angular',type=int,nargs=2,default=[48,96])
    args=p.parse_args()
    if args.rate<=32000:p.error('refinement rate must exceed 32000')
    args.output.mkdir(parents=True,exist_ok=True)
    manifest=args.output/'factors.json'
    if manifest.exists():raise ValueError('use a new output to preserve the frozen coefficient provenance')
    with np.load(args.asset) as d:
        mask=d['frequency']<=800
        model=dict(panel=json.loads(str(d['panel'])),nx=int(d['nx']),ny=int(d['ny']),boundary=str(d['boundary']),vectors=d['vectors'][:,mask])
    surface=RectangularLoad.from_plate(model,128,96)
    start=time.monotonic();control=surface_factor_sparse(surface,32000,48,96);elapsed=time.monotonic()-start
    with np.load(args.control) as d:old=d['factor']
    error=float(np.linalg.norm(control-old)/np.linalg.norm(old))
    report=dict(baseline_commit='5834c39',asset_sha256=digest(args.asset),control_sha256=digest(args.control),
        cells=[128,96],angular=args.angular,modes=surface.modes,control_angular=[48,96],
        criteria=dict(factor_equivalence_relative=1e-10),
        control=dict(rate_hz=32000,equivalence_relative=error,seconds=elapsed,passed=error<1e-10),
        method='Stable projected-cell CDF; sparse bins; same area, delay histogram, backward derivative and opposite-direction reversal as archived dense generator.',
        environment=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__))
    manifest.write_text(json.dumps(report,indent=2),encoding='utf-8')
    print('control',json.dumps(report['control']),flush=True)
    if not report['control']['passed']:raise AssertionError('sparse construction changes frozen factor')
    del control,old
    start=time.monotonic();H=surface_factor_sparse(surface,args.rate,*args.angular)
    label=f'Wave{args.rate}-Cells128x96-Angle{args.angular[0]}x{args.angular[1]}-Sparse'
    path=args.output/(label+'.npz')
    np.savez_compressed(path,factor=H,factor_rate=args.rate)
    report['generated']=dict(profile=label,rate_hz=args.rate,seconds=time.monotonic()-start,
        taps=len(H),wave_outputs=H.shape[1],wave_states=(len(H)-1)*H.shape[2],sha256=digest(path),
        dc_sum_relative=float(np.linalg.norm(H.sum(axis=0))/np.linalg.norm(H)),
        angular=args.angular,construction='Unchanged physical surface and projected-cell integral; temporal and angular quadrature are controlled refinements.')
    manifest.write_text(json.dumps(report,indent=2),encoding='utf-8')
    print('generated',json.dumps(report['generated']),flush=True)


if __name__=='__main__':main()
