"""Finite-window diagnostics of the unchanged C1 Hermite board trajectory.

Hann multiplies the actual velocity/acceleration. It never differentiates a
tapered displacement, injects endpoint forces, or changes the physical source.
These transforms do not identify full-note energies from a short window.
"""
import numpy as np
from numpy.polynomial.legendre import leggauss
from concert_grand_cell_observer import acceleration_spectrum


def velocity_spectrum(time, position, velocity, frequency, hann=False, order=12):
    t = np.asarray(time, dtype=float)
    B = np.asarray(position, dtype=float)
    V = np.asarray(velocity, dtype=float)
    f = np.atleast_1d(np.asarray(frequency, dtype=float))
    if (t.ndim != 1 or len(t) < 2 or B.ndim != 2 or B.shape != V.shape
            or len(B) != len(t) or f.ndim != 1):
        raise ValueError('time/position/velocity/frequency dimensions')
    if not all(np.isfinite(x).all() for x in (t, B, V, f)) or (np.diff(t) <= 0).any():
        raise ValueError('finite source with increasing time')
    if int(order) != order or order < 2:
        raise ValueError('quadrature order >=2')
    g, w = leggauss(order)
    u = (g + 1) / 2
    dt = np.diff(t)
    curvature = np.diff(B, axis=0) / dt[:, None] - (V[:-1] + V[1:]) / 2
    values = (V[:-1, None, :] + u[None, :, None] * np.diff(V, axis=0)[:, None, :]
              + 6 * (u - u*u)[None, :, None] * curvature[:, None, :])
    nodes = t[:-1, None] + dt[:, None] * u
    weights = dt[:, None] * w[None, :] / 2
    if hann:
        weights = weights * (.5 - .5*np.cos(2*np.pi*(nodes-t[0])/(t[-1]-t[0])))
    weighted = (values * weights[:, :, None]).reshape(-1, B.shape[1])
    return np.array([np.exp(-2j*np.pi*hz*nodes.ravel()) @ weighted for hz in f])


def hann_acceleration_spectrum(time, position, velocity, frequency):
    """Exact Hann modulation of the existing analytic acceleration transform."""
    t = np.asarray(time, dtype=float)
    f = np.atleast_1d(np.asarray(frequency, dtype=float))
    if t.ndim != 1 or len(t) < 2 or not np.isfinite(t).all() or t[-1] <= t[0]:
        raise ValueError('finite increasing source window')
    shift = 1 / (t[-1]-t[0])
    center = acceleration_spectrum(t, position, velocity, f)
    lower = acceleration_spectrum(t, position, velocity, f-shift)
    upper = acceleration_spectrum(t, position, velocity, f+shift)
    phase = np.exp(-2j*np.pi*shift*t[0])
    return .5*center - .25*phase*lower - .25*phase.conjugate()*upper


def band_norms(frequency, spectrum, bands):
    """Integrals of squared transform, no gain normalization or modal deletion."""
    f = np.asarray(frequency, dtype=float)
    y = np.asarray(spectrum)
    if (f.ndim != 1 or len(f) < 2 or y.ndim < 2 or len(y) != len(f)
            or not np.isfinite(f).all() or not np.isfinite(y).all()
            or (np.diff(f) <= 0).any()):
        raise ValueError('finite ordered spectrum')
    power = np.sum(abs(y.reshape(len(y), -1))**2, axis=1)
    result = {}
    for low, high in bands:
        mask = (f >= low) & (f <= high)
        if mask.sum() < 2 or f[mask][0] != low or f[mask][-1] != high:
            raise ValueError('band endpoints must be in frequency grid')
        result[f'{low:g}-{high:g}'] = float(np.trapezoid(power[mask], f[mask]))
    return result


def projected_pressure(observer, frequency, acceleration):
    """Same continuous cell Green integral, evaluated for one source per Hz."""
    f = np.asarray(frequency)
    A = np.asarray(acceleration)
    if f.ndim != 1 or A.shape != (len(f), observer.surface.modes):
        raise ValueError('one full-coordinate acceleration vector per frequency')
    out = []
    for hz, source in zip(f, A):
        green = np.sum(observer.area[:, None, None]
                       * np.exp(-2j*np.pi*hz*observer.distance/observer.surface.speed)
                       / observer.distance, axis=0)
        # Real surface shapes: split complex vector to retain real BLAS.
        field = observer.shapes @ source.real + 1j*(observer.shapes @ source.imag)
        out.append(observer.surface.density/(2*np.pi)*(green @ field))
    return np.asarray(out)
