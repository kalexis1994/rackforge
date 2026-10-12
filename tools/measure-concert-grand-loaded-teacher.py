"""One compact training/validation dataset of FULL board+air and two mics."""
import argparse
import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace
import time
import numpy as np
from threadpoolctl import threadpool_info, threadpool_limits
from concert_grand_wave_cache import digest
from concert_grand_cell_observer import CellObserver
from concert_grand_cell_audio import retarded_kernel
from concert_grand_loaded_teacher import wave_impedance_blocks, observer_transfer, mobility_and_pressure
import concert_grand_orthotropic_plate as plate

spec = importlib.util.spec_from_file_location('full_air_teacher', Path(__file__).with_name('audit-concert-grand-pade-contact-full.py'))
full = importlib.util.module_from_spec(spec)
spec.loader.exec_module(full)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--data', type=Path, required=True)
    parser.add_argument('--blas-threads', type=int, default=6)
    args = parser.parse_args()
    if args.report.exists() or args.data.exists() or not 1 <= args.blas_threads <= 6:
        raise ValueError('new compact dataset/report and bounded offline threads required')
    root = Path(__file__).resolve().parent.parent
    docs = root/'docs/concert-grand-physics-roadmap-2026-10-02'
    a = SimpleNamespace(full_report=docs/'medidas-P6-aire-bloques/carga128-pade2.json',
        board=root/'local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz',
        cache=root/'local/concert-grand-physics/P6-carga-causal-2026-10-06', direction_block=32)
    candidate_path = docs/'medidas-P7-cuerpo-realtime-2026-10-12/candidato.json'
    candidate = json.loads(candidate_path.read_text())
    map_path = Path(candidate['mapping_path'])
    if digest(map_path) != candidate['mapping_sha256']:
        raise ValueError('changed diagnostic force geometry')
    mapping = np.frombuffer(map_path.read_bytes(), dtype='<f8', offset=16).reshape(88, 20).copy()
    names = [Path(__file__).name, 'concert_grand_loaded_teacher.py', 'concert_grand_cell_observer.py', 'concert_grand_cell_audio.py']
    report = dict(stage='preparation of full loaded teacher', teacher_board_sha256=digest(a.board),
        candidate_report_sha256=digest(candidate_path), mapping_sha256=digest(map_path),
        source_sha256={name:digest(root/'tools'/name) for name in names},
        full_piano_realtime=False, global_physics_approved=False, no_acoustic_rank_192_reduction=True,
        new_R_or_modal_history=False, scopes=['Frozen assumed physical full harmonic operator; not identified Steinway.',
        'Linear force/velocity/pressure only; no hammer/string/notes/pedal/MIDI/transient certificate.'])
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.data.parent.mkdir(parents=True, exist_ok=True)
    began = time.perf_counter()
    def save():
        report['seconds'] = time.perf_counter()-began
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8')
    save()
    # Bit-identical construction/signatures stay at one thread. Only later
    # frequency contractions and solves use bounded parallel BLAS.
    with threadpool_limits(limits=1, user_api='blas'):
        approved, modal, _, _, surface, provider, identity, _ = full.prepare_reference(a)
        _, _, air = full.construct_air(a, approved, provider, surface, identity, 12*2**30, report, save)
        with np.load(a.board) as data:
            geometry = dict(panel=json.loads(str(data['panel'])), boundary=str(data['boundary']),
                            nx=int(data['nx']), ny=int(data['ny']), vectors=data['ritz_vectors'])
            xs = geometry['panel']['length']*(.1+.8*(np.arange(21, 109)-21)/87)
            ports = (plate.shape(geometry, xs, np.full(88, geometry['panel']['width']/2)) @ modal.basis).T
        microphone_path = docs/'medidas-P6-contacto-banda-superior/contacto-24k.json'
        microphones = approved.get('microphones', json.loads(microphone_path.read_text())['microphones'])
        observer = CellObserver(provider.surface, microphones, 4)
        kernel = retarded_kernel(observer, 128000)
        # The same pressure kernel construction used in the heard physical WAV.
        report.update(observer_kernel_shape=list(kernel.shape), observer_source_report_sha256=digest(microphone_path),
            microphones=microphones, observer_rate_hz=128000, kernel_interpolation='same cubic retarded cell kernel',
            board_coordinates=606, acoustic_coordinates=606, acoustic_correlation_shape=list(air.wave.R.shape))
    held_keys = np.array([24, 33, 42, 50, 57, 65, 72, 79, 85, 96, 104])
    validation = np.geomspace(23., 5970., 111)
    training = np.unique(np.r_[np.arange(20., 500., 1.), np.arange(500., 2000., 4.), np.arange(2000., 6000., 12.), 6000.])
    training = training[~np.isclose(training[:, None], validation, rtol=0, atol=1e-9).any(axis=1)]
    frequencies = np.unique(np.r_[training, validation])
    train_index = np.searchsorted(frequencies, training)
    val_index = np.searchsorted(frequencies, validation)
    phi = ports @ mapping
    forces = np.c_[phi, ports[:, held_keys-21]]
    n = len(frequencies)
    mobility = np.empty((n, 31, 31), complex)
    pressure = np.empty((n, 2, 31), complex)
    dry = np.empty((n, 20, 20), complex)
    omega = 2*np.pi*modal.frequency
    report.update(stage='sampling full loaded mobility and pressure', training_frequencies=len(training),
        validation_frequencies=len(validation), total_frequencies=n, held_out_input_keys=held_keys.tolist(),
        latent_ports=20, frequency_block=12, offline_blas_threads=args.blas_threads)
    save()
    controls = []
    minimum_passivity = 0.
    max_reciprocity = 0.
    last = time.perf_counter()
    with threadpool_limits(limits=args.blas_threads, user_api='blas'):
        report['offline_threadpools'] = threadpool_info()
        for first, current, wave_z in wave_impedance_blocks(air.wave, frequencies, 12):
            readouts = observer_transfer(kernel, 128000, current)
            for local, hz in enumerate(current):
                index = first+local
                s = 2j*np.pi*hz
                impedance = wave_z[local]+air.relaxation.impedance(s)
                if len(controls) < 3 and local == 0:
                    scalar = air.impedance(s)
                    relative = float(np.linalg.norm(impedance-scalar)/np.linalg.norm(scalar))
                    if relative > 1e-11:
                        raise ValueError('batched full acoustic operator changed')
                    controls.append(dict(frequency_hz=float(hz), batched_scalar_impedance_relative=relative))
                mobility[index], pressure[index] = mobility_and_pressure(omega, modal.damping, impedance, forces, readouts[local], hz)
                dry[index], _ = mobility_and_pressure(omega, modal.damping, np.zeros_like(impedance), phi, readouts[local], hz)
                scale = max(np.linalg.norm(mobility[index]), 1e-30)
                max_reciprocity = max(max_reciprocity, float(np.linalg.norm(mobility[index]-mobility[index].T)/scale))
                minimum_passivity = min(minimum_passivity, float(np.linalg.eigvalsh((mobility[index]+mobility[index].conj().T)/2).min()/scale))
            if time.perf_counter()-last > 25 or first+len(current) == n:
                report.update(progress=dict(done=first+len(current), total=n), reciprocity_relative=max_reciprocity,
                              minimum_hermitian_mobility_relative=minimum_passivity, batch_controls=controls)
                save()
                print('loaded teacher', first+len(current), n, report['seconds'], 's', flush=True)
                last = time.perf_counter()
    if max_reciprocity > 1e-10 or minimum_passivity < -1e-10:
        raise ValueError('loaded teacher reciprocity/passivity failed')
    # No q/v histories or large R copies; one compact force/response dataset.
    np.savez_compressed(args.data, training_frequency_hz=training, validation_frequency_hz=validation,
        mobility_training=mobility[train_index, :20, :20], pressure_training=pressure[train_index, :, :20],
        mobility_dry_training=dry[train_index], mobility_validation_latent=mobility[val_index, :20, :20],
        pressure_validation_latent=pressure[val_index, :, :20],
        mobility_validation_full=mobility[val_index, 20:, 20:], pressure_validation_full=pressure[val_index, :, 20:],
        mapping=mapping, held_out_input_keys=held_keys, observer_kernel=kernel,
        static_compliance_latent=phi.T @ (phi/omega[:, None]**2), high_inertance_latent=phi.T @ phi)
    geometry_mobility = np.einsum('pk,fkl,ql->fpq', mapping[held_keys-21], mobility[val_index, :20, :20], mapping[held_keys-21], optimize=True)
    geometry_pressure = pressure[val_index, :, :20] @ mapping[held_keys-21].T
    def metric(actual, expected):
        difference = np.linalg.norm(actual-expected, axis=tuple(range(1, actual.ndim)))
        norms = np.linalg.norm(expected, axis=tuple(range(1, expected.ndim)))
        return dict(rms=float(np.sqrt(np.trapezoid(difference**2, validation)/np.trapezoid(norms**2, validation))),
                    worst=float(np.max(difference/np.maximum(norms, 1e-30))))
    report.update(stage='full loaded harmonic teacher dataset measured', data_path=str(args.data.resolve()),
        data_sha256=digest(args.data), data_bytes=args.data.stat().st_size,
        geometry_only_mobility_error=metric(geometry_mobility, mobility[val_index, 20:, 20:]),
        geometry_only_pressure_error=metric(geometry_pressure, pressure[val_index, :, 20:]),
        source_and_cache_unchanged=all(digest(root/'tools'/name)==sha for name,sha in report['source_sha256'].items())
            and digest(a.board)==report['teacher_board_sha256'] and digest(a.cache/'R.npy')==report['cache_R_sha256'],
        no_acoustic_projection=True, no_parameter_fit_yet=True)
    save()
    print('loaded teacher dataset complete', report['seconds'], 's', report['data_bytes'], 'bytes', flush=True)


if __name__ == '__main__':
    main()
