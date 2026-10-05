"""Separate structural acceleration spectrum from microphone cancellation.

No sound shaping: compare pressure readouts of the same measured trajectory.
Band ratios are scale independent; structural acceleration is not audible SPL.
"""
import argparse,json,hashlib
from pathlib import Path
import numpy as np
from scipy.fft import rfft,rfftfreq
from scipy.io import wavfile
from concert_grand_cell_audio import retarded_kernel,pressure_fft
from concert_grand_cell_observer import CellObserver
from concert_grand_surface_impedance import RectangularLoad

def digest(path):
    with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()

def bands(x,rate):
    # Hann over a common early/sustain window, not an output filter.
    y=x*np.hanning(len(x))[:,None];S=abs(rfft(y,axis=0))**2
    f=rfftfreq(len(y),1/rate);power=lambda lo,hi:float(S[(f>=lo)&(f<hi)].sum())
    p=[power(a,b) for a,b in [(60,1200),(1200,3000),(3000,6000)]]
    return dict(power=p,upper_to_lower_db=10*np.log10(max(p[1]+p[2],1e-40)/max(p[0],1e-40)))

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--asset',type=Path,required=True)
    p.add_argument('--bank',type=Path,required=True);p.add_argument('--kernel',type=Path,required=True)
    p.add_argument('--report',type=Path,required=True);p.add_argument('--near-kernel',type=Path,required=True)
    a=p.parse_args()
    if a.report.exists() or a.near_kernel.exists():raise ValueError('preserve results')
    with np.load(a.asset) as d:
        model=dict(panel=json.loads(str(d['panel'])),nx=int(d['nx']),ny=int(d['ny']),boundary=str(d['boundary']),vectors=d['vectors'][:,:59])
        frequencies=d['frequency'];available=dict(count=len(frequencies),highest_hz=float(frequencies[-1]),retained_highest_hz=float(frequencies[58]))
    surface=RectangularLoad.from_plate(model,192,144)
    geometry=np.array([[.55,.5,.15],[.85,.5,.15]])
    near=retarded_kernel(CellObserver(surface,geometry,4),128000)
    # Fractional-delay check directly against the continuous Green function.
    from concert_grand_cell_audio import kernel_transfer
    probes=np.array([261.625565,800.,1200.,2000.,4500.,6000.])
    observer=CellObserver(surface,geometry,4);H=kernel_transfer(near,128000,probes);ref=observer.acceleration_transfer(probes)
    fractional=max(float(np.linalg.norm(x-y)/np.linalg.norm(y)) for x,y in zip(H,ref))
    fine=CellObserver(RectangularLoad.from_plate(model,256,192),geometry,4).acceleration_transfer(probes)
    spatial=max(float(np.linalg.norm(x-y)/np.linalg.norm(y)) for x,y in zip(ref,fine))
    with np.load(a.kernel) as d:old=d['kernel']
    rows={}
    for note in [64,67,72]:
        with np.load(a.bank/f'{note}-124'/'Coast-128000.npz') as d:B=d['board_position'];V=d['board_velocity']
        t=np.arange(len(B))/128000
        oldP=pressure_fft(t,B,V,old)[0];newP=pressure_fft(t,B,V,near)[0]
        acc=np.gradient(V,1/128000,axis=0)
        rows[str(note)]={}
        for label,lo,hi in [('early',.02,.12),('sustain',.15,.8)]:
            sl=slice(round(lo*128000),round(hi*128000))
            rows[str(note)][label]=dict(structural_acceleration=bands(acc[sl],128000),old_pressure=bands(oldP[sl],128000),near_pressure=bands(newP[sl],128000))
    a.near_kernel.parent.mkdir(parents=True,exist_ok=True)
    np.savez_compressed(a.near_kernel,kernel=near,rate_hz=128000,microphones_m=geometry)
    result=dict(available_board=available,geometry_m=geometry.tolist(),fractional_delay_relative=fractional,spatial_transfer_relative=spatial,
        notes=rows,inputs_sha256={str(x):digest(x) for x in [a.asset,a.kernel]},near_kernel_sha256=digest(a.near_kernel),
        scope='Same loaded 59-mode trajectory; moving observation only. Near placement is hypothetical, not Steinway microphone identification.')
    a.report.parent.mkdir(parents=True,exist_ok=True);a.report.write_text(json.dumps(result,indent=2),encoding='utf-8')
    print(json.dumps(result,indent=2))

if __name__=='__main__':main()
