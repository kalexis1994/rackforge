"""Resume positive dynamics on enriched, frozen training-residual weights."""
import argparse
import importlib.util
import json
from pathlib import Path
import time
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_residual_weighting import ResidualPressureProblem,residual_importance
from concert_grand_variable_projection import fit_variable
from concert_grand_joint_pressure_fit import responses,readout_transfer
from concert_grand_wave_cache import digest

spec = importlib.util.spec_from_file_location('joint_parent',Path(__file__).with_name('fit-concert-grand-joint-pressure.py'))
prior = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prior)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['teacher-report','base-report','resume-report','observer-report','report','local']:
        parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--iterations',type=int,default=200)
    a = parser.parse_args()
    if a.report.exists() or a.local.exists() or not 1<=a.iterations<=800:
        raise ValueError('new residual experiment paths required')
    root = Path(__file__).resolve().parent.parent
    teacher,base,resume,observer = [json.loads(p.read_text()) for p in [a.teacher_report,a.base_report,a.resume_report,a.observer_report]]
    for r,key,sha in [(teacher,'data_path','data_sha256'),(base,'model_path','model_sha256'),
                      (resume,'model_path','model_sha256'),(observer,'model_path','model_sha256')]:
        if digest(r[key])!=r[sha]:
            raise ValueError('frozen data changed')
        for name,source_sha in r['source_sha256'].items():
            if digest(root/'tools'/name)!=source_sha:
                raise ValueError('frozen source changed')
    if (resume['body_report_sha256']!=digest(a.base_report) or observer['body_report_sha256']!=digest(a.resume_report)
            or digest(resume['checkpoint_path'])!=resume['checkpoint_sha256']):
        raise ValueError('resume/observation provenance mismatch')
    names = [Path(__file__).name,'concert_grand_residual_weighting.py','concert_grand_variable_projection.py',
             'concert_grand_joint_pressure_fit.py','concert_grand_storage_preserving_fit.py','fit-concert-grand-joint-pressure.py']
    hashes = {n:digest(root/'tools'/n) for n in names}
    a.local.mkdir(parents=True)
    started = time.perf_counter()
    record = dict(stage='preparing residual importance',source_sha256=hashes,
        teacher_report_sha256=digest(a.teacher_report),base_report_sha256=digest(a.base_report),
        resume_report_sha256=digest(a.resume_report),observer_report_sha256=digest(a.observer_report),
        training_only_selection=True,importance_frozen_before_fit=True,readout_scale_inherited=True,
        full_piano_realtime=False,global_physics_approved=False,factory_updated=False)
    def save():
        record['seconds'] = time.perf_counter()-started
        a.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    save()
    checkpoint_path = a.local/'checkpoint.npz'
    def checkpoint(x,basis,scale,readout,row):
        temp = a.local/'checkpoint-tmp.npz'
        np.savez_compressed(temp,parameters=x,basis=basis,readout_scale=scale,readout=readout)
        temp.replace(checkpoint_path)
        record.update(checkpoint_path=str(checkpoint_path.resolve()),checkpoint_sha256=digest(checkpoint_path),progress=row)
        save()
    def progress(row):
        print('residual importance fit',row,flush=True)
    with (np.load(teacher['data_path']) as data,np.load(base['model_path']) as b,np.load(resume['model_path']) as previous,
          np.load(resume['checkpoint_path']) as old_checkpoint,np.load(observer['model_path']) as o,
          threadpool_limits(limits=1,user_api='blas')):
        f,y,p = data['training_frequency_hz'],data['mobility_training'],data['pressure_training']
        omega,ports,c0 = b['omega'],b['ports'],b['damping']
        np.testing.assert_array_equal(previous['raw_omega'],omega)
        np.testing.assert_array_equal(previous['raw_ports'],ports)
        np.testing.assert_array_equal(o['readout_scale'],previous['readout_scale'])
        _,_,before_y,before_p = responses(previous['raw_stiffness'],previous['raw_damping'],omega,ports,
            f,readout_transfer(f,previous['pressure_delays'],o['raw_readout']))
        ey = np.linalg.norm(before_y-y,axis=(1,2))/np.maximum(np.linalg.norm(y,axis=(1,2)),1e-30)
        ep = np.linalg.norm(before_p-p,axis=(1,2))/np.maximum(np.linalg.norm(p,axis=(1,2)),1e-30)
        old_indices = np.searchsorted(f,np.asarray(resume['fit']['fit_frequency_hz']))
        selected,boost_y,boost_p = residual_importance(f,ey,ep,old_indices)
        selection_path = a.local/'selection.npz'
        np.savez_compressed(selection_path,indices=selected,boost_y=boost_y,boost_p=boost_p,
                            before_error_y=ey,before_error_p=ep,frequency=f)
        record.update(selection_path=str(selection_path.resolve()),selection_sha256=digest(selection_path),
            fit_frequency_count=len(selected),previous_fit_frequency_count=len(old_indices),
            baseline_training_max_mobility=float(ey.max()),baseline_training_max_pressure=float(ep.max()),
            importance_thresholds=dict(mobility=.07,pressure=.05),importance_bounds=[1.,8.],bins=24)
        save()
        print('residual selection',len(old_indices),'->',len(selected),'training frequencies; maxima',ey.max(),ep.max(),flush=True)
        problem = ResidualPressureProblem(omega,ports,c0,old_checkpoint['basis'],f[selected],y[selected],p[selected],
            previous['pressure_delays'],fixed_scale=previous['readout_scale'],boost_y=boost_y[selected],boost_p=boost_p[selected],
            readout_ridge=resume['fit']['readout_ridge'],parameter_ridge=resume['fit']['parameter_ridge'])
        k,c,readout,details = fit_variable(problem,old_checkpoint['parameters'],a.iterations,checkpoint,progress)
        new_omega,vectors = prior.split_stiffness_modes(k,omega)
        new_c,new_ports = vectors.T @ c @ vectors,vectors.T @ ports
        new_readout = np.einsum('mgn,nj->mgj',readout,vectors,optimize=True)
        vf = data['validation_frequency_hz']
        _,_,raw_y,raw_p = responses(k,c,omega,ports,vf,readout_transfer(vf,previous['pressure_delays'],readout))
        _,_,actual_y,actual_p = responses(np.diag(new_omega**2),new_c,new_omega,new_ports,vf,
            readout_transfer(vf,previous['pressure_delays'],new_readout))
        equiv_y = float(np.linalg.norm(actual_y-raw_y)/np.linalg.norm(raw_y))
        equiv_p = float(np.linalg.norm(actual_p-raw_p)/np.linalg.norm(raw_p))
        mapping = data['mapping']
        rows = mapping[data['held_out_input_keys']-21]
        error_y = prior.metrics.metric(vf,np.einsum('pk,fkl,ql->fpq',rows,actual_y,rows,optimize=True),data['mobility_validation_full'])
        error_p = prior.metrics.metric(vf,np.einsum('fmk,pk->fmp',actual_p,rows,optimize=True),data['pressure_validation_full'])
        static_error = float(np.linalg.norm(new_ports.T @ (new_ports/new_omega[:,None]**2)-data['static_compliance_latent'])/np.linalg.norm(data['static_compliance_latent']))
        high_error = float(np.linalg.norm(new_ports.T @ new_ports-data['high_inertance_latent'])/np.linalg.norm(data['high_inertance_latent']))
        _,_,after_y,after_p = responses(k,c,omega,ports,f,readout_transfer(f,previous['pressure_delays'],readout))
        after_ey = np.linalg.norm(after_y-y,axis=(1,2))/np.maximum(np.linalg.norm(y,axis=(1,2)),1e-30)
        after_ep = np.linalg.norm(after_p-p,axis=(1,2))/np.maximum(np.linalg.norm(p,axis=(1,2)),1e-30)
        delays = previous['pressure_delays']
    model_path = a.local/'weighted-model.npz'
    np.savez_compressed(model_path,omega=new_omega,damping=new_c,ports=new_ports,mapping=mapping,
        pressure_delays=delays,pressure_readout=new_readout,raw_stiffness=k,raw_damping=c,raw_omega=omega,
        raw_ports=ports,raw_readout=readout,readout_scale=problem.scale)
    moment_gate = max(static_error,high_error,equiv_y,equiv_p)<1e-7
    record.update(stage='residual-weighted variable dynamics fitted',fit=details,modes=len(new_omega),
        model_path=str(model_path.resolve()),model_sha256=digest(model_path),model_bytes=model_path.stat().st_size,
        static_moment_relative=static_error,high_moment_relative=high_error,
        stiffness_mode_reconstruction_relative=equiv_y,pressure_mode_reconstruction_relative=equiv_p,
        minimum_loss_eigenvalue=float(np.linalg.eigvalsh(new_c).min()),
        after_training_max_mobility=float(after_ey.max()),after_training_max_pressure=float(after_ep.max()),
        full_external_validation=error_y,full_external_pressure_validation=error_p,
        mobility_gate_passed=error_y['rms']<=.05 and error_y['worst']<=.10 and moment_gate,
        pressure_gate_passed=error_p['rms']<=.05 and error_p['worst']<=.10 and moment_gate,
        source_unchanged=all(digest(root/'tools'/n)==sha for n,sha in hashes.items()))
    record['both_fidelity_gates_passed'] = record['mobility_gate_passed'] and record['pressure_gate_passed']
    save()
    print('residual fit complete',record['seconds'],'s; mobility',error_y['rms'],error_y['worst'],
          'pressure',error_p['rms'],error_p['worst'],flush=True)


if __name__=='__main__':
    main()
