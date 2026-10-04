"""Frozen C4 nonlinear contact with P2 + its full passive acoustic bath.

Offline SI reference: no fitting, audio gain, factory mutation or reduction.
Criteria are declared before runs; failed resolution checks remain recorded.
"""
import argparse,csv,hashlib,json,time,platform,sys
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
import scipy
from concert_grand_acoustic_bath import AcousticBath
from concert_grand_bath_contact import BathContact

spec=spec_from_file_location('moving_bridge_audit',Path(__file__).with_name('audit-concert-grand-moving-bridge.py'))
bridge=module_from_spec(spec);spec.loader.exec_module(bridge)

CRITERIA=dict(energy_budget=1e-9,energy_work_change=.002,contact_time_change=.001,
              bridge_displacement_velocity_rms_change=.01,impulse_force_change=.002)
COLUMNS=['time_s','bridge_displacement_m','bridge_velocity_m_s','compression_m','force_n',
         'string_j','board_j','hammer_j','felt_potential_j','air_j','felt_heat_j','material_heat_j',
         'numerical_absorption_j','air_port_work_j','work_into_board_j','budget_relative_error']


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def solve(coupled,state,rate,duration):
    core=coupled.system.string;system=coupled.system;start=time.monotonic()
    initial=coupled.energy(state);initial_parts=core.components(state[0][:-1],state[1][:-1])
    heat_f=heat_m=absorption=air_work=board_work=0.;trace=[];B=[];BV=[]
    steps=round(rate*duration);capture=rate//128000
    if rate%128000 or steps%capture:raise ValueError('common 128 kHz capture grid')
    separation=None;force_max=impulse=0.;maximum_energy=maximum_air=maximum_board=0.;negative_air_steps=0
    def record(t,force):
        nonlocal maximum_energy,maximum_air,maximum_board
        parts=core.components(state[0][:-1],state[1][:-1]);energy=coupled.energy(state)
        total_error=(energy['total_j']+heat_f+heat_m+absorption-initial['total_j'])/initial['total_j']
        air_error=(energy['air_j']-initial['air_j']+absorption-air_work)/initial['total_j']
        board_error=(parts['support_energy_j']-initial_parts['support_energy_j']+heat_m+air_work-board_work)/initial['total_j']
        maximum_energy=max(maximum_energy,abs(total_error));maximum_air=max(maximum_air,abs(air_error));maximum_board=max(maximum_board,abs(board_error))
        felt=system.felt;delta=max(system.compression(state[0]),0.)
        felt_potential=felt.stiffness*delta**(felt.exponent+1)/(felt.exponent+1)
        trace.append([t,parts['bridge_displacement_m'],parts['bridge_velocity_m_s'],delta,force,
            parts['string_energy_j'],parts['support_energy_j'],.5*state[1][-1]**2,felt_potential,energy['air_j'],
            heat_f,heat_m,absorption,air_work,board_work,total_error])
        B.append(coupled.board_map@state[0]);BV.append(coupled.board_map@state[1])
    record(0.,0.)
    for i in range(steps):
        old_delta=system.compression(state[0]);state,stats=coupled.step(state,1/rate)
        delta=system.compression(state[0]);force=stats['force_n'];force_max=max(force_max,force);impulse+=force/rate
        if separation is None and old_delta>0 and delta<=0 and state[1][-1]<0:
            separation=(i+old_delta/(old_delta-delta))/rate
        heat_f+=stats['felt_loss_j'];heat_m+=stats['material_loss_j'];absorption+=stats['numerical_bath_absorption_j']
        air_work+=stats['air_port_work_j'];board_work+=stats['work_into_board_j']
        negative_air_steps+=stats['air_port_work_j']<0
        if min(stats['felt_loss_j'],stats['material_loss_j'],stats['numerical_bath_absorption_j'])<-1e-16:
            raise AssertionError('negative dissipative work')
        if (i+1)%capture==0:record((i+1)/rate,force)
        if (i+1)%max(1,steps//4)==0:print(f'  {i+1}/{steps} steps, {time.monotonic()-start:.1f}s',flush=True)
    if separation is None:raise AssertionError('no returning-hammer separation')
    final=coupled.energy(state);parts=core.components(state[0][:-1],state[1][:-1]);interface=coupled.interface(state)
    metrics=dict(rate_hz=rate,duration_s=duration,initial_energy_j=initial['total_j'],
        initial_air_energy_j=initial['air_j'],initial_board_energy_j=initial_parts['support_energy_j'],
        final_string_energy_j=parts['string_energy_j'],final_board_energy_j=parts['support_energy_j'],
        final_hammer_energy_j=float(.5*state[1][-1]**2),final_felt_potential_j=trace[-1][8],
        final_air_energy_j=final['air_j'],felt_heat_j=heat_f,material_heat_j=heat_m,numerical_absorption_j=absorption,
        air_port_work_j=air_work,work_into_board_j=board_work,contact_time_s=separation,
        maximum_force_n=force_max,impulse_n_s=impulse,maximum_total_budget_relative_error=maximum_energy,
        maximum_air_budget_relative_error=maximum_air,maximum_board_budget_relative_error=maximum_board,
        final_interface_force_residual=interface['force_residual'],final_interface_power_residual_w=interface['power_residual_w'],
        negative_air_work_steps=int(negative_air_steps),elapsed_s=time.monotonic()-start)
    assert max(maximum_energy,maximum_air,maximum_board)<CRITERIA['energy_budget'],metrics
    return state,metrics,np.asarray(trace),np.asarray(B),np.asarray(BV)


def save(output,key,state,metrics,trace,B,BV):
    np.savez(output/f'{key}.npz',q=state[0],v=state[1],air_position=state[2],air_velocity=state[3],
             trace=trace,board_position=B,board_velocity=BV)
    with (output/f'{key}.csv').open('w',newline='') as file:
        writer=csv.writer(file);writer.writerow(COLUMNS);writer.writerows(trace)
    (output/f'{key}.json').write_text(json.dumps(metrics,indent=2),encoding='utf-8')


def compare(output,left,right):
    a=json.loads((output/f'{left}.json').read_text());b=json.loads((output/f'{right}.json').read_text())
    report=dict(left=left,right=right,changes={},passed=True)
    def add(key,value,limit):
        report['changes'][key]=dict(relative_change=float(value),limit=limit,passed=bool(value<limit))
        report['passed']=bool(report['passed'] and value<limit)
    for key in ['final_string_energy_j','final_board_energy_j','final_hammer_energy_j','final_air_energy_j',
                'material_heat_j','numerical_absorption_j','air_port_work_j','work_into_board_j']:
        add(key,abs(a[key]-b[key])/max(abs(b[key]),1e-14),CRITERIA['energy_work_change'])
    add('contact_time_s',abs(a['contact_time_s']-b['contact_time_s'])/b['contact_time_s'],CRITERIA['contact_time_change'])
    for key in ['maximum_force_n','impulse_n_s']:
        add(key,abs(a[key]-b[key])/b[key],CRITERIA['impulse_force_change'])
    with np.load(output/f'{left}.npz') as aa,np.load(output/f'{right}.npz') as bb:
        for key in ['board_position','board_velocity']:
            add(key,np.linalg.norm(aa[key]-bb[key])/max(np.linalg.norm(bb[key]),1e-30),CRITERIA['bridge_displacement_velocity_rms_change'])
        for name,index in [('bridge_displacement_m',1),('bridge_velocity_m_s',2)]:
            add(name,np.linalg.norm(aa['trace'][:,index]-bb['trace'][:,index])/max(np.linalg.norm(bb['trace'][:,index]),1e-30),CRITERIA['bridge_displacement_velocity_rms_change'])
    return report


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--input',type=Path,required=True)
    parser.add_argument('--asset',type=Path,required=True);parser.add_argument('--bath',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True);parser.add_argument('--phase',choices=['run','summary'],default='run')
    parser.add_argument('--transverse',type=int,default=48);parser.add_argument('--rates',type=int,nargs='+',default=[384000,768000])
    parser.add_argument('--relaunch',action='store_true',help='Second incident head at the finest requested rate')
    parser.add_argument('--dry',action='store_true')
    parser.add_argument('--dry-reference',type=Path)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    if args.phase=='summary':
        cases={p.stem:json.loads(p.read_text()) for p in args.output.glob('T*-*.json')}
        comparisons=[]
        for nt in sorted(set(m['transverse'] for k,m in cases.items() if k.endswith('-Air'))):
            keys=sorted((k for k,m in cases.items() if m['transverse']==nt and k.endswith('-Air')),key=lambda k:cases[k]['rate_hz'])
            comparisons.extend(compare(args.output,a,b) for a,b in zip(keys,keys[1:]))
        for rate in sorted(set(m['rate_hz'] for k,m in cases.items() if k.endswith('-Air'))):
            keys=sorted((k for k,m in cases.items() if m['rate_hz']==rate and k.endswith('-Air')),key=lambda k:cases[k]['transverse'])
            comparisons.extend(compare(args.output,a,b) for a,b in zip(keys,keys[1:]))
        report=dict(scope='C4 velocity 124 vertical one-string SI contact; frozen P2 low band and full bath',criteria=CRITERIA,cases=cases,comparisons=comparisons)
        if args.dry_reference is not None:
            key=max((k for k in cases if k.endswith('-Dry')),key=lambda k:cases[k]['rate_hz'])
            with np.load(args.output/f'{key}.npz') as a,np.load(args.dry_reference) as b:
                error_q=float(np.linalg.norm(a['q'][:-1]-b['Q'][:-1])/np.linalg.norm(b['Q'][:-1]))
                error_v=float(np.linalg.norm(a['v'][:-1]-b['P'][:-1])/np.linalg.norm(b['P'][:-1]))
            report['dry_independent_DOP853']=dict(case=key,reference=str(args.dry_reference),sha256=digest(args.dry_reference),
                body_q_relative_error=error_q,body_v_relative_error=error_v,limit=.001,passed=max(error_q,error_v)<.001)
            air=cases[key.replace('-Dry','-Air')];dry=cases[key]
            report['loading_effect']={name:dict(dry=dry[name],air=air[name],relative_change=(air[name]-dry[name])/abs(dry[name]))
                for name in ['contact_time_s','maximum_force_n','impulse_n_s','final_string_energy_j','final_hammer_energy_j',
                             'final_board_energy_j','material_heat_j','work_into_board_j']}
        report['environment']=dict(python=sys.version,platform=platform.platform(),numpy=np.__version__,scipy=scipy.__version__,
            blas_threads_per_process=1,elapsed_note='Diagnostic wall time; dry and part of T72 overlapped, not a performance benchmark')
        report['source_hashes']={p.name:digest(p) for p in [Path(__file__),Path(__file__).with_name('concert_grand_bath_contact.py'),
            Path(__file__).parent/'tests/test_concert_bath_contact.py']}
        (args.output/'summary.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        print(json.dumps(comparisons,indent=2));return
    support,support_metadata=bridge.p2_support(args.asset,60)
    system,speed=bridge.load(args.input,60,124,support,args.transverse)
    with np.load(args.bath) as data:bath=AcousticBath(data['omega_rad_s'],data['gamma_rad_s'],data['projection'],data['tail_mass'])
    coupled=BathContact(system,bath,0. if args.dry else 1.)
    provenance=dict(asset_sha256=digest(args.asset),bath_sha256=digest(args.bath),support=support_metadata,
        input_hashes={p.name:digest(p) for p in args.input.glob('*.csv')},incident_speed_m_s=speed,
        transverse=args.transverse,axial=2*args.transverse,acoustic_ports=bath.ports,criteria=CRITERIA,
        felt=vars(system.felt),wire=vars(system.string.wire),scope='One baffled face, 35 P2 modes <=800 Hz; no factory promotion',
        relaunch_scope='Prescribed incident head after separation; not action/backcheck model',coupling=coupled.coupling)
    tag='Dry' if args.dry else 'Air'
    (args.output/f'provenance-T{args.transverse}-{tag}.json').write_text(json.dumps(provenance,indent=2),encoding='utf-8')
    for rate in args.rates:
        key=f'T{args.transverse}-{rate}-'+('Dry' if args.dry else 'Air');print(key,flush=True)
        state,metrics,trace,B,BV=solve(coupled,coupled.initial_state(speed),rate,.004)
        metrics.update(transverse=args.transverse,axial=2*args.transverse)
        save(args.output,key,state,metrics,trace,B,BV);print(json.dumps(metrics),flush=True)
        if args.relaunch and rate==max(args.rates):
            before=tuple(x.copy() for x in state);state,external=coupled.relaunch(state,speed)
            preserved=all(np.array_equal(x,y) for x,y in [(before[0][:-1],state[0][:-1]),(before[1][:-1],state[1][:-1]),(before[2],state[2]),(before[3],state[3])])
            assert preserved
            state,metrics,trace,B,BV=solve(coupled,state,rate,.004)
            metrics.update(transverse=args.transverse,axial=2*args.transverse,relaunch_external_work_j=external,
                           preserved_living_states=preserved,first_case=key)
            save(args.output,key+'-Relaunch',state,metrics,trace,B,BV);print(json.dumps(metrics),flush=True)


if __name__=='__main__':main()
