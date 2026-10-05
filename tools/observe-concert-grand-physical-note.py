"""Verified cell readout and first listening file for the sustained SI pilot."""
import argparse,hashlib,json,time
from pathlib import Path
import numpy as np
from scipy.fft import rfft,rfftfreq
from scipy.signal import resample_poly
from scipy.io import wavfile
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_cell_observer import CellObserver,spectral_relative
from concert_grand_cell_audio import retarded_kernel,kernel_transfer,hermite_fft,pressure_fft

GEOMETRY=np.array([[.55,.5,.66],[.85,.5,.66]])
PROBES=np.array([0.,261.6255653,800.,1200.,2000.,4500.,6000.])
CRITERIA=dict(fractional_delay_transfer=.0002,cell_quadrature=.00001,
              spatial_transfer=.01,capture_pressure=.001)


def digest(path):
    with Path(path).open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def write(path,data):
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(json.dumps(data,indent=2,allow_nan=False),encoding='utf-8')
def relative(actual,reference):
    return float(np.linalg.norm(actual-reference)/max(np.linalg.norm(reference),1e-30))


def prepare(args):
    if args.kernel.exists() or args.report.exists():raise ValueError('preserve existing measurements')
    source=json.loads(args.contact_report.read_text());p=source['provenance']
    if not all(source['gates'].values()) or digest(args.asset)!=p['asset_sha256']:raise ValueError('validated unchanged physical asset required')
    count=p['support']['modes'];cells=p['acoustic_discretization']['cells']
    with np.load(args.asset) as d:
        frequency=d['frequency'][:count]
        model=dict(panel=json.loads(str(d['panel'])),nx=int(d['nx']),ny=int(d['ny']),boundary=str(d['boundary']),vectors=d['vectors'][:,:count])
    if not np.array_equal(frequency,p['support']['frequencies_hz']):raise ValueError('modal order changed')
    start=time.monotonic();surface=RectangularLoad.from_plate(model,*cells)
    observer=CellObserver(surface,GEOMETRY,4);kernel=retarded_kernel(observer,128000)
    reference=observer.acceleration_transfer(PROBES)
    H=kernel_transfer(kernel,128000,PROBES)
    H8=CellObserver(surface,GEOMETRY,8).acceleration_transfer(PROBES)
    Hfine=CellObserver(RectangularLoad.from_plate(model,256,192),GEOMETRY,4).acceleration_transfer(PROBES)
    checks=dict(fractional_delay_max=max(relative(a,b) for a,b in zip(H,reference)),
                quadrature_max=max(relative(a,b) for a,b in zip(reference,H8)),
                spatial_max=max(relative(a,b) for a,b in zip(reference,Hfine)))
    gates=dict(fractional_delay=checks['fractional_delay_max']<CRITERIA['fractional_delay_transfer'],
               cell_quadrature=checks['quadrature_max']<CRITERIA['cell_quadrature'],
               spatial=checks['spatial_max']<CRITERIA['spatial_transfer'])
    result=dict(criteria=CRITERIA,checks=checks,gates=gates,all_gates_passed=all(gates.values()),
        geometry_m=GEOMETRY.tolist(),cells=cells,modes=count,sample_rate_hz=128000,
        probe_frequency_hz=PROBES.tolist(),physical_units='Pressure Pa from intrinsic board acceleration sqrt(kg)*m/s^2',
        asset_sha256=digest(args.asset),contact_report_sha256=digest(args.contact_report),seconds=time.monotonic()-start,
        scope='Same loaded constant-cell surface, rigid baffle, upper half-space. No room, EQ, extra damping or microphone pattern. Transfer convergence assessed through 6kHz.',
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in ['observe-concert-grand-physical-note.py','concert_grand_cell_audio.py']})
    if result['all_gates_passed']:
        args.kernel.parent.mkdir(parents=True,exist_ok=True)
        np.savez_compressed(args.kernel,kernel=kernel,probe_frequency_hz=PROBES,continuous_transfer=reference,
                             fractional_delay_transfer=H,rate_hz=128000,microphones_m=GEOMETRY)
        result['kernel_sha256']=digest(args.kernel)
    write(args.report,result);print(json.dumps(result),flush=True)
    if not result['all_gates_passed']:raise AssertionError('readout failed')


def listen(args):
    for name in ['trajectory','trajectory_report','kernel_report','audio']:
        if getattr(args,name) is None:raise ValueError(name+' required')
    if args.audio.exists() or args.report.exists():raise ValueError('preserve measured files')
    source=json.loads(args.trajectory_report.read_text());readout=json.loads(args.kernel_report.read_text())
    if not readout['all_gates_passed'] or digest(args.kernel)!=readout['kernel_sha256'] or digest(args.trajectory)!=source['raw_sha256']:
        raise ValueError('validated unchanged trajectory/kernel required')
    if source['onset_report_sha256']!=readout['contact_report_sha256']:raise ValueError('readout/source joined to different onsets')
    start=time.monotonic()
    with np.load(args.trajectory) as d:t=d['time_s'];B=d['board_position'];V=d['board_velocity']
    with np.load(args.kernel) as d:kernel=d['kernel']
    pressure,f,P,size=pressure_fft(t,B,V,kernel)
    cf,CA=hermite_fft(t[::2],B[::2],V[::2],size//2)
    # Same padded physical duration gives identical frequency spacing;
    # only the coarse Nyquist limit is lower.
    if size%2 or not np.allclose(f[:len(cf)],cf,rtol=1e-12,atol=1e-10):
        raise ValueError('matched FFT duration required for capture comparison')
    selection=f<=min(cf[-1],6000.)
    H=rfft(kernel,n=size,axis=0)
    CP=np.einsum('fcm,fm->fc',H[:len(CA)],CA)
    error=spectral_relative(f[selection],CP[:selection.sum()],P[selection],60.,6000.)
    result=dict(criteria=CRITERIA,capture_pressure_relative=error,
        gates=dict(capture_pressure=error<CRITERIA['capture_pressure']),
        trajectory_sha256=digest(args.trajectory),trajectory_report_sha256=digest(args.trajectory_report),kernel_sha256=digest(args.kernel),
        physical_pressure_peak_pa=float(np.max(abs(pressure))),physical_duration_s=float(t[-1]),
        microphone_geometry_m=GEOMETRY.tolist(),audition_gain_pa_to_full_scale=args.gain,
        audio_rate_hz=48000,audio_channels=2,terminal_cosine_fade_s=.008,
        normalization='One explicit common listening gain for both channels; no per-note or per-channel fitting.',
        scope='One vertical C4 string at MIDI velocity 124, nonlinear SI pilot, not a full piano or an installed plugin. Source is sustained to the measured duration; the 8ms terminal fade is an edit, not damper physics.',
        unchecked=['59->90 structural convergence','long-sustain mechanical-rate refinement','independent full angular radiation integral over sustain','Steinway geometry/loss identification','unison','release/action/pedal','runtime integration'],
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in ['observe-concert-grand-physical-note.py','concert_grand_cell_audio.py']})
    if not result['gates']['capture_pressure']:
        write(args.report,result);raise AssertionError('capture pressure failed')
    # Preserve propagation delay; discard only FFT padding beyond the last
    # observed source time plus the largest geometric propagation delay.
    samples=len(t)+len(kernel)-1;pressure=pressure[:samples]
    fade=min(round(.008*128000),len(pressure))
    pressure[-fade:]*=(.5+.5*np.cos(np.linspace(0,np.pi,fade)))[:,None]
    audio=resample_poly(pressure*args.gain,3,8,axis=0)
    peak=float(np.max(abs(audio)))
    if peak>=.98:
        result['audio_peak']=peak;write(args.report,result);raise ValueError('listening gain clips; choose a lower common gain')
    audio=np.vstack([np.zeros((4800,2)),audio,np.zeros((14400,2))]).astype(np.float32)
    args.audio.parent.mkdir(parents=True,exist_ok=True);wavfile.write(args.audio,48000,audio)
    result.update(audio_peak=peak,audio_sha256=digest(args.audio),audio_seconds=len(audio)/48000,seconds=time.monotonic()-start)
    write(args.report,result);print(json.dumps(result),flush=True)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--phase',choices=['prepare','listen'],default='prepare')
    for name in ['asset','contact-report','kernel','report']:
        parser.add_argument('--'+name,type=Path,required=True)
    for name in ['trajectory','trajectory-report','kernel-report','audio']:
        parser.add_argument('--'+name,type=Path)
    parser.add_argument('--gain',type=float,default=1.)
    args=parser.parse_args()
    if not np.isfinite(args.gain) or args.gain<=0:parser.error('positive finite listening gain')
    if args.phase=='prepare':prepare(args)
    else:listen(args)


if __name__=='__main__':main()
