"""P2 global bridge geometry, with a separate source-path-only ablation.

Assumed sinusoidal 10 cm bow, no fit or measured Steinway geometry. Positive
centroidal line mass/flexion only; curved-rod torsion/composite action omitted.
Keep the former straight asset byte exact and audit the reduced curved model
before exporting the same native low-band asset format.
"""
import argparse
from dataclasses import replace
import hashlib
import json
from pathlib import Path
import shutil

import numpy as np
from scipy.linalg import cholesky

import concert_grand_orthotropic_plate as plate
from importlib.util import spec_from_file_location, module_from_spec


def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--previous',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    spec=spec_from_file_location('resolution',Path(__file__).with_name('audit-concert-grand-ritz-resolution.py'))
    resolution=module_from_spec(spec);spec.loader.exec_module(resolution)
    panel=plate.Panel();beams=plate.pilot_beams(panel)
    straight=beams[-1];curve=replace(straight,bow=.10)
    curved_beams=(*beams[:-1],curve)
    report=dict(policy=__doc__,factory_changed=False,checks={},convergence=[],banks={})
    # Independent finite-difference check of arc-length differentiation.
    x=np.linspace(.15,1.25,19);y,yp,ypp=plate.beam_path(x,panel,curve)
    qx,qy=3*np.pi/panel.length,4*np.pi/panel.width
    phi=lambda z:np.sin(qx*z)*np.sin(qy*plate.beam_path(z,panel,curve)[0])
    w1=qx*np.cos(qx*x)*np.sin(qy*y)+yp*qy*np.sin(qx*x)*np.cos(qy*y)
    w2=(-qx*qx*np.sin(qx*x)*np.sin(qy*y)+2*yp*qx*qy*np.cos(qx*x)*np.cos(qy*y)
        -yp*yp*qy*qy*np.sin(qx*x)*np.sin(qy*y)+ypp*qy*np.sin(qx*x)*np.cos(qy*y))
    h=1e-5
    finite=(phi(x+h)-2*phi(x)+phi(x-h))/h**2/(1+yp*yp)
    finite-=(phi(x+h)-phi(x-h))/(2*h)*yp*ypp/(1+yp*yp)**2
    exact=w2/(1+yp*yp)-w1*yp*ypp/(1+yp*yp)**2
    report['checks']['arc_derivative_relative_error']=float(np.linalg.norm(finite-exact)/np.linalg.norm(exact))
    assert report['checks']['arc_derivative_relative_error']<1e-6
    # Zero-curvature quadrature must recover the original analytic assembly.
    bm,bk=plate.curved_beam_matrices(panel,8,8,straight,'clamped')
    empty=plate.assemble(panel,8,8)
    old=plate.assemble(panel,8,8,(straight,))
    for key,actual,expected in [('straight_mass',bm,old[0]-empty[0]),('straight_stiffness',bk,old[1]-empty[1])]:
        error=float(np.linalg.norm(actual-expected)/np.linalg.norm(expected))
        assert error<1e-10;report['checks'][key+'_relative_error']=error
    points=[(.22,float(plate.beam_path(.22,panel,curve)[0])),
            (.70,float(plate.beam_path(.70,panel,curve)[0])),
            (1.18,float(plate.beam_path(1.18,panel,curve)[0]))]
    previous=None
    for nx,ny in ((36,24),(44,28),(52,32)):
        model=plate.solve(panel,nx,ny,curved_beams,modes=256)
        cholesky(model['mass']);cholesky(model['stiffness'])
        row=dict(nx=nx,ny=ny)
        if previous is not None:
            row['frequency_max_relative_change']=float(abs(previous['frequency']/model['frequency']-1).max())
            before=plate.mobility(previous,np.geomspace(20,1000,300),points)
            after=plate.mobility(model,np.geomspace(20,1000,300),points)
            row['mobility_relative_change_20_1000']=float(np.linalg.norm(before-after)/np.linalg.norm(after))
            assert (previous['frequency']>=model['frequency']*(1-1e-8)).all()
        report['convergence'].append(row);previous=model
        print(row,flush=True)
    assert report['convergence'][-1]['frequency_max_relative_change']<.01
    v,m,k=model['vectors'],model['mass'],model['stiffness']
    w2=(2*np.pi*model['frequency'])**2
    report['checks']['mass_orthonormality_error']=float(abs(v.T@m@v-np.eye(256)).max())
    report['checks']['eigen_relative_residual']=float(np.linalg.norm(k@v-(m@v)*w2)/np.linalg.norm(k@v))
    mobility=plate.mobility(model,np.geomspace(20,8000,700),points)
    report['checks']['minimum_conductance_eigenvalue']=float(np.linalg.eigvalsh(mobility.real).min())
    assert report['checks']['minimum_conductance_eigenvalue']>-1e-12
    report['checks']['reciprocity_max_error']=float(abs(mobility-mobility.transpose(0,2,1)).max())
    # Quadrature convergence of the curved line at the largest Ritz space.
    coarse=plate.curved_beam_matrices(panel,52,32,curve,'clamped')
    fine=plate.curved_beam_matrices(panel,52,32,curve,'clamped',quadrature=1008)
    report['checks']['line_quadrature_mass_relative_change']=float(np.linalg.norm(coarse[0]-fine[0])/np.linalg.norm(fine[0]))
    report['checks']['line_quadrature_stiffness_relative_change']=float(np.linalg.norm(coarse[1]-fine[1])/np.linalg.norm(fine[1]))
    assert max(report['checks']['line_quadrature_mass_relative_change'],report['checks']['line_quadrature_stiffness_relative_change'])<1e-8
    del coarse,fine
    source=a.previous/'Panel-costillas-puente-native.csv'
    shutil.copy2(source,a.output/'Recto-native.csv')
    old_model=resolution.load_model(a.previous/'Panel-costillas-puente.npz')
    for name,data,curved_source in [('Recto',old_model,False),('Curva-solo-trayectoria',old_model,True),('Curva-estructural',model,True)]:
        selected=np.flatnonzero(data['frequency']<800)
        x=panel.length*(.1+.8*np.linspace(0,1,16))
        y=plate.beam_path(x,panel,curve)[0] if curved_source else np.full(16,.5)
        source_shapes=plate.shape(data,x,y)[:,selected]
        dct=np.cos(np.pi*np.linspace(0,1,16)[:,None]*np.arange(16))
        coefficients=np.linalg.solve(dct,source_shapes)
        _,_,surface=resolution.surface_grid(data,1,selected)
        if name!='Recto':
            native=np.column_stack((data['frequency'][selected],source_shapes.T,coefficients.T,surface))
            np.savetxt(a.output/f'{name}-native.csv',native,delimiter=',',fmt='%.9g',
                header='CG_RITZ_V1 cutoff=800 bridge_points=16 surface=40x24 length=1.4 width=1.0 bridge_inset=0.1..0.9')
        np.savez_compressed(a.output/f'{name}.npz',frequency=data['frequency'],vectors=data['vectors'],
                            nx=data['nx'],ny=data['ny'],panel=json.dumps(data['panel']),boundary=data['boundary'])
        # Document interpolation sensitivity for this actual source path.
        xx=panel.length*(.1+.8*np.arange(88)/87)
        yy=plate.beam_path(xx,panel,curve)[0] if curved_source else np.full(88,.5)
        exact_shapes=plate.shape(data,xx,yy)[:,selected]
        approximation=resolution.interpolate(source_shapes,np.arange(88)/87)
        report['banks'][name]=dict(native_modes=len(selected),native_sha256=sha(a.output/f'{name}-native.csv'),
            file_sha256=sha(a.output/f'{name}.npz'),source_curve=curved_source,
            curved_structure=name=='Curva-estructural',source_bow_m=.10 if curved_source else 0.,
            relative_source_shape_error=float(np.linalg.norm(approximation-exact_shapes)/np.linalg.norm(exact_shapes)),
            first_12_frequencies_hz=data['frequency'][:12].tolist())
    report['script_sha256']=sha(Path(__file__))
    report['solver_sha256']=sha(Path(plate.__file__))
    report['previous_report_sha256']=sha(a.previous/'report.json')
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(report['checks'],flush=True)


if __name__=='__main__':main()
