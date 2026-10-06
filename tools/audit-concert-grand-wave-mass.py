"""Early finite-completion mass gate; no full wave correlation allocation."""
import argparse,json,time
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
from types import SimpleNamespace
import numpy as np
from concert_grand_wave_mass import static_wave_mass
from concert_grand_streaming_air import SurfaceFactorBlocks
from concert_grand_projected_support import ModalSupportMap,projected_matrices
from concert_grand_real_surface import RealArithmeticSurface

spec=spec_from_file_location('wave_mass_reference',Path(__file__).with_name('audit-concert-grand-streaming-air.py'))
reference=module_from_spec(spec);spec.loader.exec_module(reference)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['loaded-report','support-report','board','comparison-report','report']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--polar',nargs='+',type=int,default=[32,64]);p.add_argument('--rate',type=int,default=128000)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    start=time.monotonic();parent=json.loads(a.loaded_report.read_text());support=json.loads(a.support_report.read_text())
    old=json.loads(a.comparison_report.read_text())
    if not parent['all_gates_passed'] or not support['all_gates_passed']:raise ValueError('approved parents required')
    if reference.digest(a.loaded_report)!=support['parent_sha256'] or reference.digest(a.board)!=support['board_sha256']:raise ValueError('changed parents')
    if (old['board_sha256']!=support['board_sha256'] or old['rate']!=a.rate
        or old['cells']!=parent['cells'] or old['angular'][0] not in a.polar
        or old['angular'][1]!=2*old['angular'][0]):raise ValueError('matching correlation control required')
    with np.load(a.board) as d:
        mechanical={k:d[k] for k in ['frequency','low_count','tail_mass','tail_stiffness','tail_damping','low_damping']}
        model=dict(panel=json.loads(str(d['panel'])),boundary=str(d['boundary']),nx=int(d['nx']),ny=int(d['ny']),vectors=d['ritz_vectors'])
    modal=ModalSupportMap.from_matrices(*projected_matrices(mechanical));model['vectors']=model['vectors']@modal.basis
    surface=RealArithmeticSurface(model,parent['cells']);n=surface.modes;del model,mechanical
    shapes=surface.forward(np.eye(n))
    plain=SimpleNamespace(shapes=shapes,modes=n,nx=surface.nx,ny=surface.ny,hx=surface.hx,hy=surface.hy,
        length=surface.length,width=surface.width,density=surface.density,speed=surface.speed)
    target=reference.direct_matrix(surface,0).real;target=(target+target.T)/2
    r=dict(rate=a.rate,modes=n,cells=parent['cells'],rows=[],
        parent_sha256={str(path):reference.digest(path) for path in [a.loaded_report,a.support_report,a.comparison_report]},
        board_sha256=reference.digest(a.board),source_sha256={name:reference.digest(Path(__file__).with_name(name)) for name in [
            'audit-concert-grand-wave-mass.py','concert_grand_wave_mass.py','concert_grand_streaming_air.py',
            'audit-concert-grand-streaming-air.py','concert_grand_projected_support.py',
            'concert_grand_real_surface.py','concert_grand_matrixfree_air.py']},
        scope='Exact discrete wave DC mass gate only. No correlations, causal transfer/transient or musical approval.',
        preflight_only=True,all_gates_passed=False)
    for polar in a.polar:
        provider=SurfaceFactorBlocks(plain,a.rate,polar,2*polar);t=time.monotonic()
        mass=static_wave_mass(provider,progress=lambda i:print('mass',polar,'block',i,flush=True) if i%16==0 else None)
        residual=target-mass;eig=np.linalg.eigvalsh((residual+residual.T)/2)
        row=dict(angular=[polar,2*polar],mass_residual_minimum=float(eig.min()),mass_residual_maximum=float(eig.max()),
            finite_mass_possible=bool(eig.min()>1e-13*eig.max()),seconds=time.monotonic()-t,
            avoided_gram_and_correlation_bytes=provider.budget()['gram_bytes']+provider.budget()['correlation_bytes'])
        if polar==old['angular'][0]:
            row['correlation_control_eigen_relative']=float(max(abs(eig.min()-old['mass_residual_minimum']),
                abs(eig.max()-old['mass_residual_maximum']))/max(abs(eig).max(),1e-40))
            row['equivalence_passed']=row['correlation_control_eigen_relative']<1e-10
            if not row['equivalence_passed']:raise AssertionError('cumulative mass differs from correlation control')
        r['rows'].append(row);print('mass result',json.dumps(row),flush=True)
    r['peak_working_set_bytes']=reference.peak_memory();r['seconds']=time.monotonic()-start
    r['next']='Only build a full correlation candidate after its unmodified mass residual passes; complete transfer/time gates still required.'
    a.report.parent.mkdir(parents=True,exist_ok=True);a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')
    print('mass scan finished',json.dumps({k:r[k] for k in ['seconds','peak_working_set_bytes']}),flush=True)


if __name__=='__main__':main()
