"""First offline sustained physical C4 pilot, reusing the validated onset.

The factory engine is unchanged. Canonical reports belong in docs; unique
raw trajectories and requested listening files belong in SHARED.
"""
import argparse, hashlib, json, platform, time
from importlib.util import spec_from_file_location, module_from_spec
from pathlib import Path
import numpy as np
import scipy
from concert_grand_air_coefficients import load_prefix
from concert_grand_bounded_air import BoundedAir
from concert_grand_bounded_contact import BoundedContact
from concert_grand_fast_contact import FastBoundedContact

spec=spec_from_file_location('physical_note_contact',Path(__file__).with_name('audit-concert-grand-bounded-contact.py'))
contact=module_from_spec(spec);spec.loader.exec_module(contact)

CRITERIA=dict(live_state_relative=2e-8,live_work_relative=2e-8,
              onset_board_relative=1e-8,onset_state_relative=1e-8,
              onset_energy_relative=1e-9)


def digest(path):
    with Path(path).open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def write(path,data):
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(json.dumps(data,indent=2,allow_nan=False),encoding='utf-8')
def relative(actual,reference):
    return float(np.linalg.norm(actual-reference)/max(np.linalg.norm(reference),1e-30))


def initialize(args):
    report=json.loads(args.contact_report.read_text())
    p=report['provenance']
    if not all(report['gates'].values()):raise ValueError('validated onset required')
    if digest(args.asset)!=p['asset_sha256'] or digest(args.coefficients)!=p['coefficient_sha256']:
        raise ValueError('frozen structural/coefficient inputs changed')
    for name,sha in p['input_hashes'].items():
        if digest(args.input/name)!=sha:raise ValueError('frozen SI inputs changed')
    key=f"T{p['transverse']}-{args.rate}-Cold"
    trajectory=args.contact_data/(key+'.npz')
    if digest(trajectory)!=report['trajectory_sha256'][key]:raise ValueError('frozen onset changed')
    support, _ = contact.bridge.p2_support(args.asset,60,p['support']['cutoff_hz'])
    system,speed=contact.bridge.load(args.input,60,124,support,p['transverse'])
    if vars(system.felt)!=p['felt'] or vars(system.string.wire)!=p['wire'] or speed!=p['incident_speed_m_s']:
        raise ValueError('physical contact inputs changed')
    factor,rate,mass,limit,parent=load_prefix(args.coefficients,support.frequency)
    if rate!=p['acoustic_discretization']['factor_rate_hz']:raise ValueError('factor rate mismatch')
    air=BoundedAir(factor,rate,mass,limit);del factor
    original=BoundedContact(system,air);fast=FastBoundedContact(system,air)
    with np.load(trajectory) as d:
        state=tuple(d[k] for k in ['q','v','wave_state_final','relaxation_state_final'])
        trace=d['trace'];B=d['board_position'];V=d['board_velocity']
    if system.compression(state[0])>=0 or state[1][-1]>=0:raise ValueError('returning separated hammer required')
    return report,original,fast,state,trace,B,V


def profile(args,report,original,fast,state):
    result=dict(criteria=CRITERIA,steps=args.probe_steps,rate_hz=args.rate,
                onset_report_sha256=digest(args.contact_report),checks={},gates={})
    traces=[];work=[];elapsed=[]
    for solver in [original,fast]:
        local=tuple(s.copy() for s in state);w=[];states=[]
        start=time.monotonic()
        for _ in range(args.probe_steps):
            local,stats=solver.step(local,1/args.rate,defer_radiation=True)
            w.append([stats[k] for k in ['felt_loss_j','material_loss_j','air_port_work_j']])
            states.append(tuple(s.copy() for s in local))
        elapsed.append(time.monotonic()-start);traces.append(states);work.append(np.array(w))
    result['checks']['live_states_max_relative']=max(relative(a,b) for aa,bb in zip(traces[1],traces[0]) for a,b in zip(aa,bb))
    result['checks']['live_work_relative']=relative(work[1],work[0])
    result['gates']['live_states']=result['checks']['live_states_max_relative']<CRITERIA['live_state_relative']
    result['gates']['live_work']=result['checks']['live_work_relative']<CRITERIA['live_work_relative']
    result['reference_seconds'],result['accelerated_seconds']=elapsed
    result['dynamics_speedup']=elapsed[0]/elapsed[1]
    result['estimated_dynamics_seconds_for_one_second']=elapsed[1]*args.rate/args.probe_steps
    result['scope']='Live 4ms-onset continuation; dynamics only, excluding angular radiation output, construction and microphone rendering.'
    result['environment']=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__,blas_threads=1)
    result['source_sha256']={name:digest(Path(__file__).with_name(name)) for name in ['render-concert-grand-physical-note.py','concert_grand_fast_contact.py']}
    result['all_gates_passed']=all(result['gates'].values())
    write(args.report,result);print(json.dumps(result),flush=True)
    if not result['all_gates_passed']:raise AssertionError('fast physical dynamics changed')


def onset_check(args,report,fast,frozen,B,V):
    state=fast.initial_state(report['provenance']['incident_speed_m_s'])
    initial=fast.energy(state)['total_j'];loss=0.;pending=[];actual_B=[];actual_V=[];max_fallback=0
    capture=args.rate//128000
    start=time.monotonic()
    for i in range(round(.004*args.rate)+1):
        if i%capture==0:
            actual_B.append(fast.board_map@state[0]);actual_V.append(fast.board_map@state[1])
        if i==round(.004*args.rate):break
        state,stats=fast.step(state,1/args.rate,True)
        loss+=stats['loss_j'];pending.append(stats['wave_inputs'])
        max_fallback=max(max_fallback,stats['full_tangent_fallbacks'])
        if len(pending)==64:
            loss+=float(fast.air.wave.radiation_batch(np.array(pending)).sum()/args.rate);pending.clear()
    if pending:loss+=float(fast.air.wave.radiation_batch(np.array(pending)).sum()/args.rate)
    result=dict(checks=dict(board_position_relative=relative(np.array(actual_B),B),
            board_velocity_relative=relative(np.array(actual_V),V),
            final_state_max_relative=max(relative(a,b) for a,b in zip(state,frozen)),
            independent_total_energy_relative=abs(fast.energy(state)['total_j']+loss-initial)/initial),
            seconds=time.monotonic()-start,maximum_full_tangent_fallbacks=max_fallback,
            scope='Full 4ms cold onset with independently integrated full angular radiation and unchanged nonlinear equations.')
    result['gates']=dict(board=max(result['checks'][k] for k in ['board_position_relative','board_velocity_relative'])<CRITERIA['onset_board_relative'],
            state=result['checks']['final_state_max_relative']<CRITERIA['onset_state_relative'],
            energy=result['checks']['independent_total_energy_relative']<CRITERIA['onset_energy_relative'])
    result['all_gates_passed']=all(result['gates'].values());write(args.report,result)
    print(json.dumps(result),flush=True)
    if not result['all_gates_passed']:raise AssertionError('onset acceleration failed')


def coast(args,report,fast,state,trace,B,V):
    if not args.output:raise ValueError('unique raw output required')
    if args.output.exists() or args.report.exists():raise ValueError('preserve existing measurements')
    steps=round((args.duration-.004)*args.rate);capture=args.rate//128000
    if steps<=0 or steps%capture:raise ValueError('duration on 128k capture grid after 4ms')
    local=tuple(s.copy() for s in state);B=list(B);V=list(V)
    initial=fast.energy(local)['total_j'];material=felt=port=relax_loss=0.;max_fallback=0;max_force=0.
    checkpoints=[];start=time.monotonic();last_print=start
    for i in range(steps):
        local,stats=fast.step(local,1/args.rate,True)
        material+=stats['material_loss_j'];felt+=stats['felt_loss_j'];port+=stats['air_port_work_j'];relax_loss+=stats['radiation_loss_j']
        max_fallback=max(max_fallback,stats['full_tangent_fallbacks']);max_force=max(max_force,stats['force_n'])
        if (i+1)%capture==0:
            B.append(fast.board_map@local[0]);V.append(fast.board_map@local[1])
        # Independent positive storage is costly; audit a few checkpoints.
        if (i+1)%max(capture,round(args.rate*.1))==0 or i+1==steps:
            E=fast.energy(local)
            # This is a storage-based loss residual, explicitly not an
            # independent integral of angular wave power over the sustain.
            checkpoints.append(dict(time_s=.004+(i+1)/args.rate,stored_j=E['total_j'],
                material_heat_j=material,felt_heat_j=felt,air_work_j=port,
                radiation_inferred_from_storage_j=initial-E['total_j']-material-felt))
        if time.monotonic()-last_print>=15 or i+1==steps:
            elapsed=time.monotonic()-start
            print(f'coast {(i+1)/steps:.1%}, t={.004+(i+1)/args.rate:.6f}s, wall={elapsed:.1f}s',flush=True);last_print=time.monotonic()
    args.output.parent.mkdir(parents=True,exist_ok=True)
    np.savez_compressed(args.output,time_s=np.arange(len(B))/128000,board_position=np.array(B),board_velocity=np.array(V),
        q=local[0],v=local[1],wave_state_final=local[2],relaxation_state_final=local[3])
    result=dict(rate_hz=args.rate,capture_hz=128000,duration_s=args.duration,seconds=time.monotonic()-start,
        raw_sha256=digest(args.output),onset_report_sha256=digest(args.contact_report),provenance=report['provenance'],
        maximum_full_tangent_fallbacks=max_fallback,maximum_post_onset_contact_force_n=max_force,
        checkpoints=checkpoints,final_relaxation_heat_j=relax_loss,
        scope='Full nonlinear continuation from frozen cold onset; string/board/wave/relaxation and returning hammer preserved. Angular radiation integral NOT independently evaluated during long sustain.',
        factory_changed=False,
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in ['render-concert-grand-physical-note.py','concert_grand_fast_contact.py']})
    write(args.report,result);print('saved',str(args.output),flush=True)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['input','asset','coefficients','contact-report','contact-data','report']:
        parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--phase',choices=['profile','onset-check','coast'],default='profile')
    parser.add_argument('--rate',type=int,choices=[384000,768000],default=768000)
    parser.add_argument('--probe-steps',type=int,default=64)
    parser.add_argument('--duration',type=float,default=.5)
    parser.add_argument('--output',type=Path)
    args=parser.parse_args()
    if args.probe_steps<=0 or not np.isfinite(args.duration):parser.error('finite duration and positive probe steps')
    report,original,fast,state,trace,B,V=initialize(args)
    if args.phase=='profile':profile(args,report,original,fast,state)
    elif args.phase=='onset-check':onset_check(args,report,fast,state,B,V)
    else:coast(args,report,fast,state,trace,B,V)


if __name__=='__main__':main()
