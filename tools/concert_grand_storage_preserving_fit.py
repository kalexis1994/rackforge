"""Learn passive reduced dynamics without changing either storage moment.

The virtual stiffness is K=Omega [I+W(exp(-k)-1)W^T] Omega,
where W is orthonormal and W^T Omega^-1 B=0. Consequently
B^T K^-1 B stays fixed, as does B^T B (unit modal masses).
Losses are a congruence of a positive base plus LL^T. No pole repair.
"""
import time
import numpy as np
from scipy.linalg import null_space, svd, solve
from scipy.optimize import minimize
from scipy.signal import find_peaks


def storage_basis(omega, ports, damping, frequency):
    # Leave the very stiff moment-completion modes fixed. They are retained
    # in every solve; this limits fitting directions, not physical states.
    active = omega <= 2*np.pi*30000.
    small_null = null_space((ports[active]/omega[active, None]).T)
    null = np.zeros((len(omega), small_null.shape[1]))
    null[active] = small_null
    snapshots = []
    for hz in np.asarray(frequency)[::24]:
        s = 2j*np.pi*hz
        q = solve(np.diag(omega**2+s*s)+s*damping, ports, assume_a='sym', check_finite=False)
        projected = null.T @ (omega[:, None]*q)
        for value in [projected.real, projected.imag]:
            norms = np.linalg.norm(value, axis=0)
            keep = norms > 1e-25
            snapshots.append(value[:, keep]/norms[keep])
    rotation, _, _ = svd(np.hstack(snapshots), full_matrices=True, check_finite=False)
    return null @ rotation


def operators(parameters, omega, base_loss, basis, rank):
    r, n = basis.shape[1], len(omega)
    inverse_shift = np.exp(-parameters[:r])
    scaled_basis = omega[:, None]*basis
    stiffness = np.diag(omega**2)+(scaled_basis*(inverse_shift-1)) @ scaled_basis.T
    scale = np.exp(parameters[r:r+n])
    loss = scale[:, None]*base_loss*scale[None]
    u = parameters[r+n:].reshape(n, rank)
    root_loss = np.sqrt(np.diag(base_loss))
    factor = root_loss[:, None]*u
    return stiffness, loss+factor @ factor.T, loss, factor


def objective_gradient(parameters, omega, ports, base_loss, basis, frequency, target, rank, ridge=1e-7):
    k, c, scaled_loss, factor = operators(parameters, omega, base_loss, basis, rank)
    s = 2j*np.pi*np.asarray(frequency)
    q = np.array([solve(k+value*c+value*value*np.eye(len(omega)), ports,
                       assume_a='sym', check_finite=False) for value in s])
    actual = s[:, None, None]*np.einsum('na,fnb->fab', ports, q, optimize=True)
    error = actual-target
    f = np.asarray(frequency)
    df = np.r_[(f[1]-f[0])/2, (f[2:]-f[:-2])/2, (f[-1]-f[-2])/2]
    weight = df/df.sum()/np.maximum(np.linalg.norm(target, axis=(1, 2))**2, 1e-30)
    loss = .5*float(np.sum(weight*np.linalg.norm(error, axis=(1, 2))**2))
    gk = np.einsum('f,fia,fab,fjb->ij', -s*weight, q, error.conj(), q, optimize=True).real
    gc = np.einsum('f,fia,fab,fjb->ij', -s*s*weight, q, error.conj(), q, optimize=True).real
    gk, gc = (gk+gk.T)/2, (gc+gc.T)/2
    scaled = omega[:, None]*basis
    gradient = np.r_[-np.exp(-parameters[:basis.shape[1]])*np.sum(scaled*(gk @ scaled), axis=0),
                     2*np.sum(gc*scaled_loss, axis=1),
                     (2*np.sqrt(np.diag(base_loss))[:, None]*(gc @ factor)).ravel()]
    return loss+.5*ridge*float(parameters @ parameters), gradient+ridge*parameters


def fit_storage_preserving(omega, ports, base_loss, frequency, target, rank=8, progress=None):
    basis = storage_basis(omega, ports, base_loss, frequency)
    f = np.asarray(frequency)
    trace = np.trace(target.real, axis1=1, axis2=2)
    peaks, _ = find_peaks(trace, prominence=trace.max()*1e-4)
    selected = np.unique(np.r_[np.arange(0, len(f), 12),
        np.concatenate([np.arange(max(0, p-4), min(len(f), p+5)) for p in peaks]), len(f)-1]).astype(int)
    n, r = len(omega), basis.shape[1]
    initial = np.r_[np.zeros(r+n), np.random.default_rng(19).normal(0., .01, (n, rank)).ravel()]
    started = last = time.perf_counter()
    history = []
    def callback(parameters):
        nonlocal last
        if time.perf_counter()-last >= 25:
            value, gradient = objective_gradient(parameters, omega, ports, base_loss, basis, f[selected], target[selected], rank)
            row = dict(seconds=time.perf_counter()-started, objective=value, gradient_max=float(abs(gradient).max()))
            history.append(row)
            if progress:
                progress(row)
            last = time.perf_counter()
    result = minimize(lambda x:objective_gradient(x, omega, ports, base_loss, basis, f[selected], target[selected], rank),
        initial, jac=True, method='L-BFGS-B', callback=callback,
        bounds=[(-3.,3.)]*r+[(-2.5,2.5)]*n+[(None,None)]*(n*rank),
        options=dict(maxiter=800, ftol=1e-11, gtol=1e-7, maxls=40, maxcor=30))
    k, c, _, _ = operators(result.x, omega, base_loss, basis, rank)
    return k, c, dict(optimizer_success=bool(result.success), optimizer_message=str(result.message),
        optimizer_iterations=int(result.nit), objective=float(result.fun), gradient_max=float(abs(result.jac).max()),
        stiffness_complement_rank=r, extra_loss_rank=rank, fit_frequency_count=len(selected),
        fit_frequency_hz=f[selected].tolist(), stiffness_log_bounds=[-3.,3.], loss_log_bounds=[-2.5,2.5],
        fixed_seed=19, ridge=1e-7, progress=history, pole_or_loss_clipping=False)
