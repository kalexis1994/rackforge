"""Identify positive loaded dynamics and optimal causal microphones jointly."""
import argparse
import importlib.util
import json
from pathlib import Path
import time
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_variable_projection import VariablePressureProblem,fit_variable
from concert_grand_storage_preserving_fit import storage_basis
from concert_grand_joint_pressure_fit import training_indices,responses,readout_transfer
from concert_grand_wave_cache import digest

spec = importlib.util.spec_from_file_location('joint_parent',Path(__file__).with_name('fit-concert-grand-joint-pressure.py'))
parent_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parent_module)
split_stiffness_modes,metrics = parent_module.split_stiffness_modes,parent_module.metrics


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['teacher-report','body-report','observer-report','report','local']:
        parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--iterations',type=int,default=300)
    args = parser.parse_args()
    if args.report.exists() or args.local.exists() or not 1<=args.iterations<=800:
        raise ValueError('new experiment paths and bounded iterations required')
    root = Path(__file__).resolve().parent.parent
    teacher,body,observer = [json.loads(path.read_text()) for path in [args.teacher_report,args.body_report,args.observer_report]]
    for record,key,sha in [(teacher,'data_path','data_sha256'),(body,'model_path','model_sha256'),(observer,'model_path','model_sha256')]:
        if digest(record[key])!=record[sha]:
            raise ValueError('frozen parent data changed')
        for name,source_sha in record['source_sha256'].items():
            if digest(root/'tools'/name)!=source_sha:
                raise ValueError('frozen parent source changed')
    if observer['body_report_sha256']!=digest(args.body_report):
        raise ValueError('observer belongs to a different body')
    names = [Path(__file__).name,'concert_grand_variable_projection.py','concert_grand_storage_preserving_fit.py',
             'concert_grand_joint_pressure_fit.py','fit-concert-grand-joint-pressure.py','fit-concert-grand-loaded-bank.py']
    hashes = {name:digest(root/'tools'/name) for name in names}
    args.local.mkdir(parents=True)
    started = time.perf_counter()
    report = dict(stage='preparing variable projection pressure fit',source_sha256=hashes,
        teacher_report_sha256=digest(args.teacher_report),body_report_sha256=digest(args.body_report),
        observer_report_sha256=digest(args.observer_report),full_piano_realtime=False,
        global_physics_approved=False,auditory_approval=False,factory_updated=False,
        scope='161 modes with real causal optimal observation every dynamic evaluation. Fixed scaling/ridge. Harmonic identification only.')
    def save():
        report['seconds'] = time.perf_counter()-started
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    save()
    checkpoint_path = args.local/'checkpoint.npz'
    def checkpoint(x,basis,scale,readout,row):
        temporary = args.local/'checkpoint-tmp.npz'
        np.savez_compressed(temporary,parameters=x,basis=basis,readout_scale=scale,readout=readout)
        temporary.replace(checkpoint_path)
        report.update(checkpoint_path=str(checkpoint_path.resolve()),checkpoint_sha256=digest(checkpoint_path),progress=row)
        save()
    def progress(row):
        print('variable projection fit',row,flush=True)
    with np.load(teacher['data_path']) as data,np.load(body['model_path']) as b,np.load(observer['model_path']) as o:
        omega,ports,base = b['omega'],b['ports'],b['damping']
        delays = o['delays']
        f,y,p = data['training_frequency_hz'],data['mobility_training'],data['pressure_training']
        with threadpool_limits(limits=1,user_api='blas'):
            basis = storage_basis(omega,ports,base,f)
            selected = training_indices(f,y,p)
            initial = np.r_[np.zeros(basis.shape[1]+len(omega)),
                np.random.default_rng(31).normal(0.,.001,(len(omega),8)).ravel()]
            problem = VariablePressureProblem(omega,ports,base,basis,f[selected],y[selected],p[selected],delays)
            problem.prepare(initial)
        costs,reference = {},None
        for threads in [1,6]:
            with threadpool_limits(limits=threads,user_api='blas'):
                value,gradient,parts,initial_readout = problem.evaluate(initial)
                if reference is None:
                    reference = value,gradient,parts,initial_readout
                else:
                    np.testing.assert_allclose(value,reference[0],rtol=1e-8,atol=1e-12)
                    np.testing.assert_allclose(gradient,reference[1],rtol=1e-6,atol=1e-9)
                timings = []
                for _ in range(2):
                    begin = time.perf_counter()
                    problem.evaluate(initial)
                    timings.append(time.perf_counter()-begin)
                costs[threads] = timings
        threads = min(costs,key=lambda n:np.median(costs[n]))
        report.update(offline_thread_cost_seconds=costs,selected_blas_threads=threads,fit_frequency_count=len(selected),
            initial_objective=reference[0],initial_parts=reference[2],fixed_seed=31)
        print('variable preparation',len(selected),'frequencies; BLAS',threads,costs,flush=True)
        save()
        with threadpool_limits(limits=threads,user_api='blas'):
            k,c,readout,details = fit_variable(problem,initial,args.iterations,checkpoint,progress)
            frequency = data['validation_frequency_hz']
            _,_,raw_y,raw_p = responses(k,c,omega,ports,frequency,readout_transfer(frequency,delays,readout))
            new_omega,vectors = split_stiffness_modes(k,omega)
            new_c,new_ports = vectors.T @ c @ vectors,vectors.T @ ports
            new_readout = np.einsum('mgn,nj->mgj',readout,vectors,optimize=True)
            _,_,actual_y,actual_p = responses(np.diag(new_omega**2),new_c,new_omega,new_ports,frequency,
                readout_transfer(frequency,delays,new_readout))
            equivalent_y = float(np.linalg.norm(actual_y-raw_y)/np.linalg.norm(raw_y))
            equivalent_p = float(np.linalg.norm(actual_p-raw_p)/np.linalg.norm(raw_p))
            mapping = data['mapping']
            rows = mapping[data['held_out_input_keys']-21]
            ey = metrics.metric(frequency,np.einsum('pk,fkl,ql->fpq',rows,actual_y,rows,optimize=True),data['mobility_validation_full'])
            ep = metrics.metric(frequency,np.einsum('fmk,pk->fmp',actual_p,rows,optimize=True),data['pressure_validation_full'])
            _,_,_,initial_p = responses(np.diag(omega**2),base,omega,ports,frequency,
                readout_transfer(frequency,delays,reference[3]))
            initial_ep = metrics.metric(frequency,np.einsum('fmk,pk->fmp',initial_p,rows,optimize=True),data['pressure_validation_full'])
            static_error = float(np.linalg.norm(new_ports.T @ (new_ports/new_omega[:,None]**2)-data['static_compliance_latent'])/np.linalg.norm(data['static_compliance_latent']))
            high_error = float(np.linalg.norm(new_ports.T @ new_ports-data['high_inertance_latent'])/np.linalg.norm(data['high_inertance_latent']))
    model_path = args.local/'variable-model.npz'
    np.savez_compressed(model_path,omega=new_omega,damping=new_c,ports=new_ports,mapping=mapping,
        pressure_delays=delays,pressure_readout=new_readout,raw_stiffness=k,raw_damping=c,
        raw_omega=omega,raw_ports=ports,raw_readout=readout,readout_scale=problem.scale)
    moment_gate = max(static_error,high_error,equivalent_y,equivalent_p)<1e-7
    report.update(stage='variable projection pressure dynamics fitted',fit=details,modes=len(new_omega),
        model_path=str(model_path.resolve()),model_sha256=digest(model_path),model_bytes=model_path.stat().st_size,
        minimum_loss_eigenvalue=float(np.linalg.eigvalsh(new_c).min()),
        static_moment_relative=static_error,high_moment_relative=high_error,
        stiffness_mode_reconstruction_relative=equivalent_y,pressure_mode_reconstruction_relative=equivalent_p,
        initial_subset_observer_pressure_validation=initial_ep,
        full_external_validation=ey,full_external_pressure_validation=ep,
        mobility_gate_passed=ey['rms']<=.05 and ey['worst']<=.10 and moment_gate,
        pressure_gate_passed=ep['rms']<=.05 and ep['worst']<=.10 and moment_gate,
        source_unchanged=all(digest(root/'tools'/name)==sha for name,sha in hashes.items()))
    report['both_fidelity_gates_passed'] = report['mobility_gate_passed'] and report['pressure_gate_passed']
    save()
    print('variable fit complete',report['seconds'],'s; mobility',ey['rms'],ey['worst'],'pressure',ep['rms'],ep['worst'],flush=True)


if __name__=='__main__':
    main()
