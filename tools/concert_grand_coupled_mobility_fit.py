"""Fit reciprocal nonproportional losses with C=positive diagonal+LL^T.

Stiffness, masses and force/velocity ports are fixed, hence both storage
moments stay unchanged. This is a reduced loaded-port model, not new felt.
"""
import time
import numpy as np
from scipy.linalg import solve
from scipy.optimize import minimize
from scipy.signal import find_peaks


def damping(parameters, base, rank):
    n = len(base)
    diagonal = base*np.exp(parameters[:n])
    u = parameters[n:].reshape(n, rank)
    factor = np.sqrt(base)[:, None]*u
    return np.diag(diagonal)+factor @ factor.T, diagonal, factor


def objective_gradient(parameters, omega, base, ports, frequency, target, rank, ridge=1e-7):
    c, diagonal, factor = damping(parameters, base, rank)
    q, actual = [], []
    s = 2j*np.pi*np.asarray(frequency)
    for value in s:
        state = solve(np.diag(omega**2+value*value)+value*c, ports, assume_a='sym', check_finite=False)
        q.append(state)
        actual.append(value*ports.T @ state)
    q, actual = np.asarray(q), np.asarray(actual)
    error = actual-target
    f = np.asarray(frequency)
    df = np.r_[(f[1]-f[0])/2, (f[2:]-f[:-2])/2, (f[-1]-f[-2])/2]
    weights = df/df.sum()/np.maximum(np.linalg.norm(target, axis=(1, 2))**2, 1e-30)
    loss = .5*float(np.sum(weights*np.linalg.norm(error, axis=(1, 2))**2))
    gradient_c = np.einsum('f,fia,fab,fjb->ij', -s*s*weights, q, error.conj(), q, optimize=True).real
    gradient_c = (gradient_c+gradient_c.T)/2
    gradient = np.r_[np.diag(gradient_c)*diagonal,
                     (2*np.sqrt(base)[:, None]*(gradient_c @ factor)).ravel()]
    loss += .5*ridge*float(parameters @ parameters)
    gradient += ridge*parameters
    return loss, gradient


def fit_coupled_losses(bank, frequency, target, rank=8, max_iterations=500, progress=None):
    f = np.asarray(frequency)
    trace = np.trace(target.real, axis1=1, axis2=2)
    peaks, _ = find_peaks(trace, prominence=trace.max()*1e-4)
    selected = np.unique(np.r_[np.arange(0, len(f), 12),
        np.concatenate([np.arange(max(0, p-4), min(len(f), p+5)) for p in peaks]), len(f)-1]).astype(int)
    base = 2*bank.gamma
    initial = np.r_[np.zeros(len(base)), np.random.default_rng(17).normal(0., .025, size=(len(base), rank)).ravel()]
    started = last = time.perf_counter()
    history = []
    def callback(parameters):
        nonlocal last
        if time.perf_counter()-last > 25:
            loss, gradient = objective_gradient(parameters, bank.omega, base, bank.ports, f[selected], target[selected], rank)
            row = dict(seconds=time.perf_counter()-started, loss=loss, gradient_max=float(abs(gradient).max()))
            history.append(row)
            if progress:
                progress(row)
            last = time.perf_counter()
    result = minimize(lambda x:objective_gradient(x, bank.omega, base, bank.ports, f[selected], target[selected], rank),
        initial, jac=True, method='L-BFGS-B', callback=callback,
        bounds=[(-5., 5.)]*len(base)+[(None, None)]*(len(base)*rank),
        options=dict(maxiter=max_iterations, ftol=1e-11, gtol=1e-7, maxls=40, maxcor=20))
    c, _, _ = damping(result.x, base, rank)
    if not np.isfinite(c).all() or np.linalg.eigvalsh(c).min() <= 0:
        raise ValueError('positive coupled loss matrix required')
    return c, dict(optimizer_success=bool(result.success), optimizer_message=str(result.message),
        optimizer_iterations=int(result.nit), objective=float(result.fun), gradient_max=float(abs(result.jac).max()),
        coupled_loss_rank=rank, fit_frequency_count=len(selected), fit_frequency_hz=f[selected].tolist(),
        fixed_seed=17, diagonal_log_bounds=[-5.,5.], ridge=1e-7, progress=history,
        negative_poles_or_loss_repair=False)
