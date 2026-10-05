"""Dry positive mechanical reduction with independent bridge-point controls.

No sound/package update. Keep the converged symmetric P2 geometry; project
its dense tail damping and surface readout using exactly the same real map.
"""
import argparse,json,time
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from scipy.linalg import cholesky
import concert_grand_orthotropic_plate as plate
from concert_grand_basis_readout import basis_kernel
import concert_grand_compliant_projection as projection

spec=spec_from_file_location('compliant_reference',Path(__file__).with_name('audit-concert-grand-high-band-board.py'))
reference=module_from_spec(spec);spec.loader.exec_module(reference)
TRAIN=[60,64,67,72]
HOLD=[36,48,53,65,69,84,96]
SHIFTS=[0.,1500.,3000.,4500.,6000.]
CRITERIA=dict(band_relative=.01,static_training=1e-9,mass_normalization=1e-10,
    reciprocity=1e-10,minimum_dissipation=-1e-11,source_control=1e-8)


def ports(model,notes):
    x=model['panel']['length']*(.1+.8*(np.array(notes)-21)/87)
    return plate.shape(model,x,np.full(len(x),model['panel']['width']/2)).T


def transfer(kernel,rate,f):
    phase=np.exp(-2j*np.pi*np.asarray(f)[:,None]*np.arange(len(kernel))/rate)
    return (phase@kernel.reshape(len(kernel),-1)).reshape(len(f),*kernel.shape[1:])


def metrics(f,Y,P,refY,refP,notes):
    out=dict(mobility=reference.metrics(f,Y,refY),pressure=reference.metrics(f,P,refP))
    out['by_note']={str(n):dict(mobility=reference.metrics(f,Y[:,:,i],refY[:,:,i]),
        pressure=reference.metrics(f,P[:,:,i],refP[:,:,i])) for i,n in enumerate(notes)}
    out['maximum_port_band']=max(v for row in out['by_note'].values() for family in row.values() for v in family.values())
    return out


def reduced_response(reduced,f,B,kernel):
    X=projection.displacement(reduced,f,B)
    Y=2j*np.pi*f[:,None,None]*np.einsum('mp,fmq->fpq',reduced['projection'].T@B,X,optimize=True)
    H=transfer(kernel,128000,f)
    P=-(2*np.pi*f[:,None,None])**2*np.einsum('fcm,fmp->fcp',H,X,optimize=True)
    return Y,P


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for n in ['parent-report','output','report']:parser.add_argument('--'+n,type=Path,required=True)
    a=parser.parse_args()
    if a.output.exists() or a.report.exists():raise ValueError('preserve results: new output/report required')
    start=time.monotonic();parent=json.loads(a.parent_report.read_text())
    if not parent['all_spatial_gates_passed']:raise ValueError('converged spatial parent required')
    asset=Path(parent['structural_asset_path'])
    if reference.digest(asset)!=parent['structural_asset_sha256']:raise ValueError('structural input hash')
    with np.load(asset) as d:
        model=dict(frequency=d['frequency'],vectors=d['vectors'],nx=int(d['nx']),ny=int(d['ny']),
            panel=json.loads(str(d['panel'])),beams=json.loads(str(d['beams'])),boundary=str(d['boundary']))
        if str(d['reflection_subspace'])!='symmetric-y':raise ValueError('frozen symmetric input')
    row=parent['grids'][-1];control_path=asset.parent/f"Transfer-{row['nx']}x{row['ny']}.npz"
    if reference.digest(control_path)!=row['measurement_sha256']:raise ValueError('frozen full transfer hash')
    with np.load(control_path) as d:f0=d['frequency_hz'];controlY=d['mobility'];controlP=d['pressure_per_force']
    # Holdout grid shares neither the uniform old grid nor the construction
    # shifts. Low resonances remain exact, the approximation concerns the tail.
    f1=60+(np.arange(901)+.5)*(6000-60)/901
    f=np.r_[f0,f1];notes=TRAIN+HOLD;B=ports(model,notes)
    print('building full physical readout',len(f),'frequencies',len(model['frequency']),'modes',flush=True)
    basis=basis_kernel(model['panel'],model['nx'],model['ny'],model['boundary'],[192,144],128000,reference.MICS)
    # Remove identically zero parity rows before the exact matrix product.
    indices=(np.arange(model['nx'])[:,None]*model['ny']+np.arange(0,model['ny'],2)).ravel()
    active=model['vectors'][indices];flat=basis.reshape(len(basis)*2,-1)[:,indices]
    kernel=(flat@active).reshape(len(basis),2,len(model['frequency']))
    del basis,flat,active
    omega=2*np.pi*model['frequency'];Y=np.empty((len(f),len(notes),len(notes)),complex)
    P=np.empty((len(f),2,len(notes)),complex)
    for first in range(0,len(f),64):
        hz=f[first:first+64];w=2*np.pi*hz
        X=B[None]/(omega[None,:,None]**2-w[:,None,None]**2+1j*.046*w[:,None,None]*omega[None,:,None])
        Y[first:first+len(hz)]=1j*w[:,None,None]*np.einsum('mp,fmq->fpq',B,X,optimize=True)
        H=transfer(kernel,128000,hz)
        P[first:first+len(hz)]=-w[:,None,None]**2*np.einsum('fcm,fmp->fcp',H,X,optimize=True)
    original=dict(mobility=reference.metrics(f0,Y[:len(f0),:4,:4],controlY),
        pressure=reference.metrics(f0,P[:len(f0),:,:4],controlP))
    if max(v for fam in original.values() for v in fam.values())>=CRITERIA['source_control']:
        raise AssertionError('changed frozen structural/readout control')
    a.output.mkdir(parents=True)
    ref_path=a.output/'Transfer-completo-control.npz'
    np.savez_compressed(ref_path,frequency_hz=f,notes=notes,mobility=Y,pressure_per_force=P,original_grid_count=len(f0))
    r=dict(criteria=CRITERIA,source_control=original,notes=notes,holdout_notes=HOLD,
        construction_shifts_hz=SHIFTS,frequency_points=[len(f0),len(f1)],candidates=[],
        parent_report_sha256=reference.digest(a.parent_report),parent_asset_sha256=reference.digest(asset),
        reference_path=str(ref_path),reference_sha256=reference.digest(ref_path),
        source_sha256={n:reference.digest(Path(__file__).with_name(n)) for n in ['audit-concert-grand-compliant-projection.py','concert_grand_compliant_projection.py','concert_grand_basis_readout.py']},
        scope='Dry symmetric vertical mechanical congruence, four training ports plus held-out ports/frequency grid. Full dense projected damping retained. No acoustic feedback/audio/factory selection.')
    reference.write(a.report,r);selected=None
    for label,training in [('four-ports',TRAIN),('distributed-ports',sorted(set(TRAIN+list(range(21,109,6)))-set(HOLD)))]:
        begin=time.monotonic();print('projection',label,len(training),'construction ports',flush=True)
        buildB=ports(model,training);reduced=projection.build(model['frequency'],buildB,6000,SHIFTS)
        Q=reduced['projection'];n=reduced['low_count']
        reduced_kernel=np.concatenate([kernel[:,:,:n],kernel[:,:,n:]@Q[n:,n:]],axis=2)
        y,p=reduced_response(reduced,f,B,reduced_kernel)
        training_full=buildB.T@(buildB/omega[:,None]**2)
        training_reduced=(Q.T@buildB).T@projection.displacement(reduced,[0.],buildB)[0]
        static=reference.rel(training_reduced,training_full)
        mass=reference.rel(Q.T@Q,np.eye(Q.shape[1]))
        cholesky(reduced['tail_mass']);cholesky(reduced['tail_stiffness']);cholesky(reduced['tail_damping'])
        eig=np.linalg.eigvalsh((y+y.conj().transpose(0,2,1))/2)
        passive=float(eig.min()/max(np.linalg.norm(y,axis=(1,2)).max(),1e-40));reciprocity=reference.rel(y,y.transpose(0,2,1))
        first=metrics(f0,y[:len(f0)],p[:len(f0)],Y[:len(f0)],P[:len(f0)],notes)
        independent=metrics(f1,y[len(f0):],p[len(f0):],Y[len(f0):],P[len(f0):],notes)
        damping=reduced['tail_damping'];offdiag=reference.rel(damping-np.diag(np.diag(damping)),damping)
        gates=dict(transfer_original=first['maximum_port_band']<CRITERIA['band_relative'],
            transfer_independent=independent['maximum_port_band']<CRITERIA['band_relative'],
            static_training=static<CRITERIA['static_training'],mass=mass<CRITERIA['mass_normalization'],
            passive=passive>=CRITERIA['minimum_dissipation'],reciprocal=reciprocity<CRITERIA['reciprocity'])
        result=dict(label=label,construction_notes=training,retained_modes=n,tail_coordinates=reduced['high_rank'],
            total_coordinates=Q.shape[1],snapshot_columns=reduced['snapshot_columns'],
            mass_error=mass,static_training_error=static,reciprocity=reciprocity,
            minimum_dissipation=passive,tail_damping_offdiagonal_relative=offdiag,
            original_grid=first,independent_grid=independent,gates=gates,passed=all(gates.values()),seconds=time.monotonic()-begin)
        r['candidates'].append(result);reference.write(a.report,r)
        print('measured',label,Q.shape[1],json.dumps(gates),'max',first['maximum_port_band'],independent['maximum_port_band'],flush=True)
        if result['passed']:selected=(reduced,reduced_kernel,training,y,p);break
    r['selected']=selected is not None
    if selected is not None:
        reduced,kernel,training,y,p=selected;path=a.output/'Tabla-proyectada-pasiva.npz'
        np.savez_compressed(path,**reduced,ritz_vectors=model['vectors']@reduced['projection'],
            kernel_128000=kernel,construction_notes=training,test_notes=notes,frequency_hz=f,
            mobility=y,pressure_per_force=p,panel=json.dumps(model['panel']),beams=json.dumps(model['beams']),
            boundary=model['boundary'],nx=model['nx'],ny=model['ny'])
        r.update(projected_asset_path=str(path),projected_asset_sha256=reference.digest(path))
    r.update(seconds=time.monotonic()-start,next='Reciprocal acoustic load and coupled time/energy verification are still required. Do not diagonalize projected tail loss.')
    reference.write(a.report,r);print('finished selected',r['selected'],flush=True)

if __name__=='__main__':main()
