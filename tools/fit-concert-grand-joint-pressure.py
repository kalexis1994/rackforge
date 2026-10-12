"""Joint loaded-port/pressure identification with positive storage dynamics."""
import argparse
import importlib.util
import json
from pathlib import Path
import time
import numpy as np
from scipy.linalg import eigh
from threadpoolctl import threadpool_limits
from concert_grand_joint_pressure_fit import (fit_joint,responses,readout_transfer,objective_gradient,training_indices)
from concert_grand_storage_preserving_fit import storage_basis
from concert_grand_wave_cache import digest

spec = importlib.util.spec_from_file_location('loaded_metrics',Path(__file__).with_name('fit-concert-grand-loaded-bank.py'))
metrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(metrics)


def split_stiffness_modes(k,initial_omega):
    active = initial_omega<=2*np.pi*30000.
    indices = np.flatnonzero(active)
    vectors = np.eye(len(k))
    values = initial_omega**2
    new_values,new_vectors = eigh(k[np.ix_(indices,indices)],driver='evd')
    if new_values[0]<=0 or np.linalg.norm(k[np.ix_(active,~active)])>1e-12:
        raise ValueError('positive active stiffness and unchanged stiff complement required')
    vectors[np.ix_(indices,indices)] = new_vectors
    values[indices] = new_values
    return np.sqrt(values),vectors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--teacher-report',type=Path,required=True)
    parser.add_argument('--body-report',type=Path,required=True)
    parser.add_argument('--observer-report',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--local',type=Path,required=True)
    parser.add_argument('--iterations',type=int,default=400)
    args = parser.parse_args()
    if args.report.exists() or args.local.exists() or not 1<=args.iterations<=800:
        raise ValueError('new joint experiment paths and bounded iterations required')
    root = Path(__file__).resolve().parent.parent
    teacher,body,observer = [json.loads(path.read_text()) for path in [args.teacher_report,args.body_report,args.observer_report]]
    for record,key,sha in [(teacher,'data_path','data_sha256'),(body,'model_path','model_sha256'),(observer,'model_path','model_sha256')]:
        if digest(record[key])!=record[sha]:
            raise ValueError('frozen parent data changed')
        for name,source_sha in record['source_sha256'].items():
            if digest(root/'tools'/name)!=source_sha:
                raise ValueError('frozen parent source changed')
    if observer['body_report_sha256']!=digest(args.body_report):
        raise ValueError('pressure observation belongs to another body')
    names = [Path(__file__).name,'concert_grand_joint_pressure_fit.py','concert_grand_storage_preserving_fit.py',
             'fit-concert-grand-loaded-bank.py']
    hashes = {name:digest(root/'tools'/name) for name in names}
    args.local.mkdir(parents=True)
    started = time.perf_counter()
    report = dict(stage='preparing joint pressure/mobility fit',source_sha256=hashes,
        teacher_report_sha256=digest(args.teacher_report),body_report_sha256=digest(args.body_report),
        observer_report_sha256=digest(args.observer_report),full_piano_realtime=False,
        global_physics_approved=False,auditory_approval=False,factory_updated=False,
        scope='Same 161-mode family with fixed real causal observation; pressure included in dynamics objective. Harmonic validation only.')
    def save():
        report['seconds'] = time.perf_counter()-started
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    save()
    checkpoint_path = args.local/'checkpoint.npz'
    def checkpoint(x,basis,row):
        temporary = args.local/'checkpoint-tmp.npz'
        np.savez_compressed(temporary,parameters=x,basis=basis)
        temporary.replace(checkpoint_path)
        report['checkpoint_sha256'] = digest(checkpoint_path)
        report['checkpoint_path'] = str(checkpoint_path.resolve())
        report['progress'] = row
        save()
    def progress(row):
        print('joint loaded fit',row,flush=True)
    with np.load(teacher['data_path']) as data,np.load(body['model_path']) as b,np.load(observer['model_path']) as o:
        omega,ports,base = b['omega'],b['ports'],b['damping']
        delays,readout = o['delays'],o['readout']
        f,y,p = data['training_frequency_hz'],data['mobility_training'],data['pressure_training']
        # Measure the same initial objective at one and six BLAS threads.
        with threadpool_limits(limits=1,user_api='blas'):
            basis = storage_basis(omega,ports,base,f)
            selected = training_indices(f,y,p)
            initial = np.r_[np.zeros(basis.shape[1]+len(omega)),
                np.random.default_rng(23).normal(0.,.005,(len(omega),8)).ravel()]
            arguments = (omega,ports,base,basis,f[selected],y[selected],p[selected],readout_transfer(f[selected],delays,readout),8)
        thread_costs,reference = {},None
        for threads in [1,6]:
            with threadpool_limits(limits=threads,user_api='blas'):
                actual = objective_gradient(initial,*arguments)
                if reference is None:
                    reference = actual
                else:
                    np.testing.assert_allclose(actual[0],reference[0],rtol=1e-9,atol=1e-12)
                    np.testing.assert_allclose(actual[1],reference[1],rtol=1e-7,atol=1e-10)
                timings = []
                for _ in range(3):
                    begin = time.perf_counter()
                    objective_gradient(initial,*arguments)
                    timings.append(time.perf_counter()-begin)
                thread_costs[threads] = timings
        threads = min(thread_costs,key=lambda n:np.median(thread_costs[n]))
        report.update(offline_thread_cost_seconds=thread_costs,selected_blas_threads=threads,
            fit_frequency_count=len(selected),initial_objective=reference[0])
        save()
        print('joint fit preparation',len(selected),'frequencies; BLAS',threads,thread_costs,flush=True)
        k,c,details = fit_joint(omega,ports,base,f,y,p,delays,readout,max_iterations=args.iterations,
            progress=progress,checkpoint=checkpoint,blas_threads=threads)
        with threadpool_limits(limits=threads,user_api='blas'):
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
            external_y = np.einsum('pk,fkl,ql->fpq',rows,actual_y,rows,optimize=True)
            external_p = np.einsum('fmk,pk->fmp',actual_p,rows,optimize=True)
            ey = metrics.metric(frequency,external_y,data['mobility_validation_full'])
            ep = metrics.metric(frequency,external_p,data['pressure_validation_full'])
            static_error = float(np.linalg.norm(new_ports.T @ (new_ports/new_omega[:,None]**2)-data['static_compliance_latent'])/np.linalg.norm(data['static_compliance_latent']))
            high_error = float(np.linalg.norm(new_ports.T @ new_ports-data['high_inertance_latent'])/np.linalg.norm(data['high_inertance_latent']))
    model_path = args.local/'joint-model.npz'
    np.savez_compressed(model_path,omega=new_omega,damping=new_c,ports=new_ports,mapping=mapping,
        pressure_delays=delays,pressure_readout=new_readout,
        raw_stiffness=k,raw_damping=c,raw_omega=omega,raw_ports=ports,raw_readout=readout)
    moment_gate = max(static_error,high_error,equivalent_y,equivalent_p)<1e-7
    report.update(stage='joint pressure/mobility dynamics fitted',fit=details,modes=len(new_omega),
        model_path=str(model_path.resolve()),model_sha256=digest(model_path),model_bytes=model_path.stat().st_size,
        minimum_loss_eigenvalue=float(np.linalg.eigvalsh(new_c).min()),
        static_moment_relative=static_error,high_moment_relative=high_error,
        stiffness_mode_reconstruction_relative=equivalent_y,pressure_mode_reconstruction_relative=equivalent_p,
        full_external_validation=ey,full_external_pressure_validation=ep,
        mobility_gate_passed=ey['rms']<=.05 and ey['worst']<=.10 and moment_gate,
        pressure_gate_passed=ep['rms']<=.05 and ep['worst']<=.10 and moment_gate,
        source_unchanged=all(digest(root/'tools'/name)==sha for name,sha in hashes.items()))
    report['both_fidelity_gates_passed'] = report['mobility_gate_passed'] and report['pressure_gate_passed']
    save()
    print('joint fit complete',report['seconds'],'s; mobility',ey['rms'],ey['worst'],'pressure',ep['rms'],ep['worst'],flush=True)


if __name__ == '__main__':
    main()
