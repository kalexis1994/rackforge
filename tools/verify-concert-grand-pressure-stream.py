"""Verify saved pressure evidence without rerunning mechanics or loading R.

Independent DTFT, band quadrature, SciPy resampling and ledger arithmetic.
The archived analytic spectrum is the frozen observer's result, not another
independent reconstruction of the unsaved modal B/V trajectory.
"""
from pathlib import Path
import hashlib
import json

import numpy as np
from scipy.signal import resample_poly


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    root = Path(__file__).resolve().parent.parent
    folder = root / 'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-presion-bloques-2026-10-11'
    report = json.loads((folder / 'presion.json').read_text())
    prior_path = folder.parent / 'medidas-P6-entrada-MIDI-2026-10-11/registro.json'
    prior = json.loads(prior_path.read_text())
    if not all(digest(root / 'tools' / name) == expected
               for name, expected in report['source_sha256'].items()):
        raise ValueError('measured source changed')
    if digest(prior_path) != report['reference_report_sha256']:
        raise ValueError('reference report changed')
    archive = folder / 'presion.npz'
    if digest(archive) != report['data']['sha256'] or archive.stat().st_size != report['data']['bytes']:
        raise ValueError('archive hash/size mismatch')
    with np.load(archive) as data:
        if not all(np.isfinite(data[name]).all() for name in data.files):
            raise ValueError('nonfinite contracted evidence')
        f, reference, actual = (data[name] for name in
                               ['frequency_hz', 'analytic_pressure', 'streamed_pressure'])
        shapes = dict(frequency_hz=(1913,), analytic_pressure=(1913, 2),
                      streamed_pressure=(1913, 2), streamed_native=(1218, 2),
                      legacy_native=(1213, 2), streamed_audio=(457, 2), legacy_audio=(455, 2))
        if any(data[name].shape != shape for name, shape in shapes.items()):
            raise ValueError('unexpected archive shape')
        bands = [(60., 800.), (800., 2500.), (2500., 6000.), (6000., 12000.)]

        def integrals(frequency, values):
            power = np.sum(abs(values)**2, axis=1)
            result = []
            for low, high in bands:
                mask = (frequency >= low) & (frequency <= high)
                if frequency[mask][0] != low or frequency[mask][-1] != high:
                    raise ValueError('band edges missing')
                result.append(np.trapezoid(power[mask], frequency[mask]))
            return np.array(result)

        energy, difference = integrals(f, reference), integrals(f, actual - reference)
        relative, absolute = np.sqrt(difference / energy), np.sqrt(difference / energy.sum())
        for row, r, a, share in zip(report['analytic_pressure_bands']['bands'],
                                    relative, absolute, energy / energy.sum()):
            np.testing.assert_allclose([r, a, share],
                [row['relative'], row['absolute_relative_to_total'], row['reference_energy_share']],
                rtol=2e-13, atol=0)
        np.testing.assert_allclose(np.sqrt(difference.sum() / energy.sum()),
                                   report['analytic_pressure_bands']['total_relative'], rtol=2e-13)
        coarse_energy = integrals(f[::2], reference[::2])
        coarse_difference = integrals(f[::2], (actual - reference)[::2])
        for row, r in zip(report['nested_pressure_bands']['bands'],
                          np.sqrt(coarse_difference / coarse_energy)):
            np.testing.assert_allclose(r, row['relative'], rtol=2e-13)
        grid_rows = []
        for i, (low, high) in enumerate(bands):
            energy_change = abs(coarse_energy[i] - energy[i]) / energy[i]
            error_change = abs(np.sqrt(coarse_difference[i] / coarse_energy.sum()) - absolute[i])
            grid_rows.append(dict(lower_hz=low, upper_hz=high,
                energy_relative_change=float(energy_change), absolute_error_change=float(error_change),
                energy_gate_passed=bool(energy_change <= .001), error_gate_passed=bool(error_change <= .0001)))
        # Direct transform, independent of the pressure streaming helper.
        dtft = np.exp(-2j * np.pi * f[:, None] * np.arange(1218) / 128000) @ data['streamed_native'] / 128000
        np.testing.assert_allclose(dtft, actual, rtol=2e-13, atol=1e-20)
        resampling_errors = {}
        for name in ['streamed', 'legacy']:
            expected = resample_poly(data[name + '_native'] * .25, 3, 8, axis=0)
            observed = data[name + '_audio']
            np.testing.assert_allclose(expected, observed, rtol=1e-13, atol=1e-16)
            resampling_errors[name] = float(np.max(abs(expected - observed)))
        waveform = float(np.linalg.norm(data['streamed_audio'][:455] - data['legacy_audio'])
                         / np.linalg.norm(data['legacy_audio']))
        np.testing.assert_allclose(waveform, report['legacy_wav_comparison']['waveform_relative'], rtol=2e-13)
        probe = report['probe']
        if any(value != prior['probe'][key] for key, value in probe.items() if key != 'seconds'):
            raise ValueError('physical trajectory metrics changed')
        # Producer convention: final energy + losses/air work - applied work.
        residual = sum(probe[key] for key in ['felt_heat_j', 'material_heat_j',
            'damper_heat_j', 'air_port_work_j', 'final_mechanical_j']) - probe['external_head_work_j']
        np.testing.assert_allclose(residual, probe['mechanical_work_residual_j'], rtol=0, atol=1e-19)
        result = dict(stage='independent contracted-data verification', verifier_sha256=digest(__file__),
            source_hashes_passed=True, reference_hash_passed=True, archive_hash_size_and_finiteness_passed=True,
            archive_shapes={name: list(data[name].shape) for name in data.files},
            pressure_band_metrics_reproduced=True, direct_dtft_passed=True, scipy_resampling_passed=True,
            resampling_maximum_absolute_errors=resampling_errors, waveform_relative=waveform,
            same_physical_probe_metrics_except_time=True,
            mechanical_work_residual_relative=abs(residual) / probe['external_head_work_j'],
            nested_grid_criteria=dict(energy_relative=.001, absolute_error=.0001),
            nested_grid_bands=grid_rows,
            nested_grid_all_passed=all(row['energy_gate_passed'] and row['error_gate_passed'] for row in grid_rows),
            local_observer_controls_verified=True, global_physics_approved=False,
            scope='Archive/source integrity, direct pressure DTFT, independent band quadrature, SciPy resampling and ledger arithmetic. Analytic source is archived frozen-observer output; B/V not retained on disk. Nested grids control band integration of this 6ms probe only.')
    (folder / 'verificacion.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
