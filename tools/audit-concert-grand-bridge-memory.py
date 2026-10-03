"""Passive envelope control for the frozen-frequency bridge approximation.

One collective string partial exchanges energy with the exported native board
mode bank. This is a rotating-envelope approximation, not a replacement piano
or exact bidirectional string solver. Its steady admittance matches the native
loss and reactance exactly at the tested carrier. Hammer, unequal string losses,
horizontal dark modes, receiver and auxiliary paths are excluded. Consequently
string-envelope results must not be called measured microphone improvement.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path

import numpy as np
from scipy.linalg import eig, solve


def generator(modes, frequency, norm, intrinsic_rate, sample_rate=44100):
    omega = 2*sample_rate*np.tan(np.pi*frequency/sample_rate)
    delta = (modes['omega2']-omega**2)/(2*omega)
    sigma = modes['sigma']
    g2 = norm*modes['tension_length_decay_scale']*modes['inverse_mass']*modes['projection']**2/2
    g = np.sqrt(g2)*np.sign(modes['projection'])
    a = -np.diag(np.r_[complex(intrinsic_rate), sigma+1j*delta])
    a[0, 1:] = -1j*g
    a[1:, 0] = -1j*g
    steady = np.sum(g2/(sigma+1j*delta))
    return a, steady, np.r_[intrinsic_rate, sigma]


def evolve(a, times):
    poles, vectors = eig(a)
    initial = np.zeros(len(a), dtype=complex)
    initial[0] = 1
    weights = solve(vectors, initial)
    states = vectors @ (weights[:, None]*np.exp(poles[:, None]*times[None, :]))
    assert np.max(abs(states[:, 0]-initial)) < 1e-8
    # Check the defining ODE against the eigensystem, independent of the
    # diagonal energy identity. A bad eigensolve must not pass by passivity.
    derivative = vectors @ ((poles*weights)[:, None]*np.exp(poles[:, None]*times[None, :]))
    residual = np.linalg.norm(derivative-a@states)/max(np.linalg.norm(derivative), 1e-20)
    assert residual < 1e-8, residual
    return states, residual, float(poles.real.max())


def window_fall(amplitude, times):
    level = lambda lo, hi: 10*np.log10(np.mean(abs(amplitude[(times >= lo)&(times < hi)])**2)+1e-30)
    return float(level(.5, 1.5)-level(.05, .2))


def read_csv(path):
    with path.open() as stream:
        rows = list(csv.DictReader(stream))
    return {key: np.array([float(row[key]) for row in rows]) for key in rows[0]}


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for key in ('modes', 'loading', 'output'):
        p.add_argument('--'+key, type=Path, required=True)
    p.add_argument('--modes-renderer', type=Path, required=True)
    p.add_argument('--expected-sha256', required=True)
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=True)
    times = np.arange(0, 1.701, .002)
    assert sha(a.modes_renderer) == a.expected_sha256
    report = dict(policy=__doc__, rows={}, initial_string_energy=1, initial_board_energy=0,
                  time_step_s=.002, tuning_changes=False, modes_renderer_sha256=a.expected_sha256, sources={})
    for note in range(36, 97, 3):
        modes = read_csv(a.modes/f'modes{note:03}.csv')
        loading = read_csv(a.loading/f'loading{note:03}v60.csv')
        report['sources'][str(note)] = dict(modes_sha256=sha(a.modes/f'modes{note:03}.csv'),
                                            loading_sha256=sha(a.loading/f'loading{note:03}v60.csv'))
        for partial in range(min(3, len(loading['frequency_hz']))):
            f, norm, r = (loading[key][partial] for key in
                         ('frequency_hz', 'mechanical_norm', 'intrinsic_amplitude_rate_s'))
            matrix, steady, losses = generator(modes, f, norm, r)
            native_gamma = norm*loading['intended_rate_s'][partial]
            assert abs(steady.real-native_gamma) < 2e-5*max(native_gamma, 1), (steady, native_gamma)
            states, residual, pole = evolve(matrix, times)
            energy = np.sum(abs(states)**2, axis=0)
            assert energy.max() <= 1+1e-8 and np.max(np.diff(energy)) < 1e-8
            analytic_loss = -2*np.sum(losses[:, None]*abs(states)**2, axis=0)
            ode_energy = 2*np.real(np.sum(np.conj(states)*(matrix@states), axis=0))
            assert np.max(abs(analytic_loss-ode_energy)) < 1e-8
            frozen = np.exp(-(r+steady)*times)
            fall = window_fall(states[0], times)
            fixed_fall = window_fall(frozen, times)
            report['rows'][f'{note}:{partial+1}'] = dict(frequency_hz=float(f),
                intrinsic_rate_s=float(r), collective_bridge_rate_s=float(steady.real),
                native_collective_bridge_rate_s=float(native_gamma), steady_reactivity_s=float(-steady.imag),
                dynamic_string_fall_db=fall, frozen_string_fall_db=fixed_fall,
                fall_change_db=fall-fixed_fall, maximum_energy=float(energy.max()),
                final_board_energy=float(np.sum(abs(states[1:, -1])**2)),
                maximum_pole_real_s=pole, eigensolve_relative_residual=residual)
        print(note, 'H1 fall change', round(report['rows'][f'{note}:1']['fall_change_db'], 4), flush=True)
    report['maximum_absolute_fall_change_db'] = max(abs(row['fall_change_db']) for row in report['rows'].values())
    (a.output/'memory-report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
