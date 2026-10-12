"""Energy-coordinate POD of a mechanical board, with exact held-force steps.

This is a collocated force/average-velocity component, not a piano or an
acoustic-pressure synthesizer. No diagonalization of losses, sampled notes,
air approximation, neural training or change to the reference equations.
"""
from dataclasses import dataclass
import struct
from pathlib import Path

import numpy as np
from scipy.linalg import expm, solve, svd, qr


@dataclass
class Body:
    a: np.ndarray
    b: np.ndarray

    def __post_init__(self):
        self.a = np.asarray(self.a, dtype=float)
        self.b = np.asarray(self.b, dtype=float)
        n = len(self.a)
        if (not n or self.a.shape != (n, n) or self.b.ndim != 2
                or self.b.shape[0] != n or not self.b.shape[1]
                or not np.isfinite(self.a).all() or not np.isfinite(self.b).all()):
            raise ValueError('finite nonempty energy-state body required')
        scale = max(np.linalg.norm(self.a, 2), 1.)
        if np.linalg.eigvalsh(-(self.a + self.a.T) / 2).min() < -1e-12 * scale:
            raise ValueError('passive energy-state dynamics required')

    @classmethod
    def mechanical(cls, omega, damping, ports):
        omega = np.asarray(omega, dtype=float)
        damping = np.asarray(damping, dtype=float)
        ports = np.asarray(ports, dtype=float)
        n = len(omega)
        if (omega.shape != (n,) or not np.isfinite(omega).all() or np.any(omega <= 0)
                or damping.shape != (n, n) or ports.ndim != 2 or ports.shape[0] != n
                or not np.isfinite(damping).all() or not np.isfinite(ports).all()
                or not np.allclose(damping, damping.T, rtol=0, atol=1e-12 * max(np.linalg.norm(damping), 1.))):
            raise ValueError('positive modal frequencies and reciprocal losses required')
        # x=(Omega*q,v), E=|x|^2/2, y=B^T*x. Full loss matrix retained.
        a = np.block([[np.zeros((n, n)), np.diag(omega)],
                      [-np.diag(omega), -damping]])
        return cls(a, np.vstack((np.zeros_like(ports), ports)))

    def response(self, frequency):
        return np.array([self.b.T @ solve(2j * np.pi * hz * np.eye(len(self.a)) - self.a,
                                         self.b, check_finite=False) for hz in frequency])

    def project(self, basis):
        """Generic energy projection preserves passivity, not reciprocity."""
        v = np.asarray(basis, dtype=float)
        if (v.ndim != 2 or v.shape[0] != len(self.a) or not v.shape[1]
                or not np.isfinite(v).all()
                or not np.allclose(v.T @ v, np.eye(v.shape[1]), rtol=0, atol=2e-12)):
            raise ValueError('orthonormal energy projection required')
        return Body(v.T @ self.a @ v, v.T @ self.b)

    def project_mechanical(self, spatial_basis):
        """Paired kinematics preserve reciprocal mechanical force ports.

        Vv=U, Vq=orth(Omega U). Thus K=(Vq^T Omega U)^T
        (Vq^T Omega U) and D=U^T D U, with force and velocity collocated.
        General mixed q/v POD does not provide this guarantee.
        """
        n = len(self.a) // 2
        u = np.asarray(spatial_basis, dtype=float)
        if (u.ndim != 2 or u.shape[0] != n or not u.shape[1]
                or not np.isfinite(u).all()
                or not np.allclose(u.T @ u, np.eye(u.shape[1]), rtol=0, atol=2e-12)):
            raise ValueError('orthonormal spatial projection required')
        upper, _ = qr(self.a[:n, n:] @ u, mode='economic')
        v = np.zeros((2*n, 2*u.shape[1]))
        v[:n, :u.shape[1]] = upper
        v[n:, u.shape[1]:] = u
        return self.project(v)

    def held_step(self, rate):
        if not np.isfinite(rate) or rate <= 0:
            raise ValueError('positive rate')
        n, p = self.b.shape
        # Integrate x and its integral together; no inverse of A or eigenvalue
        # clipping. Singular/overdamped reduced systems are supported.
        augmented = np.zeros((2 * n + p, 2 * n + p))
        augmented[:n, :n] = self.a
        augmented[:n, n:n+p] = self.b
        augmented[n+p:, :n] = np.eye(n)
        e = expm(augmented / rate)
        return HeldBody(e[:n, :n], e[:n, n:n+p], e[n+p:, :n], e[n+p:, n:n+p], self.b, 1 / rate)


@dataclass
class HeldBody:
    transition: np.ndarray
    drive: np.ndarray
    integral_state: np.ndarray
    integral_drive: np.ndarray
    ports: np.ndarray
    dt: float

    def step(self, state, force):
        integral = self.integral_state @ state + self.integral_drive @ force
        return self.transition @ state + self.drive @ force, self.ports.T @ integral / self.dt

    def write(self, path):
        # Versioned f64 little-endian coefficients. All allocations/load work
        # happen before processing in the native consumer.
        n, p = self.ports.shape
        with Path(path).open('xb') as stream:
            stream.write(struct.pack('<8sIId', b'RFBDY001', n, p, self.dt))
            for value in (self.transition, self.drive, self.integral_state, self.integral_drive, self.ports):
                stream.write(np.asarray(value, dtype='<f8').tobytes(order='C'))


def modal_snapshots(omega, damping, ports, frequencies):
    """Full second-order responses, avoiding a 1212-state shifted solve."""
    snapshots = []
    omega = np.asarray(omega)
    for hz in frequencies:
        s = 2j * np.pi * hz
        q = solve(np.diag(omega**2 + s*s) + s*damping, ports, check_finite=False)
        # Shared spatial basis for both q and v. Normalize harmonic columns
        # rather than claim balanced truncation or a global error bound.
        x = q
        for part in (x.real, x.imag):
            norms = np.linalg.norm(part, axis=0)
            keep = norms > 1e-25
            snapshots.append(part[:, keep] / norms[keep])
    u, singular, _ = svd(np.hstack(snapshots), full_matrices=False, check_finite=False)
    return u, singular


def modal_response(omega, damping, ports, frequencies):
    return np.array([ports.T @ solve(np.diag(omega**2 - (2*np.pi*hz)**2)
                                    + 2j*np.pi*hz*damping, 2j*np.pi*hz*ports,
                                    check_finite=False) for hz in frequencies])
