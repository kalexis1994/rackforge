"""Check full modal velocity error behind held-out pressure/bridge agreement.

Mass-normalized state norm is a whole-mechanics diagnostic, not an acoustic
feedback validation. Elastic/loss weighted errors are recorded separately.
"""
import argparse,json
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
import concert_grand_compliant_projection as projection

spec=spec_from_file_location('projected_state_reference',Path(__file__).with_name('audit-concert-grand-compliant-projection.py'))
base=module_from_spec(spec);spec.loader.exec_module(base)
reference=base.reference


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for n in ['mechanical-report','projection-report','report']:p.add_argument('--'+n,type=Path,required=True)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    mechanical=json.loads(a.mechanical_report.read_text());parent=json.loads(a.projection_report.read_text())
    if not parent['selected'] or reference.digest(a.mechanical_report)!=parent['mechanical_report_sha256']:
        raise ValueError('verified frozen projection required')
    asset=Path(mechanical['structural_asset_path']);projected=Path(parent['projected_asset_path'])
    if reference.digest(asset)!=parent['parent_asset_sha256'] or reference.digest(projected)!=parent['projected_asset_sha256']:
        raise ValueError('asset hashes changed')
    with np.load(asset) as d:model=dict(frequency=d['frequency'],vectors=d['vectors'],nx=int(d['nx']),ny=int(d['ny']),panel=json.loads(str(d['panel'])),boundary=str(d['boundary']))
    with np.load(projected) as d:
        red={n:d[n] for n in ['projection','frequency','tail_mass','tail_stiffness','tail_damping','low_damping']}
        red['low_count']=int(d['low_count']);notes=d['test_notes'].tolist();ritz=d['ritz_vectors']
    mapped=reference.rel(model['vectors']@red['projection'],ritz)
    if mapped>1e-11:raise AssertionError('inconsistent stored surface projection')
    B=base.ports(model,notes);Q=red['projection'];w0=2*np.pi*model['frequency'];f=60+(np.arange(901)+.5)*5940/901
    powers={name:[np.zeros((len(f),len(notes))),np.zeros((len(f),len(notes)))] for name in ['mass_velocity','elastic_state','viscous_velocity']}
    for first in range(0,len(f),64):
        hz=f[first:first+64];w=2*np.pi*hz
        full=B[None]/(w0[None,:,None]**2-w[:,None,None]**2+1j*.046*w[:,None,None]*w0[None,:,None])
        reduced=np.einsum('nm,fmp->fnp',Q,projection.displacement(red,hz,B),optimize=True)
        difference=abs(full-reduced)**2;power=abs(full)**2
        for name,weight in [('mass_velocity',w[:,None]**2*np.ones((1,len(w0)))),
            ('elastic_state',np.broadcast_to(w0*w0,(len(hz),len(w0)))),
            ('viscous_velocity',w[:,None]**2*(.046*w0)[None])]:
            for destination,values in zip(powers[name],[difference,power]):
                destination[first:first+len(hz)]=np.einsum('fm,fmp->fp',weight,values,optimize=True)
    out={}
    for i,n in enumerate(notes):
        out[str(n)]={}
        for name,(error,power) in powers.items():
            result={}
            for band,lo,hi in [('low',60,1200),('middle',1200,3000),('upper',3000,6000)]:
                mask=(f>=lo)&(f<=hi)
                result[band]=float(np.sqrt(np.trapezoid(error[mask,i],f[mask])/np.trapezoid(power[mask,i],f[mask])))
            out[str(n)][name]=result
    maximum=max(v for row in out.values() for v in row['mass_velocity'].values())
    report=dict(parent_report_sha256=reference.digest(a.projection_report),mechanical_report_sha256=reference.digest(a.mechanical_report),
        inputs_sha256={str(asset):reference.digest(asset),str(projected):reference.digest(projected)},
        source_sha256={n:reference.digest(Path(__file__).with_name(n)) for n in ['audit-concert-grand-projected-state.py','concert_grand_compliant_projection.py']},
        frequency_points=len(f),by_note=out,stored_ritz_relative=mapped,maximum_mass_velocity_relative=maximum,
        mass_velocity_criterion=.01,mass_velocity_passed=maximum<.01,
        definitions=dict(mass_velocity='sqrt(integral(w^2*sum(abs(error_modal_q)^2))/integral(w^2*sum(abs(full_modal_q)^2)))',
            elastic_state='same with modal omega^2 weights; diagnostic, no selection threshold',
            viscous_velocity='same with w^2*modal damping weights; diagnostic, no selection threshold'),
        scope='Full mass-normalized dry modal motion, four construction-note ports and seven held-out integer ports. Not actual surface-relative velocity, reciprocal air loading or a time-domain note.')
    reference.write(a.report,report);print(json.dumps(dict(maximum_mass_velocity_relative=maximum,passed=report['mass_velocity_passed'])),flush=True)

if __name__=='__main__':main()
