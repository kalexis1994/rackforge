"""Authenticate and replay variable-projection fit using unscaled solves."""
import importlib.util
import json
from pathlib import Path
import numpy as np
from scipy.linalg import solve
from concert_grand_storage_preserving_fit import operators
from concert_grand_realtime_body import modal_response
from concert_grand_causal_pressure_fit import acceleration_response,pressure_response
from concert_grand_wave_cache import digest

spec = importlib.util.spec_from_file_location('loaded_metrics',Path(__file__).with_name('fit-concert-grand-loaded-bank.py'))
metrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(metrics)


def main():
    root = Path(__file__).resolve().parent.parent
    folder = root/'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P7-cuerpo-cargado-2026-10-12'
    names = ['maestro','almacenamiento-4-directo','presion-almacenamiento-4','conjunto-161',
             'presion-conjunto-161-refit','variable-161','variable-161-observador-completo','runtime-variable-161']
    records = {name:json.loads((folder/(name+'.json')).read_text()) for name in names}
    hashes = {name:digest(folder/(name+'.json')) for name in names}
    for name,record in records.items():
        for source,sha in record.get('source_sha256',{}).items():
            if digest(root/'tools'/source)!=sha:
                raise ValueError('measured source changed')
        for path,sha in [('data_path','data_sha256'),('model_path','model_sha256'),('checkpoint_path','checkpoint_sha256')]:
            if path in record and digest(record[path])!=record[sha]:
                raise ValueError('artifact changed')
        for key in ['teacher_report_sha256','body_report_sha256','observer_report_sha256']:
            if key in record and record[key] not in hashes.values():
                raise ValueError('provenance broken')
        if any(record.get(k) for k in ['full_piano_realtime','global_physics_approved','factory_updated']):
            raise ValueError('scope exceeds diagnostic')
    record = records['variable-161']
    with np.load(record['model_path']) as b,np.load(record['checkpoint_path']) as checkpoint,np.load(records['maestro']['data_path']) as data,np.load(records['conjunto-161']['model_path']) as parent:
        basis = checkpoint['basis']
        np.testing.assert_allclose(basis.T @ basis,np.eye(basis.shape[1]),atol=2e-12,rtol=0)
        null_relative = float(np.linalg.norm(basis.T @ (parent['ports']/parent['omega'][:,None]))/np.linalg.norm(parent['ports']/parent['omega'][:,None]))
        if null_relative>1e-12:
            raise ValueError('static complement changed')
        k,c,_,_ = operators(checkpoint['parameters'],parent['omega'],parent['damping'],basis,8)
        np.testing.assert_allclose(k,b['raw_stiffness'],rtol=1e-13,atol=1e-10)
        np.testing.assert_allclose(c,b['raw_damping'],rtol=1e-13,atol=1e-10)
        np.testing.assert_array_equal(checkpoint['readout_scale'],b['readout_scale'])
        np.testing.assert_array_equal(checkpoint['readout'],b['raw_readout'])
        loss_min = float(np.linalg.eigvalsh(b['damping']).min())
        if loss_min<=0:
            raise ValueError('positive losses required')
        train = np.asarray(record['fit']['fit_frequency_hz'])
        indices = np.searchsorted(data['training_frequency_hz'],train)
        np.testing.assert_array_equal(data['training_frequency_hz'][indices],train)
        if np.intersect1d(train,data['validation_frequency_hz']).size:
            raise ValueError('frequency leakage')
        target = data['pressure_training'][indices]
        s = 2j*np.pi*train
        q = np.array([solve(k+value*c+value*value*np.eye(len(k)),b['raw_ports'],check_finite=False)
                      for value in s])
        acceleration = s[:,None,None]**2*q
        phase = np.exp(-s[:,None,None]*b['pressure_delays'][None])
        df = np.r_[(train[1]-train[0])/2,(train[2:]-train[:-2])/2,(train[-1]-train[-2])/2]
        weights = np.sqrt(df/df.sum())/np.maximum(np.linalg.norm(target,axis=(1,2)),1e-30)
        stationarity = []
        for m in range(target.shape[1]):
            # Independently assemble each delayed block from physical acceleration.
            design = np.concatenate([acceleration.transpose(0,2,1)*phase[:,m,g,None,None]
                                     for g in range(b['pressure_delays'].shape[1])],axis=2).reshape(len(train)*q.shape[2],-1)
            design *= np.repeat(weights,q.shape[2])[:,None]
            design /= b['readout_scale'][m]
            beta = b['raw_readout'][m].ravel()*b['readout_scale'][m]
            rhs = (target[:,m]*weights[:,None]).reshape(-1)
            normal = (design.conj().T @ (design @ beta-rhs)).real+record['fit']['readout_ridge']*beta
            defect = float(np.linalg.norm(normal)/max(np.linalg.norm((design.conj().T @ rhs).real),1e-30))
            if defect>1e-8:
                raise ValueError('inner readout is not the regularized optimum')
            stationarity.append(defect)
        f = data['validation_frequency_hz']
        rows = data['mapping'][data['held_out_input_keys']-21]
        y = modal_response(b['omega'],b['damping'],b['ports'],f)
        ey = metrics.metric(f,np.einsum('pk,fkl,ql->fpq',rows,y,rows,optimize=True),data['mobility_validation_full'])
        p = pressure_response(acceleration_response(b['omega'],b['damping'],b['ports'],f),f,b['pressure_delays'],b['pressure_readout'])
        ep = metrics.metric(f,np.einsum('fmk,pk->fmp',p,rows,optimize=True),data['pressure_validation_full'])
        for actual,key in [(ey,'full_external_validation'),(ep,'full_external_pressure_validation')]:
            expected = record[key]
            np.testing.assert_allclose([actual['rms'],actual['worst']],[expected['rms'],expected['worst']],rtol=1e-8,atol=1e-12)
        static = b['ports'].T @ (b['ports']/b['omega'][:,None]**2)
        high = b['ports'].T @ b['ports']
        moments = [float(np.linalg.norm(value-data[key])/np.linalg.norm(data[key])) for value,key in [(static,'static_compliance_latent'),(high,'high_inertance_latent')]]
        moment_gate = max(*moments,record['stiffness_mode_reconstruction_relative'],record['pressure_mode_reconstruction_relative'])<1e-7
        for error,key in [(ey,'mobility_gate_passed'),(ep,'pressure_gate_passed')]:
            if record[key]!=(error['rms']<=.05 and error['worst']<=.10 and moment_gate):
                raise ValueError('fidelity gate drift')
        full_observer_record = records['variable-161-observador-completo']
        with np.load(full_observer_record['model_path']) as observer:
            np.testing.assert_array_equal(observer['readout_scale'],b['readout_scale'])
            pressure_all = pressure_response(acceleration_response(b['omega'],b['damping'],b['ports'],f),
                                             f,observer['delays'],observer['readout'])
            full_error = metrics.metric(f,np.einsum('fmk,pk->fmp',pressure_all,rows,optimize=True),data['pressure_validation_full'])
        expected = full_observer_record['full_external_validation']
        np.testing.assert_allclose([full_error['rms'],full_error['worst']],[expected['rms'],expected['worst']],rtol=1e-8,atol=1e-12)
        if full_observer_record['pressure_gate_passed']!=(full_error['rms']<=.05 and full_error['worst']<=.10):
            raise ValueError('all-training observer gate drift')
    native = records['runtime-variable-161']
    for rate in native['rates']:
        if digest(rate['coefficient_path'])!=rate['coefficient_sha256']:
            raise ValueError('native bundle changed')
        checks,bench = rate['verification'],rate['benchmark']
        if not checks['native_python_final_states_outputs_and_work_passed'] or checks['python_reconstructed_step_energy_excess_relative']>1e-10:
            raise ValueError('native state/work checks failed')
        passed = bench['p99_ms']/bench['deadline_ms']<=.20 and bench['max_ms']<=bench['deadline_ms'] and bench['callback_allocations']==0
        if passed!=bench['observed_body_budget_passed']:
            raise ValueError('CPU gate drift')
    result = dict(stage='variable projection independent replay verified',verifier_sha256=digest(__file__),reports_sha256=hashes,
        checkpoint_operators_reconstructed=True,static_complement_relative=null_relative,static_and_high_relative=moments,
        minimum_loss_eigenvalue=loss_min,inner_stationarity_relative=stationarity,
        mobility=dict(rms=ey['rms'],worst=ey['worst']),pressure=dict(rms=ep['rms'],worst=ep['worst']),
        all_training_pressure=dict(rms=full_error['rms'],worst=full_error['worst']),
        native_controls_recorded=2,gates_recomputed=True,full_piano_realtime=False,
        scope='Unscaled harmonic/normal-equation replay and recorded native work controls only. No full-air time, sampled pressure, whole piano or WASM certificate.')
    (folder/'verificacion-variable-161.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print('variable projection replay passed; inner optimum and gates verified')


if __name__=='__main__':
    main()
