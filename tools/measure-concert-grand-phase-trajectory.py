"""Authenticate and measure a conservative string-only trajectory experiment.

Numerical energy correction is reported separately from temporal accuracy.
All rejection cases remain recorded; no contact or instrument approval follows.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_native_strings import fixture,write_fixture
from concert_grand_phase_forces import relative
from concert_grand_wave_cache import digest


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--parent',type=Path,required=True)
    parser.add_argument('--teacher-ref',required=True)
    parser.add_argument('--local',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--buffers',type=int,default=8)
    args=parser.parse_args()
    if args.local.exists() or args.report.exists() or not 8<=args.buffers<=64:
        raise ValueError('fresh paths and bounded benchmark required')
    root=Path(__file__).resolve().parents[1];parent=json.loads(args.parent.read_text())
    for name,sha in parent['source_sha256'].items():
        frozen=subprocess.check_output(['git','show',args.teacher_ref+':tools/'+name],cwd=root)
        if hashlib.sha256(frozen).hexdigest()!=sha:raise ValueError(f'teacher source mismatch: {name}')
        if name.endswith('.py') and digest(root/'tools'/name)!=sha:
            raise ValueError(f'physical provider changed: {name}')
    teacher=Path(parent['executable_path'])
    if not parent.get('checked_reference') or digest(teacher)!=parent['executable_sha256']:
        raise ValueError('authenticated work-checked teacher required')
    names=list(parent['source_sha256'])+['concert-grand-phase-forces.rs','concert-grand-phase-trajectory.rs',
        Path(__file__).name,'concert_grand_phase_forces.py','tests/test_concert_phase_trajectory.py']
    source={name:digest(root/'tools'/name) for name in names}
    args.local.mkdir(parents=True);args.report.parent.mkdir(parents=True,exist_ok=True)
    exe=args.local/'trajectory.exe';current_native=args.local/'native-current.exe'
    command=['rustc','--edition=2021','-O','-C','target-cpu=native','--cfg','concert_phase_library',
        str(root/'tools/concert-grand-phase-trajectory.rs'),'-o',str(exe)]
    subprocess.run(command,check=True)
    subprocess.run(['rustc','--edition=2021','-O','-C','target-cpu=native',
        str(root/'tools/concert-grand-native-strings.rs'),'-o',str(current_native)],check=True)
    started=time.perf_counter()
    record=dict(stage='phase-coupled string trajectory with forced axial waveform',
        source_sha256=source,parent_path=str(args.parent.resolve()),parent_sha256=digest(args.parent),
        teacher_sources_sha256=parent['source_sha256'],teacher_ref=args.teacher_ref,
        teacher_executable_path=str(teacher),teacher_executable_sha256=digest(teacher),
        executable_path=str(exe.resolve()),executable_sha256=digest(exe),compile_command=command,
        current_native_executable_path=str(current_native.resolve()),current_native_executable_sha256=digest(current_native),
        transverse=96,axial=192,note=60,seed=83,duration_s=.006,mechanical_coordinates=1153,
        fixed_bridge=True,launch_speeds=[0.,0.,0.],contact_included=False,pressure_included=False,pedals_included=False,
        numerical_energy_constraint=True,physical_dissipation=False,heat_j=0.,
        full_piano_realtime=False,global_physics_approved=False,factory_updated=False,
        teacher_cases=[],cases=[],native_controls=[],quadrature_cases=[])
    def save():
        record['seconds']=time.perf_counter()-started
        args.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    def run(label,executable,path,mode):
        begin=time.perf_counter()
        output=subprocess.run([str(executable),str(path),*map(str,mode)],capture_output=True,text=True)
        entry=dict(label=label,command=[str(executable),str(path),*map(str,mode)],
            coefficient_path=str(path.resolve()),coefficient_sha256=digest(path),elapsed_s=time.perf_counter()-begin)
        if output.returncode:
            entry.update(completed=False,returncode=output.returncode,failure=output.stderr)
            return entry,None
        result=json.loads(output.stdout);result_path=args.local/(label+'.json')
        result_path.write_text(output.stdout,encoding='utf-8')
        entry.update(result_path=str(result_path.resolve()),result_sha256=digest(result_path),completed=result.get('completed',True))
        for key in ['failure','work_relative_max','raw_local_work_relative_max','numerical_lambda_abs_max',
            'midpoint_residual_relative_max','maximum_iterations','allocations','known_prepared_bytes',
            'forced_axial_waveform','numerical_energy_constraint','high_axial_modes_per_wire','seconds']:
            if key in result:entry[key]=result[key]
        entry['work_1e9_gate_passed']=entry['completed'] and result['work_relative_max']<=1e-9
        return entry,result
    def metric(data,omega):return np.r_[omega*np.asarray(data['q'])[:-1],data['v']]
    def coast(rate,duration=.006):
        f=fixture(transverse=96,rate=rate,seed=83,duration=duration);f['launch_speeds'][:]=0.
        return f
    def physical_signature(f):
        signature=hashlib.sha256()
        for key in ['parameters','shapes','tensions','launch_speeds','times','q','endpoint','v']:
            values=np.asarray(f[key],dtype='<f8')
            if key=='parameters':values=np.delete(values,4)
            signature.update(values.tobytes())
        return signature.hexdigest()
    save()
    with threadpool_limits(limits=1):
        f=fixture(transverse=96,rate=128000,seed=83,duration=.0005)
        path=args.local/'native-live-control.bin';write_fixture(path,f)
        for mode in [['verify'],['verify-ramp',16]]:
            old_entry,old=run('old-native-'+mode[0],teacher,path,mode)
            new_entry,new=run('current-native-'+mode[0],current_native,path,mode)
            keys=['q','v','energy_j','heat_j','launch_work_j','work_relative_max','force_trace','endpoint_nonlinear_force']
            equal=all(old[key]==new[key] for key in keys)
            record['native_controls'].append(dict(mode=mode,old=old_entry,current=new_entry,arrays_and_work_equal=equal))
            if not equal:raise ValueError('library exposure changed the existing native solver')
        previous=None;fine=None
        for rate in [8192000,16384000]:
            f=coast(rate);path=args.local/f'coast-{rate}.bin';write_fixture(path,f)
            signature=physical_signature(f)
            if record.get('physical_input_sha256',signature)!=signature:raise ValueError('clock changed physical input')
            record['physical_input_sha256']=signature
            omega=np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
            entry,data=run(f'teacher-{rate}',teacher,path,['verify']);entry['rate_hz']=rate
            if data is None:raise ValueError('reference failed')
            entry.update(force_peak_n=data['force_peak_n'],checked_reference=data['checked_reference'],
                reference_local_work_max=data['reference_local_work_max'])
            if data['force_peak_n']!=0. or data['heat_j']!=0.:raise ValueError('coast reference contacted the hammer')
            record['teacher_cases'].append(entry)
            if previous is not None:
                change=relative(metric(previous,omega),metric(data,omega))
                record.update(reference_state_refinement_relative=change,
                    reference_pair_passed=change<=.01 and entry['work_1e9_gate_passed'] and record['teacher_cases'][-2]['work_1e9_gate_passed'])
            previous=data;fine=data;save()
            print('teacher',rate,'work',data['work_relative_max'],flush=True)
        specifications=[('carrier-control',128000,16,64,['--no-axial-waveform']),
            ('forced128-N16',128000,16,96,[]),('forced128-N8',128000,8,64,[]),
            ('forced128-N4',128000,4,40,[]),('forced128-N1',128000,1,24,[]),
            ('forced256-N1',256000,1,16,[]),('forced512-N1',512000,1,12,[]),
            ('forced1024-N1',1024000,1,8,[]),('forced2048-N1',2048000,1,8,[]),
            ('unconstrained128-N16',128000,16,96,['--unconstrained'])]
        for label,rate,frames,order,flags in specifications:
            f=coast(rate);path=args.local/f'{label}.bin';write_fixture(path,f)
            if physical_signature(f)!=record['physical_input_sha256']:raise ValueError('candidate changed physical input')
            entry,data=run(label,exe,path,['verify',frames,order,*flags])
            entry.update(rate_hz=rate,micro_block=frames,quadrature_order=order,macro_duration_s=frames/rate)
            if data is not None and entry['completed']:
                error=relative(metric(data,omega),metric(fine,omega))
                entry.update(state_relative=error,state_1percent_gate_passed=error<=.01,
                    scoped_coast_accuracy_passed=record['reference_pair_passed'] and error<=.01 and entry['work_1e9_gate_passed'])
            record['cases'].append(entry);save()
            print(label,'completed',entry['completed'],'state',entry.get('state_relative'),'work',entry.get('work_relative_max'),flush=True)
        f=coast(2048000,.000125);path=args.local/'quadrature-input.bin';write_fixture(path,f)
        before=None
        for order in [8,16]:
            entry,data=run(f'quadrature-{order}',exe,path,['verify',1,order])
            entry.update(rate_hz=2048000,duration_s=.000125,quadrature_order=order)
            record['quadrature_cases'].append(entry)
            if before is not None and data is not None:
                change=relative(metric(before,omega),metric(data,omega))
                record.update(quadrature_state_change_relative=change,quadrature_1e9_gate_passed=change<=1e-9)
            before=data;save()
        entry,data=run('benchmark2048',exe,path,['bench',1,8,args.buffers])
        if data is not None:
            entry['benchmark']={key:data[key] for key in ['buffers','mean_ms','p99_ms','max_ms','allocations']}
            entry['benchmark'].update(rate_hz=2048000,microframes_per_buffer=256,
                deadline_ms=256/2048000*1000,physical_duration_s=256*args.buffers/2048000,
                isolated_component_budget_passed=entry['completed'] and data['buffers']==args.buffers and data['max_ms']<=256/2048000*1000 and data['allocations']==0)
        record['benchmark']=entry
    record['sources_unchanged']=all(digest(root/'tools'/name)==sha for name,sha in source.items())
    record['parent_unchanged']=digest(args.parent)==record['parent_sha256']
    save()


if __name__=='__main__':main()
