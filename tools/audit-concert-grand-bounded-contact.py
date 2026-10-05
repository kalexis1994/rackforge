"""Frozen C4 SI contacts with compact finite-limit air and persistent states."""
import argparse,hashlib,json,platform,time
from importlib.util import spec_from_file_location,module_from_spec
from pathlib import Path
import numpy as np
import scipy
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_bounded_air import BoundedAir
from concert_grand_bounded_contact import BoundedContact
from concert_grand_air_coefficients import load_prefix

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


def observer_probe(coupled,state,rate,steps):
    """Compare both observers on a live prefix, without changing the bank state."""
    if steps<=0:raise ValueError('positive observer prefix')
    scalar=tuple(s.copy() for s in state);batched=tuple(s.copy() for s in state)
    expected=[];relaxation=[];inputs=[];equal=True;work_equal=True
    for _ in range(steps):
        scalar,a=coupled.step(scalar,1/rate)
        batched,b=coupled.step(batched,1/rate,defer_radiation=True)
        equal &= all(np.array_equal(s,t) for s,t in zip(scalar,batched))
        work_equal &= all(a[k]==b[k] for k in ['force_n','felt_loss_j','material_loss_j','air_port_work_j','work_into_board_j'])
        expected.append(a['radiation_loss_j']);relaxation.append(b['radiation_loss_j']);inputs.append(b['wave_inputs'])
    expected=np.asarray(expected);measured=coupled.air.wave.radiation_batch(np.array(inputs))/rate+relaxation
    relative=float(np.linalg.norm(measured-expected)/max(np.linalg.norm(expected),1e-30))
    return dict(rate_hz=rate,steps=steps,duration_s=steps/rate,board_modes=coupled.air.modes,acoustic_states=coupled.air.states,
        initial_wave_norm=float(np.linalg.norm(state[2])),initial_relaxation_norm=float(np.linalg.norm(state[3])),
        states_bitwise_equal_each_step=bool(equal),work_bitwise_equal_each_step=bool(work_equal),
        radiation_relative_error=relative,relative_limit=1e-12,passed=bool(equal and work_equal and relative<1e-12),
        scope='Live prefix after prescribed relaunch; not a full scalar trajectory or performance benchmark.')


def basis_contrast(args,report):
    previous=json.loads(args.basis_reference.read_text());current=report['provenance'];old=previous['provenance']
    if any(current[k]!=old[k] for k in ['asset_sha256','input_hashes','incident_speed_m_s','felt','wire','transverse','axial']):
        raise ValueError('basis comparison must preserve physical/contact inputs')
    legacy=dict(cells=[128,96],factor_rate_hz=64000.)
    if current.get('acoustic_discretization',legacy)!=old.get('acoustic_discretization',legacy):
        raise ValueError('basis comparison must preserve acoustic discretization')
    if current['support']['frequencies_hz'][:old['support']['modes']]!=old['support']['frequencies_hz']:
        raise ValueError('ordered nested structural basis required')
    changes={}
    for key,new in report['cases'].items():
        if key not in previous['cases']:raise ValueError('matching cold/reiterated reference required')
        prior=previous['cases'][key]
        values={k:(new[k]-prior[k])/max(abs(prior[k]),1e-30) for k in
            ['contact_time_s','maximum_force_n','impulse_n_s','final_string_energy_j','final_board_energy_j','final_air_energy_j',
             'material_heat_j','radiation_loss_j','air_port_work_j','work_into_board_j']}
        path=args.basis_data/(key+'.npz')
        if digest(path)!=previous['trajectory_sha256'][key]:raise ValueError('frozen reference trajectory changed')
        with np.load(path) as a,np.load(args.output/(key+'.npz')) as b:
            for label,index in [('bridge_displacement_rms',1),('bridge_velocity_rms',2)]:
                values[label]=float(np.linalg.norm(b['trace'][:,index]-a['trace'][:,index])/np.linalg.norm(a['trace'][:,index]))
            count=old['support']['modes']
            for name in ['board_position','board_velocity']:
                values['shared_'+name+'_rms']=float(np.linalg.norm(b[name][:,:count]-a[name])/np.linalg.norm(a[name]))
        changes[key]=values
    return dict(reference_summary_sha256=digest(args.basis_reference),reference_contact_implementation_commit=previous.get('contact_implementation_commit'),
        old_modes=old['support']['modes'],new_modes=current['support']['modes'],changes=changes,
        scope='Structural-basis sensitivity with the same physical inputs; not temporal convergence, sample matching or an audible claim.')


def summarize(args):
    cases={path.stem:json.loads(path.read_text()) for path in args.reports.glob('T*-*.json')}
    comparisons=[]
    for phase in ['Cold','Relaunch']:
        keys=sorted((key for key in cases if key.endswith('-'+phase)),key=lambda key:cases[key]['rate_hz'])
        comparisons.extend(compare(args.output,args.reports,a,b) for a,b in zip(keys,keys[1:]))
    provenance=json.loads((args.reports/'provenance.json').read_text())
    report=dict(baseline_commit=args.baseline_commit,criteria=CRITERIA,cases=cases,comparisons=comparisons,
        provenance=provenance,selected_factory_profile=None,
        gates=dict(energy=bool(cases) and all(c['energy_passed'] for c in cases.values()),
            cold_refinement=bool([c for c in comparisons if c['left'].endswith('-Cold')]) and all(c['passed'] for c in comparisons if c['left'].endswith('-Cold')),
            reiterated_refinement=bool([c for c in comparisons if c['left'].endswith('-Relaunch')]) and all(c['passed'] for c in comparisons if c['left'].endswith('-Relaunch'))),
        scope=f'C4/124 vertical one-string nonlinear SI contact, {provenance["support"]["modes"]} P2 modes and {provenance["acoustic_states"]}-state passive air; prescribed separated-head relaunch, not key action or audio.')
    report['contact_implementation_commit']='a526d50'
    source_names=['audit-concert-grand-bounded-contact.py','concert_grand_air_coefficients.py','concert_grand_bounded_contact.py',
        'concert_grand_bounded_air.py','concert_grand_rational_air.py','concert_grand_acoustic_bath.py','concert_grand_surface_impedance.py',
        'concert_grand_moving_bridge.py','concert_grand_coupled_contact.py','concert_grand_contact_study.py',
        'concert_grand_nonlinear_string.py','concert_grand_orthotropic_plate.py',
        'audit-concert-grand-moving-bridge.py','audit-concert-grand-coupled-contact.py']
    report['source_sha256']={name:digest(Path(__file__).with_name(name)) for name in source_names}
    report['trajectory_sha256']={key:digest(args.output/(key+'.npz')) for key in cases}
    sequences=[]
    for key,second in cases.items():
        if not key.endswith('-Relaunch'):continue
        first=cases[second['first_case']]
        scale=first['initial_energy_j']+abs(second['relaunch_external_work_j'])
        continuity=abs(second['initial_energy_j']-first['energy_checkpoints'][-1]['stored_total_j']-second['relaunch_external_work_j'])/scale
        loss=sum(first[k]+second[k] for k in ['felt_heat_j','material_heat_j','radiation_loss_j'])
        budget=abs(second['energy_checkpoints'][-1]['stored_total_j']+loss-first['initial_energy_j']-second['relaunch_external_work_j'])/scale
        sequences.append(dict(rate_hz=second['rate_hz'],preserved_living_states=second['preserved_living_states'],
            relaunch_continuity_relative_error=continuity,cumulative_budget_relative_error=budget,
            passed=second['preserved_living_states'] and max(continuity,budget)<CRITERIA['energy_budget']))
    report['two_contact_sequences']=sequences
    report['gates']['persistent_sequence']=bool(sequences) and all(s['passed'] for s in sequences)
    scalar=args.output.parent/f'T{args.transverse}-384000-Cold.npz'
    scalar_report=args.reports/'control-escalar-384000.json'
    key=f'T{args.transverse}-384000-Cold'
    if scalar.exists() and scalar_report.exists() and key in cases:
        old=json.loads(scalar_report.read_text())
        with np.load(scalar) as aa,np.load(args.output/(key+'.npz')) as bb:
            equality={name:bool(np.array_equal(aa[name],bb[name])) for name in ['q','v','wave_state_final','relaxation_state_final','board_position','board_velocity']}
            indices=[i for i in range(len(COLUMNS)) if i!=11]
            equality['trace_except_radiation']=bool(np.array_equal(aa['trace'][:,indices],bb['trace'][:,indices]))
            trace_error=float(np.linalg.norm(aa['trace'][:,11]-bb['trace'][:,11])/np.linalg.norm(aa['trace'][:,11]))
        radiation_error=abs(old['radiation_loss_j']-cases[key]['radiation_loss_j'])/old['radiation_loss_j']
        observer=dict(scalar_trajectory_sha256=digest(scalar),scalar_report_sha256=digest(scalar_report),
            dynamics_bitwise_equal=equality,radiation_trajectory_relative_error=trace_error,
            final_radiation_relative_error=radiation_error,relative_limit=1e-12,
            scalar_elapsed_s=old['elapsed_s'],batched_elapsed_s=cases[key]['elapsed_s'],
            passed=all(equality.values()) and max(trace_error,radiation_error)<1e-12)
        report['observer_equivalence']=observer;report['gates']['observer_equivalence']=observer['passed']
    else:
        probe=args.reports/'observer-live.json'
        if probe.exists():
            report['observer_live_prefix']=json.loads(probe.read_text())
            observed=report['observer_live_prefix']
            if observed['board_modes']!=provenance['support']['modes'] or observed['acoustic_states']!=provenance['acoustic_states'] or digest(args.output/(observed['first_case']+'.npz'))!=observed['first_trajectory_sha256']:
                raise ValueError('observer probe does not belong to frozen contact bank')
            report['gates']['observer_live_prefix']=report['observer_live_prefix']['passed']
        else:report['gates']['observer_equivalence']=False
    if args.basis_reference:report['basis_contrast']=basis_contrast(args,report)
    if args.reference:
        ref=args.reference/f'T{args.transverse}-768000-Air.json'
        old=json.loads(ref.read_text());fine=cases[f'T{args.transverse}-768000-Cold']
        report['finite_bath_model_contrast']=dict(reference_sha256=digest(ref),note='Control of different radiation approximation; not an exact broadband truth or promotion gate.',
            relative_changes={key:(fine[key]-old[key])/abs(old[key]) for key in ['contact_time_s','maximum_force_n','impulse_n_s','final_string_energy_j','final_board_energy_j','final_hammer_energy_j','work_into_board_j']})
    report['all_contact_gates_passed']=all(report['gates'].values())
    write(args.summary,report);print('summary',json.dumps(report['gates']),flush=True)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['input','asset','coefficients','mass','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--phase',choices=['run','summary','preflight'],default='run');p.add_argument('--transverse',type=int,default=72)
    p.add_argument('--rates',type=int,nargs='+',default=[384000,768000]);p.add_argument('--relaunch',action='store_true');p.add_argument('--reference',type=Path)
    p.add_argument('--observer-batch',type=int,default=64)
    p.add_argument('--observer-probe-steps',type=int,default=0)
    p.add_argument('--cutoff',type=float,default=800.)
    p.add_argument('--cells',type=int,nargs=2,default=[128,96])
    p.add_argument('--baseline-commit',default='919a3c5')
    p.add_argument('--load-report',type=Path)
    p.add_argument('--basis-reference',type=Path);p.add_argument('--basis-data',type=Path)
    documents=Path(__file__).resolve().parents[1]/'docs'/'concert-grand-physics-roadmap-2026-10-02'
    p.add_argument('--reports',type=Path);p.add_argument('--summary',type=Path)
    args=p.parse_args()
    if not np.isfinite(args.cutoff) or args.cutoff<=0:p.error('positive finite cutoff required')
    if min(args.cells)<2:p.error('at least two surface cells per axis required')
    if bool(args.basis_reference)!=bool(args.basis_data):p.error('--basis-reference and --basis-data required together')
    if args.observer_probe_steps<0 or (args.observer_probe_steps and not args.relaunch):p.error('live observer probe requires relaunch and nonnegative step count')
    suffix='' if args.cutoff==800 else f'-{args.cutoff:g}'
    args.reports=args.reports or documents/f'medidas-P6-contacto-compacto{suffix}'
    args.summary=args.summary or documents/f'resumen-P6-contacto-compacto{suffix}.json'
    if args.phase!='preflight':
        args.output.mkdir(parents=True,exist_ok=True);args.reports.mkdir(parents=True,exist_ok=True);args.summary.parent.mkdir(parents=True,exist_ok=True)
    if args.phase=='summary':summarize(args);return
    support,metadata=bridge.p2_support(args.asset,60,args.cutoff);system,speed=bridge.load(args.input,60,124,support,args.transverse)
    with np.load(args.asset) as d:
        mask=d['frequency']<=args.cutoff;model=dict(panel=json.loads(str(d['panel'])),nx=int(d['nx']),ny=int(d['ny']),boundary=str(d['boundary']),vectors=d['vectors'][:,mask])
    surface=RectangularLoad.from_plate(model,*args.cells);x=surface.shapes.reshape(-1,surface.modes)
    limit=surface.density*surface.speed*surface.hx*surface.hy*x.T@x
    with np.load(args.coefficients) as d:
        ordered='frequency_hz' in d
        if not ordered:
            factor=d['factor'];rate=float(d['factor_rate']);parent_modes=factor.shape[2]
            selected_mass=d['static_mass'] if 'static_mass' in d else None
            selected_limit=d['high_limit'] if 'high_limit' in d else None
    if ordered:factor,rate,selected_mass,selected_limit,parent_modes=load_prefix(args.coefficients,support.frequency)
    if args.mass.resolve()==args.coefficients.resolve() and selected_mass is not None:mass=selected_mass
    else:
        with np.load(args.mass) as d:
            mass=d['static_mass']
            if 'frequency_hz' in d:
                if not np.array_equal(d['frequency_hz'][:surface.modes],np.asarray(support.frequency)):raise ValueError('mass source modal order changed')
                mass=mass[:surface.modes,:surface.modes]
    if mass.shape!=(surface.modes,surface.modes) or factor.shape[2]!=surface.modes:raise ValueError('selected mass/factor dimensions do not match physical support')
    if selected_limit is not None and np.linalg.norm(selected_limit-limit)>1e-10*np.linalg.norm(limit):raise ValueError('coefficient high limit does not match physical surface')
    if selected_mass is not None and np.linalg.norm(selected_mass-mass)>1e-10*np.linalg.norm(mass):raise ValueError('coefficient static mass does not match mass source')
    air=BoundedAir(factor,rate,mass,limit);del factor
    if args.load_report:
        load=json.loads(args.load_report.read_text())
        if not load['all_load_gates_passed'] or not all(load['gates'].values()) or load['cutoff_hz']!=args.cutoff or load['modes']!=air.modes or load['cells']!=args.cells or load['factor_rate_hz']!=rate or load['coefficient_sha256']!=digest(args.coefficients):raise ValueError('verified frozen load with matched discretization required')
    coupled=BoundedContact(system,air)
    provenance=dict(asset_sha256=digest(args.asset),coefficient_sha256=digest(args.coefficients),mass_source_sha256=digest(args.mass),support=metadata,
        input_hashes={path.name:digest(path) for path in args.input.glob('*.csv')},incident_speed_m_s=speed,felt=vars(system.felt),wire=vars(system.string.wire),
        transverse=args.transverse,axial=2*args.transverse,acoustic_states=air.states,criteria=CRITERIA,
        acoustic_discretization=dict(cells=args.cells,factor_rate_hz=rate),
        coefficient_selection=dict(parent_modes=parent_modes,selected_modes=air.modes),
        mechanical_coordinates='Mixed-mass normalized; C maps to physical P2 modes. No instantaneous air mass.',
        environment=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__,blas_threads_per_process=1),
        observer_batch_size=args.observer_batch,
        relaunch='Prescribed separated incident head at 4ms; string, board, wave and relaxation states preserved; external work recorded.')
    if args.load_report:provenance['load_report_sha256']=digest(args.load_report)
    if args.basis_reference:
        old=json.loads(args.basis_reference.read_text())
        if not all(old['gates'].values()) or any(provenance[k]!=old['provenance'][k] for k in ['asset_sha256','input_hashes','incident_speed_m_s','felt','wire','transverse','axial']):
            raise ValueError('validated reference with unchanged physical inputs required')
        if old['provenance'].get('acoustic_discretization',dict(cells=[128,96],factor_rate_hz=64000.))!=provenance['acoustic_discretization']:
            raise ValueError('matched acoustic discretization required for basis reference')
        for rate in args.rates:
            for phase in ['Cold','Relaunch'] if args.relaunch else ['Cold']:
                key=f'T{args.transverse}-{rate}-{phase}'
                if digest(args.basis_data/(key+'.npz'))!=old['trajectory_sha256'][key]:raise ValueError('frozen reference trajectory changed')
    for rate in args.rates:
        for phase in ['Cold','Relaunch'] if args.relaunch else ['Cold']:
            key=f'T{args.transverse}-{rate}-{phase}'
            if (args.reports/(key+'.json')).exists() or (args.output/(key+'.npz')).exists():raise ValueError('measured case already exists; preserve it')
    path=args.reports/'provenance.json'
    if path.exists() and json.loads(path.read_text())!=provenance:raise ValueError('frozen contact inputs changed; use a new output')
    if args.phase=='preflight':
        energy=coupled.energy(coupled.initial_state(speed))
        print('preflight',json.dumps(dict(provenance=provenance,initial_energy_j=energy['total_j'],
            scope='Initialization and frozen-load compatibility only; no time trajectory or contact gate measured.')),flush=True)
        return
    write(path,provenance)
    for rate in args.rates:
        key=f'T{args.transverse}-{rate}-Cold'
        if (args.reports/(key+'.json')).exists() or (args.output/(key+'.npz')).exists():raise ValueError('measured case already exists; preserve it')
        print(key,flush=True);state,metrics,trace,B,BV=solve(coupled,coupled.initial_state(speed),rate,batch_size=args.observer_batch)
        if args.basis_reference:
            count=json.loads(args.basis_reference.read_text())['provenance']['support']['modes']
            metrics['additional_modes_final_board_energy_j']=float(.5*np.sum(BV[-1,count:]**2+(B[-1,count:]*system.string.omega[count:])**2))
        save(args.output,args.reports,key,state,metrics,trace,B,BV);print('case',key,json.dumps(metrics),flush=True)
        if args.relaunch:
            before=state;state,work=coupled.relaunch(state,speed)
            preserved=all(np.array_equal(a,b) for a,b in [(before[0][:-1],state[0][:-1]),(before[1][:-1],state[1][:-1]),(before[2],state[2]),(before[3],state[3])])
            assert preserved
            if args.observer_probe_steps and not (args.reports/'observer-live.json').exists():
                probe=observer_probe(coupled,state,rate,args.observer_probe_steps)
                probe.update(first_case=key,first_trajectory_sha256=digest(args.output/(key+'.npz')))
                write(args.reports/'observer-live.json',probe);print('observer',json.dumps(probe),flush=True)
            second=key.replace('-Cold','-Relaunch');print(second,flush=True)
            state,metrics,trace,B,BV=solve(coupled,state,rate,batch_size=args.observer_batch)
            metrics.update(preserved_living_states=preserved,relaunch_external_work_j=work,first_case=key)
            if args.basis_reference:
                metrics['additional_modes_final_board_energy_j']=float(.5*np.sum(BV[-1,count:]**2+(B[-1,count:]*system.string.omega[count:])**2))
            save(args.output,args.reports,second,state,metrics,trace,B,BV);print('case',second,json.dumps(metrics),flush=True)
    summarize(args)


if __name__=='__main__':main()
