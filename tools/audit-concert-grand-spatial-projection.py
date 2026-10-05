"""Refine construction points using frozen held-out dry transfers.

Virtual forces at fractional bridge positions enrich space without inserting
the held-out MIDI notes themselves. Positive congruence and dense loss remain.
"""
import argparse,json,time
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from scipy.linalg import cholesky
from concert_grand_basis_readout import basis_kernel
import concert_grand_compliant_projection as projection

spec=spec_from_file_location('spatial_projection_reference',Path(__file__).with_name('audit-concert-grand-compliant-projection.py'))
base=module_from_spec(spec);spec.loader.exec_module(base)
reference=base.reference


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for n in ['mechanical-report','parent-report','output','report']:parser.add_argument('--'+n,type=Path,required=True)
    a=parser.parse_args()
    if a.output.exists() or a.report.exists():raise ValueError('new output/report required')
    begin=time.monotonic();parent=json.loads(a.parent_report.read_text());mechanical=json.loads(a.mechanical_report.read_text())
    if not mechanical['all_spatial_gates_passed'] or reference.digest(a.mechanical_report)!=parent['parent_report_sha256']:
        raise ValueError('frozen converged mechanical input')
    asset=Path(mechanical['structural_asset_path']);control=Path(parent['reference_path'])
    if reference.digest(asset)!=parent['parent_asset_sha256'] or reference.digest(control)!=parent['reference_sha256']:
        raise ValueError('frozen numerical inputs changed')
    with np.load(asset) as d:model=dict(frequency=d['frequency'],vectors=d['vectors'],nx=int(d['nx']),ny=int(d['ny']),
        panel=json.loads(str(d['panel'])),beams=json.loads(str(d['beams'])),boundary=str(d['boundary']))
    with np.load(control) as d:f=d['frequency_hz'];notes=d['notes'].tolist();Y=d['mobility'];P=d['pressure_per_force'];count=int(d['original_grid_count'])
    B=base.ports(model,notes);omega=2*np.pi*model['frequency']
    r=dict(criteria=base.CRITERIA,notes=notes,holdout_notes=parent['holdout_notes'],construction_shifts_hz=base.SHIFTS,
        parent_report_sha256=reference.digest(a.parent_report),mechanical_report_sha256=reference.digest(a.mechanical_report),
        parent_asset_sha256=reference.digest(asset),reference_path=str(control),reference_sha256=reference.digest(control),
        source_sha256={n:reference.digest(Path(__file__).with_name(n)) for n in ['audit-concert-grand-spatial-projection.py','audit-concert-grand-compliant-projection.py','concert_grand_compliant_projection.py','concert_grand_basis_readout.py']},
        candidates=[],scope='Dry symmetric P2 projection; virtual fractional-note bridge force locations, held-out integer-note ports and independent frequency grid. No acoustic feedback/audio/factory change.')
    a.output.mkdir(parents=True);reference.write(a.report,r)
    print('building invariant basis readout',flush=True)
    basis=basis_kernel(model['panel'],model['nx'],model['ny'],model['boundary'],[192,144],128000,reference.MICS)
    selected=None
    for spacing in [3,2,1]:
        start=time.monotonic();training=sorted(set(base.TRAIN+[float(v) for v in np.arange(21.5,108,spacing)]+[21.,108.]))
        if set(training)&set(parent['holdout_notes']):raise ValueError('held-out port used in construction')
        print('construction spacing',spacing,'virtual-note units',len(training),'ports',flush=True)
        buildB=base.ports(model,training);red=projection.build(model['frequency'],buildB,6000,base.SHIFTS);Q=red['projection']
        # Compute readout only in the selected subspace; reference is frozen.
        ritz=model['vectors']@Q
        kernel=(basis.reshape(len(basis)*2,-1)@ritz).reshape(len(basis),2,Q.shape[1])
        y=np.empty_like(Y);p=np.empty_like(P)
        for first in range(0,len(f),64):
            hz=f[first:first+64];y[first:first+len(hz)],p[first:first+len(hz)]=base.reduced_response(red,hz,B,kernel)
        trained_full=buildB.T@(buildB/omega[:,None]**2)
        trained_reduced=(Q.T@buildB).T@projection.displacement(red,[0.],buildB)[0]
        static=reference.rel(trained_reduced,trained_full);mass=reference.rel(Q.T@Q,np.eye(Q.shape[1]))
        for name in ['tail_mass','tail_stiffness','tail_damping']:cholesky(red[name])
        eig=np.linalg.eigvalsh((y+y.conj().transpose(0,2,1))/2)
        passive=float(eig.min()/max(np.linalg.norm(y,axis=(1,2)).max(),1e-40));reciprocity=reference.rel(y,y.transpose(0,2,1))
        original=base.metrics(f[:count],y[:count],p[:count],Y[:count],P[:count],notes)
        independent=base.metrics(f[count:],y[count:],p[count:],Y[count:],P[count:],notes)
        gates=dict(transfer_original=original['maximum_port_band']<base.CRITERIA['band_relative'],
            transfer_independent=independent['maximum_port_band']<base.CRITERIA['band_relative'],
            static_training=static<base.CRITERIA['static_training'],mass=mass<base.CRITERIA['mass_normalization'],
            passive=passive>=base.CRITERIA['minimum_dissipation'],reciprocal=reciprocity<base.CRITERIA['reciprocity'])
        result=dict(spacing_virtual_semitones=spacing,construction_positions_note_units=training,retained_modes=red['low_count'],
            tail_coordinates=red['high_rank'],total_coordinates=Q.shape[1],mass_error=mass,static_training_error=static,
            minimum_dissipation=passive,reciprocity=reciprocity,original_grid=original,independent_grid=independent,
            gates=gates,passed=all(gates.values()),seconds=time.monotonic()-start)
        r['candidates'].append(result);reference.write(a.report,r)
        print('measured',spacing,Q.shape[1],json.dumps(gates),'max',original['maximum_port_band'],independent['maximum_port_band'],flush=True)
        if result['passed']:selected=(red,ritz,kernel,training,y,p);break
    r['selected']=selected is not None
    if selected is not None:
        red,ritz,kernel,training,y,p=selected;path=a.output/'Tabla-proyectada-pasiva.npz'
        np.savez_compressed(path,**red,ritz_vectors=ritz,kernel_128000=kernel,
            construction_positions_note_units=training,test_notes=notes,frequency_hz=f,mobility=y,pressure_per_force=p,
            panel=json.dumps(model['panel']),beams=json.dumps(model['beams']),boundary=model['boundary'],nx=model['nx'],ny=model['ny'])
        r.update(projected_asset_path=str(path),projected_asset_sha256=reference.digest(path))
        # Explicit loss ablation, diagnostic only. Never save this as the asset.
        diagonal=dict(red,tail_damping=np.diag(np.diag(red['tail_damping'])))
        dy=np.empty_like(Y);dp=np.empty_like(P)
        for first in range(0,len(f),64):
            hz=f[first:first+64];dy[first:first+len(hz)],dp[first:first+len(hz)]=base.reduced_response(diagonal,hz,B,kernel)
        r['discard_dense_damping_control']=base.metrics(f[count:],dy[count:],dp[count:],y[count:],p[count:],notes)
    r.update(seconds=time.monotonic()-begin,next='Validate reciprocal air load in this same projected surface. Dry held-out transfer does not validate coupled motion or the full keyboard.')
    reference.write(a.report,r);print('finished selected',r['selected'],flush=True)

if __name__=='__main__':main()
