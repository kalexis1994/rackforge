"""Refine the same delta-line P2 geometry using its exact vertical symmetry.

Frozen full-grid pressure/mobility is the control, not an altered loss curve.
No full-band/modal-truncation approval is inferred from symmetry alone.
"""
import argparse,json,time
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from scipy.linalg import cholesky
import concert_grand_orthotropic_plate as plate
import concert_grand_symmetric_plate as symmetric
from concert_grand_basis_readout import basis_kernel
from concert_grand_cell_audio import kernel_transfer
spec=spec_from_file_location('symmetric_band_reference',Path(__file__).with_name('audit-concert-grand-high-band-board.py'));reference=module_from_spec(spec);spec.loader.exec_module(reference)
GRIDS=[(76,56),(108,88),(124,104),(132,112),(140,120)]

def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['asset','parent-report','output','report']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--max-grids',type=int,default=5);a=p.parse_args()
    if not 2<=a.max_grids<=5 or a.output.exists() or a.report.exists():raise ValueError('new output/report, two to five grids')
    start=time.monotonic();parent=json.loads(a.parent_report.read_text());base=parent['grids'][-1]
    if (base['nx'],base['ny'])!=(76,56) or reference.digest(a.asset)!=parent['asset_sha256']:raise ValueError('fixed full 76x56 control')
    controls=Path(parent['structural_asset_path']).parent/'Transfer-76x56.npz'
    if reference.digest(controls)!=base['measurement_sha256']:raise ValueError('full-grid control changed')
    with np.load(controls) as d:f=d['frequency_hz'];fullY=d['mobility'];fullP=d['pressure_per_force']
    panel=plate.Panel(**parent['panel']);beams=tuple(plate.Beam(**b) for b in parent['beams']);boundary=parent['boundary']
    maxx,maxy=GRIDS[a.max_grids-1];print('symmetric invariant readout',maxx,maxy,flush=True)
    kernel=basis_kernel(parent['panel'],maxx,maxy,boundary,parent['cells'],128000,reference.MICS)
    H=kernel_transfer(kernel.reshape(len(kernel),2,-1),128000,f).reshape(len(f),2,maxx,maxy)
    a.output.mkdir(parents=True);r=dict(criteria=reference.CRITERIA,parent_report_sha256=reference.digest(a.parent_report),
        inputs_sha256={str(a.asset):reference.digest(a.asset),str(controls):reference.digest(controls)},grids=[],gates={},
        source_sha256={n:reference.digest(Path(__file__).with_name(n)) for n in ['audit-concert-grand-symmetric-band.py','concert_grand_symmetric_plate.py','concert_grand_basis_readout.py','audit-concert-grand-high-band-board.py']},
        scope='Same delta-line mechanical geometry and all modes in its exact reflection-symmetric vertical subspace. No air feedback, tonal fit or asymmetric/lateral piano claim.')
    reference.write(a.report,r);previous=None;selected=None
    for nx,ny in GRIDS[:a.max_grids]:
        begin=time.monotonic();print('solving symmetric full basis',nx,ny,nx*((ny+1)//2),'coordinates',flush=True)
        model=symmetric.solve(panel,nx,ny,beams,boundary);cholesky(model['mass']);cholesky(model['stiffness'])
        indices=np.unique(np.linspace(0,len(model['frequency'])-1,80,dtype=int));U=model['active_vectors'][:,indices];values=(2*np.pi*model['frequency'][indices])**2
        m=reference.rel(U.T@model['mass']@U,np.eye(len(indices)));residual=model['stiffness']@U-(model['mass']@U)*values
        e=float(np.max(np.linalg.norm(residual,axis=0)/np.maximum(np.linalg.norm(model['stiffness']@U,axis=0),1e-40)))
        Hj=H[:,:,:nx,:ny].reshape(len(f),2,nx*ny);Y,P=reference.responses(model,f,Hj)
        row=dict(nx=nx,ny=ny,active_coordinates=len(model['frequency']),full_ritz_coordinates=nx*ny,
            active_modes_to_6000=int(sum(model['frequency']<=6000)),mass_error=m,modal_residual=e)
        if previous is None:
            row['same_grid_full_control']=dict(mobility=reference.metrics(f,Y,fullY),pressure=reference.metrics(f,P,fullP))
            r['gates']['same_physics']=max(v for family in row['same_grid_full_control'].values() for v in family.values())<1e-8
        else:row['previous_grid_change']=dict(mobility=reference.metrics(f,previous[0],Y),pressure=reference.metrics(f,previous[1],P))
        path=a.output/f'Transfer-{nx}x{ny}.npz';np.savez_compressed(path,frequency_hz=f,mobility=Y,pressure_per_force=P,structural_frequency_hz=model['frequency'])
        row.update(seconds=time.monotonic()-begin,measurement_sha256=reference.digest(path));r['grids'].append(row);reference.write(a.report,r)
        print('measured',json.dumps(row),flush=True)
        if m>=reference.CRITERIA['mass_normalization'] or e>=reference.CRITERIA['modal_residual'] or not r['gates']['same_physics']:raise AssertionError('symmetric mechanical verification')
        selected=(model,Hj,Y,P)
        if previous is not None and max(v for family in row['previous_grid_change'].values() for v in family.values())<reference.CRITERIA['structural_band_relative']:
            r['gates']['spatial_structure']=True;break
        previous=(Y,P)
    r['gates'].setdefault('spatial_structure',False);model,Hj,Y,P=selected
    fine=basis_kernel(model['panel'],model['nx'],model['ny'],boundary,[256,192],128000,reference.MICS)
    Hfine=kernel_transfer(fine.reshape(len(fine),2,-1),128000,f);_,Pfine=reference.responses(model,f,Hfine)
    r['readout_cell_change']=reference.metrics(f,P,Pfine);r['gates']['readout_spatial']=max(r['readout_cell_change'].values())<reference.CRITERIA['readout_spatial']
    asset=a.output/'Panel-simetrico-banda-alta.npz'
    np.savez_compressed(asset,frequency=model['frequency'],vectors=model['vectors'],nx=model['nx'],ny=model['ny'],panel=json.dumps(model['panel']),beams=json.dumps(model['beams']),boundary=boundary,reflection_subspace='symmetric-y')
    r.update(structural_asset_path=str(asset),structural_asset_sha256=reference.digest(asset),seconds=time.monotonic()-start,
        all_spatial_gates_passed=all(r['gates'].values()),next='Modal/compliant-tail compression and reciprocal acoustic load still required before new listening.')
    reference.write(a.report,r);print('finished',json.dumps(r['gates']),flush=True)

if __name__=='__main__':main()
