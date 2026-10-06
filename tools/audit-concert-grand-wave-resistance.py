"""Separate angular, Cayley/difference and histogram errors on real board probes.

This contraction diagnoses resistance only; it is not full modal/loaded approval.
The probe spans the eleven continuously loaded states at 6 kHz plus fixed
random directions. It never changes the complete 606-coordinate candidate.
"""
import argparse,hashlib,importlib.util,json,time
from pathlib import Path
from types import SimpleNamespace
import numpy as np
from concert_grand_projected_support import ModalSupportMap,projected_matrices
from concert_grand_real_surface import RealArithmeticSurface
from concert_grand_streaming_air import SurfaceFactorBlocks
import concert_grand_orthotropic_plate as plate


def digest(path):
    with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()


def angular_resistance(provider,frequencies,progress=None):
    """Exact rectangular-cell Fourier integral, independent of temporal bins."""
    surface=provider.surface;n=provider.modes
    out=np.zeros((len(frequencies),n,n))
    for first in range(0,len(provider.directions),provider.block):
        directions=provider.directions[first:first+provider.block]
        weights=provider.weights[first:first+provider.block]
        distance=provider.positions@directions.T
        for i,omega in enumerate(frequencies):
            phase=np.exp(-1j*omega/surface.speed*distance)
            field=phase.T@provider.shapes*(surface.hx*surface.hy)
            cell=np.sinc(omega*directions[:,0]*surface.hx/(2*np.pi*surface.speed))
            cell*=np.sinc(omega*directions[:,1]*surface.hy/(2*np.pi*surface.speed))
            field*= (omega*cell*np.sqrt(surface.density*weights/(2*np.pi**2*surface.speed)))[:,None]
            # Opposite direction is conjugate: half the complete Gram is Re.
            out[i]+=(field.conj().T@field).real
        if progress is not None:progress(first//provider.block+1)
    return out


def wave_resistance(provider,omega,progress=None):
    theta=2*np.arctan(omega/(2*provider.rate))
    phase=np.exp(-1j*theta*np.arange(provider.taps));out=np.zeros((provider.modes,provider.modes))
    for i,H in enumerate(provider.blocks()):
        field=np.einsum('t,tom->om',phase,H[:,::2],optimize=True)
        out+=(field.conj().T@field).real
        if progress is not None:progress(i+1)
    return out


def relative(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-40))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['loaded-report','support-report','board','preflight','report']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--rates',type=int,nargs='+',default=[128000,256000])
    p.add_argument('--frequency',type=float,default=6000.);p.add_argument('--random-probes',type=int,default=8)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    if a.frequency<=0 or not np.isfinite(a.frequency) or min(a.rates)<=0 or a.random_probes<1:raise ValueError('positive probes/rates/frequency')
    start=time.monotonic();parent=json.loads(a.loaded_report.read_text());support=json.loads(a.support_report.read_text())
    previous=json.loads(a.preflight.read_text())
    if not parent['all_gates_passed'] or not support['all_gates_passed']:raise ValueError('approved mechanical/continuous parents required')
    if (digest(a.board)!=support['board_sha256'] or digest(a.loaded_report)!=support['parent_sha256']
        or previous['board_sha256']!=support['board_sha256'] or previous['cells']!=parent['cells']):raise ValueError('unchanged parent chain required')
    reference_path=Path(__file__).with_name('audit-concert-grand-streaming-air.py')
    spec=importlib.util.spec_from_file_location('stream_reference',reference_path);reference=importlib.util.module_from_spec(spec);spec.loader.exec_module(reference)
    with np.load(a.board) as d:
        mechanical={k:d[k] for k in ['frequency','low_count','tail_mass','tail_stiffness','tail_damping','low_damping']}
        model=dict(panel=json.loads(str(d['panel'])),boundary=str(d['boundary']),nx=int(d['nx']),ny=int(d['ny']),vectors=d['ritz_vectors'])
        notes=d['test_notes'].tolist()
    modal=ModalSupportMap.from_matrices(*projected_matrices(mechanical));model['vectors']=model['vectors']@modal.basis
    surface=RealArithmeticSurface(model,parent['cells']);n=surface.modes;omega=2*np.pi*a.frequency;s=1j*omega
    x=model['panel']['length']*(.1+.8*(np.array(notes)-21)/87)
    B=plate.shape(model,x,np.full(len(x),model['panel']['width']/2)).T
    print('continuous reference',flush=True);Z=reference.direct_matrix(surface,s)
    q=np.linalg.solve(np.diag((2*np.pi*modal.frequency)**2+s*s)+s*(modal.damping+Z),B)
    seed=61006;rng=np.random.default_rng(seed)
    probes=np.column_stack((q.real,q.imag,rng.normal(size=(n,a.random_probes))))
    probes/=np.maximum(np.linalg.norm(probes,axis=0),1e-300)
    T,singular,_=np.linalg.svd(probes,full_matrices=False);T=T[:,singular>1e-12*singular[0]]
    shapes=surface.forward(T);exact=T.T@Z.real@T
    plain=SimpleNamespace(shapes=shapes,modes=T.shape[1],nx=surface.nx,ny=surface.ny,hx=surface.hx,hy=surface.hy,
        length=surface.length,width=surface.width,density=surface.density,speed=surface.speed)
    polar,azimuth=previous['angular']
    providers=[SurfaceFactorBlocks(plain,rate,polar,azimuth,fold_y=previous['fold_y']) for rate in a.rates]
    effective=[2*rate*np.arctan(omega/(2*rate)) for rate in a.rates]
    report=dict(frequency_hz=a.frequency,modes=n,probe_rank=T.shape[1],random_seed=seed,random_probes=a.random_probes,
        angular=previous['angular'],cells=parent['cells'],notes=notes,
        loaded_state_span_relative=relative(T@(T.T@q),q),
        parent_sha256={str(path):digest(path) for path in [a.loaded_report,a.support_report,a.preflight]},
        board_sha256=digest(a.board),source_sha256={name:digest(Path(__file__).with_name(name)) for name in [
            Path(__file__).name,'audit-concert-grand-streaming-air.py','concert_grand_streaming_air.py',
            'concert_grand_projected_support.py','concert_grand_real_surface.py','concert_grand_matrixfree_air.py']},
        preflight_only=True,all_gates_passed=False,
        scope='Resistance diagnostic on a real projected probe span only. No full-matrix, imaginary, limit, loaded transfer, transient or timbral approval.')
    print('angular probes',T.shape[1],flush=True)
    continuous=angular_resistance(providers[0],[omega,*effective],progress=lambda i:print('angular block',i,flush=True) if i%64==0 else None)
    report['angular_continuous_relative']=relative(continuous[0],exact);report['rows']=[]
    for j,provider in enumerate(providers):
        wave=wave_resistance(provider,omega,progress=lambda i:print('wave rate',provider.rate,'block',i,flush=True) if i%64==0 else None)
        theta=effective[j]/provider.rate
        # Backward difference followed by adjacent averaging has magnitude rate*sin(theta).
        prediction=continuous[j+1]*(provider.rate*np.sin(theta)/effective[j])**2
        row=dict(rate=provider.rate,effective_frequency_hz=effective[j]/(2*np.pi),
            continuous_warp_relative=relative(continuous[j+1],exact),
            warp_and_difference_relative=relative(prediction,exact),
            histogram_additional_relative=relative(wave,prediction),wave_reference_relative=relative(wave,exact))
        report['rows'].append(row);print('resistance result',json.dumps(row),flush=True)
    report['seconds']=time.monotonic()-start;report['peak_working_set_bytes']=reference.peak_memory()
    a.report.parent.mkdir(parents=True,exist_ok=True);a.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    print('diagnostic finished',report['seconds'],flush=True)


if __name__=='__main__':main()
