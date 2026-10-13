"""Independent real held-state replay of the measured causal stereo readout.

The reference uses a real augmented matrix exponential, rather than the
consumer's complex poles. Checks digital transfer and two pressure DC moments.
"""
import argparse
import json
from pathlib import Path
import numpy as np
from scipy.linalg import solve
from threadpoolctl import threadpool_limits
from concert_grand_realtime_body import Body
from concert_grand_wave_cache import digest


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['runtime-report','pressure-report','body-report','observer-report','teacher-report','report']:
        parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args()
    if args.report.exists():
        raise ValueError('preserve previous verification')
    root=Path(__file__).resolve().parent.parent
    runtime,pressure,body,observer,teacher=[json.loads(p.read_text()) for p in
        [args.runtime_report,args.pressure_report,args.body_report,args.observer_report,args.teacher_report]]
    if runtime['parent_report_sha256']!=digest(args.pressure_report) or runtime['body_report_sha256']!=digest(args.body_report):
        raise ValueError('runtime provenance mismatch')
    for source,sha in runtime['source_sha256'].items():
        if digest(root/'tools'/source)!=sha:
            raise ValueError('runtime source changed')
    for record,path,sha in [(body,'model_path','model_sha256'),(observer,'model_path','model_sha256'),(teacher,'data_path','data_sha256')]:
        if digest(record[path])!=record[sha]:
            raise ValueError('reference artifact changed')
    rows=[]
    with np.load(body['model_path']) as b,np.load(observer['model_path']) as original,np.load(teacher['data_path']) as t,threadpool_limits(limits=1):
        mechanical=Body.mechanical(b['omega'],b['damping'],b['ports'])
        frequency=t['validation_frequency_hz']; indices=t['held_out_input_keys']-21
        mapping=b['mapping'][indices]
        static=np.vstack([b['ports']/b['omega'][:,None],np.zeros_like(b['ports'])])
        for record,timing in zip(pressure['rates'],runtime['rates']):
            rate=record['rate_hz'];dt=1/rate
            if rate!=timing['rate_hz'] or digest(record['coefficient_path'])!=record['coefficient_sha256']:
                raise ValueError('coefficient provenance mismatch')
            fitted=record['fitted_observer']
            if digest(fitted['model_path'])!=fitted['model_sha256']:
                raise ValueError('readout changed')
            with np.load(fitted['model_path']) as o:
                c,d,delays=o['state_readout'],o['force_readout'],o['delays']
                if np.intersect1d(o['training_frequency_hz'],frequency).size:
                    raise ValueError('validation leaked into observation fit')
                np.testing.assert_array_equal(delays,original['delays'])
            held=mechanical.held_step(rate)
            first=-dt*solve(np.eye(len(held.transition))-held.transition,static,check_finite=False)
            dc=c @ static+d
            first_residual=c @ first
            dc_relative=float(np.linalg.norm(dc)/max(np.linalg.norm(d),1e-30))
            slope_relative=float(np.linalg.norm(first_residual)/max(np.linalg.norm(c)*np.linalg.norm(first),1e-30))
            if dc_relative>1e-8 or slope_relative>1e-8:
                raise ValueError('pressure double zero at DC changed')
            fractional=delays*rate;integer=np.floor(fractional).astype(int)
            taps=np.sinc(np.arange(16)[None,None]-7-(fractional-integer)[:,:,None])*np.blackman(16)
            taps/=taps.sum(axis=2)[:,:,None]
            transfer=[];mobilities=[]
            for hz in frequency:
                z=np.exp(2j*np.pi*hz/rate)
                x=solve(z*np.eye(len(held.transition))-held.transition,held.drive,check_finite=False)
                groups=np.einsum('mgn,np->mgp',c,x)+d
                fir=np.sum(taps*z**(-(integer[:,:,None]+np.arange(16))),axis=2)
                p=np.sum(groups*fir[:,:,None],axis=1)*np.exp(2j*np.pi*hz*7/rate)
                transfer.append(p @ mapping.T)
                average=(held.integral_state @ x+held.integral_drive)/dt
                y=held.ports.T @ average
                mobilities.append(mapping @ y @ mapping.T)
            def metric(actual,target):
                df=np.r_[(frequency[1]-frequency[0])/2,(frequency[2:]-frequency[:-2])/2,(frequency[-1]-frequency[-2])/2]
                e=np.linalg.norm(np.asarray(actual)-target,axis=(1,2))**2
                n=np.linalg.norm(target,axis=(1,2))**2
                return dict(rms=float(np.sqrt(df @ e/(df @ n))),worst=float(np.sqrt(np.max(e/np.maximum(n,1e-30)))))
            ep=metric(transfer,t['pressure_validation_full'])
            ey=metric(mobilities,t['mobility_validation_full'])
            for actual,key in [(ep,'loaded_teacher_pressure'),(ey,'loaded_teacher_mobility')]:
                saved=record[key]
                np.testing.assert_allclose([actual['rms'],actual['worst']],[saved['rms'],saved['worst']],rtol=1e-6,atol=1e-9)
                if saved['passed']!=(actual['rms']<=.05 and actual['worst']<=.10):
                    raise ValueError('fidelity gate changed')
            for benchmark in timing['benchmarks']:
                passed=benchmark['p99_ms']<=.2*benchmark['deadline_ms'] and benchmark['max_ms']<=benchmark['deadline_ms'] and benchmark['allocations']==0
                if benchmark['body_and_pressure_20percent_budget_passed']!=passed:
                    raise ValueError('recorded CPU gate changed')
            rows.append(dict(rate_hz=rate,dc_pressure_relative=dc_relative,first_order_pressure_relative=slope_relative,
                real_exponential_pressure=ep,real_exponential_mobility=ey,training_validation_disjoint=True))
    result=dict(stage='real held-state pressure and mobility replay verified',verifier_sha256=digest(__file__),
        runtime_report_sha256=digest(args.runtime_report),pressure_report_sha256=digest(args.pressure_report),rates=rows,
        native_recorded_controls=4,gates_recomputed=True,full_piano_realtime=False,global_physics_approved=False,
        scope='Real augmented exponential against complex-pole harmonic realization; causal FIR, double DC zero, held-out teachers and recorded CPU gates. No original-air transient, live strings/contact/pedals or host callback.')
    args.report.write_text(json.dumps(result,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    print('real exponential pressure/mobility replay and physical DC moments passed')


if __name__=='__main__':
    main()
