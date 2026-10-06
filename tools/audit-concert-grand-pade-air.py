"""Higher-order temporal wave probe on the frozen real 606-board subspace.

No full modal approval or fitted coefficients. Regenerate each base factor
once, measuring legacy/Pade resistance while constructing the fourth-order R.
"""
import argparse,hashlib,importlib.util,json,time
from pathlib import Path
import numpy as np
from concert_grand_projected_support import ModalSupportMap,projected_matrices
from concert_grand_real_surface import RealArithmeticSurface
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_streaming_air import SurfaceFactorBlocks,StreamingRationalAir
from concert_grand_pade_air import FourthFactorBlocks,PadeAir,PadeBoundedAir,pade_delay
import concert_grand_orthotropic_plate as plate


def digest(path):
    with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()

def relative(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-40))

class ObservedBlocks:
    def __init__(self,provider,observe):self.provider=provider;self.observe=observe
    def __getattr__(self,name):return getattr(self.provider,name)
    def blocks(self):
        for H in self.provider.blocks():self.observe(H);yield H


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['loaded-report','support-report','board','diagnostic','report']:p.add_argument('--'+name,type=Path,required=True)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    start=time.monotonic();parent=json.loads(a.loaded_report.read_text());support=json.loads(a.support_report.read_text())
    control=json.loads(a.diagnostic.read_text())
    if not parent['all_gates_passed'] or not support['all_gates_passed']:raise ValueError('approved mechanical/continuous parents required')
    if (digest(a.board)!=support['board_sha256'] or digest(a.loaded_report)!=support['parent_sha256']
        or control['board_sha256']!=support['board_sha256'] or control['cells']!=parent['cells']):raise ValueError('unchanged parent chain required')
    path=Path(__file__).with_name('audit-concert-grand-streaming-air.py')
    spec=importlib.util.spec_from_file_location('stream_reference',path);reference=importlib.util.module_from_spec(spec);spec.loader.exec_module(reference)
    with np.load(a.board) as d:
        mechanical={k:d[k] for k in ['frequency','low_count','tail_mass','tail_stiffness','tail_damping','low_damping']}
        model=dict(panel=json.loads(str(d['panel'])),boundary=str(d['boundary']),nx=int(d['nx']),ny=int(d['ny']),vectors=d['ritz_vectors'])
        notes=d['test_notes'].tolist()
    modal=ModalSupportMap.from_matrices(*projected_matrices(mechanical));model['vectors']=model['vectors']@modal.basis
    surface=RealArithmeticSurface(model,parent['cells']);omega=2*np.pi*control['frequency_hz'];s=1j*omega
    x=model['panel']['length']*(.1+.8*(np.array(notes)-21)/87)
    B=plate.shape(model,x,np.full(len(x),model['panel']['width']/2)).T
    print('continuous state reference',flush=True);Z=reference.direct_matrix(surface,s)
    q=np.linalg.solve(np.diag((2*np.pi*modal.frequency)**2+s*s)+s*(modal.damping+Z),B)
    rng=np.random.default_rng(control['random_seed'])
    probes=np.column_stack((q.real,q.imag,rng.normal(size=(surface.modes,control['random_probes']))))
    probes/=np.maximum(np.linalg.norm(probes,axis=0),1e-300)
    T,singular,_=np.linalg.svd(probes,full_matrices=False);T=T[:,singular>1e-12*singular[0]]
    if T.shape[1]!=control['probe_rank']:raise ValueError('changed diagnostic span')
    actual=RectangularLoad(surface.length,surface.width,surface.forward(T),surface.density,surface.speed)
    exact=T.T@Z@T
    rate=128000;polar,azimuth=control['angular'];base=SurfaceFactorBlocks(actual,rate,polar,azimuth,fold_y=True)
    cayley=(2*rate-s)/(2*rate+s);pade=pade_delay(s,rate)
    phases=[t**np.arange(base.taps) for t in [cayley,pade]]
    resistance=[np.zeros((actual.modes,actual.modes)) for _ in phases]
    def observe(H):
        for phase,R in zip(phases,resistance):
            field=np.einsum('t,tom->om',phase,H[:,::2],optimize=True);R+=(field.conj().T@field).real
    provider=FourthFactorBlocks(ObservedBlocks(base,observe))
    report=dict(rate=rate,modes=surface.modes,probe_rank=actual.modes,angular=control['angular'],cells=parent['cells'],
        loaded_state_span_relative=relative(T@(T.T@q),q),
        parent_sha256={str(path):digest(path) for path in [a.loaded_report,a.support_report,a.diagnostic]},
        board_sha256=digest(a.board),source_sha256={name:digest(Path(__file__).with_name(name)) for name in [
            Path(__file__).name,'concert_grand_pade_air.py','concert_grand_streaming_air.py','concert_grand_rational_air.py',
            'concert_grand_bounded_air.py','concert_grand_surface_impedance.py','audit-concert-grand-streaming-air.py',
            'concert_grand_projected_support.py','concert_grand_real_surface.py','concert_grand_matrixfree_air.py']},
        budget=provider.budget(real_gram=True),max_array_bytes=2**30,
        criteria=dict(probe_ZRX_relative=.02,probe_Laplace_relative=.02,passivity_relative=-1e-10),
        preflight_only=True,all_gates_passed=False,
        scope='30-coordinate contracted air probe only. No full modal residual/transfer, coupled-time, timbral or runtime approval.')
    print('wave construction',json.dumps(report['budget']),flush=True)
    wave=StreamingRationalAir(provider,report['max_array_bytes'],real_gram=True,
        progress=lambda i:print('wave block',i,flush=True) if i%64==0 else None)
    corrected=PadeAir(wave)
    report['resistance_ablation']={name:relative(R,exact.real) for name,R in zip(['legacy_Cayley','legacy_Pade2'],resistance)}
    report['resistance_ablation']['fourth_Cayley']=relative(wave.impedance(s).real,exact.real)
    report['resistance_ablation']['fourth_Pade2']=relative(corrected.impedance(s).real,exact.real)
    old=next(row for row in control['rows'] if row['rate']==rate)['wave_reference_relative']
    report['legacy_reproduction_error']=abs(report['resistance_ablation']['legacy_Cayley']-old)
    if report['legacy_reproduction_error']>1e-10:raise AssertionError('changed original resistance control')
    target=actual.added_mass_static();limit=actual.density*actual.speed*actual.hx*actual.hy*(base.shapes.T@base.shapes)
    eig=np.linalg.eigvalsh((target-corrected.static_mass()+target.T-corrected.static_mass().T)/2)
    report['probe_mass_residual_minimum']=float(eig.min());report['probe_mass_residual_maximum']=float(eig.max())
    report['probe_states']=corrected.states;report['original_probe_states']=wave.states
    report['poles_real']=-3*rate;report['poles_imaginary_magnitude']=np.sqrt(3)*rate
    print('factor ablation',json.dumps(report['resistance_ablation']),flush=True)
    try:
        candidate=PadeBoundedAir(corrected,target,limit);report['finite_completion']=True
        report['realized_mass_relative']=relative(candidate.static_mass(),target)
        report['realized_limit_relative']=relative(candidate.high_limit(),limit)
    except ValueError as error:
        candidate=corrected;report['finite_completion']=False;report['completion_rejection']=str(error)
    report['rows']=[]
    for hz in [60.,1881.9607122235952,control['frequency_hz']]:
        point=2j*np.pi*hz;ref=actual.impedance(point);z=candidate.impedance(point)
        row=dict(frequency_hz=hz,impedance_ZRX_relative=[relative(z,ref),relative(z.real,ref.real),relative(z.imag,ref.imag)],
            hermitian_minimum_relative=float(np.linalg.eigvalsh((z+z.conj().T)/2).min()/max(np.linalg.norm(z),1e-40)))
        report['rows'].append(row);print('transfer',json.dumps(row),flush=True)
    report['laplace']=[]
    for point in [100+4000j,1000+20000j,5000+40000j]:
        ref=actual.impedance(point);z=candidate.impedance(point)
        row=dict(point=[point.real,point.imag],impedance_relative=relative(z,ref),
            hermitian_minimum_relative=float(np.linalg.eigvalsh((z+z.conj().T)/2).min()/max(np.linalg.norm(z),1e-40)))
        report['laplace'].append(row);print('Laplace',json.dumps(row),flush=True)
    report['probe_gates']=dict(finite_completion=report['finite_completion'],
        impedance=report['finite_completion'] and max(max(row['impedance_ZRX_relative']) for row in report['rows'])<.02,
        Laplace=report['finite_completion'] and max(row['impedance_relative'] for row in report['laplace'])<.02,
        passive=min(row['hermitian_minimum_relative'] for row in report['rows']+report['laplace'])>=-1e-10)
    report['seconds']=time.monotonic()-start;report['peak_working_set_bytes']=reference.peak_memory()
    a.report.parent.mkdir(parents=True,exist_ok=True);a.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    print('probe finished',json.dumps(report['probe_gates']),report['seconds'],flush=True)


if __name__=='__main__':main()
