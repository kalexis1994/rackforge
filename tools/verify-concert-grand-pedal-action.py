"""Verify contracted pedal evidence and signed work without rerunning dynamics."""
import hashlib
import json
from pathlib import Path

import numpy as np
from scipy.signal import resample_poly


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    root = Path(__file__).resolve().parent.parent
    folder = root / 'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-pedales-continuos-2026-10-11'
    report = json.loads((folder / 'pedales.json').read_text())
    if report['stage'] != 'continuous pedals and signed parameter work measured':
        raise ValueError('incomplete probe')
    if not all(digest(root / 'tools' / name) == expected
               for name, expected in report['source_sha256'].items()):
        raise ValueError('measured sources changed')
    plan_path = folder.parent / 'medidas-P6-entrada-MIDI-2026-10-11/plan.json'
    if digest(plan_path) != report['plan_sha256']:
        raise ValueError('preserved plan changed')
    plan = next(row['plan'] for row in json.loads(plan_path.read_text())['cases']
                if 'campanella' in Path(row['plan']['midi']).stem)
    if digest(plan['midi']) != report['midi_sha256']:
        raise ValueError('original MIDI changed')
    first = next(event for event in plan['events'] if event['kind'] == 'on')
    prefix = plan['events'][:plan['events'].index(first)]
    data_path = folder / 'presion.npz'
    if digest(data_path) != report['data']['sha256'] or data_path.stat().st_size != report['data']['bytes']:
        raise ValueError('contracted data changed')
    rows = []
    with np.load(data_path) as data:
        for case in report['cases']:
            name = case['name']
            if case['original_pre_attack_events'] != prefix or case['attack_event'] != first:
                raise ValueError('changed original pre-strike events')
            native, audio = data[name + '_native'], data[name + '_audio']
            if native.shape != (1218, 2) or audio.shape != (457, 2):
                raise ValueError('unexpected pressure/audio duration')
            if not np.isfinite(native).all() or not np.isfinite(audio).all():
                raise ValueError('nonfinite data')
            np.testing.assert_allclose(audio, resample_poly(native * .25, 3, 8, axis=0),
                                       rtol=1e-13, atol=1e-16)
            np.testing.assert_allclose(np.max(abs(audio)), case['audio_peak'], rtol=1e-14)
            np.testing.assert_allclose(np.sqrt(np.mean(audio**2)), case['audio_rms'], rtol=1e-14)
            metrics = case['metrics']
            if metrics['contact_steps'] + 2 * metrics['coast_steps'] != 1536:
                raise ValueError('mechanical clock mismatch')
            heat_names = ['felt_heat_j', 'material_heat_j', 'damper_heat_j']
            if any(metrics[key] < 0 for key in heat_names):
                raise ValueError('negative dissipative heat')
            residual = (metrics['final_mechanical_j'] - metrics['external_head_work_j']
                        - metrics['pedal_parameter_work_j']
                        + sum(metrics[key] for key in heat_names) + metrics['air_port_work_j'])
            np.testing.assert_allclose(residual, metrics['mechanical_work_residual_j'],
                                       rtol=0, atol=1e-19)
            denominator = abs(metrics['external_head_work_j']) + abs(metrics['pedal_parameter_work_j'])
            relative = abs(residual) / denominator
            np.testing.assert_allclose(relative, case['mechanical_work_relative'], rtol=1e-7, atol=1e-15)
            if relative > report['criteria']['mechanical_work_relative']:
                raise ValueError('mechanical work gate failed')
            if name == 'original_pre_attack_controllers':
                if case['final_shift'] != 1. or metrics['pedal_parameter_work_j'] != 0.:
                    raise ValueError('settled original soft action changed')
            else:
                if metrics['pedal_parameter_work_j'] == 0. or metrics['damper_heat_j'] <= 0.:
                    raise ValueError('motion did not reach work or dissipation')
            rows.append(dict(name=name, clock_passed=True, original_pre_strike_values_preserved=True,
                scipy_resampling_passed=True, peak_rms_passed=True, nonnegative_heat_passed=True,
                signed_parameter_work_j=metrics['pedal_parameter_work_j'],
                mechanical_work_relative=relative, mechanical_work_gate_passed=True))
    result = dict(stage='independent pedal contracted-data verification',
        verifier_sha256=digest(__file__), sources_plan_midi_and_archive_hashes_passed=True,
        cases=rows, local_controls_verified=True, global_physics_approved=False,
        scope='Integrity, clock, scalar energy arithmetic and SciPy resampling of archived capsule pressure. Does not reconstruct modal states, actuator forces, or certify temporal/spatial convergence or piano timbre.')
    (folder / 'verificacion.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
