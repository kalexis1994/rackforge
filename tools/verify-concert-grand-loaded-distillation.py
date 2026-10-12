"""Replay loaded harmonic validation and preserve every failed gate.

Authenticates compact teacher/model/source provenance, positive losses,
storage moments and recorded native bundles. No new full R or WAV.
"""
import importlib.util
import json
from pathlib import Path
import numpy as np
from scipy.linalg import solve
from threadpoolctl import threadpool_limits
from concert_grand_realtime_body import modal_response
from concert_grand_causal_pressure_fit import acceleration_response, pressure_response
from concert_grand_wave_cache import digest

spec = importlib.util.spec_from_file_location('loaded_metrics',Path(__file__).with_name('fit-concert-grand-loaded-bank.py'))
metrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(metrics)


def main():
    root = Path(__file__).resolve().parent.parent
    folder = root/'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P7-cuerpo-cargado-2026-10-12'
    names = ['maestro','ajuste-2','acoplado-8','acoplado-8-v2','almacenamiento-8',
             'presion-acoplado-8','presion-almacenamiento-8','runtime-114',
             'direcciones','ajuste-4','acoplado-4-woodbury','presion-acoplado-4',
             'presion-acoplado-4-retardos8',
             'woodbury-coste','almacenamiento-4-directo','presion-almacenamiento-4','runtime-161','runtime-161-sin-ajuste']
    reports = {name:json.loads((folder/(name+'.json')).read_text()) for name in names}
    hashes = {name:digest(folder/(name+'.json')) for name in names}
    for name, report in reports.items():
        for source, sha in report.get('source_sha256',{}).items():
            if digest(root/'tools'/source) != sha:
                raise ValueError('measured source changed: '+source)
        if 'producer_sha256' in report:
            producer = {'direcciones':'diagnose-concert-grand-loaded-directions.py',
                        'runtime-161-sin-ajuste':'remeasure-concert-grand-loaded-runtime.py'}[name]
            if digest(root/'tools'/producer) != report['producer_sha256']:
                raise ValueError('diagnostic producer changed')
        for key in ['teacher_report_sha256','bank_report_sha256','body_report_sha256','base_report_sha256','parent_report_sha256']:
            if key in report and report[key] not in hashes.values():
                raise ValueError('broken report parent: '+name)
        for path, sha in [('data_path','data_sha256'),('model_path','model_sha256')]:
            if path in report and digest(report[path]) != report[sha]:
                raise ValueError('measured artifact changed: '+name)
        if report.get('full_piano_realtime') or report.get('global_physics_approved') or report.get('factory_updated'):
            raise ValueError('diagnostic represented as piano/plugin approval')
    teacher = reports['maestro']
    cache = root/'local/concert-grand-physics/P6-carga-causal-2026-10-06/R.npy'
    if digest(cache) != teacher['cache_R_sha256']:
        raise ValueError('single full acoustic cache changed')
    results = []
    with np.load(teacher['data_path']) as data, threadpool_limits(limits=6,user_api='blas'):
        f = data['validation_frequency_hz']
        if np.intersect1d(f,data['training_frequency_hz']).size:
            raise ValueError('training/validation frequency leakage')
        rows = data['mapping'][data['held_out_input_keys']-21]
        for name in ['ajuste-2','acoplado-8-v2','almacenamiento-8','ajuste-4','acoplado-4-woodbury','almacenamiento-4-directo']:
            report = reports[name]
            with np.load(report['model_path']) as model:
                omega, ports = model['omega'], model['ports']
                damping = model['damping'] if 'damping' in model else np.diag(2*model['gamma'])
                if not np.allclose(damping,damping.T,rtol=0,atol=1e-10*max(np.linalg.norm(damping),1.)) or np.linalg.eigvalsh(damping).min() <= 0:
                    raise ValueError('positive reciprocal losses missing')
                if not np.array_equal(model['mapping'],data['mapping']):
                    raise ValueError('force/velocity spatial map changed')
                static = ports.T @ (ports/omega[:,None]**2)
                high = ports.T @ ports
                moments = [float(np.linalg.norm(actual-data[key])/np.linalg.norm(data[key]))
                           for actual,key in [(static,'static_compliance_latent'),(high,'high_inertance_latent')]]
                response = modal_response(omega,damping,ports,f)
                external = np.einsum('pk,fkl,ql->fpq',rows,response,rows,optimize=True)
                error = metrics.metric(f,external,data['mobility_validation_full'])
                recorded = report.get('full_external_validation',report.get('full_external_mobility_validation'))
                np.testing.assert_allclose([error['rms'],error['worst']],[recorded['rms'],recorded['worst']],rtol=1e-9,atol=1e-12)
                passed = error['rms']<=.05 and error['worst']<=.10
                if 'stiffness_mode_reconstruction_relative' in report:
                    passed = passed and max(*moments,report['stiffness_mode_reconstruction_relative'])<1e-7
                if passed != report['mobility_gate_passed']:
                    raise ValueError('mobility gate drift')
                results.append(dict(model=name,mobility=error,static_and_high_relative=moments,gate_passed=passed))
        for pressure_name,body_name in [('presion-acoplado-8','acoplado-8-v2'),('presion-almacenamiento-8','almacenamiento-8'),
                                         ('presion-acoplado-4','acoplado-4-woodbury'),('presion-acoplado-4-retardos8','acoplado-4-woodbury'),
                                         ('presion-almacenamiento-4','almacenamiento-4-directo')]:
            report = reports[pressure_name]
            with np.load(report['model_path']) as observer,np.load(reports[body_name]['model_path']) as model:
                acc = acceleration_response(model['omega'],model['damping'],model['ports'],f)
                prediction = pressure_response(acc,f,observer['delays'],observer['readout'])
                external = np.einsum('fmk,pk->fmp',prediction,rows,optimize=True)
                error = metrics.metric(f,external,data['pressure_validation_full'])
                recorded = report['full_external_validation']
                np.testing.assert_allclose([error['rms'],error['worst']],[recorded['rms'],recorded['worst']],rtol=1e-9,atol=1e-12)
                passed = error['rms']<=.05 and error['worst']<=.10
                if passed != report['pressure_gate_passed'] or report['parent_mobility_gate_passed'] != reports[body_name]['mobility_gate_passed']:
                    raise ValueError('pressure or inherited mobility gate drift')
                results.append(dict(model=pressure_name,pressure=error,gate_passed=passed))
    native_checks = 0
    for name in ['runtime-114','runtime-161']:
        report = reports[name]
        budget_passes = []
        for rate in report['rates']:
            if digest(rate['coefficient_path']) != rate['coefficient_sha256']:
                raise ValueError('native bundle changed')
            bench, check = rate['benchmark'],rate['verification']
            passed = bench['p99_ms']/bench['deadline_ms']<=.20 and bench['max_ms']<=bench['deadline_ms'] and bench['callback_allocations']==0
            if passed != bench['observed_body_budget_passed'] or not check['native_python_final_states_outputs_and_work_passed'] or check['python_reconstructed_step_energy_excess_relative']>1e-10:
                raise ValueError('native budget/work verification drift')
            budget_passes.append(passed)
            native_checks += 1
        if all(budget_passes) != report['body_20percent_budget_passed_both_rates']:
            raise ValueError('joint rate gate drift')
    retimed = reports['runtime-161-sin-ajuste']
    for rate, original in zip(retimed['rates'],reports['runtime-161']['rates']):
        if rate['coefficient_sha256'] != original['coefficient_sha256']:
            raise ValueError('retimed coefficients differ')
        bench = rate['benchmark']
        passed = bench['p99_ms']/bench['deadline_ms']<=.20 and bench['max_ms']<=bench['deadline_ms'] and bench['callback_allocations']==0
        if passed != bench['observed_body_budget_passed']:
            raise ValueError('retimed budget gate drift')
    if all(r['benchmark']['observed_body_budget_passed'] for r in retimed['rates']) != retimed['body_20percent_budget_passed_both_rates']:
        raise ValueError('retimed joint gate drift')
    result = dict(stage='loaded harmonic model replay and integrity verified',verifier_sha256=digest(__file__),
        reports_sha256=hashes, source_data_cache_integrity=True, frequency_sets_disjoint=True,
        replayed_results=results, native_recorded_controls=native_checks,
        failed_fidelity_gates_preserved=True, full_piano_realtime=False,
        scope='Independent harmonic replay/scalar gate checks; native recorded final states/checksum/work. No original-air time replay, pressure sampled-clock, strings, hammer, pedals, full piano or WASM certificate.')
    (folder/'verificacion.json').write_text(json.dumps(result,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    print('loaded validation replay',len(results),'models/observers; native controls',native_checks)


if __name__ == '__main__':
    main()
