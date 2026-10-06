"""Resource-bounded causal air preflight on the actual 606-coordinate board.

No saved factor. A coarse angular preflight is never full-band approval.
Finite-limit residuals must already be positive; no clipping or gain repair.
"""
import argparse,json,time,hashlib,ctypes,sys
from pathlib import Path
from types import SimpleNamespace
import numpy as np
from scipy.fft import fft2,ifft2
from concert_grand_streaming_air import SurfaceFactorBlocks,StreamingRationalAir,StreamingBoundedAir
from concert_grand_real_surface import RealArithmeticSurface
from concert_grand_matrixfree_air import CellAir
from concert_grand_projected_support import ModalSupportMap,projected_matrices
import concert_grand_orthotropic_plate as plate


def digest(path):
    with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()


def peak_memory():
    if sys.platform!='win32':return None
    from ctypes import wintypes
    class Counters(ctypes.Structure):
        _fields_=[('cb',wintypes.DWORD),('faults',wintypes.DWORD)]+[(n,ctypes.c_size_t) for n in ['peak','working','peak_paged','paged','peak_nonpaged','nonpaged','pagefile','peak_pagefile']]
    d=Counters();d.cb=ctypes.sizeof(d)
    if not ctypes.windll.psapi.GetProcessMemoryInfo(ctypes.c_void_p(-1),ctypes.byref(d),d.cb):return None
    return int(d.peak)


def relative(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-40))


def direct_matrix(surface,s,block=16):
    air=CellAir(surface,s);out=np.empty((surface.modes,surface.modes),complex)
    for first in range(0,surface.modes,block):
        indices=np.arange(first,min(first+block,surface.modes));q=np.zeros((surface.modes,len(indices)))
        q[indices,np.arange(len(indices))]=1
        field=surface.forward(q)
        convolved=ifft2(air.kernel_fft[:,:,None]*fft2(field,s=air.fft_size,axes=(0,1)),axes=(0,1))[:surface.nx,:surface.ny]
        out[:,indices]=surface.adjoint(convolved)*surface.density/(2*np.pi)
    return out if s==0 else s*out


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['loaded-report','support-report','board','report']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--rate',type=int,default=128000);p.add_argument('--polar',type=int,default=16)
    p.add_argument('--azimuth',type=int,default=32);p.add_argument('--direction-block',type=int,default=8)
    p.add_argument('--max-array-gib',type=float,default=8.)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    if not np.isfinite(a.max_array_gib) or a.max_array_gib<=0:raise ValueError('positive memory budget')
    start=time.monotonic();parent=json.loads(a.loaded_report.read_text());support=json.loads(a.support_report.read_text())
    if not parent['all_gates_passed'] or not support['all_gates_passed']:raise ValueError('approved parents required')
    if digest(a.loaded_report)!=support['parent_sha256'] or digest(a.board)!=support['board_sha256']:raise ValueError('changed parent chain')
    with np.load(a.board) as d:
        mechanical={name:d[name] for name in ['frequency','low_count','tail_mass','tail_stiffness','tail_damping','low_damping']}
        model=dict(panel=json.loads(str(d['panel'])),boundary=str(d['boundary']),nx=int(d['nx']),ny=int(d['ny']),vectors=d['ritz_vectors'])
        notes=d['test_notes'].tolist()
    modal=ModalSupportMap.from_matrices(*projected_matrices(mechanical))
    model['vectors']=model['vectors']@modal.basis
    surface=RealArithmeticSurface(model,parent['cells']);n=surface.modes
    x=model['panel']['length']*(.1+.8*(np.array(notes)-21)/87)
    B=plate.shape(model,x,np.full(len(x),model['panel']['width']/2)).T
    del model,mechanical
    shapes=surface.forward(np.eye(n))
    plain=SimpleNamespace(shapes=shapes,modes=n,nx=surface.nx,ny=surface.ny,
        hx=surface.hx,hy=surface.hy,length=surface.length,width=surface.width,
        density=surface.density,speed=surface.speed)
    provider=SurfaceFactorBlocks(plain,a.rate,a.polar,a.azimuth,a.direction_block)
    r=dict(rate=a.rate,angular=[a.polar,a.azimuth],cells=parent['cells'],modes=n,notes=notes,
        budget=provider.budget(),max_array_bytes=int(a.max_array_gib*2**30),
        parent_sha256={str(a.loaded_report):digest(a.loaded_report),str(a.support_report):digest(a.support_report)},
        board_sha256=digest(a.board),source_sha256={name:digest(Path(__file__).with_name(name)) for name in [
            'audit-concert-grand-streaming-air.py','concert_grand_streaming_air.py','concert_grand_rational_air.py',
            'concert_grand_bounded_air.py','concert_grand_projected_support.py',
            'concert_grand_real_surface.py','concert_grand_matrixfree_air.py']},
        criteria=dict(impedance_ZRX_relative=.02,loaded_port_relative=.01,passivity_relative=-1e-10),
        preflight_only=True,all_gates_passed=False,
        scope='Causal construction preflight only. No full angular/spatial/frequency or coupled-time approval; no WAV or saved factor.')
    print('budget',json.dumps(r['budget']),flush=True)
    wave=StreamingRationalAir(provider,r['max_array_bytes'],progress=lambda i:print('wave block',i,flush=True))
    r['wave_reciprocity_relative']=float(wave.reciprocity_error);r['wave_dc_relative']=wave.dc_error
    print('static mass reference',flush=True)
    target=direct_matrix(surface,0).real;target=(target+target.T)/2
    limit=surface.density*surface.speed*surface.hx*surface.hy*(provider.shapes.T@provider.shapes)
    residual=target-wave.static_mass();residual=(residual+residual.T)/2
    eig=np.linalg.eigvalsh(residual)
    r['mass_residual_minimum']=float(eig.min());r['mass_residual_maximum']=float(eig.max())
    try:
        candidate=StreamingBoundedAir(wave,target,limit);r['finite_completion']=True
    except ValueError as error:
        r['finite_completion']=False;r['completion_rejection']=str(error);candidate=wave
    r['rows']=[]
    for hz in [60.,1881.9607122235952,6000.]:
        s=2j*np.pi*hz;reference=direct_matrix(surface,s);Z=candidate.impedance(s)
        dynamic=np.diag((2*np.pi*modal.frequency)**2+s*s)+s*modal.damping
        q=np.linalg.solve(dynamic+s*Z,B);exact=np.linalg.solve(dynamic+s*reference,B)
        v=s*q;ev=s*exact
        errors=np.linalg.norm(B.T@(v-ev),axis=0)/np.maximum(np.linalg.norm(B.T@ev,axis=0),1e-40)
        microphones=[[.55,.5,.66],[.85,.5,.66]]
        P=surface.pressure(hz,q,microphones);p0=surface.pressure(hz,exact,microphones)
        perr=np.linalg.norm(P-p0,axis=0)/np.maximum(np.linalg.norm(p0,axis=0),1e-40)
        row=dict(frequency_hz=hz,candidate='bounded' if r['finite_completion'] else 'wave only, incomplete limits',
            impedance_ZRX_relative=[relative(Z,reference),relative(Z.real,reference.real),relative(Z.imag,reference.imag)],
            mobility_relative=errors.tolist(),pressure_relative=perr.tolist(),
            hermitian_minimum_relative=float(np.linalg.eigvalsh((Z+Z.conj().T)/2).min()/max(np.linalg.norm(Z),1e-40)))
        r['rows'].append(row);print('transfer',json.dumps(row),flush=True)
    r['preflight_gates']=dict(finite_completion=r['finite_completion'],
        impedance=r['finite_completion'] and max(max(x['impedance_ZRX_relative']) for x in r['rows'])<.02,
        loaded=r['finite_completion'] and max(max(x[k]) for x in r['rows'] for k in ['mobility_relative','pressure_relative'])<.01,
        passive=min(x['hermitian_minimum_relative'] for x in r['rows'])>=-1e-10)
    r['peak_working_set_bytes']=peak_memory();r['seconds']=time.monotonic()-start
    r['next']='Refine angular/rate construction and compare all frozen harmonic/spatial/Laplace/transient criteria before coupled listening. Incomplete limits are rejected, never repaired.'
    a.report.parent.mkdir(parents=True,exist_ok=True);a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')
    print('preflight finished',json.dumps({k:r[k] for k in ['preflight_gates','seconds','peak_working_set_bytes']}),flush=True)


if __name__=='__main__':main()
