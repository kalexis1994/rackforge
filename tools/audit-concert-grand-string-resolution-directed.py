"""Directed 2x2 wire/time refinement after the measured resolution failures.

Same full 606-coordinate board and authenticated air cache. Reconstructs
the old 48-mode trajectory; finite 0–3.5 ms pressure, no timbral approval.
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

spec=importlib.util.spec_from_file_location('directed_wire_reference',Path(__file__).with_name('audit-concert-grand-string-resolution.py'))
reference=importlib.util.module_from_spec(spec);spec.loader.exec_module(reference)
field=reference.field
MODES=(48,96)
RATES=(1536000,3072000)
WORK=('mechanical_final_j','relaxation_final_j','felt_heat_j','material_heat_j','relaxation_loss_j','air_port_work_j')


def case_key(nt,rate):return f'nt{nt}-nl{2*nt}-r{rate}-c256000'


def comparisons(observations,frequency):
    """Both spatial rows and both temporal columns must be measured."""
    expected={case_key(nt,rate) for nt in MODES for rate in RATES}
    if set(observations)!=expected:raise ValueError('complete 48/96 by 1536/3072 matrix required')
    rows=[]
    for kind,pairs in [('spatial',[(case_key(48,r),case_key(96,r)) for r in RATES]),
                       ('contact_temporal',[(case_key(n,RATES[0]),case_key(n,RATES[1])) for n in MODES])]:
        for actual,fine in pairs:
            rows.append(dict(kind=kind,case=actual,reference=fine,
                **compare_observation(observations[actual],observations[fine],frequency)))
    return rows


def comparison_gates(rows,criteria):
    if len(rows)!=4 or sum(row['kind']=='spatial' for row in rows)!=2:
        raise ValueError('two spatial and two temporal controls required')
    gates={}
    for row in rows:
        spatial=row['kind']=='spatial'
        if not spatial and row['kind']!='contact_temporal':raise ValueError('unknown comparison')
        tolerance=criteria['spatial_pressure_relative'] if spatial else criteria['temporal_pressure_relative']
        key=row['kind']+'_'+row['case']
        gates[key+'_pressure']=bool(max([*row['complex_pressure_relative'].values(),
                                        row['temporal_pressure_relative']])<tolerance)
        if spatial:
            gates[key+'_board']=bool(max(row['physical_board_position_relative'],
                                        row['physical_board_velocity_relative'])<criteria['spatial_board_relative'])
    return gates


def baseline_differences(row,prior):
    """Compare measured physics/pressure, excluding runtime and solver counters."""
    groups={}
    for group in ['contact','coast']:
        groups[group]={name:reference.difference(row[group][name],old)
            for name,old in prior[group].items()
            if (name.endswith('_j') or name in ['compression_final_m','final_wave_norm','initial_wave_norm',
                'peak_centered_contact_force_n','contact_impulse_n_s','hammer_momentum_change_n_s'])}
    groups['pressure']={name:field.relative(np.asarray(row['pressure'][name]),np.asarray(prior['pressure'][name]))
        for name in ['pressure_peak_pa','pressure_rms_pa']}
    groups['pressure_bands']={name:reference.difference(row['pressure']['positive_band_integral_pa2_s'][name],value)
        for name,value in prior['pressure']['positive_band_integral_pa2_s'].items()}
    return groups


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['full-report','resolution-report','board','cache','report']:
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--direction-block',type=int,default=32)
    p.add_argument('--max-array-gib',type=float,default=12.)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    if not np.isfinite(a.max_array_gib) or a.max_array_gib<=0 or a.direction_block<=0:
        raise ValueError('positive memory budget and direction block required')
    if not a.cache.exists() or not a.cache.resolve().is_relative_to(Path('local/concert-grand-physics').resolve()):
        raise ValueError('existing single local cache required')
    old=json.loads(a.resolution_report.read_text())
    if (old.get('stage')!='full board string resolution preflight completed'
            or old.get('all_gates_passed') is not False or old.get('resolution_gates_passed') is not False
            or old['resolution_gates']['spatial_pressure'] or old['resolution_gates']['contact_temporal_pressure']
            or digest(a.full_report) not in old['parent_sha256'].values() or digest(a.board)!=old['board_sha256']):
        raise ValueError('completed matched resolution report with both original failures required')
    for path,sha in old['parent_sha256'].items():
        if digest(Path(path))!=sha:raise ValueError('changed parent report: '+path)
    for name,sha in old['source_sha256'].items():
        if digest(Path(__file__).with_name(name))!=sha:raise ValueError('changed frozen source: '+name)
    sources=set(old['source_sha256'])|{Path(__file__).name}
    start=time.perf_counter();criteria=old['criteria'].copy()
    r=dict(stage='construction',all_gates_passed=False,preflight_only=True,criteria=criteria,
        cases=[],source_sha256={name:digest(Path(__file__).with_name(name)) for name in sorted(sources)},
        parent_sha256={str(path):digest(path) for path in [a.full_report,a.resolution_report]},
        board_sha256=digest(a.board),geometry=old['geometry'],windows=old['windows'],
        scope=old['scope'],transverse=list(MODES),contact_rates=list(RATES),
        limitations=['Finite-window pressure, one assumed C4 wire, no full angular certificate.',
                     'Air load validated through 6 kHz only; refined wire includes higher free frequencies.',
                     'No plugin/felt/gain change, unison, whole-note or Steinway fidelity approval.'])
    def checkpoint():
        r['seconds']=time.perf_counter()-start;r['peak_working_set_bytes']=field.full.reference.peak_memory()
        a.report.parent.mkdir(parents=True,exist_ok=True)
        a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')
    checkpoint()
    approved,modal,source_B,notes,surface,provider,identity,_=field.full.prepare_reference(a)
    # No angular step/tap/rank history: only native physical board endpoints,
    # the live acoustic state, and common pressure-grid observations in RAM.
    endpoints=round(.003*max(RATES))+129
    state_bytes=approved['states']*8
    trace_bytes=endpoints*provider.modes*8*2
    common_bytes=4*2*897*provider.modes*8
    estimate=provider.budget(real_gram=True)['estimated_array_peak_bytes']+8*state_bytes+4*trace_bytes+common_bytes+512*2**20
    r['array_estimates']=dict(maximum_bytes=int(a.max_array_gib*2**30),estimated_bytes=int(estimate),
        native_trace_bytes=int(trace_bytes),angular_history_bytes=0,
        explanation='Conservative array estimate, not an operating-system memory cap.')
    checkpoint()
    if estimate>a.max_array_gib*2**30:raise MemoryError('declared array budget exceeded')
    _,_,air=field.full.construct_air(a,approved,provider,surface,identity,int(a.max_array_gib*2**30),r,checkpoint)
    if r['cache_R_sha256']!=old['cache_R_sha256']:raise ValueError('changed acoustic load')
    observer=CellObserver(provider.surface,old['microphones'],4)
    kernel=retarded_kernel(observer,256000);frequency=np.r_[0.,np.arange(60.,6001.,60.)]
    transfer=kernel_transfer(kernel,256000,frequency)
    r['microphones']=old['microphones'];r['kernel_bytes']=kernel.nbytes
    support=modal.support(source_B[:,notes.index(60)]);observations={}
    for nt in MODES:
        for rate in RATES:
            key=case_key(nt,rate);r['stage']='case '+key;checkpoint()
            system=field.BridgeHammer(field.MovingBridge(field.Wire(.62,.0081,850),support,transverse=nt,axial=2*nt),
                                     field.Felt(.002,7.5e8,2.6,.125,.02,.1))
            pilot=field.FastBoundedContact(system,air)
            seed,contact,t,B,V=capture_contact(pilot,5.,rate,.003,
                lambda i,n:print('contact',key,i,n,flush=True))
            if contact['compression_final_m']>=0:
                r['rejected_contact']=dict(case=key,diagnostics=contact);checkpoint()
                raise AssertionError('common 3 ms contact does not end separated')
            _,coast,_,tc,Bc,Vc=run_coast(field.ExponentialCoast(system,air),seed,256000,.0005)
            whole=join_capture((t,B,V),(tc,Bc,Vc))
            observed,pressure=pressure_observation(whole,frequency,transfer,kernel,256000)
            frequencies=(np.sqrt(np.diag(system.string.base.kt))/(2*np.pi)).tolist()
            row=dict(case=key,transverse_per_direction=nt,axial=2*nt,contact_rate=rate,coast_rate=256000,
                cosine_nodes=system.string.base.nodes,free_fixed_endpoint_transverse_hz=frequencies,
                modes_at_or_below_6khz=sum(hz<=6000 for hz in frequencies),contact=contact,coast=coast,pressure=pressure)
            if nt==48 and rate==RATES[0]:
                prior=next(c for c in old['cases'] if c['case']==key)
                r['baseline_reproduction']=baseline_differences(row,prior)
                if max(value for group in r['baseline_reproduction'].values() for value in group.values())>=criteria['reproduction_relative']:
                    r['rejected_baseline']=row;checkpoint();raise AssertionError('changed measured 48-mode baseline')
            r['cases'].append(row);observations[key]=observed;checkpoint()
            print('case measured',key,contact['peak_centered_contact_force_n'],pressure['positive_band_integral_pa2_s'],flush=True)
            del whole,t,B,V,tc,Bc,Vc,seed,pilot,system
    r['comparisons']=comparisons(observations,frequency)
    r['contact_work_refinement']={}
    for nt in MODES:
        coarse=next(c for c in r['cases'] if c['case']==case_key(nt,RATES[0]))
        fine=next(c for c in r['cases'] if c['case']==case_key(nt,RATES[1]))
        r['contact_work_refinement'][str(nt)]={name:field.work.energy_change(coarse['contact'],fine['contact'],name,
            dict(near_zero_j=criteria['work_floor_j'],energy_work_relative=criteria['temporal_work_relative'])) for name in WORK}
    gates=comparison_gates(r['comparisons'],criteria)
    gates.update(baseline_reproduced=True,
        mechanical_work=max(c['contact']['maximum_mechanical_budget_relative'] for c in r['cases'])<criteria['mechanical_work_relative'],
        hammer_impulse=max(c['contact']['impulse_momentum_relative'] for c in r['cases'])<criteria['impulse_relative'],
        no_negative_heat=all(c[g]['minimum_step_heat_j']>=-1e-16 for c in r['cases'] for g in ['contact','coast']),
        separated=all(c['contact']['compression_final_m']<0 for c in r['cases']),
        capture=max(c['pressure']['capture_decimation_relative'] for c in r['cases'])<criteria['capture_relative'],
        contact_work_refinement=all(term['passed'] for terms in r['contact_work_refinement'].values() for term in terms.values()),
        source_unchanged=all(digest(Path(__file__).with_name(name))==sha for name,sha in r['source_sha256'].items()))
    r['directed_gates']={name:bool(value) for name,value in gates.items()}
    r['directed_gates_passed']=all(r['directed_gates'].values())
    r['stage']='full board directed string resolution preflight completed';checkpoint()
    print('finished',json.dumps(r['directed_gates']),r['seconds'],flush=True)


if __name__=='__main__':main()
