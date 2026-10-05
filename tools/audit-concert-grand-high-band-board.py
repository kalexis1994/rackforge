"""Wide-band structural/Rayleigh readout convergence before new coupled audio.

Same hypothetical panel/ribs/bridge; no sample fitting or new EQ. The dry
structure is a prerequisite diagnostic, not the reciprocal loaded air model.
Numerical arrays go to repo-local storage, never to SHARED in this stage.
"""
import argparse,json,time,hashlib,platform
from pathlib import Path
import numpy as np
import scipy
from scipy.linalg import cholesky
from concert_grand_basis_readout import basis_kernel
from concert_grand_cell_audio import kernel_transfer
import concert_grand_orthotropic_plate as plate

GRIDS=[(52,32),(60,40),(68,48),(76,56),(84,64),(92,72)]
MICS=np.array([[.55,.5,.66],[.85,.5,.66]])
NOTES=np.array([60,64,67,72])
CRITERIA=dict(structural_band_relative=.01,modal_cut_relative=.01,readout_spatial=.01,
    mass_normalization=1e-8,modal_residual=1e-8,reciprocity=1e-11,passivity=1e-11)

def digest(path):
    with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def write(path,data):
    path.parent.mkdir(parents=True,exist_ok=True);path.write_text(json.dumps(data,indent=2,allow_nan=False),encoding='utf-8')
def rel(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-30))

def metrics(f,a,b):
    out={}
    for name,lo,hi in [('low',60,1200),('middle',1200,3000),('upper',3000,6000)]:
        idx=(f>=lo)&(f<=hi)
        power=lambda x:np.sum(abs(x.reshape(len(x),-1))**2,axis=1)
        norm=np.trapezoid(power(b[idx]),f[idx]);error=np.trapezoid(power(a[idx]-b[idx]),f[idx])
        out[name]=float(np.sqrt(error/max(norm,1e-40)))
    return out

def responses(model,f,H,cutoff=np.inf):
    mask=model['frequency']<=cutoff;omega=2*np.pi*model['frequency'][mask];U=model['vectors'][:,mask]
    points=np.c_[model['panel']['length']*(.1+.8*(NOTES-21)/87),np.full(4,model['panel']['width']/2)]
    phi=plate.shape(dict(model,vectors=U),points[:,0],points[:,1])
    w=2*np.pi*f;response=1/(omega[None,:]**2-w[:,None]**2+1j*.046*w[:,None]*omega[None,:])
    Y=np.einsum('pm,fm,qm->fpq',phi,1j*w[:,None]*response,phi,optimize=True)
    coefficients=np.einsum('nm,fm,pm->fnp',U,response,phi,optimize=True)
    P=-w[:,None,None]**2*np.einsum('fcn,fnp->fcp',H,coefficients,optimize=True)
    return Y,P

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--asset',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--report',type=Path,required=True)
    p.add_argument('--max-grids',type=int,default=4);a=p.parse_args()
    if a.output.exists() or a.report.exists():raise ValueError('preserve measurements')
    if not 2<=a.max_grids<=len(GRIDS):p.error('at least two grids, no more than six')
    started=time.monotonic();a.output.mkdir(parents=True)
    with np.load(a.asset) as d:
        panel=plate.Panel(**json.loads(str(d['panel'])));beams=tuple(plate.Beam(**b) for b in json.loads(str(d['beams'])))
        boundary=str(d['boundary']);old_frequency=d['frequency']
    f0=old_frequency[(old_frequency>=60)&(old_frequency<=6000)]
    fundamentals=440*2**((NOTES-69)/12)
    harmonics=np.concatenate([v*np.arange(1,int(6000/v)+1) for v in fundamentals])
    f=np.unique(np.r_[np.linspace(60,6000,793),f0,f0*.99,f0*1.01,harmonics]);f=f[(f>=60)&(f<=6000)]
    maxx,maxy=GRIDS[a.max_grids-1];print('building common separable readout',maxx,maxy,len(f),'frequencies',flush=True)
    kernel=basis_kernel(plate.asdict(panel),maxx,maxy,boundary,[192,144],128000,MICS)
    H=kernel_transfer(kernel.reshape(len(kernel),2,-1),128000,f).reshape(len(f),2,maxx,maxy)
    result=dict(criteria=CRITERIA,asset_sha256=digest(a.asset),panel=plate.asdict(panel),beams=[plate.asdict(b) for b in beams],boundary=boundary,
        cells=[192,144],microphones_m=MICS.tolist(),notes=NOTES.tolist(),frequency_points=len(f),grids=[],cutoffs={},gates={},
        environment=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__,blas_threads=1),
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in ['audit-concert-grand-high-band-board.py','concert_grand_basis_readout.py','concert_grand_orthotropic_plate.py','concert_grand_cell_audio.py']},
        scope='Structural and readout convergence of the assumed P2 panel through 6kHz, without reciprocal air feedback. Not a loaded sound, identified Steinway or factory change.')
    write(a.report,result);previous=None;selected=None
    for nx,ny in GRIDS[:a.max_grids]:
        begin=time.monotonic();print('solving full structural basis',nx,ny,flush=True)
        model=plate.solve(panel,nx,ny,beams,boundary);cholesky(model['mass']);cholesky(model['stiffness'])
        # Full generalized residual and mass Gram on a deterministic subset
        # distributed through the full spectrum, not only the low modes.
        indices=np.unique(np.linspace(0,len(model['frequency'])-1,80,dtype=int));U=model['vectors'][:,indices];values=(2*np.pi*model['frequency'][indices])**2
        mass_error=rel(U.T@model['mass']@U,np.eye(len(indices)))
        residual=model['stiffness']@U-(model['mass']@U)*values
        residual_error=float(np.max(np.linalg.norm(residual,axis=0)/np.maximum(np.linalg.norm(model['stiffness']@U,axis=0),1e-40)))
        Hj=H[:,:,:nx,:ny].reshape(len(f),2,nx*ny)
        Y,P=responses(model,f,Hj)
        eigen=np.linalg.eigvalsh((Y+Y.conj().transpose(0,2,1))/2)
        passive=float(eigen.min()/max(np.linalg.norm(Y,axis=(1,2)).max(),1e-40))
        reciprocity=rel(Y,Y.transpose(0,2,1))
        row=dict(nx=nx,ny=ny,modes=len(model['frequency']),modes_to_6000=int(sum(model['frequency']<=6000)),
            highest_mode_hz=float(model['frequency'][-1]),mass_error=mass_error,modal_residual=residual_error,
            reciprocity_relative=reciprocity,minimum_dissipation_relative=passive)
        if previous is not None:
            oldY,oldP=previous;row['previous_grid_change']=dict(mobility=metrics(f,oldY,Y),pressure=metrics(f,oldP,P))
        path=a.output/f'Transfer-{nx}x{ny}.npz';np.savez_compressed(path,frequency_hz=f,mobility=Y,pressure_per_force=P,structural_frequency_hz=model['frequency'])
        row.update(seconds=time.monotonic()-begin,measurement_sha256=digest(path));result['grids'].append(row);write(a.report,result)
        print('measured',json.dumps(row),flush=True)
        if mass_error>=CRITERIA['mass_normalization'] or residual_error>=CRITERIA['modal_residual'] or reciprocity>=CRITERIA['reciprocity'] or passive< -CRITERIA['passivity']:raise AssertionError('physical structural checks')
        selected=(model,Hj,Y,P)
        if previous is not None and max(v for family in row['previous_grid_change'].values() for v in family.values())<CRITERIA['structural_band_relative']:
            result['gates']['spatial_structure']=True;break
        previous=(Y,P);del model
    result['gates'].setdefault('spatial_structure',False)
    model,Hj,Y,P=selected
    for cutoff in [1200,1500,3000,6000,9000,12000,18000]:
        y,p0=responses(model,f,Hj,cutoff)
        result['cutoffs'][str(cutoff)]=dict(modes=int(sum(model['frequency']<=cutoff)),mobility=metrics(f,y,Y),pressure=metrics(f,p0,P))
    adequate=[int(c) for c,r in result['cutoffs'].items() if max(v for fam in ['mobility','pressure'] for v in r[fam].values())<CRITERIA['modal_cut_relative']]
    result['selected_cutoff_hz']=min(adequate) if adequate else None;result['gates']['modal_cut']=bool(adequate)
    # Same modal response, only cell subdivision changes. The quadrature and
    # fractional delay are independently checked against direct Green values.
    nx,ny=model['nx'],model['ny'];fine=basis_kernel(model['panel'],nx,ny,boundary,[256,192],128000,MICS)
    Hfine=kernel_transfer(fine.reshape(len(fine),2,-1),128000,f)
    _,Pfine=responses(model,f,Hfine)
    spatial=metrics(f,P,Pfine);result['readout_cell_change']=spatial
    result['gates']['readout_spatial']=max(spatial.values())<CRITERIA['readout_spatial']
    del fine,Hfine
    asset=a.output/'Panel-costillas-puente-banda-alta.npz'
    np.savez_compressed(asset,frequency=model['frequency'],vectors=model['vectors'],nx=nx,ny=ny,panel=json.dumps(model['panel']),beams=json.dumps(model['beams']),boundary=boundary)
    result.update(structural_asset_path=str(asset),structural_asset_sha256=digest(asset),seconds=time.monotonic()-started,
        all_structural_gates_passed=all(result['gates'].values()),decision='Structural prerequisite measured; reciprocal air load still required before new audio.')
    write(a.report,result);print('finished',json.dumps(dict(gates=result['gates'],cutoff=result['selected_cutoff_hz'],seconds=result['seconds'])),flush=True)

if __name__=='__main__':main()
