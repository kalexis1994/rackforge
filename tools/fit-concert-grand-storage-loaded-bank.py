"""Fit positive stiffness/losses while keeping loaded-port storage moments."""
import argparse
import importlib.util
import json
from pathlib import Path
import time
import numpy as np
from scipy.linalg import eigh, solve
from threadpoolctl import threadpool_limits
from concert_grand_storage_preserving_fit import fit_storage_preserving
from concert_grand_realtime_body import modal_response
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
    args = parser.parse_args()
    if args.report.exists() or args.model.exists():
        raise ValueError('new fit paths required')
    root = Path(__file__).resolve().parent.parent
    parent = json.loads(args.teacher_report.read_text())
    baseline = json.loads(args.body_report.read_text())
    for report, key, sha in [(parent,'data_path','data_sha256'), (baseline,'model_path','model_sha256')]:
        if digest(report[key]) != report[sha]:
            raise ValueError('teacher or base body changed')
        for name, source_sha in report['source_sha256'].items():
            if digest(root/'tools'/name) != source_sha:
                raise ValueError('parent source changed')
    names = [Path(__file__).name, 'concert_grand_storage_preserving_fit.py', 'concert_grand_realtime_body.py']
    started = time.perf_counter()
    report = dict(stage='fitting storage-preserving loaded dynamics',
        teacher_report_sha256=digest(args.teacher_report), base_report_sha256=digest(args.body_report),
        source_sha256={name:digest(root/'tools'/name) for name in names},
        full_piano_realtime=False, global_physics_approved=False, auditory_approval=False,
        scope='Virtual positive mechanical realization of loaded harmonic mobility; stiffness-complement fit, fixed static/high moments. No teacher physics edited.')
    def save():
        report['seconds'] = time.perf_counter()-started
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8')
    def progress(row):
        report['progress'] = row
        save()
        print('storage-preserving loaded fit', row, flush=True)
    save()
    with np.load(baseline['model_path']) as b, np.load(parent['data_path']) as data:
        with threadpool_limits(limits=6, user_api='blas'):
            k, c, details = fit_storage_preserving(b['omega'], b['ports'], b['damping'],
                data['training_frequency_hz'], data['mobility_training'], progress=progress)
            # Symmetric orthogonal mode change; no pole or loss clipping.
            values, vectors = eigh(k, driver='evd')
            if values[0] <= 0:
                raise ValueError('positive stiffness required')
            omega = np.sqrt(values)
            damping = vectors.T @ c @ vectors
            ports = vectors.T @ b['ports']
            frequency = data['validation_frequency_hz']
            actual = modal_response(omega, damping, ports, frequency)
            direct = np.array([s*b['ports'].T @ solve(k+s*c+s*s*np.eye(len(k)), b['ports'], assume_a='sym', check_finite=False)
                               for s in 2j*np.pi*frequency])
            realization_error = float(np.linalg.norm(actual-direct)/np.linalg.norm(direct))
            latent_error = prior.metric(frequency, actual, data['mobility_validation_latent'])
            mapping = data['mapping']
            rows = mapping[data['held_out_input_keys']-21]
            external = np.einsum('pk,fkl,ql->fpq', rows, actual, rows, optimize=True)
            external_error = prior.metric(frequency, external, data['mobility_validation_full'])
            static_error = float(np.linalg.norm(ports.T @ (ports/omega[:, None]**2)-data['static_compliance_latent'])/np.linalg.norm(data['static_compliance_latent']))
            high_error = float(np.linalg.norm(ports.T @ ports-data['high_inertance_latent'])/np.linalg.norm(data['high_inertance_latent']))
    np.savez_compressed(args.model, omega=omega, damping=damping, ports=ports, mapping=mapping)
    report.update(stage='storage-preserving loaded dynamics fitted', fit=details,
        model_path=str(args.model.resolve()), model_sha256=digest(args.model), model_bytes=args.model.stat().st_size,
        modes=len(omega), minimum_loss_eigenvalue=float(np.linalg.eigvalsh(damping).min()),
        stiffness_fit_support_max_hz=30000., stiffness_mode_reconstruction_relative=realization_error,
        static_moment_relative=static_error, high_moment_relative=high_error,
        latent_validation=latent_error, full_external_validation=external_error,
        mobility_gate_passed=external_error['rms']<=.05 and external_error['worst']<=.10 and max(static_error,high_error,realization_error)<1e-7,
        pressure_fit_done=False, source_unchanged=all(digest(root/'tools'/n)==sha for n,sha in report['source_sha256'].items()))
    save()
    print('storage fit complete', report['seconds'], 's; external', external_error['rms'], external_error['worst'], flush=True)


if __name__ == '__main__':
    main()
