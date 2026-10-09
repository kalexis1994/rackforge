"""Cold, live physical phrase: three-wire choirs, ONE 606-board and ONE air.

This small register diagnostic exercises sympathetic resonance, overlapping
notes, physical restrikes and passive note-off. No sampled-note assembly.
It retains the prior acoustic reduction's FAILED accuracy gate explicitly.
Not a complete keyboard, identified Steinway, room or realtime plugin.
"""
import argparse
import importlib.util
import json
import time
from pathlib import Path
from types import SimpleNamespace
import numpy as np
from scipy.linalg import lu_factor,lu_solve
from scipy.interpolate import CubicHermiteSpline
from scipy.signal import resample_poly
from scipy.io import wavfile
from concert_grand_wave_cache import digest
from concert_grand_audition_air import project_air_preserving_relaxation
from concert_grand_shared_strings import (WirePort,SharedStrings,SharedHammers,
    SharedContact,SharedCoast,SharedDampers)
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
from concert_grand_damper_control import DamperControl
from concert_grand_cell_observer import CellObserver
from concert_grand_cell_audio import retarded_kernel,pressure_fft
import concert_grand_orthotropic_plate as plate

spec=importlib.util.spec_from_file_location('shared_phrase_reference',Path(__file__).with_name('render-concert-grand-current-audition.py'))
reference=importlib.util.module_from_spec(spec);spec.loader.exec_module(reference)
parent,prior,field=reference.parent,reference.prior,reference.field


def prepare(report,save):
    a=SimpleNamespace(full_report=prior.DOC/'medidas-P6-aire-bloques/carga128-pade2.json',
        board=prior.LOCAL/'P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz',
        cache=prior.LOCAL/'P6-carga-causal-2026-10-06',direction_block=32)
    approved,modal,_,_,surface,provider,identity,_=field.full.prepare_reference(a)
    if not (a.cache/'R.npy').is_file():
        raise ValueError('existing single full R required')
    _,_,full=field.full.construct_air(a,approved,provider,surface,identity,12*2**30,report,save)
    frozen_path=prior.DOC/'medidas-P6-escucha-2026-10-08/escucha-pasos-medidos.json'
    frozen=json.loads(frozen_path.read_text(encoding='utf-8'))
    if digest(a.board)!=frozen['board_sha256'] or report['cache_R_sha256']!=frozen['cache_R_sha256']:
        raise ValueError('changed frozen board/air')
    with np.load(a.board) as d:
        geometry=dict(panel=json.loads(str(d['panel'])),boundary=str(d['boundary']),
            nx=int(d['nx']),ny=int(d['ny']),vectors=d['ritz_vectors']@modal.basis)
    train_notes=np.array([60,65,67,72])
    xs=geometry['panel']['length']*(.1+.8*(train_notes-21)/87)
    phi=plate.shape(geometry,xs,np.full(4,geometry['panel']['width']/2)).T
    mass=full.wave.static_mass();values,vectors=np.linalg.eigh((mass+mass.T)/2)
    root=(vectors*np.sqrt(values))@vectors.T;inverse=(vectors/np.sqrt(values))@vectors.T
    snapshots=[];omega2=(2*np.pi*modal.frequency)**2
    for index,hz in enumerate(np.geomspace(70.,12000.,48)):
        s=2j*np.pi*hz;Z=full.impedance(s)
        D=np.diag(omega2+s*s)+s*modal.damping
        V=s*lu_solve(lu_factor(D+s*Z),phi)
        w=root@V;force=inverse@(Z-full.relaxation.impedance(s))@V
        for part in [w.real,w.imag,force.real,force.imag]:
            snapshots.append(part/np.maximum(np.linalg.norm(part,axis=0),1e-30))
        if (index+1)%12==0:
            print('shared acoustic snapshots',index+1,48,flush=True)
    U,_singular,_=np.linalg.svd(np.hstack(snapshots),full_matrices=False)
    basis_hash=__import__('hashlib').sha256(U[:,:192].tobytes()).hexdigest()
    if basis_hash!=frozen['reduction_basis_sha256']:
        raise ValueError('changed frozen acoustic reduction basis')
    air=project_air_preserving_relaxation(full,U[:,:192])
    observer=CellObserver(provider.surface,approved.get('microphones',json.loads((prior.DOC/'medidas-P6-contacto-banda-superior/contacto-24k.json').read_text())['microphones']),4)
    kernel=retarded_kernel(observer,128000)
    report.update(board_sha256=digest(a.board),cache_manifest_sha256=digest(a.cache/'manifest.json'),
        physical_source_report_sha256=digest(frozen_path),reduction_basis_sha256=basis_hash,
        acoustic_accuracy_gate_passed=False,
        inherited_worst_loaded_pressure_relative=frozen['acoustic_reduction_cases'][0]['worst_pressure_relative'],
        acoustic_rank=192,board_coordinates=606)
    save()
    return modal,geometry,air,kernel,a


def make_system(modal,geometry,transverse,choir):
    keys=[60,67,72]
    xs=geometry['panel']['length']*(.1+.8*(np.array(keys)-21)/87)
    phi=plate.shape(geometry,xs,np.full(len(keys),geometry['panel']['width']/2)).T
    # Small explicit tension irregularity, NOT fitted nor randomized in time.
    cents=[-.4,0.,.6] if choir==3 else [0.]
    ports=[]
    for index,key in enumerate(keys):
        for offset in cents:
            ports.append(WirePort(key,Wire(.62*2**((60-key)/12),.0081,850.*2**(offset/600)),phi[:,index]))
    core=SharedStrings(ports,modal.frequency,modal.damping,transverse,2*transverse)
    return SharedHammers(core,Felt(.002,7.5e8,2.6,.125,.02,.1)),cents


def trajectory(system,air,pedal,duration,rate,save_progress=None):
    contact=SharedContact(system,air);coast=SharedCoast(system,air)
    dampers=SharedDampers(system.string,system.nh)
    state=contact.initial_state(0.)
    # Three pitches, overlap under pedal, then a restrike into surviving motion.
    schedule=[(0.,'on',60,84),(.14,'off',60,0),(.25,'on',67,84),
        (.39,'off',67,0),(.50,'on',72,84),(.64,'off',72,0),
        (.75,'on',60,84),(.89,'off',60,0),(1.05,'pedal_off',0,0)]
    cursor=0;controller=DamperControl(system.keys,pedal)
    def board(q):
        return system.string.board_transform@q[system.string.sb]
    q,v=state[:2];times=[0.];B=[board(q)];V=[board(v)]
    tick=0;fine=768000;coarse=fine//rate
    heat_felt=heat_damper=heat_material=0.;work=0.;maximum_force=0.
    launches=[];sympathetic=0.;iterations=0
    begin=last=time.perf_counter()
    while tick<round(duration*fine):
        now=tick/fine
        while cursor<len(schedule) and round(schedule[cursor][0]*fine)<=tick:
            _,kind,key,velocity=schedule[cursor];cursor+=1
            controller.event(kind,key,now)
            if kind=='on':
                before_q=state[0][:system.string.size].copy()
                before_v=state[1][:system.string.size].copy()
                state,external=system.launch(state,key,5.*(velocity/124)**1.5)
                if not np.array_equal(before_q,state[0][:system.string.size]) or not np.array_equal(before_v,state[1][:system.string.size]):
                    raise AssertionError('launch reset a live wire or board')
                work+=external;launches.append(dict(time_s=now,key=key,external_work_j=external))
        in_contact=system.compression(state[0])>=-1e-12
        # Finish each fine contact interval on the same coarse time grid.
        dt=1/fine if in_contact or tick%coarse else 1/rate
        step=1 if dt==1/fine else coarse
        levels=controller.levels(now+dt/2)
        vel,heat0=dampers.dissipate(state[1],dt/2,levels)
        if in_contact or step==1:
            middle,stats=contact.step((state[0],vel,state[2],state[3]),dt,True)
        else:
            try:
                middle,stats=coast.step((state[0],vel,state[2],state[3]),dt,True)
            except ValueError as error:
                if 'recontact' not in str(error):raise
                # No state advance was accepted; retry at the contact rate.
                dt=1/fine;step=1
                levels=controller.levels(now+dt/2)
                vel,heat0=dampers.dissipate(state[1],dt/2,levels)
                middle,stats=contact.step((state[0],vel,state[2],state[3]),dt,True)
        vel,heat1=dampers.dissipate(middle[1],dt/2,levels)
        state=(middle[0],vel,middle[2],middle[3]);tick+=step
        heat_damper+=heat0+heat1;heat_felt+=stats['felt_loss_j'];heat_material+=stats['material_loss_j']
        maximum_force=max(maximum_force,stats['force_n']);iterations=max(iterations,stats.get('mechanical_iterations',0))
        if tick%6==0:
            times.append(tick/fine);B.append(board(state[0]));V.append(board(state[1]))
        if now<.24 and tick%coarse==0:
            z=system.string.physical(state[0][:system.string.size])
            # Nonzero intrinsic motion in G4 before its first hammer strike.
            sympathetic=max(sympathetic,max(float(np.linalg.norm(z[e['sv']])/np.sqrt(e['port'].wire.modal_mass)) for e in system.string.entries if e['port'].key==67))
        if time.perf_counter()-last>25:
            progress=dict(physical_time_s=tick/fine,duration_s=duration,elapsed_s=time.perf_counter()-begin,pedal=pedal)
            print('shared phrase',json.dumps(progress),flush=True)
            if save_progress:save_progress(progress)
            last=time.perf_counter()
    if times[-1]!=tick/fine:
        times.append(tick/fine);B.append(board(state[0]));V.append(board(state[1]))
    times=np.asarray(times);B=np.asarray(B);V=np.asarray(V)
    spline=CubicHermiteSpline(times,B,V,axis=0)
    uniform=np.arange(round(duration*128000)+1)/128000
    return uniform,spline(uniform),spline(uniform,1),dict(seconds=time.perf_counter()-begin,
        felt_heat_j=heat_felt,damper_heat_j=heat_damper,material_heat_j=heat_material,
        external_head_work_j=work,launches=launches,maximum_felt_force_n=maximum_force,
        sympathetic_G4_before_strike_m=sympathetic,max_coast_iterations=iterations,
        final_mechanical_j=system.energy(*state[:2]),
        radiation_ledger_deferred=True,complete_energy_certificate=False)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report',type=Path,required=True)
    p.add_argument('--audio-dir',type=Path,required=True)
    p.add_argument('--transverse',type=int,default=96)
    p.add_argument('--choir',type=int,choices=[1,3],default=3)
    p.add_argument('--duration',type=float,default=2.)
    p.add_argument('--rate',type=int,choices=[16000,32000,64000,128000],default=16000)
    controls=p.add_mutually_exclusive_group()
    controls.add_argument('--control',action='store_true')
    controls.add_argument('--control-only',action='store_true')
    args=p.parse_args()
    if args.report.exists() or args.duration<1.1:
        raise ValueError('new report and a complete phrase/release required')
    begin=time.perf_counter()
    report=dict(stage='shared physical phrase construction',all_gates_passed=False,
        sampled_audio_used=False,single_shared_board=True,single_shared_air=True,
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in [
            Path(__file__).name,'concert_grand_shared_strings.py','concert_grand_audition_air.py',
            'concert_grand_damper_control.py']},
        scope='Assumed three-note register, three-wire choirs; existing Euler/extensible string and vertical bridge. '
              'Not full keyboard, in-plane plate coupling, Timoshenko, identified felt/materials, room or plugin.',
        keys=[60,67,72],wires_per_key=args.choir,transverse=args.transverse,axial=2*args.transverse,
        coast_rate_hz=args.rate,contact_rate_hz=768000,capture_rate_hz=128000,
        complete_polyphonic_temporal_convergence=False,audio_files=[],cases=[])
    def save():
        report['seconds']=time.perf_counter()-begin
        args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    save();modal,geometry,air,kernel,paths=prepare(report,save)
    system,cents=make_system(modal,geometry,args.transverse,args.choir)
    report['wire_tuning_offsets_cents']=cents
    report['mechanical_coordinates']=system.size
    args.audio_dir.mkdir(parents=True,exist_ok=True)
    for pedal in ([False] if args.control_only else [True,False] if args.control else [True]):
        report['stage']='live shared phrase, pedal '+str(pedal);save()
        def progress(value):report['progress']=value;save()
        t,B,V,metrics=trajectory(system,air,pedal,args.duration,args.rate,progress)
        pressure,_,_,_=pressure_fft(t,B,V,kernel)
        pressure=pressure[:len(t)+len(kernel)-1]
        audio=resample_poly(pressure*.25,3,8,axis=0).astype(np.float32)
        if not np.isfinite(audio).all() or np.max(np.abs(audio))>=.98:
            raise ValueError('fixed physical gain clips or nonfinite waveform')
        metrics.update(pedal=pedal,pressure_peak_pa=float(np.max(abs(pressure))),audio_peak=float(np.max(abs(audio))),
            held_rms=float(np.sqrt(np.mean(audio[24000:48000]**2))),
            final_rms=float(np.sqrt(np.mean(audio[-4800:]**2))))
        name='Frase-fisica-pedal.wav' if pedal else 'Frase-fisica-sin-pedal.wav'
        destination=args.audio_dir/name
        if destination.exists():raise ValueError('preserve previous physical output')
        wavfile.write(destination,48000,np.vstack([np.zeros((4800,2),np.float32),audio]))
        report['cases'].append(metrics)
        report['audio_files'].append(dict(path=str(destination.resolve()),sha256=digest(destination),seconds=.1+len(audio)/48000))
        save();print('PHYSICAL PHRASE WROTE',destination,metrics,flush=True)
        del t,B,V,pressure,audio
    report['source_and_cache_unchanged']=bool(digest(paths.board)==report['board_sha256'] and digest(paths.cache/'R.npy')==report['cache_R_sha256'] and digest(paths.cache/'manifest.json')==report['cache_manifest_sha256'])
    report['stage']='live shared physical phrase completed';save()


if __name__=='__main__':main()
