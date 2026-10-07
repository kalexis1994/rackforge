"""Full-board early pressure sensitivity to transverse/axial wire resolution.

Same assumed C4 wire/felt, 606 board modes, authenticated live air load.
Resolution sweep, directed contact/axial/coast controls. This is an attack
preflight, not a complete angular ledger, whole-note or real-piano approval.
"""
import argparse
import importlib.util
import json
import time
from pathlib import Path
import numpy as np
from concert_grand_string_resolution import capture_contact,join_capture,pressure_observation,compare_observation,BANDS
from concert_grand_coast_measurements import run_coast
from concert_grand_cell_observer import CellObserver
from concert_grand_cell_audio import retarded_kernel,kernel_transfer
from concert_grand_wave_cache import digest

spec=importlib.util.spec_from_file_location('string_field_reference',Path(__file__).with_name('audit-concert-grand-full-coast-field.py'))
field=importlib.util.module_from_spec(spec);spec.loader.exec_module(field)


def difference(a,b):return float(abs(a-b)/max(abs(b),1e-14))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['full-report','field-report','control-report','board','cache','report']:
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--transverse',type=int,nargs='+',default=[3,6,12,24,48])
    p.add_argument('--contact-rate',type=int,default=768000)
    p.add_argument('--direction-block',type=int,default=32)
    p.add_argument('--max-array-gib',type=float,default=12.)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    if (a.transverse[0]!=3 or len(a.transverse)<3 or sorted(set(a.transverse))!=a.transverse
            or a.contact_rate!=768000 or not np.isfinite(a.max_array_gib) or a.max_array_gib<=0):
        raise ValueError('nested modal sweep from frozen three-mode 768 kHz contact required')
    if not a.cache.exists() or not a.cache.resolve().is_relative_to(Path('local/concert-grand-physics').resolve()):
        raise ValueError('existing single local cache required')
    old=json.loads(a.field_report.read_text());control=json.loads(a.control_report.read_text())
    if (old.get('stage')!='full coast field audit completed' or old.get('duration_s')!=.0005
            or control.get('stage')!='full cell projection observation audit completed'
            or not control.get('projection_gates_passed')
            or digest(a.field_report) not in control['parent_sha256'].values()
            or digest(a.full_report) not in control['parent_sha256'].values()
            or digest(a.board)!=old['board_sha256'] or old['board_sha256']!=control['board_sha256']):
        raise ValueError('matched completed load/field/control reports required')
    for name,sha in control['source_sha256'].items():
        if digest(Path(__file__).with_name(name))!=sha:raise ValueError('changed frozen source: '+name)
    sources=set(control['source_sha256'])|{Path(__file__).name,'concert_grand_string_resolution.py','concert_grand_contact_study.py'}
    start=time.perf_counter();criteria=dict(reproduction_relative=1e-10,mechanical_work_relative=1e-9,
        impulse_relative=1e-10,spatial_pressure_relative=.01,spatial_board_relative=.01,
        temporal_pressure_relative=.001,capture_relative=.001,temporal_work_relative=.002,work_floor_j=1e-14)
    r=dict(stage='construction',all_gates_passed=False,preflight_only=True,criteria=criteria,cases=[],
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in sorted(sources)},
        parent_sha256={str(path):digest(path) for path in [a.full_report,a.field_report,a.control_report]},
        board_sha256=digest(a.board),transverse=a.transverse,
        scope='Assumed C4, same 5 m/s hammer and all board/air coordinates. Finite 0–3.5 ms '
              'acceleration-window pressure, no endpoint impulses, gain fitting, full field ledger or audible approval.',
        geometry=dict(wire=dict(length_m=.62,linear_density_kg_m=.0081,tension_n=850.,young_pa=200e9,density_kg_m3=7850.),
            felt=dict(mass_kg=.002,stiffness=7.5e8,exponent=2.6,strike_fraction=.125,width_fraction=.02,rate_damping_s_m=.1),
            bending='Euler',vertical_moving_endpoint=True,incident_speed_m_s=5.,board_coordinates=606),
        windows=dict(contact_s=.003,coast_s=.0005,pressure_rate=256000,bands_hz=BANDS,
                     explanation='Short finite acceleration-window contribution, not resolved sustain partial levels or release.'))

    def checkpoint():
        r['seconds']=time.perf_counter()-start;r['peak_working_set_bytes']=field.full.reference.peak_memory()
        a.report.parent.mkdir(parents=True,exist_ok=True)
        a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')

    checkpoint()
    approved,modal,source_B,notes,surface,provider,identity,_=field.full.prepare_reference(a)
    estimate=provider.budget(real_gram=True)['estimated_array_peak_bytes']+512*2**20
    r['array_estimates']=dict(maximum_bytes=int(a.max_array_gib*2**30),estimated_bytes=estimate)
    if estimate>a.max_array_gib*2**30:raise MemoryError('declared array budget exceeded')
    _,_,air=field.full.construct_air(a,approved,provider,surface,identity,int(a.max_array_gib*2**30),r,checkpoint)
    if r['cache_R_sha256']!=old['cache_R_sha256']:raise ValueError('changed acoustic load')
    observer=CellObserver(provider.surface,old['microphones'],4)
    kernel=retarded_kernel(observer,256000);frequency=np.r_[0.,np.arange(60.,6001.,60.)]
    transfer=kernel_transfer(kernel,256000,frequency)
    r['microphones']=old['microphones'];r['kernel_bytes']=kernel.nbytes
    support=modal.support(source_B[:,notes.index(60)])
    observations={};contacts={}
    largest=a.transverse[-1]
    specs=[(nt,2*nt,768000,256000) for nt in a.transverse]
    specs.extend([(largest,2*largest,1536000,256000),(largest,4*largest,1536000,256000),
                  (largest,2*largest,1536000,512000)])
    for nt,nl,rate,coast_rate in specs:
        key=f'nt{nt}-nl{nl}-r{rate}-c{coast_rate}';contact_key=(nt,nl,rate)
        r['stage']='case '+key;checkpoint()
        if contact_key not in contacts:
            system=field.BridgeHammer(field.MovingBridge(field.Wire(.62,.0081,850),support,transverse=nt,axial=nl),
                                     field.Felt(.002,7.5e8,2.6,.125,.02,.1))
            pilot=field.FastBoundedContact(system,air)
            seed,contact,t,B,V=capture_contact(pilot,5.,rate,.003,
                lambda i,n:print('contact',key,i,n,flush=True))
            if contact['compression_final_m']>=0:
                r['rejected_contact']=dict(case=key,diagnostics=contact);checkpoint()
                raise AssertionError('common 3 ms contact does not end separated')
            # Retain only the finest contact for the one independent coast
            # refinement; other histories and full air states are discarded.
            if contact_key==(largest,2*largest,1536000):contacts[contact_key]=(system,seed,contact,(t,B,V))
        else:system,seed,contact,(t,B,V)=contacts[contact_key]
        coast_pilot=field.ExponentialCoast(system,air)
        _,coast,_,tc,Bc,Vc=run_coast(coast_pilot,seed,coast_rate,.0005)
        whole=join_capture((t,B,V),(tc,Bc,Vc))
        observed,pressure=pressure_observation(whole,frequency,transfer,kernel,256000)
        frequencies=(np.sqrt(np.diag(system.string.base.kt))/(2*np.pi)).tolist()
        row=dict(case=key,transverse_per_direction=nt,axial=nl,contact_rate=rate,coast_rate=coast_rate,
            cosine_nodes=system.string.base.nodes,free_fixed_endpoint_transverse_hz=frequencies,
            modes_at_or_below_6khz=sum(hz<=6000 for hz in frequencies),
            highest_axial_hz=float(nl*np.sqrt(system.string.wire.young/system.string.wire.density)/(2*system.string.wire.length)),
            contact=contact,coast=coast,pressure=pressure)
        if nt==3:
            row['baseline_seed_reproduction']={name:difference(contact[name],old['seed'][name]) for name in old['seed']}
            prior=next(c for c in old['cases'] if c['rate']==256000)
            row['baseline_coast_reproduction']={name:difference(coast[name],prior[name]) for name in [
                'mechanical_final_j','relaxation_final_j','final_wave_norm','compression_final_m',
                'felt_heat_j','material_heat_j','relaxation_loss_j','air_port_work_j']}
            _,check=pressure_observation((tc,Bc,Vc),frequency,transfer,kernel,256000)
            row['baseline_coast_pressure_reproduction']={name:field.relative(np.asarray(check[name]),np.asarray(prior[name]))
                for name in ['pressure_peak_pa','pressure_rms_pa']}
            if max([*row['baseline_seed_reproduction'].values(),*row['baseline_coast_reproduction'].values(),
                    *row['baseline_coast_pressure_reproduction'].values()])>=1e-10:
                r['rejected_baseline']=row;checkpoint();raise AssertionError('changed measured baseline')
        r['cases'].append(row);observations[key]=observed;checkpoint()
        print('case measured',key,contact['peak_centered_contact_force_n'],pressure['positive_band_integral_pa2_s'],flush=True)
    reference=f'nt{largest}-nl{2*largest}-r768000-c256000'
    r['spatial_comparisons']=[dict(case=f'nt{nt}-nl{2*nt}-r768000-c256000',reference=reference,
        **compare_observation(observations[f'nt{nt}-nl{2*nt}-r768000-c256000'],observations[reference],frequency))
        for nt in a.transverse[:-1]]
    fine=f'nt{largest}-nl{2*largest}-r1536000-c256000'
    axial=f'nt{largest}-nl{4*largest}-r1536000-c256000'
    coast_fine=f'nt{largest}-nl{2*largest}-r1536000-c512000'
    r['contact_temporal_comparison']=compare_observation(observations[reference],observations[fine],frequency)
    r['axial_comparison']=compare_observation(observations[fine],observations[axial],frequency)
    r['coast_temporal_comparison']=compare_observation(observations[fine],observations[coast_fine],frequency)
    coarse_row=next(c for c in r['cases'] if c['case']==reference)
    fine_row=next(c for c in r['cases'] if c['case']==fine)
    r['contact_work_refinement']={name:field.work.energy_change(coarse_row['contact'],fine_row['contact'],name,
        dict(near_zero_j=criteria['work_floor_j'],energy_work_relative=criteria['temporal_work_relative']))
        for name in ['mechanical_final_j','relaxation_final_j','felt_heat_j','material_heat_j','relaxation_loss_j','air_port_work_j']}
    def pressure_pass(comparison,tolerance):
        return max([*comparison['complex_pressure_relative'].values(),comparison['temporal_pressure_relative']])<tolerance
    finest_spatial=r['spatial_comparisons'][-1]
    r['resolution_gates']=dict(
        baseline_reproduced=True,
        mechanical_work=max(c['contact']['maximum_mechanical_budget_relative'] for c in r['cases'])<1e-9,
        hammer_impulse=max(c['contact']['impulse_momentum_relative'] for c in r['cases'])<1e-10,
        no_negative_heat=all(c['contact']['minimum_step_heat_j']>=-1e-16 and c['coast']['minimum_step_heat_j']>=-1e-16 for c in r['cases']),
        separated=all(c['contact']['compression_final_m']<0 for c in r['cases']),
        capture=max(c['pressure']['capture_decimation_relative'] for c in r['cases'])<criteria['capture_relative'],
        spatial_pressure=pressure_pass(finest_spatial,criteria['spatial_pressure_relative']),
        spatial_board=max(finest_spatial['physical_board_position_relative'],finest_spatial['physical_board_velocity_relative'])<criteria['spatial_board_relative'],
        axial_pressure=pressure_pass(r['axial_comparison'],criteria['spatial_pressure_relative']),
        contact_temporal_pressure=pressure_pass(r['contact_temporal_comparison'],criteria['temporal_pressure_relative']),
        coast_temporal_pressure=pressure_pass(r['coast_temporal_comparison'],criteria['temporal_pressure_relative']),
        contact_work_refinement=all(c['passed'] for c in r['contact_work_refinement'].values()),
        source_unchanged=all(digest(Path(__file__).with_name(name))==sha for name,sha in r['source_sha256'].items()))
    r['resolution_gates']={name:bool(value) for name,value in r['resolution_gates'].items()}
    r['resolution_gates_passed']=all(r['resolution_gates'].values())
    r['stage']='full board string resolution preflight completed';checkpoint()
    print('finished',json.dumps(r['resolution_gates']),r['seconds'],flush=True)


if __name__=='__main__':main()
