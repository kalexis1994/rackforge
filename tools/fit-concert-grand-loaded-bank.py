"""Distill the full loaded mobility into constrained positive virtual modes."""
import argparse
import json
from pathlib import Path
import time
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_wave_cache import digest
from concert_grand_passive_mobility import fit_positive_bank


def metric(frequency, actual, expected):
    difference = np.linalg.norm(actual-expected, axis=(1, 2))
    norm = np.linalg.norm(expected, axis=(1, 2))
    return dict(rms=float(np.sqrt(np.trapezoid(difference**2, frequency)/np.trapezoid(norm**2, frequency))),
        worst=float(np.max(difference/np.maximum(norm, 1e-30))),
        per_frequency=(difference/np.maximum(norm, 1e-30)).tolist())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--teacher-report', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--directions', type=int, default=2)
    args = parser.parse_args()
    if args.report.exists() or args.model.exists() or not 1 <= args.directions <= 4:
        raise ValueError('new fit/model and bounded spatial directions required')
    root = Path(__file__).resolve().parent.parent
    parent = json.loads(args.teacher_report.read_text())
    dataset = Path(parent['data_path'])
    if digest(dataset) != parent['data_sha256']:
        raise ValueError('loaded teacher dataset changed')
    for name, sha in parent['source_sha256'].items():
        if digest(root/'tools'/name) != sha:
            raise ValueError('loaded teacher source changed')
    names = [Path(__file__).name, 'concert_grand_passive_mobility.py']
    report = dict(stage='fitting passive loaded mobility bank', teacher_report_sha256=digest(args.teacher_report),
        dataset_sha256=digest(dataset), source_sha256={name:digest(root/'tools'/name) for name in names},
        full_piano_realtime=False, global_physics_approved=False, auditory_approval=False,
        scope='Approximate full loaded harmonic mobility only; pressure readout and native timing pending.',
        directions_per_training_resistance=args.directions,
        criteria=dict(complex_mobility_rms=.05, complex_mobility_worst=.10))
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.model.parent.mkdir(parents=True, exist_ok=True)
    started = time.perf_counter()
    def save():
        report['seconds'] = time.perf_counter()-started
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8')
    save()
    with np.load(dataset) as data:
        f, target = data['training_frequency_hz'], data['mobility_training']
        static, high = data['static_compliance_latent'], data['high_inertance_latent']
        with np.load(root/'local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz') as board:
            loss = float(board['loss'])
        print('positive loaded fit', len(f), 'frequencies', args.directions, 'spatial directions', flush=True)
        with threadpool_limits(limits=6, user_api='blas'):
            bank, details = fit_positive_bank(f, target, static, high, loss, args.directions)
            validation = data['validation_frequency_hz']
            actual = bank.response(validation)
            latent_error = metric(validation, actual, data['mobility_validation_latent'])
            mapping = data['mapping']
            held = data['held_out_input_keys']-21
            external = np.einsum('pk,fkl,ql->fpq', mapping[held], actual, mapping[held], optimize=True)
            external_error = metric(validation, external, data['mobility_validation_full'])
        np.savez_compressed(args.model, omega=bank.omega, gamma=bank.gamma, ports=bank.ports,
                            mapping=mapping, frequency_validation=validation)
    report.update(stage='positive loaded mobility bank fitted', model_path=str(args.model.resolve()),
        model_sha256=digest(args.model), model_bytes=args.model.stat().st_size, fit=details,
        latent_mobility_validation=latent_error, full_external_mobility_validation=external_error,
        mobility_gate_passed=external_error['rms']<=.05 and external_error['worst']<=.10,
        full_teacher_air_used=True, moments_preserved=True, pressure_fit_done=False,
        source_and_dataset_unchanged=all(digest(root/'tools'/name)==sha for name,sha in report['source_sha256'].items())
            and digest(dataset)==report['dataset_sha256'])
    save()
    print('positive loaded fit complete', report['seconds'], 's', details,
          'latent', latent_error['rms'], latent_error['worst'],
          'external', external_error['rms'], external_error['worst'], flush=True)


if __name__ == '__main__':
    main()
