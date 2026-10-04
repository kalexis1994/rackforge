"""Frozen C4 SI contacts with compact finite-limit air and persistent states."""
import argparse,hashlib,json,platform,time
from importlib.util import spec_from_file_location,module_from_spec
from pathlib import Path
import numpy as np
import scipy
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_bounded_air import BoundedAir
from concert_grand_bounded_contact import BoundedContact

spec=spec_from_file_location('bounded_bridge_audit',Path(__file__).with_name('audit-concert-grand-moving-bridge.py'))
bridge=module_from_spec(spec);spec.loader.exec_module(bridge)
CRITERIA=dict(energy_budget=1e-9,energy_work_change=.002,contact_time_change=.001,
    bridge_displacement_velocity_rms_change=.01,impulse_force_change=.002)
COLUMNS=['time_s','bridge_displacement_m','bridge_velocity_m_s','compression_m','force_n','string_j','board_j','hammer_j','felt_potential_j',
    'felt_heat_j','material_heat_j','radiation_j','air_port_work_j','work_into_board_j']


def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda:f.read(1024*1024),b''):h.update(block)
    return h.hexdigest()
def write(path,data):path.write_text(json.dumps(data,indent=2,allow_nan=False),encoding='utf-8')


def solve(coupled,state,rate,duration=.004,batch_size=64):
    core=coupled.system.string;system=coupled.system;start=time.monotonic()
    steps=round(rate*duration);capture=rate//128000
    if rate%128000 or steps%capture or rate%2000:raise ValueError('128k capture and 0.5ms budget grid')
    initial=coupled.energy(state);parts0=core.components(state[0][:-1],state[1][:-1])
    felt=material=radiation=air_work=board_work=0.;trace=[];B=[];BV=[];budgets=[]
    separation=None;force_max=impulse=0.;negative=0;max_newton=0
    if batch_size<0:raise ValueError('nonnegative observer batch size')
    pending=[];indices=[];radiation_steps=np.zeros(steps)
    def flush():
        nonlocal radiation
        if not pending:return
        losses=coupled.air.wave.radiation_batch(np.array(pending))/rate
        radiation_steps[indices]+=losses;radiation+=float(losses.sum())
        pending.clear();indices.clear()
    def record(t,force,budget):
        parts=core.components(state[0][:-1],state[1][:-1]);delta=max(system.compression(state[0]),0.)
        potential=system.felt.stiffness*delta**(system.felt.exponent+1)/(system.felt.exponent+1)
        trace.append([t,parts['bridge_displacement_m'],parts['bridge_velocity_m_s'],delta,force,
            parts['string_energy_j'],parts['support_energy_j'],.5*state[1][-1]**2,potential,felt,material,radiation,air_work,board_work])
        B.append(coupled.board_map@state[0]);BV.append(coupled.board_map@state[1])
        if budget:
            energy=coupled.energy(state)
            budgets.append(dict(time_s=t,stored_total_j=energy['total_j'],stored_air_j=energy['air_j'],
                total_relative=abs(energy['total_j']+felt+material+radiation-initial['total_j'])/initial['total_j'],
                air_relative=abs(energy['air_j']-initial['air_j']+radiation-air_work)/initial['total_j'],
                board_relative=abs(parts['support_energy_j']-parts0['support_energy_j']+material+air_work-board_work)/initial['total_j']))
    record(0.,0.,True)
    for i in range(steps):
        before=system.compression(state[0]);state,stats=coupled.step(state,1/rate,defer_radiation=bool(batch_size))
        after=system.compression(state[0]);force=stats['force_n'];force_max=max(force_max,force);impulse+=force/rate
        if separation is None and before>0 and after<=0 and state[1][-1]<0:separation=(i+before/(before-after))/rate
        felt+=stats['felt_loss_j'];material+=stats['material_loss_j'];radiation+=stats['radiation_loss_j']
        radiation_steps[i]=stats['radiation_loss_j']
        if batch_size:
            pending.append(stats['wave_inputs']);indices.append(i)
            if len(pending)>=batch_size or (i+1)%(rate//2000)==0 or i+1==steps:flush()
        air_work+=stats['air_port_work_j'];board_work+=stats['work_into_board_j'];negative+=stats['air_port_work_j']<0
        max_newton=max(max_newton,stats['newton_iterations'])
        if min(stats['felt_loss_j'],stats['material_loss_j'],stats['radiation_loss_j'])<-1e-16:raise AssertionError('negative dissipative work')
        if (i+1)%capture==0:record((i+1)/rate,force,(i+1)%(rate//2000)==0 or i+1==steps)
        if (i+1)%max(1,steps//4)==0:print(f'  {i+1}/{steps} steps, {time.monotonic()-start:.1f}s',flush=True)
    if separation is None:raise AssertionError('no returning-hammer separation')
    trace=np.array(trace)
    # Capture can lie inside a batch. Fill its radiation ledger from every
    # completed step rather than flushing the observer at each capture.
    trace[:,11]=np.r_[0.,np.cumsum(radiation_steps)][::capture]
    # Final budget checkpoint already contains the independent storage value.
    final=budgets[-1];parts=core.components(state[0][:-1],state[1][:-1]);interface=coupled.interface(state)
    metrics=dict(rate_hz=rate,duration_s=duration,initial_energy_j=initial['total_j'],initial_air_energy_j=initial['air_j'],
        final_string_energy_j=parts['string_energy_j'],final_board_energy_j=parts['support_energy_j'],final_hammer_energy_j=float(.5*state[1][-1]**2),
        final_air_energy_j=final['stored_air_j'],felt_heat_j=felt,material_heat_j=material,radiation_loss_j=radiation,air_port_work_j=air_work,
        work_into_board_j=board_work,contact_time_s=separation,maximum_force_n=force_max,impulse_n_s=impulse,
        negative_air_work_steps=int(negative),max_newton_iterations=max_newton,energy_checkpoints=budgets,
        maximum_total_budget_relative_error=max(b['total_relative'] for b in budgets),
        maximum_air_budget_relative_error=max(b['air_relative'] for b in budgets),maximum_board_budget_relative_error=max(b['board_relative'] for b in budgets),
        final_interface_force_residual=interface['force_residual'],final_interface_power_residual_w=interface['power_residual_w'],elapsed_s=time.monotonic()-start,
        observer_batch_size=batch_size,
        budget_scope='Independent storage evaluated each 0.5ms. Loss and port work accumulated every step. Small-fixture tests check every-step identity.')
    metrics['energy_passed']=max(metrics[k] for k in ['maximum_total_budget_relative_error','maximum_air_budget_relative_error','maximum_board_budget_relative_error'])<CRITERIA['energy_budget']
    return state,metrics,trace,np.array(B),np.array(BV)


def save(output,reports,key,state,metrics,trace,B,BV):
    np.savez_compressed(output/(key+'.npz'),q=state[0],v=state[1],wave_state_final=state[2],relaxation_state_final=state[3],trace=trace,
        board_position=B,board_velocity=BV,trace_columns=np.array(COLUMNS))
    # The NPZ is the single stored trajectory; no duplicate CSV or SHARED
    # report. Measurements and summaries live in the repository.
    write(reports/(key+'.json'),metrics)


def compare(output,reports,left,right):
    a=json.loads((reports/(left+'.json')).read_text());b=json.loads((reports/(right+'.json')).read_text())
    result=dict(left=left,right=right,changes={},passed=True)
    def add(key,error,limit):
        result['changes'][key]=dict(relative_change=float(error),limit=limit,passed=bool(error<limit));result['passed']&=bool(error<limit)
    for key in ['final_string_energy_j','final_board_energy_j','final_hammer_energy_j','final_air_energy_j','material_heat_j','radiation_loss_j','air_port_work_j','work_into_board_j']:
        add(key,abs(a[key]-b[key])/max(abs(b[key]),1e-14),CRITERIA['energy_work_change'])
    add('contact_time_s',abs(a['contact_time_s']-b['contact_time_s'])/b['contact_time_s'],CRITERIA['contact_time_change'])
    for key in ['maximum_force_n','impulse_n_s']:add(key,abs(a[key]-b[key])/b[key],CRITERIA['impulse_force_change'])
    with np.load(output/(left+'.npz')) as aa,np.load(output/(right+'.npz')) as bb:
        for key in ['board_position','board_velocity']:add(key,np.linalg.norm(aa[key]-bb[key])/np.linalg.norm(bb[key]),CRITERIA['bridge_displacement_velocity_rms_change'])
        for key,index in [('bridge_displacement_m',1),('bridge_velocity_m_s',2)]:add(key,np.linalg.norm(aa['trace'][:,index]-bb['trace'][:,index])/np.linalg.norm(bb['trace'][:,index]),CRITERIA['bridge_displacement_velocity_rms_change'])
    return result


def summarize(args):
    cases={path.stem:json.loads(path.read_text()) for path in args.reports.glob('T*-*.json')}
    comparisons=[]
    for phase in ['Cold','Relaunch']:
        keys=sorted((key for key in cases if key.endswith('-'+phase)),key=lambda key:cases[key]['rate_hz'])
        comparisons.extend(compare(args.output,args.reports,a,b) for a,b in zip(keys,keys[1:]))
    report=dict(baseline_commit='919a3c5',criteria=CRITERIA,cases=cases,comparisons=comparisons,
        provenance=json.loads((args.reports/'provenance.json').read_text()),selected_factory_profile=None,
        gates=dict(energy=bool(cases) and all(c['energy_passed'] for c in cases.values()),
            cold_refinement=bool([c for c in comparisons if c['left'].endswith('-Cold')]) and all(c['passed'] for c in comparisons if c['left'].endswith('-Cold')),
            reiterated_refinement=bool([c for c in comparisons if c['left'].endswith('-Relaunch')]) and all(c['passed'] for c in comparisons if c['left'].endswith('-Relaunch'))),
        scope='C4/124 vertical one-string nonlinear SI contact, 35 low P2 modes and 11445-state passive air; prescribed separated-head relaunch, not key action or audio.')
    if args.reference:
        ref=args.reference/f'T{args.transverse}-768000-Air.json'
        old=json.loads(ref.read_text());fine=cases[f'T{args.transverse}-768000-Cold']
        report['finite_bath_model_contrast']=dict(reference_sha256=digest(ref),note='Control of different radiation approximation; not an exact broadband truth or promotion gate.',
            relative_changes={key:(fine[key]-old[key])/abs(old[key]) for key in ['contact_time_s','maximum_force_n','impulse_n_s','final_string_energy_j','final_board_energy_j','final_hammer_energy_j','work_into_board_j']})
    write(args.summary,report);print('summary',json.dumps(report['gates']),flush=True)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['input','asset','coefficients','mass','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--phase',choices=['run','summary'],default='run');p.add_argument('--transverse',type=int,default=72)
    p.add_argument('--rates',type=int,nargs='+',default=[384000,768000]);p.add_argument('--relaunch',action='store_true');p.add_argument('--reference',type=Path)
    p.add_argument('--observer-batch',type=int,default=64)
    documents=Path(__file__).resolve().parents[1]/'docs'/'concert-grand-physics-roadmap-2026-10-02'
    p.add_argument('--reports',type=Path,default=documents/'medidas-P6-contacto-compacto')
    p.add_argument('--summary',type=Path,default=documents/'resumen-P6-contacto-compacto.json')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True);args.reports.mkdir(parents=True,exist_ok=True);args.summary.parent.mkdir(parents=True,exist_ok=True)
    if args.phase=='summary':summarize(args);return
    support,metadata=bridge.p2_support(args.asset,60);system,speed=bridge.load(args.input,60,124,support,args.transverse)
    with np.load(args.asset) as d:
        mask=d['frequency']<=800;model=dict(panel=json.loads(str(d['panel'])),nx=int(d['nx']),ny=int(d['ny']),boundary=str(d['boundary']),vectors=d['vectors'][:,mask])
    surface=RectangularLoad.from_plate(model,128,96);x=surface.shapes.reshape(-1,surface.modes)
    limit=surface.density*surface.speed*surface.hx*surface.hy*x.T@x
    with np.load(args.mass) as d:mass=d['static_mass']
    with np.load(args.coefficients) as d:air=BoundedAir(d['factor'],float(d['factor_rate']),mass,limit)
    coupled=BoundedContact(system,air)
    provenance=dict(asset_sha256=digest(args.asset),coefficient_sha256=digest(args.coefficients),mass_source_sha256=digest(args.mass),support=metadata,
        input_hashes={path.name:digest(path) for path in args.input.glob('*.csv')},incident_speed_m_s=speed,felt=vars(system.felt),wire=vars(system.string.wire),
        transverse=args.transverse,axial=2*args.transverse,acoustic_states=air.states,criteria=CRITERIA,
        mechanical_coordinates='Mixed-mass normalized; C maps to physical P2 modes. No instantaneous air mass.',
        environment=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__,blas_threads_per_process=1),
        observer_batch_size=args.observer_batch,
        relaunch='Prescribed separated incident head at 4ms; string, board, wave and relaxation states preserved; external work recorded.')
    path=args.reports/'provenance.json'
    if path.exists() and json.loads(path.read_text())!=provenance:raise ValueError('frozen contact inputs changed; use a new output')
    write(path,provenance)
    for rate in args.rates:
        key=f'T{args.transverse}-{rate}-Cold'
        if (args.reports/(key+'.json')).exists() or (args.output/(key+'.npz')).exists():raise ValueError('measured case already exists; preserve it')
        print(key,flush=True);state,metrics,trace,B,BV=solve(coupled,coupled.initial_state(speed),rate,batch_size=args.observer_batch)
        save(args.output,args.reports,key,state,metrics,trace,B,BV);print('case',key,json.dumps(metrics),flush=True)
        if args.relaunch:
            before=state;state,work=coupled.relaunch(state,speed)
            preserved=all(np.array_equal(a,b) for a,b in [(before[0][:-1],state[0][:-1]),(before[1][:-1],state[1][:-1]),(before[2],state[2]),(before[3],state[3])])
            assert preserved
            second=key.replace('-Cold','-Relaunch');print(second,flush=True)
            state,metrics,trace,B,BV=solve(coupled,state,rate,batch_size=args.observer_batch)
            metrics.update(preserved_living_states=preserved,relaunch_external_work_j=work,first_case=key)
            save(args.output,args.reports,second,state,metrics,trace,B,BV);print('case',second,json.dumps(metrics),flush=True)
    summarize(args)


if __name__=='__main__':main()
