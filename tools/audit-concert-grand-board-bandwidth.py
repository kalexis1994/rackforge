"""Dry structural transfer truncation diagnostic, not a loaded audio renderer.

Compare modal pressure/force transfers for the same hypothetical P2 geometry.
Air feedback is intentionally absent in every control; never splice this dry
high-frequency response into the coupled note without reciprocal loading.
"""
import argparse,json
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from scipy.fft import rfft,rfftfreq
from scipy.io import wavfile
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_cell_observer import CellObserver
from concert_grand_cell_audio import retarded_kernel
s=spec_from_file_location('colour_metrics',Path(__file__).with_name('audit-concert-grand-phrase-colour.py'));metrics=module_from_spec(s);s.loader.exec_module(metrics)
s=spec_from_file_location('band_bridge',Path(__file__).with_name('audit-concert-grand-moving-bridge.py'));bridge=module_from_spec(s);s.loader.exec_module(bridge)

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--asset',type=Path,required=True);p.add_argument('--report',type=Path,required=True)
    p.add_argument('--reference',type=Path,required=True);p.add_argument('--c4',type=Path,required=True);p.add_argument('--c5',type=Path,required=True)
    a=p.parse_args()
    if a.report.exists():raise ValueError('preserve measurements')
    with np.load(a.asset) as d:
        model=dict(panel=json.loads(str(d['panel'])),nx=int(d['nx']),ny=int(d['ny']),boundary=str(d['boundary']),vectors=d['vectors'])
    geometry=np.array([[.55,.5,.66],[.85,.5,.66]])
    surface=RectangularLoad.from_plate(model,192,144);kernel=retarded_kernel(CellObserver(surface,geometry,4),128000)
    size=16384;f=rfftfreq(size,1/128000);mask=(f>=60)&(f<=6000);f=f[mask];H=rfft(kernel,n=size,axis=0)[mask]
    rows={}
    for note in [60,64,67,72]:
        support,_=bridge.p2_support(a.asset,note,6000);w=2*np.pi*f
        B=np.asarray(support.projection)[None,:]/((2*np.pi*np.asarray(support.frequency))**2-w[:,None]**2+1j*w[:,None]*.046*2*np.pi*np.asarray(support.frequency))
        spectra={n:-w[:,None]**2*np.einsum('fcm,fm->fc',H[:,:,:n],B[:,:n]) for n in [59,90,256]}
        rows[str(note)]={}
        for label,lo,hi in [('low',60,1200),('middle',1200,3000),('upper',3000,6000)]:
            idx=(f>=lo)&(f<hi);ref=spectra[256][idx];norm=np.linalg.norm(ref)
            rows[str(note)][label]={str(n):dict(relative_to_256=float(np.linalg.norm(spectra[n][idx]-ref)/norm),power_relative_db=float(20*np.log10(np.linalg.norm(spectra[n][idx])/norm))) for n in [59,90]}
    audio={}
    for label,path in [('C4-pilot',a.c4),('C5-pilot',a.c5),('C4-reference',a.reference/'C4v16.wav'),('C5-reference',a.reference/'C5v16.wav')]:
        rate,x=wavfile.read(path);x=x.astype(float)/(32768 if x.dtype==np.int16 else 1) if np.issubdtype(x.dtype,np.integer) else x.astype(float)
        start=.1 if label.endswith('pilot') else 0
        audio[label]={name:metrics.bands(x[round((start+lo)*rate):round((start+hi)*rate)],rate) for name,lo,hi in [('early',.02,.12),('sustain',.15,.8)]}
    result=dict(structural_controls=rows,audio_band_metrics=audio,highest_available_mode_hz=max(support.frequency),
        source_sha256=metrics.digest(Path(__file__)),inputs_sha256={str(x):metrics.digest(x) for x in [a.asset,a.c4,a.c5,a.reference/'C4v16.wav',a.reference/'C5v16.wav']},
        scope='Dry harmonic unit bridge force, same loss .046 and readout; diagnostic of modal truncation only. No air feedback, real note waveform or high-mode convergence claim. Reference windows are recording-start aligned, not fitted.')
    a.report.parent.mkdir(parents=True,exist_ok=True);a.report.write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps(result,indent=2))

if __name__=='__main__':main()
