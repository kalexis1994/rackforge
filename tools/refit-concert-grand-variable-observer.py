"""All-training linear readout control with dynamics/scales/ridge frozen."""
import argparse
import importlib.util
import json
from pathlib import Path
import time
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_variable_projection import VariablePressureProblem
from concert_grand_causal_pressure_fit import acceleration_response,pressure_response
from concert_grand_wave_cache import digest

spec = importlib.util.spec_from_file_location('joint_parent',Path(__file__).with_name('fit-concert-grand-joint-pressure.py'))
prior = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prior)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['teacher-report','body-report','report','model']:
        parser.add_argument('--'+name,type=Path,required=True)
    a = parser.parse_args()
    if a.report.exists() or a.model.exists():
        raise ValueError('new linear control paths required')
    root = Path(__file__).resolve().parent.parent
    teacher,body = [json.loads(path.read_text()) for path in [a.teacher_report,a.body_report]]
    for record,key,sha in [(teacher,'data_path','data_sha256'),(body,'model_path','model_sha256')]:
        if digest(record[key])!=record[sha]:
            raise ValueError('frozen data changed')
        for name,source_sha in record['source_sha256'].items():
            if digest(root/'tools'/name)!=source_sha:
                raise ValueError('frozen parent changed')
    names = [Path(__file__).name,'concert_grand_variable_projection.py','concert_grand_causal_pressure_fit.py',
             'fit-concert-grand-joint-pressure.py','concert_grand_joint_pressure_fit.py']
    hashes = {name:digest(root/'tools'/name) for name in names}
    started = time.perf_counter()
    with np.load(teacher['data_path']) as data,np.load(body['model_path']) as b,threadpool_limits(limits=1,user_api='blas'):
        problem = VariablePressureProblem(b['raw_omega'],b['raw_ports'],b['raw_damping'],np.empty((len(b['omega']),0)),
            data['training_frequency_hz'],data['mobility_training'],data['pressure_training'],b['pressure_delays'],
            readout_ridge=body['fit']['readout_ridge'])
        problem.scale = b['readout_scale']
        q,factors = problem.states(b['raw_stiffness'],b['raw_damping'])
        del factors
        raw_readout,penalty,stationarity = problem.optimal_readout(q)
        _,vectors = prior.split_stiffness_modes(b['raw_stiffness'],b['raw_omega'])
        np.testing.assert_allclose(vectors.T @ b['raw_ports'],b['ports'],rtol=1e-10,atol=1e-12)
        readout = np.einsum('mgn,nj->mgj',raw_readout,vectors,optimize=True)
        f = data['validation_frequency_hz']
        prediction = pressure_response(acceleration_response(b['omega'],b['damping'],b['ports'],f),f,b['pressure_delays'],readout)
        rows = data['mapping'][data['held_out_input_keys']-21]
        error = prior.metrics.metric(f,np.einsum('fmk,pk->fmp',prediction,rows,optimize=True),data['pressure_validation_full'])
        np.savez_compressed(a.model,delays=b['pressure_delays'],readout=readout,raw_readout=raw_readout,
            readout_scale=b['readout_scale'])
    record = dict(stage='all-training optimal pressure control with frozen dynamics',seconds=time.perf_counter()-started,
        source_sha256=hashes,teacher_report_sha256=digest(a.teacher_report),body_report_sha256=digest(a.body_report),
        model_path=str(a.model.resolve()),model_sha256=digest(a.model),model_bytes=a.model.stat().st_size,
        fixed_readout_scale=True,readout_ridge=body['fit']['readout_ridge'],readout_penalty=penalty,
        inner_stationarity_relative=stationarity,training_frequency_count=len(problem.frequency),
        full_external_validation=error,pressure_gate_passed=error['rms']<=.05 and error['worst']<=.10,
        parent_mobility_gate_passed=body['mobility_gate_passed'],full_piano_realtime=False,
        global_physics_approved=False,factory_updated=False,
        source_unchanged=all(digest(root/'tools'/n)==sha for n,sha in hashes.items()),
        scope='Linear observation refit using all 1189 training frequencies; no dynamics, scale, propagation, gain or validation-frequency changes.')
    a.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    print('all-training observer',record['seconds'],'s; pressure',error['rms'],error['worst'],flush=True)


if __name__=='__main__':
    main()
