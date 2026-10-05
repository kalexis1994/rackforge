"""Separate spatial, angular and Cayley-warp errors at failed air-load probes.

Diagnostic only: a predicted resistance is not a new passive realization.
Reports belong in the repository; only new numerical arrays go in SHARED.
"""
import argparse,hashlib,json,time
from pathlib import Path
import numpy as np
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_bounded_air import BoundedAir


def digest(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()

def rel(a,b):return float(np.linalg.norm(a-b)/np.linalg.norm(b))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['asset','coefficients','load-report','report','output']:p.add_argument('--'+name,type=Path,required=True)
    args=p.parse_args();target=args.output/'discretization.npz'
    if args.report.exists() or target.exists():raise ValueError('preserve measured diagnostics')
    started=time.monotonic();load=json.loads(args.load_report.read_text())
    if digest(args.coefficients)!=load['coefficient_sha256']:raise ValueError('frozen coefficient changed')
    with np.load(args.asset) as data:
        mask=data['frequency']<=load['cutoff_hz']
        model=dict(panel=json.loads(str(data['panel'])),nx=int(data['nx']),ny=int(data['ny']),boundary=str(data['boundary']),vectors=data['vectors'][:,mask])
    frequencies=np.array([load['transfer']['high_max_frequency_hz'],6000.])
    surfaces={n:RectangularLoad.from_plate(model,n,n*3//4) for n in [128,192,256]}
    with np.load(args.coefficients) as data:
        rate=float(data['factor_rate']);air=BoundedAir(data['factor'],rate,data['static_mass'],data['high_limit'])
    wave=np.array([air.wave.impedance(2j*np.pi*f) for f in frequencies])
    relaxation=np.array([air.relaxation.impedance(2j*np.pi*f) for f in frequencies])
    del air
    saved=dict(frequency_hz=frequencies);rows=[]
    for i,f in enumerate(frequencies):
        geometry={n:surface.impedance(2j*np.pi*f,8) for n,surface in surfaces.items()}
        R96=surfaces[128].angular_resistance_streamed(f,96,96)
        R192=surfaces[128].angular_resistance_streamed(f,192,192)
        row=dict(frequency_hz=float(f),spatial128_to192=rel(geometry[128],geometry[192]),
            spatial192_to256=rel(geometry[192],geometry[256]),angular96_to192=rel(R96,R192),
            angular192_to_duffy=rel(R192,geometry[128].real),
            actual_complex_error=rel(wave[i]+relaxation[i],geometry[128]),
            actual_resistance_error=rel(wave[i].real+relaxation[i].real,geometry[128].real),
            actual_reactance_error=rel(wave[i].imag+relaxation[i].imag,geometry[128].imag),warp={})
        for test_rate in [64000,96000,128000]:
            theta=2*np.arctan(2*np.pi*f/(2*test_rate));warped=test_rate*theta/(2*np.pi)
            Rwarp=surfaces[128].impedance(2j*np.pi*warped,8).real*(np.sin(theta)/theta)**2
            # The adjacent-tap smoothing changes the backward derivative
            # to rate*sin(theta). This estimates its exact-cell wave Gram;
            # histogram discretization and the mass completion are separate.
            row['warp'][str(test_rate)]=dict(effective_frequency_hz=float(warped),
                wave_resistance_difference=rel(Rwarp,geometry[128].real))
            if test_rate==rate:
                row['warp'][str(test_rate)]['actual_wave_vs_prediction']=rel(wave[i].real,Rwarp)
                row['warp'][str(test_rate)]['completed_prediction_error']=rel(Rwarp+relaxation[i].real,geometry[128].real)
            saved[f'warped_resistance_F{i}_R{test_rate}']=Rwarp
        for n,Z in geometry.items():saved[f'geometric_impedance_F{i}_N{n}']=Z
        saved[f'angular_resistance_F{i}_96']=R96;saved[f'angular_resistance_F{i}_192']=R192
        rows.append(row);print('probe',json.dumps(row),flush=True)
    args.output.mkdir(parents=True,exist_ok=True);np.savez_compressed(target,**saved)
    report=dict(baseline_commit='68a508a',scope='Two failed/high-band diagnostic probes; no candidate acceptance, new damping or full-resolution convergence claim.',
        criteria_changed=False,inputs={str(path):digest(path) for path in [args.asset,args.coefficients,args.load_report]},
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in ['audit-concert-grand-air-discretization.py','concert_grand_surface_impedance.py','concert_grand_bounded_air.py','concert_grand_rational_air.py']},
        rows=rows,measurement_sha256=digest(target),seconds=time.monotonic()-started)
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')


if __name__=='__main__':main()
