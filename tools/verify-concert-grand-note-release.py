"""Authenticate the delivered note and independently integrate saved spectra.

Does not rerun dynamics or duplicate audio/states. Grid checks remain local;
the saved spectra cannot establish an independent Hermite source oracle.
"""
import argparse
import hashlib
import json
from pathlib import Path

import numpy as np
from scipy.io import wavfile


def digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(8 * 1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError('preserve existing verification')
    report = json.loads(args.report.read_text(encoding='utf-8'))
    if not report['approved_for_audition']:
        raise ValueError('producer not completed')
    root = Path(__file__).resolve().parent.parent
    sources = {name: digest(root / 'tools' / name) == expected
               for name, expected in report['source_sha256'].items()}
    audio_entry = report['audio_file']
    rate, audio = wavfile.read(audio_entry['path'])
    assert rate == 48000 and audio.dtype == np.float32
    assert audio.shape == (audio_entry['frames'], 2)
    assert np.isfinite(audio).all()
    assert digest(audio_entry['path']) == audio_entry['sha256']
    assert float(abs(audio).max()) == audio_entry['peak']
    lead = round(report['lead_s'] * rate)
    assert not audio[:lead].any()
    held = float(np.sqrt(np.mean(audio[lead + 2400:lead + 7200].astype(float)**2)))
    tail = float(np.sqrt(np.mean(audio[-960:].astype(float)**2)))
    ratio = tail / held
    assert abs(ratio - report['tail_check']['relative']) < 1e-14
    trajectory = report['trajectory']
    ledger = report['mechanical_work_ledger']
    residual = (trajectory['final_mechanical_j'] - trajectory['external_head_work_j']
                + trajectory['felt_heat_j'] + trajectory['material_heat_j']
                + trajectory['damper_heat_j'] + ledger['air_port_work_j'])
    assert residual == ledger['residual_j']
    assert abs(residual) / trajectory['external_head_work_j'] == ledger['relative']
    spectral = report['release_spectral_data']
    assert digest(spectral['path']) == spectral['sha256']
    rows = []
    bands = [(60., 800.), (800., 2500.), (2500., 6000.), (6000., 12000.)]
    with np.load(spectral['path'], allow_pickle=False) as archive:
        f = archive['frequency_hz']
        assert len(f) == 1913 and np.isfinite(f).all() and (np.diff(f) > 0).all()
        def norms(x, stride):
            grid, power = f[::stride], np.sum(abs(x[::stride])**2, axis=1)
            result = []
            for low, high in bands:
                mask = (grid >= low) & (grid <= high)
                assert grid[mask][0] == low and grid[mask][-1] == high
                result.append(np.trapezoid(power[mask], grid[mask]))
            return np.asarray(result)
        for case in report['release_cases']:
            for window in ['rectangular', 'hann']:
                actual = archive[f"release{case['rate_hz']}_{window}"]
                reference = archive[f'release256000_{window}']
                assert np.isfinite(actual).all() and np.isfinite(reference).all()
                energies = norms(reference, 1)
                relative = np.sqrt(norms(actual - reference, 1) / energies)
                registered = np.array([b['relative'] for b in case['windows'][window]['bands']])
                assert np.allclose(relative, registered, rtol=1e-12, atol=1e-14)
                coarse_energy = norms(reference, 2)
                coarse_relative = np.sqrt(norms(actual - reference, 2) / coarse_energy)
                energy_change = float(max(abs(coarse_energy - energies) / energies))
                error_change = float(max(abs(coarse_relative - relative)))
                rows.append(dict(rate_hz=case['rate_hz'], window=window,
                                 band_relative=relative.tolist(),
                                 nested_energy_relative=energy_change,
                                 nested_error_absolute=error_change,
                                 grid_passed=bool(energy_change <= report['criteria']['band_energy_grid_relative']
                                                  and error_change <= report['criteria']['band_error_grid_absolute'])))
    board = root / 'local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    residue = root / 'local/concert-grand-physics/P6-carga-causal-2026-10-06/R.npy'
    caches = dict(board=digest(board) == report['board_sha256'],
                  residue=digest(residue) == report['cache_R_sha256'])
    assert all(sources.values()) and all(caches.values())
    result = dict(report_sha256=digest(args.report), verifier_sha256=digest(__file__),
                  source_verified=sources, cache_verified=caches,
                  audio_verified=True, audio_peak=audio_entry['peak'],
                  tail_relative=ratio, spectral_archive_verified=True,
                  mechanical_ledger_arithmetic_verified=True,
                  independently_integrated_bands=rows,
                  nested_grid_passed=all(row['grid_passed'] for row in rows),
                  independent_hermite_oracle_measured=False,
                  full_temporal_convergence_established=False)
    args.output.write_text(json.dumps(result, indent=2, allow_nan=False), encoding='utf-8')
    print(json.dumps(dict(audio_verified=True, tail_relative=ratio,
                         sources=len(sources), caches=caches,
                         nested_grid_passed=result['nested_grid_passed'])))


if __name__ == '__main__':
    main()
