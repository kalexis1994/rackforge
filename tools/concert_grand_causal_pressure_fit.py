"""Real acceleration readouts with delays inside the teacher's acoustic support.

This observation fit never feeds pressure into the force/velocity component.
Delay groups operate on scalar microphone sums, not per-mode audio buffers.
It is a band-limited harmonic identification, not a transient certificate.
"""
import numpy as np
from scipy.linalg import solve


def support_delays(kernel, rate, groups=4):
    k = np.asarray(kernel)
    if k.ndim != 3 or not np.isfinite(k).all() or rate <= 0 or groups < 1:
        raise ValueError('finite retarded kernel and positive clock/groups')
    delays = []
    for microphone in range(k.shape[1]):
        active = np.flatnonzero(np.any(k[:, microphone] != 0, axis=1))
        if not len(active):
            raise ValueError('empty microphone propagation support')
        # No advance or delay invented beyond the existing retarded kernel.
        delays.append(np.linspace(active[0], active[-1], groups)/rate)
    return np.asarray(delays)


def acceleration_response(omega, damping, ports, frequency):
    omega, ports = np.asarray(omega), np.asarray(ports)
    return np.array([s*s*solve(np.diag(omega**2+s*s)+s*damping,
                              ports, assume_a='sym', check_finite=False)
                     for s in 2j*np.pi*np.asarray(frequency)])


def pressure_response(acceleration, frequency, delays, readout):
    phase = np.exp(-2j*np.pi*np.asarray(frequency)[:, None, None]*delays[None])
    return np.einsum('fmg,mgn,fnp->fmp', phase, readout, acceleration, optimize=True)


def fit_readout(acceleration, frequency, target, delays, ridge=1e-6):
    f, target = np.asarray(frequency), np.asarray(target)
    if (target.shape != (len(f), len(delays), acceleration.shape[2])
            or ridge <= 0 or not np.isfinite(target).all()):
        raise ValueError('matching pressure target and positive regularization')
    df = np.r_[(f[1]-f[0])/2, (f[2:]-f[:-2])/2, (f[-1]-f[-2])/2]
    weights = np.sqrt(df/df.sum())/np.maximum(np.linalg.norm(target, axis=(1, 2)), 1e-30)
    readouts, conditions = [], []
    for microphone, delay in enumerate(delays):
        phase = np.exp(-2j*np.pi*f[:, None]*delay)
        design = np.einsum('fg,fnp->fpgn', phase, acceleration).reshape(len(f)*acceleration.shape[2], -1)
        weight = np.repeat(weights, acceleration.shape[2])
        design *= weight[:, None]
        rhs = target[:, microphone].reshape(-1)*weight
        scale = np.maximum(np.linalg.norm(design, axis=0), 1e-30)
        design /= scale
        # A real readout is essential for a real causal microphone signal.
        gram = (design.conj().T @ design).real
        vector = (design.conj().T @ rhs).real
        coefficients = solve(gram+ridge*np.eye(len(gram)), vector, assume_a='pos')/scale
        readouts.append(coefficients.reshape(len(delay), acceleration.shape[1]))
        conditions.append(float(np.linalg.cond(gram+ridge*np.eye(len(gram)))))
    return np.asarray(readouts), dict(ridge=ridge, normalized_gram_conditions=conditions,
        training_frequency_count=len(f), real_readout=True,
        arbitrary_microphone_gain_or_individual_normalization=False)
