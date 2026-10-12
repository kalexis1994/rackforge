"""Retain the measured 20-port dry body; do not approve the failed CPU reserve."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import time
import numpy as np
from scipy.linalg import solve, svd
from concert_grand_projected_support import ModalSupportMap, projected_matrices
from concert_grand_realtime_body import Body
from concert_grand_modal_runtime import ModalRuntime
from concert_grand_reciprocal_runtime import ReciprocalRuntime
import concert_grand_orthotropic_plate as plate


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--local', type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists() or args.local.exists():
        raise ValueError('preserve previous diagnostics')
    root = Path(__file__).resolve().parent.parent
    parent_path = root/'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P7-cuerpo-realtime-2026-10-12/reciproco.json'
    parent = json.loads(parent_path.read_text())
    for name, sha in parent['source_sha256'].items():
        if digest(root/'tools'/name) != sha:
            raise ValueError('measured implementation changed')
    case = next(row for row in parent['cases'] if row['latent_ports'] == 20)
    if not case['transfer_gate_passed']:
        raise ValueError('no qualified dry mobility diagnostic')
    for rate in case['rates']:
        bench = rate['benchmark']
        if bench['p99_ms'] >= bench['deadline_ms'] or bench['max_ms'] >= bench['deadline_ms']:
            raise ValueError('standalone deadline failed')
    source = root/'local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    if digest(source) != parent['teacher_sha256']:
        raise ValueError('teacher changed')
    started = time.perf_counter()
    with np.load(source) as data:
        support = ModalSupportMap.from_matrices(*projected_matrices(data))
        geometry = dict(panel=json.loads(str(data['panel'])), boundary=str(data['boundary']),
                        nx=int(data['nx']), ny=int(data['ny']), vectors=data['ritz_vectors'])
        xs = geometry['panel']['length']*(.1+.8*(np.arange(21, 109)-21)/87)
        ports = (plate.shape(geometry, xs, np.full(88, geometry['panel']['width']/2)) @ support.basis).T
    omega = 2*np.pi*support.frequency
    full_modal = ModalRuntime.from_body(Body.mechanical(omega, support.damping, ports))
    train = ports[:, np.array(parent['training_keys'])-21]
    snapshots = []
    for hz in parent['training_frequency_hz']:
        s = 2j*np.pi*hz
        velocity = solve(np.diag(omega**2+s*s)+s*support.damping, s*train, check_finite=False)
        response = ports.T @ velocity
        scale = max(np.linalg.norm(response), 1e-30)
        snapshots.extend([response.real/scale, response.imag/scale])
    mapping, _, _ = svd(np.hstack(snapshots), full_matrices=False, check_finite=False)
    mapping = mapping[:, :20]
    modal = ModalRuntime(full_modal.roots, full_modal.vectors, full_modal.drive @ mapping,
                        (mapping.T @ full_modal.output.T).T, full_modal.multiplicity,
                        full_modal.condition, full_modal.eigen_residual)
    canonical = ReciprocalRuntime(modal)
    args.local.mkdir(parents=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    path = args.local/'ports-20.bin'
    with path.open('xb') as stream:
        stream.write(struct.pack('<8sII', b'RFPRT001', 88, 20))
        stream.write(np.asarray(mapping, dtype='<f8').tobytes())
    if digest(path) != case['mapping_sha256']:
        raise ValueError('diagnostic port map differs from measured artifact')
    rates = []
    for measured in case['rates']:
        rate = round(measured['benchmark']['rate_hz'])
        bundle = args.local/f'body-{rate}.bin'
        canonical.write(bundle, rate)
        if digest(bundle) != measured['coefficient_sha256']:
            raise ValueError('diagnostic model differs from measured artifact')
        rates.append(dict(rate_hz=rate, coefficient_path=str(bundle.resolve()),
            coefficient_sha256=digest(bundle), coefficient_bytes=bundle.stat().st_size,
            previously_measured_benchmark=measured['benchmark']))
    result = dict(stage='hash-identical standalone dry-body diagnostic retained',
        exporter_sha256=digest(__file__), measured_report_sha256=digest(parent_path),
        teacher_sha256=parent['teacher_sha256'], model='606 coordinates, full material loss, 20 reciprocal spatial force/velocity channels',
        mechanical_poles_removed=0, external_ports=88, mapping_path=str(path.resolve()),
        mapping_sha256=digest(path), mapping_bytes=path.stat().st_size, rates=rates,
        mobility_rms_relative=case['mobility_rms_relative'], mobility_worst_relative=case['mobility_worst_relative'],
        diagnostic_only=True, observed_standalone_deadline_passed=True,
        strict_body_20percent_budget_passed=case['body_budget_passed_both_rates'],
        full_piano_realtime=False, global_physics_approved=False, factory_updated=False,
        scope=parent['scope'], seconds=time.perf_counter()-started)
    args.report.write_text(json.dumps(result, indent=2, allow_nan=False)+'\n', encoding='utf-8')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
