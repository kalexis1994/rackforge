"""Native measured body PLUS causal stereo pressure; separate digital fidelity.

All loaded/held-out fidelity failures stay explicit. No string, hammer,
pedal or complete-piano runtime claim is made by this component benchmark.
"""
import argparse
import json
from pathlib import Path
import subprocess
import time
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_realtime_pressure import PressureRuntime,fit_digital_readout,fit_digital_state_readout
from concert_grand_causal_pressure_fit import acceleration_response,pressure_response
from concert_grand_wave_cache import digest


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for flag in ['body-report','observer-report','teacher-report','local','report']:
        parser.add_argument('--'+flag,type=Path,required=True)
    parser.add_argument('--digital-observer',action='store_true')
    parser.add_argument('--state-observer',action='store_true')
    parser.add_argument('--readout-ridge',type=float,default=1e-6)
    args=parser.parse_args()
    if args.state_observer and not args.digital_observer:
        raise ValueError('state observation requires the digital observation fit')
    if args.local.exists() or args.report.exists():
        raise ValueError('new measurement paths required')
    root=Path(__file__).resolve().parent.parent
    body,observer,teacher=[json.loads(p.read_text()) for p in [args.body_report,args.observer_report,args.teacher_report]]
    for record in [body,observer,teacher]:
        for p,h in [('model_path','model_sha256'),('data_path','data_sha256')]:
            if p in record and digest(record[p])!=record[h]:
                raise ValueError('frozen artifact changed')
    if observer['body_report_sha256']!=digest(args.body_report):
        raise ValueError('observer and body mismatch')
    args.local.mkdir(parents=True)
    args.report.parent.mkdir(parents=True,exist_ok=True)
    exe=args.local/'pressure-body.exe'
    subprocess.run(['rustc','--edition=2021','-O','-C','target-cpu=native',str(root/'tools/concert-grand-pressure-body.rs'),'-o',str(exe)],check=True)
    started=time.perf_counter(); rows=[]
    with np.load(body['model_path']) as b,np.load(observer['model_path']) as o,np.load(teacher['data_path']) as t,threadpool_limits(limits=1):
        vf=t['validation_frequency_hz']
        continuous=pressure_response(acceleration_response(b['omega'],b['damping'],b['ports'],vf),vf,o['delays'],o['readout']) @ b['mapping'].T
        for rate in [44100,48000]:
            runtime=PressureRuntime(b['omega'],b['damping'],b['ports'],b['mapping'],o['delays'],o['readout'],rate)
            fitted_observer={}
            if args.digital_observer:
                tf=t['training_frequency_hz']
                target=pressure_response(acceleration_response(b['omega'],b['damping'],b['ports'],tf),tf,o['delays'],o['readout'])
                if not np.isfinite(args.readout_ridge) or args.readout_ridge<=0:
                    raise ValueError('positive readout regularization required')
                observer_path=args.local/f'observer-{rate}.npz'
                if args.state_observer:
                    state_readout,force_readout=fit_digital_state_readout(runtime,tf,target,ridge=args.readout_ridge)
                    runtime.set_state_readout(state_readout,force_readout)
                    np.savez_compressed(observer_path,state_readout=state_readout,force_readout=force_readout,delays=o['delays'],training_frequency_hz=tf)
                else:
                    digital_readout=fit_digital_readout(runtime,tf,target,ridge=args.readout_ridge)
                    runtime.set_readout(digital_readout)
                    np.savez_compressed(observer_path,readout=digital_readout,delays=o['delays'],training_frequency_hz=tf)
                fitted_observer=dict(model_path=str(observer_path.resolve()),model_sha256=digest(observer_path),training_frequency_count=len(tf))
            path=args.local/f'body-pressure-{rate}.bin'; runtime.write(path)
            native=json.loads(subprocess.check_output([str(exe),str(path),'verify'],text=True))
            pressure=[]; work=checksum=0.
            for frame in range(1200):
                force=np.array([((frame*17+port*13)%97-48)/48 if frame<400 else 0. for port in range(len(b['mapping']))])
                v,p=runtime.process(force); pressure.append(p)
                work+=runtime.step.dt*force @ v; checksum+=np.arange(1,len(v)+1) @ v
            pressure=np.asarray(pressure)
            error=float(np.linalg.norm(pressure-native['pressure'])/max(np.linalg.norm(pressure),1e-30))
            np.testing.assert_allclose(native['real'],runtime.state.real,rtol=1e-8,atol=1e-12)
            np.testing.assert_allclose(native['imag'],runtime.state.imag,rtol=1e-8,atol=1e-12)
            np.testing.assert_allclose([native['work_j'],native['checksum']],[work,checksum],rtol=1e-8,atol=1e-12)
            if not np.isfinite(error) or error>1e-8:
                raise ValueError('native pressure differs from reference')
            # Output is interval-mean acceleration, tagged at sample start.
            # Align only the declared FIR pipeline latency. The held source's
            # half-sample lag cancels the interval-mean observation's advance;
            # keep remaining held-force dynamics and aliasing errors visible.
            sampled=runtime.harmonic_response(vf)*np.exp(2j*np.pi*vf[:,None,None]*runtime.latency_samples/rate)
            relative=np.linalg.norm(sampled-continuous,axis=(1,2))/np.maximum(np.linalg.norm(continuous,axis=(1,2)),1e-30)
            # Keep complete held-out teacher errors distinct from the conversion
            # error against the same already-reduced continuous body.
            indices=t['held_out_input_keys']-21
            rows_mapping=b['mapping'][indices]
            pressure_actual=sampled[:,:,indices]
            mobility_actual=np.einsum('pk,fkl,ql->fpq',rows_mapping,runtime.mobility_response(vf),rows_mapping,optimize=True)
            def metric(actual,target):
                df=np.r_[(vf[1]-vf[0])/2,(vf[2:]-vf[:-2])/2,(vf[-1]-vf[-2])/2]
                error=np.linalg.norm(actual-target,axis=(1,2))**2
                norm=np.linalg.norm(target,axis=(1,2))**2
                rms=float(np.sqrt(df @ error/(df @ norm)))
                worst=float(np.sqrt(np.max(error/np.maximum(norm,1e-30))))
                return dict(rms=rms,worst=worst,passed=rms<=.05 and worst<=.10)
            loaded_mobility=metric(mobility_actual,t['mobility_validation_full'])
            loaded_pressure=metric(pressure_actual,t['pressure_validation_full'])
            bench=json.loads(subprocess.check_output([str(exe),str(path),'bench'],text=True))
            bench['body_and_pressure_20percent_budget_passed']=bench['p99_ms']<=.2*bench['deadline_ms'] and bench['max_ms']<=bench['deadline_ms'] and bench['allocations']==0
            rows.append(dict(rate_hz=rate,coefficient_path=str(path.resolve()),coefficient_sha256=digest(path),
                native_pressure_relative=error,state_ports_work_parity=True,benchmark=bench,
                digital_pressure_max_relative=float(relative.max()),digital_pressure_max_1percent_passed=bool(relative.max()<=.01),
                digital_pressure_per_frequency=relative.tolist(),fir_latency_samples=runtime.latency_samples,
                delay_history_frames=runtime.history_frames,pressure_group_histories=runtime.groups,
                fitted_observer=fitted_observer,loaded_teacher_mobility=loaded_mobility,loaded_teacher_pressure=loaded_pressure))
            print('body and pressure',rate,'P99',bench['p99_ms'],'digital max',float(relative.max()),flush=True)
    result=dict(stage='native loaded body and stereo pressure measured',seconds=time.perf_counter()-started,
        body_report_sha256=digest(args.body_report),observer_report_sha256=digest(args.observer_report),teacher_report_sha256=digest(args.teacher_report),
        source_sha256={n:digest(root/'tools'/n) for n in [Path(__file__).name,'concert_grand_realtime_pressure.py','concert-grand-pressure-body.rs']},
        native_executable_sha256=digest(exe),rates=rows,pressure_included=True,strings_included=False,digital_observer_fitted=args.digital_observer,
        digital_readout_ridge=args.readout_ridge,training_only_readout_fit=True,
        state_and_force_observer=args.state_observer,
        full_piano_realtime=False,global_physics_approved=False,factory_updated=False,
        inherited_mobility_gate_passed=body['mobility_gate_passed'],inherited_pressure_gate_passed=observer['pressure_gate_passed'])
    args.report.write_text(json.dumps(result,indent=2,allow_nan=False)+'\n',encoding='utf-8')


if __name__=='__main__':
    main()
