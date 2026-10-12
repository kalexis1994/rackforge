"""Retain storage moments; fit shared positive losses against loaded teacher."""
import argparse
import importlib.util
import json
from pathlib import Path
import time
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_passive_mobility import MobilityBank
from concert_grand_coupled_mobility_fit import fit_coupled_losses
from concert_grand_realtime_body import modal_response
from concert_grand_wave_cache import digest

spec = importlib.util.spec_from_file_location('loaded_bank_parent', Path(__file__).with_name('fit-concert-grand-loaded-bank.py'))
prior = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prior)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--teacher-report', type=Path, required=True)
    parser.add_argument('--bank-report', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--rank', type=int, default=8)
    args = parser.parse_args()
    if args.report.exists() or args.model.exists() or not 1 <= args.rank <= 16:
        raise ValueError('new coupled fit report/model and bounded rank')
    root = Path(__file__).resolve().parent.parent
    parent = json.loads(args.teacher_report.read_text())
    baseline = json.loads(args.bank_report.read_text())
    if digest(parent['data_path']) != parent['data_sha256'] or digest(baseline['model_path']) != baseline['model_sha256']:
        raise ValueError('teacher or passive bank changed')
    for name, sha in baseline['source_sha256'].items():
        if digest(root/'tools'/name) != sha:
            raise ValueError('baseline construction changed')
    names = [Path(__file__).name, 'concert_grand_coupled_mobility_fit.py', 'concert_grand_realtime_body.py']
    started = time.perf_counter()
    report = dict(stage='fitting coupled loaded loss', teacher_report_sha256=digest(args.teacher_report),
        bank_report_sha256=digest(args.bank_report), source_sha256={name:digest(root/'tools'/name) for name in names},
        full_piano_realtime=False, global_physics_approved=False, auditory_approval=False,
        scope='Loaded harmonic mobility with fixed stiffness/mass/ports, positive diagonal+low-rank coupled damping. Pressure/runtime pending.')
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.model.parent.mkdir(parents=True, exist_ok=True)
    def save():
        report['seconds'] = time.perf_counter()-started
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8')
    save()
    def progress(row):
        report['progress'] = row
        save()
        print('coupled loaded loss', row, flush=True)
    with np.load(baseline['model_path']) as data:
        bank = MobilityBank(data['omega'], data['gamma'], data['ports'])
    with np.load(parent['data_path']) as data:
        with threadpool_limits(limits=6, user_api='blas'):
            c, details = fit_coupled_losses(bank, data['training_frequency_hz'], data['mobility_training'], args.rank, progress=progress)
            frequency = data['validation_frequency_hz']
            actual = modal_response(bank.omega, c, bank.ports, frequency)
            latent_error = prior.metric(frequency, actual, data['mobility_validation_latent'])
            mapping = data['mapping']
            held = data['held_out_input_keys']-21
            external = np.einsum('pk,fkl,ql->fpq', mapping[held], actual, mapping[held], optimize=True)
            external_error = prior.metric(frequency, external, data['mobility_validation_full'])
        static_error = float(np.linalg.norm(bank.static_compliance()-data['static_compliance_latent'])/np.linalg.norm(data['static_compliance_latent']))
        high_error = float(np.linalg.norm(bank.high_inertance()-data['high_inertance_latent'])/np.linalg.norm(data['high_inertance_latent']))
    np.savez_compressed(args.model, omega=bank.omega, damping=c, ports=bank.ports, mapping=mapping)
    report.update(stage='coupled loaded mobility fitted', fit=details,
        model_path=str(args.model.resolve()), model_sha256=digest(args.model), model_bytes=args.model.stat().st_size,
        modes=len(bank.omega), minimum_loss_eigenvalue=float(np.linalg.eigvalsh(c).min()),
        static_moment_relative=static_error, high_moment_relative=high_error,
        latent_validation=latent_error, full_external_validation=external_error,
        mobility_gate_passed=external_error['rms']<=.05 and external_error['worst']<=.10,
        pressure_fit_done=False, source_unchanged=all(digest(root/'tools'/name)==sha for name,sha in report['source_sha256'].items()))
    save()
    print('coupled loaded fit complete', report['seconds'], 's; objective', details['objective'],
          'external', external_error['rms'], external_error['worst'], flush=True)


if __name__ == '__main__':
    main()
