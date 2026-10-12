"""Fit positive mechanical dynamics against mobility and causal pressure.

The existing real delayed acceleration readout is fixed for this experiment.
Static compliance/high inertance stay fixed. Pressure is an observation, never
feedback. Energy-scaled solves avoid mixing audible and MHz stiffness scales.
"""
import time
import numpy as np
from scipy.linalg import solve
from scipy.optimize import minimize
from scipy.signal import find_peaks
from concert_grand_storage_preserving_fit import operators, storage_basis


def balanced_solve(stiffness, damping, omega, s, rhs):
    inverse = 1/omega
    matrix = (stiffness+s*damping+s*s*np.eye(len(omega)))*inverse[:, None]*inverse[None]
    return inverse[:, None]*solve(matrix, inverse[:, None]*rhs, assume_a='sym', check_finite=False)


def readout_transfer(frequency, delays, readout):
    phase = np.exp(-2j*np.pi*np.asarray(frequency)[:, None, None]*delays[None])
    return np.einsum('fmg,mgn->fmn',phase,readout,optimize=True)


def responses(stiffness, damping, omega, ports, frequency, observation):
    q, t = [], []
    s = 2j*np.pi*np.asarray(frequency)
    for value, observer in zip(s, observation):
        both = balanced_solve(stiffness,damping,omega,value,np.hstack((ports,observer.T)))
        q.append(both[:, :ports.shape[1]])
        t.append(both[:, ports.shape[1]:])
    q, t = np.asarray(q), np.asarray(t)
    y = s[:, None, None]*np.einsum('na,fnb->fab',ports,q,optimize=True)
    p = s[:, None, None]**2*np.einsum('fmn,fnb->fmb',observation,q,optimize=True)
    return q,t,y,p


def quadrature_weights(frequency, target):
    f = np.asarray(frequency)
    df = np.r_[(f[1]-f[0])/2,(f[2:]-f[:-2])/2,(f[-1]-f[-2])/2]
    return df/df.sum()/np.maximum(np.linalg.norm(target,axis=(1,2))**2,1e-30)


def objective_gradient(x, omega, ports, base_loss, basis, frequency, target_y, target_p, observation, rank, ridge=1e-7):
    k,c,scaled_loss,factor = operators(x,omega,base_loss,basis,rank)
    q,t,y,p = responses(k,c,omega,ports,frequency,observation)
    ey,ep = y-target_y,p-target_p
    wy,wp = quadrature_weights(frequency,target_y),quadrature_weights(frequency,target_p)
    s = 2j*np.pi*np.asarray(frequency)
    loss_y = .5*float(np.sum(wy*np.linalg.norm(ey,axis=(1,2))**2))
    loss_p = .5*float(np.sum(wp*np.linalg.norm(ep,axis=(1,2))**2))
    ry = np.einsum('fna,fab->fnb',q,ey.conj(),optimize=True)
    rp = np.einsum('fna,fab->fnb',t,ep.conj(),optimize=True)
    scaled = omega[:, None]*basis
    projected_q = np.einsum('fnp,nr->fpr',q,scaled,optimize=True)
    projected_y = np.einsum('fnp,nr->fpr',ry,scaled,optimize=True)
    projected_p = np.einsum('fnp,nr->fpr',rp,scaled,optimize=True)
    gk = (np.einsum('f,fpr,fpr->r',-s*wy,projected_y,projected_q,optimize=True)
        + np.einsum('f,fpr,fpr->r',-s*s*wp,projected_p,projected_q,optimize=True)).real
    cq = np.einsum('nm,fmp->fnp',scaled_loss,q,optimize=True)
    crp = np.einsum('nm,fmp->fnp',scaled_loss,rp,optimize=True)
    gd = (np.einsum('f,fnp,fnp->n',-2*s*s*wy,ry,cq,optimize=True)
        + np.einsum('f,fnp,fnp->n',-s**3*wp,rp,cq,optimize=True)
        + np.einsum('f,fnp,fnp->n',-s**3*wp,crp,q,optimize=True)).real
    ql = np.einsum('fnp,nr->fpr',q,factor,optimize=True)
    rpl = np.einsum('fnp,nr->fpr',rp,factor,optimize=True)
    gl = (np.einsum('f,fnp,fpr->nr',-2*s*s*wy,ry,ql,optimize=True)
        + np.einsum('f,fnp,fpr->nr',-s**3*wp,rp,ql,optimize=True)
        + np.einsum('f,fnp,fpr->nr',-s**3*wp,q,rpl,optimize=True)).real
    gradient = np.r_[-np.exp(-x[:basis.shape[1]])*gk,gd,
        (np.sqrt(np.diag(base_loss))[:, None]*gl).ravel()]+ridge*x
    return loss_y+loss_p+.5*ridge*float(x @ x),gradient,dict(mobility_objective=loss_y,pressure_objective=loss_p)


def training_indices(frequency, mobility, pressure):
    f = np.asarray(frequency)
    probes = np.searchsorted(f,np.geomspace(f[0],f[-1],96)).clip(0,len(f)-1)
    peaks = []
    for value in [np.trace(mobility.real,axis1=1,axis2=2),np.linalg.norm(pressure,axis=(1,2))]:
        indices, _ = find_peaks(value,prominence=value.max()*1e-4)
        peaks.extend(indices)
    neighborhoods = [np.arange(max(0,p-1),min(len(f),p+2)) for p in peaks]
    return np.unique(np.r_[probes,np.concatenate(neighborhoods),0,len(f)-1]).astype(int)


def fit_joint(omega,ports,base_loss,frequency,target_y,target_p,delays,readout,rank=8,
              max_iterations=400,progress=None,checkpoint=None,blas_threads=1):
    from threadpoolctl import threadpool_limits
    with threadpool_limits(limits=blas_threads,user_api='blas'):
        basis = storage_basis(omega,ports,base_loss,frequency)
        indices = training_indices(frequency,target_y,target_p)
        f = frequency[indices]
        observation = readout_transfer(f,delays,readout)
        n,r = len(omega),basis.shape[1]
        initial = np.r_[np.zeros(r+n),np.random.default_rng(23).normal(0.,.005,(n,rank)).ravel()]
        arguments = (omega,ports,base_loss,basis,f,target_y[indices],target_p[indices],observation,rank)
        started = last = time.perf_counter()
        history = []
        iterations = 0
        def callback(x):
            nonlocal last,iterations
            iterations += 1
            if time.perf_counter()-last>=25:
                value,g,parts = objective_gradient(x,*arguments)
                row = dict(seconds=time.perf_counter()-started,iteration=iterations,objective=value,
                    gradient_max=float(abs(g).max()),**parts)
                history.append(row)
                if checkpoint:
                    checkpoint(x,basis,row)
                if progress:
                    progress(row)
                last = time.perf_counter()
        def evaluate(x):
            value,g,_ = objective_gradient(x,*arguments)
            return value,g
        result = minimize(evaluate,initial,jac=True,method='L-BFGS-B',callback=callback,
            bounds=[(-3.,3.)]*r+[(-2.5,2.5)]*n+[(None,None)]*(n*rank),
            options=dict(maxiter=max_iterations,ftol=1e-10,gtol=1e-7,maxls=40,maxcor=30))
        k,c,_,_ = operators(result.x,omega,base_loss,basis,rank)
        value,gradient,parts = objective_gradient(result.x,*arguments)
        if checkpoint:
            checkpoint(result.x,basis,dict(iteration=int(result.nit),objective=value,**parts))
    return k,c,dict(optimizer_success=bool(result.success),optimizer_message=str(result.message),
        optimizer_iterations=int(result.nit),objective=value,gradient_max=float(abs(gradient).max()),**parts,
        equal_mobility_pressure_objective_weights=True,fixed_acceleration_observer=True,
        fit_frequency_count=len(f),fit_frequency_hz=f.tolist(),stiffness_complement_rank=r,extra_loss_rank=rank,
        fixed_seed=23,blas_threads=blas_threads,progress=history,negative_loss_or_pole_repair=False)
