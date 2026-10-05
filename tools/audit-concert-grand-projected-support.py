"""Verify actual projected board adapter and short full-coordinate contact.

Sparse harmonic coordinate-equivalence checks, no acoustic time realization,
no new fit, waveform or plugin. Save small report only; no duplicate factors.
"""
import argparse,json,hashlib,time
from pathlib import Path
import numpy as np
from concert_grand_projected_support import ModalSupportMap,projected_matrices
from concert_grand_moving_bridge import MovingBridge,BridgeHammer
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
import concert_grand_orthotropic_plate as plate


def digest(path):
    with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()


def relative(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-40))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--loaded-report',type=Path,required=True)
    p.add_argument('--board',type=Path,required=True);p.add_argument('--report',type=Path,required=True)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    start=time.monotonic();parent=json.loads(a.loaded_report.read_text())
    if not parent['all_gates_passed'] or parent['preflight_only']:raise ValueError('approved loaded parent required')
    if digest(a.board)!=parent['inputs_sha256'][str(a.board)]:raise ValueError('changed board input')
    with np.load(a.board) as d:
        model={name:d[name] for name in ['frequency','low_count','tail_mass','tail_stiffness','tail_damping','low_damping']}
        geometry=dict(panel=json.loads(str(d['panel'])),boundary=str(d['boundary']),nx=int(d['nx']),ny=int(d['ny']),vectors=d['ritz_vectors'])
        notes=d['test_notes'].tolist()
    M,K,C=projected_matrices(model);modal=ModalSupportMap.from_matrices(M,K,C);V=modal.basis
    x=geometry['panel']['length']*(.1+.8*(np.array(notes)-21)/87)
    B=plate.shape(geometry,x,np.full(len(x),geometry['panel']['width']/2)).T
    del geometry
    b=V.T@B;n=len(V);w2=(2*np.pi*modal.frequency)**2
    criteria=dict(congruence_relative=1e-11,harmonic_state_relative=1e-10,
        harmonic_port_relative=1e-10,power_relative=1e-10,time_energy_relative=1e-9,
        interface_force_n=1e-9,interface_power_w=1e-9)
    r=dict(criteria=criteria,modes=n,notes=notes,rows=[],
        mass_congruence=relative(V.T@M@V,np.eye(n)),
        stiffness_congruence=relative(V.T@K@V,np.diag(w2)),
        loss_congruence=relative(modal.damping,V.T@C@V),
        full_damping_minimum=float(np.linalg.eigvalsh(modal.damping).min()),
        damping_offdiagonal_relative=float(np.linalg.norm(modal.damping-np.diag(np.diag(modal.damping)))/np.linalg.norm(modal.damping)),
        parent_sha256=digest(a.loaded_report),board_sha256=digest(a.board),
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in [
            'audit-concert-grand-projected-support.py','concert_grand_projected_support.py',
            'concert_grand_moving_bridge.py','concert_grand_bounded_contact.py',
            'concert_grand_bath_contact.py','concert_grand_exponential_coast.py',
            'concert_grand_damped_coast.py']})
    for hz in parent['frequencies_hz']:
        s=2j*np.pi*hz
        reference=np.linalg.solve(K+s*C+s*s*M,B)
        q=np.linalg.solve(np.diag(w2)+s*modal.damping+s*s*np.eye(n),b)
        v=s*q;old_v=s*reference
        old_heat=np.real(np.einsum('ip,ij,jp->p',old_v.conj(),C,old_v))
        new_heat=np.real(np.einsum('ip,ij,jp->p',v.conj(),modal.damping,v))
        row=dict(frequency_hz=hz,state_relative=relative(V@q,reference),
            port_relative=relative(s*b.T@q,s*B.T@reference),
            power_relative=float(np.max(abs(old_heat-new_heat)/np.maximum(abs(old_heat),1e-40))))
        r['rows'].append(row)
    # An explicit short mechanical pilot in all 606 board coordinates.
    # The wire/felt are assumptions from the existing test fixture, not a fit.
    core=MovingBridge(Wire(.62,.0081,850),modal.support(B[:,notes.index(60)]),transverse=3)
    system=BridgeHammer(core,Felt(.002,7.5e8,2.6,.125,.02,.1))
    q,v=system.initial_state(5.);initial=system.energy(q,v);heat=0.;max_energy=max_force=max_power=0.
    dt=1/1536000;steps=192
    for step in range(steps):
        q,v,stats=system.step(q,v,dt);heat+=stats['loss_j']
        max_energy=max(max_energy,abs(system.energy(q,v)+heat-initial)/initial)
        force,_=system.continuous_force(q,v);interface=system.interface(q,v,-force)
        max_force=max(max_force,interface['force_equation_residual']);max_power=max(max_power,abs(interface['power_residual_w']))
    r['time_pilot']=dict(note=60,seconds=steps*dt,steps=steps,dt=dt,speed_m_s=5.,
        initial_energy_j=initial,final_energy_j=system.energy(q,v),heat_j=heat,
        energy_relative=max_energy,interface_force_n=max_force,interface_power_w=max_power,
        scope='Short mechanical contact only; no acoustic states, release, melody or timbral approval.')
    r['gates']=dict(congruence=max(r[k] for k in ['mass_congruence','stiffness_congruence','loss_congruence'])<criteria['congruence_relative'],
        state=max(row['state_relative'] for row in r['rows'])<criteria['harmonic_state_relative'],
        port=max(row['port_relative'] for row in r['rows'])<criteria['harmonic_port_relative'],
        power=max(row['power_relative'] for row in r['rows'])<criteria['power_relative'],
        passive=r['full_damping_minimum']>=0,
        time_energy=max_energy<criteria['time_energy_relative'],
        interface=max_force<criteria['interface_force_n'] and max_power<criteria['interface_power_w'])
    r['gates']={key:bool(value) for key,value in r['gates'].items()}
    r['all_gates_passed']=all(r['gates'].values());r['seconds']=time.monotonic()-start
    r['scope']='Coordinate adapter only, retaining complete projected mass/stiffness/damping. No causal air construction or audio.'
    a.report.parent.mkdir(parents=True,exist_ok=True);a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')
    print(json.dumps({k:r[k] for k in ['gates','time_pilot','seconds']}),flush=True)
    if not r['all_gates_passed']:raise SystemExit(1)


if __name__=='__main__':main()
