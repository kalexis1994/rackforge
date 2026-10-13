"""Replay frozen training selection, weighted inner optimum and held validation."""
import importlib.util
import json
from pathlib import Path
import numpy as np
from scipy.linalg import solve
from concert_grand_storage_preserving_fit import operators
from concert_grand_residual_weighting import residual_importance
from concert_grand_realtime_body import modal_response
from concert_grand_causal_pressure_fit import acceleration_response,pressure_response
from concert_grand_wave_cache import digest

spec = importlib.util.spec_from_file_location('metric_parent',Path(__file__).with_name('fit-concert-grand-loaded-bank.py'))
metrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(metrics)


def main():
    root = Path(__file__).resolve().parent.parent
    folder = root/'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P7-cuerpo-cargado-2026-10-12'
    names = ['maestro','acoplado-4-woodbury','almacenamiento-4-directo','presion-almacenamiento-4','conjunto-161',
        'variable-161','presion-conjunto-161-refit','variable-161-observador-completo',
        'residual-161','residual-161-observador-completo','runtime-residual-161']
    records = {name:json.loads((folder/(name+'.json')).read_text()) for name in names}
    hashes = {name:digest(folder/(name+'.json')) for name in names}
    for name,record in records.items():
        for source,sha in record.get('source_sha256',{}).items():
            if digest(root/'tools'/source)!=sha:
                raise ValueError('source changed')
        for path,sha in [('data_path','data_sha256'),('model_path','model_sha256'),
                         ('selection_path','selection_sha256'),('checkpoint_path','checkpoint_sha256')]:
            if path in record and digest(record[path])!=record[sha]:
                raise ValueError('artifact changed')
        for key in ['teacher_report_sha256','body_report_sha256','base_report_sha256','resume_report_sha256','observer_report_sha256']:
            if key in record and record[key] not in hashes.values():
                raise ValueError('report provenance mismatch')
        if any(record.get(key) for key in ['factory_updated','full_piano_realtime','global_physics_approved']):
            raise ValueError('diagnostic scope exceeded')
    record = records['residual-161']
    with (np.load(record['model_path']) as b,np.load(record['checkpoint_path']) as cp,
          np.load(record['selection_path']) as selection,np.load(records['maestro']['data_path']) as data,
          np.load(records['conjunto-161']['model_path']) as base,
          np.load(records['variable-161']['model_path']) as previous,
          np.load(records['variable-161-observador-completo']['model_path']) as previous_observer):
        np.testing.assert_array_equal(data['training_frequency_hz'],selection['frequency'])
        indices = selection['indices']
        old_indices = np.searchsorted(selection['frequency'],np.asarray(records['variable-161']['fit']['fit_frequency_hz']))
        expected,by,bp = residual_importance(selection['frequency'],selection['before_error_y'],selection['before_error_p'],old_indices)
        np.testing.assert_array_equal(indices,expected)
        np.testing.assert_array_equal(selection['boost_y'],by)
        np.testing.assert_array_equal(selection['boost_p'],bp)
        if np.intersect1d(selection['frequency'][indices],data['validation_frequency_hz']).size:
            raise ValueError('frequency leakage')
        f = selection['frequency']
        s = 2j*np.pi*f
        q = np.array([solve(previous['raw_stiffness']+value*previous['raw_damping']+value*value*np.eye(len(base['omega'])),
                            base['ports'],check_finite=False) for value in s])
        y = s[:,None,None]*np.einsum('na,fnb->fab',base['ports'],q,optimize=True)
        phase = np.exp(-s[:,None,None]*previous['pressure_delays'][None])
        p = np.einsum('fmg,mgn,fnp->fmp',phase,previous_observer['raw_readout'],s[:,None,None]**2*q,optimize=True)
        for actual,key,target in [(y,'before_error_y',data['mobility_training']),
                                  (p,'before_error_p',data['pressure_training'])]:
            error = np.linalg.norm(actual-target,axis=(1,2))/np.maximum(np.linalg.norm(target,axis=(1,2)),1e-30)
            np.testing.assert_allclose(error,selection[key],rtol=1e-8,atol=1e-11)
        basis = cp['basis']
        np.testing.assert_allclose(basis.T @ basis,np.eye(basis.shape[1]),atol=2e-12,rtol=0)
        null_relative = float(np.linalg.norm(basis.T @ (base['ports']/base['omega'][:,None]))/np.linalg.norm(base['ports']/base['omega'][:,None]))
        if null_relative>1e-12:
            raise ValueError('storage complement changed')
        k,c,_,_ = operators(cp['parameters'],base['omega'],base['damping'],basis,8)
        np.testing.assert_allclose(k,b['raw_stiffness'],rtol=1e-13,atol=1e-10)
        np.testing.assert_allclose(c,b['raw_damping'],rtol=1e-13,atol=1e-10)
        np.testing.assert_array_equal(cp['readout_scale'],previous['readout_scale'])
        np.testing.assert_array_equal(b['readout_scale'],previous['readout_scale'])
        np.testing.assert_array_equal(cp['readout'],b['raw_readout'])
        loss_min = float(np.linalg.eigvalsh(b['damping']).min())
        if loss_min<=0:
            raise ValueError('positive loss missing')
        tf = f[indices]
        ts = 2j*np.pi*tf
        q = np.array([solve(k+value*c+value*value*np.eye(len(k)),base['ports'],check_finite=False) for value in ts])
        acc = ts[:,None,None]**2*q
        target = data['pressure_training'][indices]
        df = np.r_[(tf[1]-tf[0])/2,(tf[2:]-tf[:-2])/2,(tf[-1]-tf[-2])/2]
        weight = df/df.sum()/np.maximum(np.linalg.norm(target,axis=(1,2))**2,1e-30)
        boost = bp[indices]
        weight *= boost/(df @ boost/df.sum())
        stationarity = []
        for m in range(target.shape[1]):
            phase = np.exp(-ts[:,None]*b['pressure_delays'][m])
            design = np.concatenate([acc.transpose(0,2,1)*phase[:,g,None,None] for g in range(phase.shape[1])],axis=2)
            design = design.reshape(len(tf)*q.shape[2],-1)*np.repeat(np.sqrt(weight),q.shape[2])[:,None]
            design /= b['readout_scale'][m]
            beta = b['raw_readout'][m].ravel()*b['readout_scale'][m]
            rhs = (target[:,m]*np.sqrt(weight)[:,None]).reshape(-1)
            normal = (design.conj().T @ (design @ beta-rhs)).real+record['fit']['readout_ridge']*beta
            defect = float(np.linalg.norm(normal)/max(np.linalg.norm((design.conj().T @ rhs).real),1e-30))
            if defect>1e-8:
                raise ValueError('weighted inner optimum missing')
            stationarity.append(defect)
        vf = data['validation_frequency_hz']
        rows = data['mapping'][data['held_out_input_keys']-21]
        y = modal_response(b['omega'],b['damping'],b['ports'],vf)
        ey = metrics.metric(vf,np.einsum('pk,fkl,ql->fpq',rows,y,rows,optimize=True),data['mobility_validation_full'])
        acc = acceleration_response(b['omega'],b['damping'],b['ports'],vf)
        p = pressure_response(acc,vf,b['pressure_delays'],b['pressure_readout'])
        ep = metrics.metric(vf,np.einsum('fmk,pk->fmp',p,rows,optimize=True),data['pressure_validation_full'])
        static = b['ports'].T @ (b['ports']/b['omega'][:,None]**2)
        high = b['ports'].T @ b['ports']
        moments = [float(np.linalg.norm(value-data[key])/np.linalg.norm(data[key])) for value,key in [(static,'static_compliance_latent'),(high,'high_inertance_latent')]]
        moment_gate = max(*moments,record['stiffness_mode_reconstruction_relative'],record['pressure_mode_reconstruction_relative'])<1e-7
        for error,key,gate in [(ey,'full_external_validation','mobility_gate_passed'),(ep,'full_external_pressure_validation','pressure_gate_passed')]:
            expected = record[key]
            np.testing.assert_allclose([error['rms'],error['worst']],[expected['rms'],expected['worst']],rtol=1e-8,atol=1e-12)
            if record[gate]!=(error['rms']<=.05 and error['worst']<=.10 and moment_gate):
                raise ValueError('fidelity gate drift')
        full_record = records['residual-161-observador-completo']
        with np.load(full_record['model_path']) as observer:
            np.testing.assert_array_equal(observer['readout_scale'],previous['readout_scale'])
            p = pressure_response(acc,vf,observer['delays'],observer['readout'])
            full_error = metrics.metric(vf,np.einsum('fmk,pk->fmp',p,rows,optimize=True),data['pressure_validation_full'])
        expected = full_record['full_external_validation']
        np.testing.assert_allclose([full_error['rms'],full_error['worst']],[expected['rms'],expected['worst']],rtol=1e-8,atol=1e-12)
        if full_record['pressure_gate_passed']!=(full_error['rms']<=.05 and full_error['worst']<=.10):
            raise ValueError('full observer gate drift')
    native = records['runtime-residual-161']
    for rate in native['rates']:
        if digest(rate['coefficient_path'])!=rate['coefficient_sha256']:
            raise ValueError('native bundle changed')
        check,bench = rate['verification'],rate['benchmark']
        if not check['native_python_final_states_outputs_and_work_passed'] or check['python_reconstructed_step_energy_excess_relative']>1e-10:
            raise ValueError('native state/work failed')
        passed = bench['p99_ms']/bench['deadline_ms']<=.20 and bench['max_ms']<=bench['deadline_ms'] and bench['callback_allocations']==0
        if passed!=bench['observed_body_budget_passed']:
            raise ValueError('CPU gate drift')
    result = dict(stage='residual selection and weighted variable replay verified',verifier_sha256=digest(__file__),
        reports_sha256=hashes,baseline_training_residuals_replayed=True,selection_and_importance_reconstructed=True,
        storage_complement_relative=null_relative,static_and_high_relative=moments,minimum_loss_eigenvalue=loss_min,
        weighted_inner_stationarity_relative=stationarity,mobility=dict(rms=ey['rms'],worst=ey['worst']),
        pressure=dict(rms=ep['rms'],worst=ep['worst']),all_training_pressure=dict(rms=full_error['rms'],worst=full_error['worst']),
        native_recorded_controls=2,gates_recomputed=True,full_piano_realtime=False,
        scope='Unscaled harmonic replay, training-only selection/normal equations and recorded native work. No original-air time, sampled pressure or whole-piano certificate.')
    (folder/'verificacion-residual-161.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print('residual replay passed; training selection, weighted optimum and gates verified')


if __name__=='__main__':
    main()
