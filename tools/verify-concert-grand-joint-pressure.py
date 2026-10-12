"""Replay joint fit with independent unscaled solves and preserve failures."""
import importlib.util
import json
from pathlib import Path
import numpy as np
from concert_grand_wave_cache import digest
from concert_grand_storage_preserving_fit import operators
from concert_grand_realtime_body import modal_response
from concert_grand_causal_pressure_fit import acceleration_response,pressure_response

spec = importlib.util.spec_from_file_location('loaded_metrics',Path(__file__).with_name('fit-concert-grand-loaded-bank.py'))
metrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(metrics)


def main():
    root = Path(__file__).resolve().parent.parent
    folder = root/'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P7-cuerpo-cargado-2026-10-12'
    names = ['maestro','almacenamiento-4-directo','presion-almacenamiento-4','conjunto-161',
             'presion-conjunto-161-refit','runtime-conjunto-161','runtime-conjunto-161-repeticion']
    records = {name:json.loads((folder/(name+'.json')).read_text()) for name in names}
    hashes = {name:digest(folder/(name+'.json')) for name in names}
    for name,record in records.items():
        for source,sha in record.get('source_sha256',{}).items():
            if digest(root/'tools'/source)!=sha:
                raise ValueError('measured source changed')
        for path,sha in [('data_path','data_sha256'),('model_path','model_sha256'),('checkpoint_path','checkpoint_sha256')]:
            if path in record and digest(record[path])!=record[sha]:
                raise ValueError('artifact changed')
        for key in ['teacher_report_sha256','body_report_sha256','observer_report_sha256','parent_report_sha256']:
            if key in record and record[key] not in hashes.values():
                raise ValueError('report provenance broken')
        if any(record.get(k) for k in ['factory_updated','full_piano_realtime','global_physics_approved']):
            raise ValueError('diagnostic represented as accepted piano')
    joint = records['conjunto-161']
    with np.load(joint['model_path']) as b,np.load(joint['checkpoint_path']) as checkpoint,np.load(records['maestro']['data_path']) as data,np.load(records['almacenamiento-4-directo']['model_path']) as parent:
        basis = checkpoint['basis']
        np.testing.assert_allclose(basis.T @ basis,np.eye(basis.shape[1]),atol=2e-12,rtol=0)
        relative_null = float(np.linalg.norm(basis.T @ (parent['ports']/parent['omega'][:,None]))/np.linalg.norm(parent['ports']/parent['omega'][:,None]))
        if relative_null>1e-12:
            raise ValueError('static-preserving complement changed')
        k,c,_,_ = operators(checkpoint['parameters'],parent['omega'],parent['damping'],basis,8)
        np.testing.assert_allclose(k,b['raw_stiffness'],rtol=1e-13,atol=1e-10)
        np.testing.assert_allclose(c,b['raw_damping'],rtol=1e-13,atol=1e-10)
        loss_min = float(np.linalg.eigvalsh(b['damping']).min())
        if loss_min<=0:
            raise ValueError('positive loss required')
        f = data['validation_frequency_hz']
        if np.intersect1d(f,data['training_frequency_hz']).size:
            raise ValueError('frequency leakage')
        rows = data['mapping'][data['held_out_input_keys']-21]
        y = modal_response(b['omega'],b['damping'],b['ports'],f)
        ey = metrics.metric(f,np.einsum('pk,fkl,ql->fpq',rows,y,rows,optimize=True),data['mobility_validation_full'])
        recorded = joint['full_external_validation']
        np.testing.assert_allclose([ey['rms'],ey['worst']],[recorded['rms'],recorded['worst']],rtol=1e-8,atol=1e-12)
        static = b['ports'].T @ (b['ports']/b['omega'][:,None]**2)
        high = b['ports'].T @ b['ports']
        moments = [float(np.linalg.norm(value-data[key])/np.linalg.norm(data[key])) for value,key in [(static,'static_compliance_latent'),(high,'high_inertance_latent')]]
        moment_gate = max(*moments,joint['stiffness_mode_reconstruction_relative'],joint['pressure_mode_reconstruction_relative'])<1e-7
        if joint['mobility_gate_passed'] != (ey['rms']<=.05 and ey['worst']<=.10 and moment_gate):
            raise ValueError('mobility gate drift')
        acc = acceleration_response(b['omega'],b['damping'],b['ports'],f)
        pressure_results = []
        for name in ['conjunto-161','presion-conjunto-161-refit']:
            record = records[name]
            if name=='conjunto-161':
                delays,readout = b['pressure_delays'],b['pressure_readout']
                recorded = record['full_external_pressure_validation']
            else:
                with np.load(record['model_path']) as observer:
                    delays,readout = observer['delays'],observer['readout']
                recorded = record['full_external_validation']
            p = pressure_response(acc,f,delays,readout)
            error = metrics.metric(f,np.einsum('fmk,pk->fmp',p,rows,optimize=True),data['pressure_validation_full'])
            np.testing.assert_allclose([error['rms'],error['worst']],[recorded['rms'],recorded['worst']],rtol=1e-8,atol=1e-12)
            passed = error['rms']<=.05 and error['worst']<=.10
            if record['pressure_gate_passed'] != passed:
                raise ValueError('pressure gate drift')
            pressure_results.append(dict(model=name,rms=error['rms'],worst=error['worst'],gate_passed=passed))
    native = records['runtime-conjunto-161']
    for rate in native['rates']:
        if digest(rate['coefficient_path'])!=rate['coefficient_sha256']:
            raise ValueError('native bundle changed')
        check = rate['verification']
        if not check['native_python_final_states_outputs_and_work_passed'] or check['python_reconstructed_step_energy_excess_relative']>1e-10:
            raise ValueError('recorded native state/work control missing')
    budget_results = []
    for name in ['runtime-conjunto-161','runtime-conjunto-161-repeticion']:
        record = records[name]
        passes = []
        for rate,original in zip(record['rates'],native['rates']):
            if rate['coefficient_sha256']!=original['coefficient_sha256']:
                raise ValueError('timed coefficients differ')
            bench = rate['benchmark']
            passed = bench['p99_ms']/bench['deadline_ms']<=.20 and bench['max_ms']<=bench['deadline_ms'] and bench['callback_allocations']==0
            if passed!=bench['observed_body_budget_passed']:
                raise ValueError('budget gate drift')
            passes.append(passed)
        if all(passes)!=record['body_20percent_budget_passed_both_rates']:
            raise ValueError('joint budget gate drift')
        budget_results.append(dict(report=name,both_rates_passed=all(passes)))
    result = dict(stage='joint dynamics and observer replay verified',verifier_sha256=digest(__file__),reports_sha256=hashes,
        storage_complement_relative=relative_null,static_and_high_relative=moments,minimum_loss_eigenvalue=loss_min,
        reconstructed_checkpoint_operators=True,unscaled_validation_replay=True,mobility=dict(rms=ey['rms'],worst=ey['worst']),
        pressure=pressure_results,native_controls_recorded=2,budget=budget_results,
        failed_gates_preserved=True,full_piano_realtime=False,
        scope='Unscaled harmonic replay, source/data/checkpoint authentication and recorded native state/work verification. No full air transient, sampled pressure or whole-instrument certificate.')
    (folder/'verificacion-conjunto-161.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print('joint replay passed; failed acoustic/CPU reserve gates preserved')


if __name__ == '__main__':
    main()
