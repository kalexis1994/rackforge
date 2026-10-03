"""P2a structural verification before soundboard replacement in the renderer.

Assumed geometry and supports. Published wood values are contextual, not a
Steinway identification. Test analytic bare plate, boundary conditions,
positive matrices, generalized modal residual, reciprocal passive mobility,
and convergence before producing a structural bank.
"""
import argparse
import hashlib
import json
from pathlib import Path

import numpy as np
from scipy.linalg import cholesky

import concert_grand_orthotropic_plate as plate


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args(); a.output.mkdir(parents=True,exist_ok=True)
    panel=plate.Panel()
    report=dict(policy=__doc__,source='https://arxiv.org/html/1305.3057v1#S2.T1',
                panel=plate.asdict(panel),rigidities_nm=panel.rigidities(),factory_changed=False,
                limits=['rectangular aligned-grain panel, not concert-grand outline',
                        'clamped frame without rim compliance, prestress or crown',
                        'centroidal Euler beams without torsion/eccentric membrane coupling',
                        'no acoustic/air loading or identified microphone transfer'],checks={},convergence=[],banks={})
    bare=plate.solve(panel,8,8,(),boundary='simply-supported')
    dx,dy,d12,d66=panel.rigidities()
    kx,ky=np.meshgrid(np.pi*np.arange(1,9)/panel.length,np.pi*np.arange(1,9)/panel.width,indexing='ij')
    expected=np.sort(np.sqrt((dx*kx**4+2*(d12+2*d66)*kx**2*ky**2+dy*ky**4)/(panel.density*panel.thickness)).ravel()/(2*np.pi))
    error=float(np.max(abs(bare['frequency']/expected-1)))
    assert error<1e-10
    report['checks']['bare_plate_analytic_max_relative_error']=error
    x=np.array([0.0,panel.length]); p0,p1,_=plate.basis(x,panel.length,24,'clamped')
    assert max(abs(p0).max(),abs(p1).max())<1e-10
    report['checks']['clamped_edge_displacement_slope_max']=float(max(abs(p0).max(),abs(p1).max()))
    prior=None
    beams=plate.pilot_beams(panel)
    final=None
    points=[(.22,.5),(.70,.5),(1.18,.5)]
    for nx,ny in ((12,12),(20,16),(28,20),(36,24),(44,28),(52,32)):
        model=plate.solve(panel,nx,ny,beams,modes=256)
        cholesky(model['mass']);cholesky(model['stiffness'])
        row=dict(nx=nx,ny=ny,modes=len(model['frequency']),first_frequency_hz=float(model['frequency'][0]))
        if prior is not None:
            count=min(len(prior['frequency']),len(model['frequency']))
            relative=abs(prior['frequency'][:count]/model['frequency'][:count]-1)
            row['relative_frequency_change_first_40']=float(relative[:40].max())
            row['relative_frequency_change_first_80']=float(relative[:80].max())
            row['relative_frequency_change_all_common']=float(relative.max())
            if count == 256:
                row['mobility_convergence']={}
                for low,high in ((20,250),(250,1000),(1000,2500)):
                    frequencies=np.geomspace(low,high,150)
                    before=plate.mobility(prior,frequencies,points)
                    after=plate.mobility(model,frequencies,points)
                    row['mobility_convergence'][f'{low}-{high}']=float(np.linalg.norm(before-after)/np.linalg.norm(after))
            # Nested Ritz eigenvalues must descend; mode crossings need not
            # preserve individual vectors, so this only compares ordered poles.
            assert (prior['frequency'][:count]>=model['frequency'][:count]*(1-1e-8)).all()
        report['convergence'].append(row)
        print(json.dumps(row),flush=True)
        prior=final=model
    v,m,k=final['vectors'],final['mass'],final['stiffness']
    w2=(2*np.pi*final['frequency'])**2
    residual=k@v-(m@v)*w2
    report['checks']['mass_orthonormality_error']=float(abs(v.T@m@v-np.eye(v.shape[1])).max())
    report['checks']['eigen_residual_relative']=float(np.linalg.norm(residual)/np.linalg.norm(k@v))
    assert report['checks']['mass_orthonormality_error']<1e-9
    assert report['checks']['eigen_residual_relative']<1e-9
    hz=np.geomspace(20,8000,700)
    y=plate.mobility(final,hz,points)
    passive_min=float(np.linalg.eigvalsh(y.real).min())
    assert passive_min>-1e-12
    reciprocity=float(abs(y-y.transpose(0,2,1)).max())
    assert reciprocity<1e-12
    report['checks']['minimum_conductance_eigenvalue']=passive_min
    report['checks']['reciprocity_max_error']=reciprocity
    for name,ribs,bridge in (('Panel',False,False),('Panel-puente',False,True),('Panel-costillas-puente',True,True)):
        model=final if ribs and bridge else plate.solve(panel,final['nx'],final['ny'],plate.pilot_beams(panel,ribs,bridge),modes=256)
        np.savez_compressed(a.output/f'{name}.npz',frequency=model['frequency'],vectors=model['vectors'],
                            nx=model['nx'],ny=model['ny'],panel=json.dumps(model['panel']),
                            beams=json.dumps(model['beams']),boundary=model['boundary'])
        shapes=plate.shape(model,np.array([p[0] for p in points]),np.array([p[1] for p in points]))
        report['banks'][name]=dict(first_12_frequencies_hz=model['frequency'][:12].tolist(),
            maximum_frequency_hz=float(model['frequency'][-1]),beams=model['beams'],
            modes_below_2500=int(sum(model['frequency']<2500)),
            file_sha256=hashlib.sha256((a.output/f'{name}.npz').read_bytes()).hexdigest())
        np.savetxt(a.output/f'{name}-modos.csv',np.column_stack((model['frequency'],shapes.T)),
                   delimiter=',',header='frequency_hz,shape_left_kg^-0.5,shape_middle_kg^-0.5,shape_right_kg^-0.5')
        # Native low-band hybrid. Map the note bridge inside the clamped rim;
        # values on sixteen drive points and their exact DCT-I expansion use
        # the same piecewise interpolation for string load and board source.
        selected=np.flatnonzero(model['frequency']<800)
        note_positions=np.linspace(0,1,16)
        bridge_x=panel.length*(.1+.8*note_positions)
        bridge_shape=plate.shape(model,bridge_x,np.full(16,.5*panel.width))[:,selected]
        dct=np.cos(np.pi*note_positions[:,None]*np.arange(16))
        coefficients=np.linalg.solve(dct,bridge_shape)
        assert abs(dct@coefficients-bridge_shape).max()<1e-12
        gx=(np.arange(40)+.5)/40*panel.length
        gy=(np.arange(24)+.5)/24*panel.width
        x,y=np.meshgrid(gx,gy,indexing='ij')
        surface=plate.shape(model,x,y)[:,selected]
        native=np.column_stack((model['frequency'][selected],bridge_shape.T,coefficients.T,surface.T))
        np.savetxt(a.output/f'{name}-native.csv',native,delimiter=',',fmt='%.9g',
                   header='CG_RITZ_V1 cutoff=800 bridge_points=16 surface=40x24 length=1.4 width=1.0 bridge_inset=0.1..0.9')
        report['banks'][name]['native_modes']=len(selected)
        report['banks'][name]['native_sha256']=hashlib.sha256((a.output/f'{name}-native.csv').read_bytes()).hexdigest()
    report['native_integration']=False
    report['frequency_first_80_converged_within_1_percent']=report['convergence'][-1]['relative_frequency_change_first_80']<.01
    report['converged_for_audio']=False
    report['integration_gates_pending']=['native loading/source/microphone projection equivalence',
        'bridge endpoints inside plate, not on clamped rim',
        'converged retained band and a documented high-frequency complement',
        'full renderer gain, nonlinear dynamics and release controls']
    report['script_sha256']=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    report['solver_sha256']=hashlib.sha256(Path(plate.__file__).read_bytes()).hexdigest()
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps(report['checks']),flush=True)
    print('First 80 poles converged within 1%:',report['frequency_first_80_converged_within_1_percent'],flush=True)


if __name__=='__main__':main()
