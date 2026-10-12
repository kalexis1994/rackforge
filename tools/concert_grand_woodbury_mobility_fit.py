"""Same positive loss family and objective; low-rank solves/adjoints.

Woodbury reduces each dynamic solve to the rank of LL^T. Direct contractions
compute parameter gradients without constructing the dense loss gradient.
Only preparation/identification uses this; callback coefficients stay exact.
"""
import time
import numpy as np
from scipy.linalg import solve
from scipy.optimize import minimize
from scipy.signal import find_peaks


def response_states(parameters, omega, base, ports, frequency, rank):
    n = len(base)
    diagonal = base*np.exp(parameters[:n])
    factor = np.sqrt(base)[:, None]*parameters[n:].reshape(n, rank)
    s = 2j*np.pi*np.asarray(frequency)
    inverse = 1/(omega[None]**2+s[:, None]**2+s[:, None]*diagonal)
    free = inverse[:, :, None]*ports
    dynamic_factor = inverse[:, :, None]*factor
    small = np.einsum('nr,fnt->frt', factor, dynamic_factor, optimize=True)
    small += np.eye(rank)[None]/s[:, None, None]
    rhs = np.einsum('nr,fnp->frp', factor, free, optimize=True)
    correction = np.array([solve(a,b,assume_a='sym',check_finite=False) for a,b in zip(small,rhs)])
    q = free-np.einsum('fnr,frp->fnp', dynamic_factor, correction, optimize=True)
    return q, diagonal, factor


def objective_gradient(parameters, omega, base, ports, frequency, target, rank, ridge=1e-7):
    q, diagonal, factor = response_states(parameters, omega, base, ports, frequency, rank)
    s = 2j*np.pi*np.asarray(frequency)
    actual = s[:, None, None]*np.einsum('na,fnb->fab', ports, q, optimize=True)
    error = actual-target
    f = np.asarray(frequency)
    df = np.r_[(f[1]-f[0])/2, (f[2:]-f[:-2])/2, (f[-1]-f[-2])/2]
    weights = df/df.sum()/np.maximum(np.linalg.norm(target,axis=(1,2))**2,1e-30)
    loss = .5*float(np.sum(weights*np.linalg.norm(error,axis=(1,2))**2))
    adjoint = np.einsum('fna,fab->fnb',q,error.conj(),optimize=True)
    local = np.einsum('fnp,nr->fpr',q,factor,optimize=True)
    gdiag = np.einsum('f,fnp,fnp->n',-s*s*weights,adjoint,q,optimize=True).real*diagonal
    gfactor = np.einsum('f,fnp,fpr->nr',-2*s*s*weights,adjoint,local,optimize=True).real
    gradient = np.r_[gdiag,(np.sqrt(base)[:,None]*gfactor).ravel()]
    return loss+.5*ridge*float(parameters @ parameters), gradient+ridge*parameters


def fit_coupled_losses(bank, frequency, target, rank=8, max_iterations=500, progress=None):
    f = np.asarray(frequency)
    peaks, _ = find_peaks(np.trace(target.real,axis1=1,axis2=2),
                         prominence=np.trace(target.real,axis1=1,axis2=2).max()*1e-4)
    selected = np.unique(np.r_[np.arange(0,len(f),12),
        np.concatenate([np.arange(max(0,p-4),min(len(f),p+5)) for p in peaks]),len(f)-1]).astype(int)
    base = 2*bank.gamma
    initial = np.r_[np.zeros(len(base)), np.random.default_rng(17).normal(0.,.025,(len(base),rank)).ravel()]
    started = last = time.perf_counter()
    history = []
    def callback(parameters):
        nonlocal last
        if time.perf_counter()-last >= 25:
            loss, gradient = objective_gradient(parameters,bank.omega,base,bank.ports,f[selected],target[selected],rank)
            row = dict(seconds=time.perf_counter()-started,loss=loss,gradient_max=float(abs(gradient).max()))
            history.append(row)
            if progress:
                progress(row)
            last = time.perf_counter()
    result = minimize(lambda x:objective_gradient(x,bank.omega,base,bank.ports,f[selected],target[selected],rank),
        initial,jac=True,method='L-BFGS-B',callback=callback,
        bounds=[(-5.,5.)]*len(base)+[(None,None)]*(len(base)*rank),
        options=dict(maxiter=max_iterations,ftol=1e-11,gtol=1e-7,maxls=40,maxcor=20))
    _, diagonal, factor = response_states(result.x,bank.omega,base,bank.ports,f[selected[:1]],rank)
    c = np.diag(diagonal)+factor @ factor.T
    if not np.isfinite(c).all() or np.linalg.eigvalsh(c).min() <= 0:
        raise ValueError('positive coupled loss required')
    return c,dict(optimizer_success=bool(result.success),optimizer_message=str(result.message),
        optimizer_iterations=int(result.nit),objective=float(result.fun),gradient_max=float(abs(result.jac).max()),
        coupled_loss_rank=rank,fit_frequency_count=len(selected),fit_frequency_hz=f[selected].tolist(),
        fixed_seed=17,diagonal_log_bounds=[-5.,5.],ridge=1e-7,progress=history,
        negative_poles_or_loss_repair=False,linear_solver='Woodbury rank solve and direct parameter adjoints')
