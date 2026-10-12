"""Positive-residue mobility fit and a passive two-moment completion.

Virtual mechanical modes reproduce a port operator, not individual measured
strings. Frequency poles/losses come from teacher peaks; weights remain >=0.
The completed bank preserves both static compliance and high inertance.
"""
from dataclasses import dataclass
import numpy as np
from scipy.linalg import cholesky, solve_triangular, eigh, expm
from scipy.optimize import minimize, nnls
from scipy.signal import find_peaks, peak_widths


@dataclass
class MobilityBank:
    omega: np.ndarray
    gamma: np.ndarray
    ports: np.ndarray

    def __post_init__(self):
        self.omega = np.asarray(self.omega, dtype=float)
        self.gamma = np.asarray(self.gamma, dtype=float)
        self.ports = np.asarray(self.ports, dtype=float)
        n = len(self.omega)
        if (self.omega.shape != (n,) or self.gamma.shape != (n,) or self.ports.ndim != 2
                or self.ports.shape[0] != n or not n or not self.ports.shape[1]
                or not all(np.isfinite(v).all() for v in [self.omega, self.gamma, self.ports])
                or np.any(self.omega <= 0) or np.any(self.gamma < 0)):
            raise ValueError('finite positive-frequency passive mobility bank required')

    def response(self, frequencies):
        s = 2j*np.pi*np.asarray(frequencies)
        weights = s[:, None]/(s[:, None]**2+2*self.gamma*s[:, None]+self.omega**2)
        return np.einsum('fj,jp,jq->fpq', weights, self.ports, self.ports, optimize=True)

    def static_compliance(self):
        return self.ports.T @ (self.ports/self.omega[:, None]**2)

    def high_inertance(self):
        return self.ports.T @ self.ports

    def coefficients(self, rate):
        if not np.isfinite(rate) or rate <= 0:
            raise ValueError('positive clock')
        e, g = [], []
        for omega, gamma in zip(self.omega, self.gamma):
            # Energy coordinates (omega*q,v); exact force-held update.
            a = np.array([[0., omega, 0.], [-omega, -2*gamma, 1.], [0., 0., 0.]])
            value = expm(a/rate)
            e.append(value[:2, :2])
            g.append(value[:2, 2])
        return np.asarray(e), np.asarray(g)

    def step(self, state, force, rate, coefficients):
        e, g = coefficients
        local_force = self.ports @ force
        new = np.einsum('nij,nj->ni', e, state)+g*local_force[:, None]
        average_velocity = (new[:, 0]-state[:, 0])*rate/self.omega
        return new, self.ports.T @ average_velocity


def pole_candidates(frequency, target, directions=3):
    f = np.asarray(frequency)
    h = np.asarray(target)
    trace = np.trace(h.real, axis1=1, axis2=2)
    peaks, _ = find_peaks(trace, prominence=trace.max()*1e-4)
    widths = peak_widths(trace, peaks, rel_height=.5)
    left = np.interp(widths[2], np.arange(len(f)), f)
    right = np.interp(widths[3], np.arange(len(f)), f)
    anchors = np.unique(np.searchsorted(f, np.geomspace(24., 5900., 18)).clip(0, len(f)-1))
    entries = [(int(index), np.pi*max(float(r-l), .5)) for index, l, r in zip(peaks, left, right)]
    entries += [(int(index), 2*np.pi*f[index]*.04) for index in anchors]
    omega, gamma, rows = [], [], []
    for index, width in entries:
        center = float(f[index])
        if index in peaks and 0 < index < len(f)-1:
            local_f = f[index-1:index+2]-center
            polynomial = np.polyfit(local_f, np.log(np.maximum(trace[index-1:index+2], 1e-30)), 2)
            if polynomial[0] < 0:
                center += float(np.clip(-polynomial[1]/(2*polynomial[0]), local_f[0], local_f[-1]))
        resistance = (h[index].real+h[index].real.T)/2
        values, vectors = eigh(resistance)
        if values[0] < -1e-10*max(values[-1], 1e-30):
            raise ValueError('teacher real mobility not positive')
        count = 0
        for j in range(len(values)-1, -1, -1):
            if values[j] <= max(values[-1]*1e-5, 1e-30) or count >= directions:
                break
            for factor in [.65, 1., 1.5]:
                omega.append(2*np.pi*center)
                gamma.append(width*factor)
                rows.append(np.sqrt(values[j])*vectors[:, j])
            count += 1
    return np.asarray(omega), np.asarray(gamma), np.asarray(rows), dict(trace_peaks=len(peaks), background_anchors=len(anchors))


def fit_positive_bank(frequency, target, static, high, material_loss=.046, directions=3):
    f = np.asarray(frequency)
    target = np.asarray(target)
    omega, gamma, rows, metadata = pole_candidates(f, target, directions)
    s = 2j*np.pi*f[:, None]
    coefficient = 2*gamma*s/(s*s+2*gamma*s+omega**2)
    df = np.r_[(f[1]-f[0])/2, (f[2:]-f[:-2])/2, (f[-1]-f[-2])/2]
    norm = np.maximum(np.linalg.norm(target, axis=(1, 2)), 1e-30)
    weight = np.sqrt(df)/norm
    weighted = coefficient*weight[:, None]
    gram = (weighted.conj().T @ weighted).real*(rows @ rows.T)**2
    projected = np.einsum('jp,fpq,jq->fj', rows, target, rows, optimize=True)
    rhs = np.sum((coefficient.conj()*projected).real*weight[:, None]**2, axis=0)
    normalization = max(float(np.trace(gram)/len(gram)), 1e-30)
    gram /= normalization
    rhs /= normalization
    ridge = 1e-8
    gram += ridge*np.eye(len(gram))
    factor = cholesky(gram, lower=True)
    unconstrained, _ = nnls(factor.T, solve_triangular(factor, rhs, lower=True), maxiter=30*len(gram))
    static_factor = cholesky(static, lower=True)
    high_factor = cholesky(high, lower=True)
    raw = np.sqrt(2*gamma)[:, None]*rows
    whites = [solve_triangular(static_factor, (raw/omega[:, None]).T, lower=True).T,
              solve_triangular(high_factor, raw.T, lower=True).T]
    def slack(k, white):
        values, vectors = eigh(white.T @ (k[:, None]*white))
        vector = vectors[:, -1]
        return 1-1e-8-values[-1], -(white @ vector)**2
    initial = unconstrained.copy()
    for white in whites:
        initial *= min(1., .95/max(eigh(white.T @ (initial[:, None]*white), eigvals_only=True)[-1], 1e-30))
    if all(slack(unconstrained, white)[0] >= 0 for white in whites):
        weights = unconstrained
        iterations = 0
        message = 'nonnegative unconstrained fit already satisfies storage limits'
    else:
        constraints = [dict(type='ineq', fun=lambda k,w=w:slack(k,w)[0], jac=lambda k,w=w:slack(k,w)[1]) for w in whites]
        result = minimize(lambda k:.5*k @ gram @ k-rhs @ k, initial,
            jac=lambda k:gram @ k-rhs, bounds=[(0.,None)]*len(omega), constraints=constraints,
            method='SLSQP', options=dict(maxiter=1500, ftol=1e-11))
        if not result.success or min(slack(result.x, w)[0] for w in whites) < -1e-9:
            raise RuntimeError('positive mobility fit failed: '+str(result.message))
        weights, iterations, message = result.x, int(result.nit), str(result.message)
    keep = weights > 1e-10*max(weights.max(), 1e-30)
    bank = MobilityBank(omega[keep], gamma[keep], raw[keep]*np.sqrt(weights[keep, None]))
    remaining_static = (static-bank.static_compliance())
    remaining_high = (high-bank.high_inertance())
    remaining_static = (remaining_static+remaining_static.T)/2
    remaining_high = (remaining_high+remaining_high.T)/2
    # The complement is derived from two physical moments, never a fitted
    # gain, pole clipping or a negative-residue correction.
    values, vectors = eigh(remaining_static, remaining_high)
    if values[0] <= 0:
        raise ValueError('positive two-moment completion required')
    tail_omega = 1/np.sqrt(values)
    tail_ports = vectors.T @ remaining_high
    tail_gamma = material_loss*tail_omega/2
    completed = MobilityBank(np.r_[bank.omega, tail_omega], np.r_[bank.gamma, tail_gamma], np.vstack((bank.ports, tail_ports)))
    errors = [np.linalg.norm(completed.static_compliance()-static)/np.linalg.norm(static),
              np.linalg.norm(completed.high_inertance()-high)/np.linalg.norm(high)]
    if max(errors) > 1e-9:
        raise ValueError('storage moments changed in completion')
    metadata.update(candidate_poles=len(omega), fitted_modes=len(bank.omega), completed_modes=len(completed.omega),
        tail_modes=len(tail_omega), tail_frequency_hz=(tail_omega/(2*np.pi)).tolist(),
        static_moment_relative=float(errors[0]), high_moment_relative=float(errors[1]),
        optimizer_iterations=iterations, optimizer_message=message, ridge=ridge,
        constraint_slack=[float(slack(weights, w)[0]) for w in whites],
        dropped_weight_threshold_relative=1e-10, material_loss_for_tail=material_loss)
    return completed, metadata
