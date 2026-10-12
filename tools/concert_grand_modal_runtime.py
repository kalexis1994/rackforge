"""Exact damped-modal realization of the dry linear body, not a loss refit."""
from dataclasses import dataclass
from pathlib import Path
import struct
import numpy as np
from scipy.linalg import eig, solve


@dataclass
class ModalRuntime:
    roots: np.ndarray
    vectors: np.ndarray
    drive: np.ndarray
    output: np.ndarray
    multiplicity: np.ndarray
    condition: float
    eigen_residual: float

    @classmethod
    def from_body(cls, body):
        roots, vectors = eig(body.a)
        condition = float(np.linalg.cond(vectors))
        residual = float(np.linalg.norm(body.a @ vectors - vectors*roots)/np.linalg.norm(body.a))
        if condition > 1e8 or residual > 1e-10 or np.any(roots.real > 1e-10):
            raise ValueError('ill-conditioned or active damped modal realization')
        indices = np.flatnonzero(roots.imag >= 0)
        multiplier = np.where(roots[indices].imag == 0, 1., 2.)
        if int(multiplier.sum()) != len(roots):
            raise ValueError('incomplete conjugate pairs')
        drive = solve(vectors, body.b)[indices]
        return cls(roots[indices], vectors[:, indices], drive,
                   (body.b.T @ vectors[:, indices]).T, multiplier, condition, residual)

    def coefficients(self, rate):
        if not np.isfinite(rate) or rate <= 0:
            raise ValueError('positive rate')
        z = self.roots / rate
        p1 = np.empty_like(z)
        p2 = np.empty_like(z)
        small = abs(z) < .1
        p1[~small] = np.expm1(z[~small])/z[~small]
        term1 = np.ones(small.sum(), dtype=complex)
        total1 = term1.copy()
        for k in range(1, 20):
            term1 *= z[small]/(k+1)
            total1 += term1
        p1[small] = total1
        p2[~small] = (np.expm1(z[~small])-z[~small])/z[~small]**2
        term = np.full(small.sum(), .5, dtype=complex)
        total = term.copy()
        for k in range(1, 20):
            term *= z[small]/(k+2)
            total += term
        p2[small] = total
        return ModalStep(np.exp(z), p1, p1/rate, p2/rate, self.drive,
                         self.output*self.multiplicity[:, None], 1/rate)

    def reconstruct(self, state):
        return (self.vectors @ (self.multiplicity*state)).real

    def response(self, frequencies, ports):
        b, c = self.drive[:, ports], self.output[:, ports]
        paired = self.multiplicity == 2
        result = []
        for hz in frequencies:
            s = 2j*np.pi*hz
            value = c.T @ (b/(s-self.roots[:, None]))
            value += c[paired].conj().T @ (b[paired].conj()/(s-self.roots[paired, None].conj()))
            result.append(value)
        return np.asarray(result)


@dataclass
class ModalStep:
    transition: np.ndarray
    average_state: np.ndarray
    drive_step: np.ndarray
    drive_average: np.ndarray
    drive: np.ndarray
    output: np.ndarray
    dt: float

    def step(self, state, force):
        u = self.drive @ force
        average = self.average_state*state + self.drive_average*u
        velocity = (self.output.T @ average).real
        return self.transition*state + self.drive_step*u, velocity

    def write(self, path):
        n, p = self.drive.shape
        coefficients = np.column_stack([part for value in [self.transition, self.average_state,
                                self.drive_step, self.drive_average] for part in [value.real, value.imag]])
        with Path(path).open('xb') as stream:
            stream.write(struct.pack('<8sIId', b'RFMDL001', n, p, self.dt))
            for value in [coefficients, self.drive.real, self.drive.imag, self.output.real, self.output.imag]:
                stream.write(np.asarray(value, dtype='<f8').tobytes(order='C'))
