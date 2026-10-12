"""Fit causal pressure readouts, preserving rejected mobility gates."""
import argparse
import importlib.util
import json
from pathlib import Path
import time
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_causal_pressure_fit import (support_delays, acceleration_response,
                                             pressure_response, fit_readout)
from concert_grand_wave_cache import digest

spec = importlib.util.spec_from_file_location('loaded_bank_parent', Path(__file__).with_name('fit-concert-grand-loaded-bank.py'))
prior = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prior)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--teacher-report', type=Path, required=True)
    parser.add_argument('--body-report', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--groups', type=int, default=4)
    args = parser.parse_args()
    if args.report.exists() or args.model.exists() or not 1 <= args.groups <= 8:
        raise ValueError('new report/model and bounded causal groups required')
    started = time.perf_counter()
    root = Path(__file__).resolve().parent.parent
    teacher = json.loads(args.teacher_report.read_text())
    body = json.loads(args.body_report.read_text())
    for report, path_key, sha_key in [(teacher, 'data_path', 'data_sha256'), (body, 'model_path', 'model_sha256')]:
        if digest(report[path_key]) != report[sha_key]:
            raise ValueError('teacher or body data changed')
        for name, sha in report['source_sha256'].items():
            if digest(root/'tools'/name) != sha:
                raise ValueError('teacher/body sources changed')
    names = [Path(__file__).name, 'concert_grand_causal_pressure_fit.py']
    source_sha = {name:digest(root/'tools'/name) for name in names}
    with np.load(body['model_path']) as b, np.load(teacher['data_path']) as data:
        omega, damping, ports = b['omega'], b['damping'], b['ports']
        mapping = data['mapping']
        delays = support_delays(data['observer_kernel'], teacher['observer_rate_hz'], args.groups)
        with threadpool_limits(limits=6, user_api='blas'):
            training = data['training_frequency_hz']
            acc = acceleration_response(omega, damping, ports, training)
            readout, details = fit_readout(acc, training, data['pressure_training'], delays)
            frequency = data['validation_frequency_hz']
            prediction = pressure_response(acceleration_response(omega, damping, ports, frequency), frequency, delays, readout)
            latent_error = prior.metric(frequency, prediction, data['pressure_validation_latent'])
            external = np.einsum('fmk,pk->fmp', prediction, mapping[data['held_out_input_keys']-21], optimize=True)
            external_error = prior.metric(frequency, external, data['pressure_validation_full'])
    np.savez_compressed(args.model, delays=delays, readout=readout)
    report = dict(stage='causal acceleration pressure readout fitted', seconds=time.perf_counter()-started,
        teacher_report_sha256=digest(args.teacher_report), body_report_sha256=digest(args.body_report),
        source_sha256=source_sha, source_unchanged=all(digest(root/'tools'/n)==sha for n,sha in source_sha.items()),
        model_path=str(args.model.resolve()), model_sha256=digest(args.model), model_bytes=args.model.stat().st_size,
        pressure_fit=details, modes=len(omega), delay_groups=args.groups, propagation_delays_seconds=delays.tolist(),
        latent_validation=latent_error, full_external_validation=external_error,
        pressure_gate_passed=external_error['rms']<=.05 and external_error['worst']<=.10,
        parent_mobility_gate_passed=bool(body['mobility_gate_passed']),
        full_piano_realtime=False, global_physics_approved=False, auditory_approval=False,
        feedback_pressure=False, transient_or_sampled_clock_certificate=False,
        scope='Continuous harmonic pressure on reserved frequencies/forces; original propagation support, real acceleration readouts, physical Pa scale.')
    args.report.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8')
    print('causal pressure fit', report['seconds'], 's; external', external_error['rms'], external_error['worst'], flush=True)


if __name__ == '__main__':
    main()
