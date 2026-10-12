"""Batch exact full Padé correlations; one R view, no acoustic projection."""
import numpy as np
from scipy.linalg import solve


def wave_impedance_blocks(wave, frequencies, block=12):
    f = np.asarray(frequencies, dtype=float)
    if (f.ndim != 1 or not np.isfinite(f).all() or np.any(f < 0)
            or not isinstance(block, int) or block < 1):
        raise ValueError('nonnegative frequencies and positive block')
    if not wave.R.flags.c_contiguous:
        raise ValueError('contiguous shared R view required; never duplicate R')
    flat = wave.R[1:].reshape(wave.order, -1)
    if not np.shares_memory(flat, wave.R):
        raise ValueError('correlation view unexpectedly copied')
    for first in range(0, len(f), block):
        current = f[first:first+block]
        delay = np.array([wave.delay(2j*np.pi*hz) for hz in current])
        weights = delay[:, None]**np.arange(1, wave.order+1)
        real = weights.real @ flat
        imag = weights.imag @ flat
        z = (real+1j*imag).reshape(len(current), wave.modes, wave.modes)
        z += wave.D
        yield first, current, z


def observer_transfer(kernel, rate, frequencies):
    f = np.asarray(frequencies, dtype=float)
    k = np.asarray(kernel, dtype=float)
    if (k.ndim != 3 or not np.isfinite(k).all() or not np.isfinite(rate)
            or rate <= 0 or f.ndim != 1 or not np.isfinite(f).all()):
        raise ValueError('finite retarded observer kernel and clock')
    phase = np.exp(-2j*np.pi*f[:, None]*np.arange(len(k))/rate)
    return (phase @ k.reshape(len(k), -1)).reshape(len(f), k.shape[1], k.shape[2])


def mobility_and_pressure(omega, damping, impedance, forces, readout, hz):
    s = 2j*np.pi*hz
    dynamic = np.diag(np.asarray(omega)**2+s*s)+s*damping+s*impedance
    velocity = solve(dynamic, s*forces, check_finite=False)
    return forces.T @ velocity, s*readout @ velocity
