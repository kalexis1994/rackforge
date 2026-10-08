"""Local Hann moments of live Hermite V/A; no global spectrum subtraction.

Use actual segment widths, a bounded local Taylor expansion of sin^2, and
compensated accumulation of real-BLAS segment blocks. The Taylor remainder
is an absolute window-weight bound, not a relative spectral certificate.
Original observers remain independent controls; no physical source changes.
"""
import math
import numpy as np


def truncation_bounds(time, frequency, order=8):
    t = np.asarray(time, dtype=float)
    f = np.atleast_1d(np.asarray(frequency, dtype=float))
    if (t.ndim != 1 or len(t) < 2 or f.ndim != 1 or not len(f)
            or not np.isfinite(t).all() or not np.isfinite(f).all()
            or (np.diff(t) <= 0).any()):
        raise ValueError('finite increasing window and finite frequencies required')
    if int(order) != order or not 6 <= order <= 12:
        raise ValueError('local window order between 6 and 12')
    alpha = 2*np.pi*np.max(np.diff(t))/(t[-1]-t[0])
    phase = 2*np.pi*np.max(abs(f))*np.max(np.diff(t))
    if alpha > .02 or phase > .5:
        raise ValueError('local window/phase expansion domain exceeded')
    return dict(window_weight_absolute=.5*math.exp(alpha)*alpha**(order+1)/math.factorial(order+1),
                exponential_moment_absolute=math.exp(phase)*phase**28/math.factorial(28))


def local_hann_spectra(time, position, velocity, frequency, order=8,
                       frequency_block=16, segment_block=256):
    """Integrate w*V and w*A, with w=sin^2(pi*(t-t0)/T).

V(u)=V0+u*dV+6*(u-u^2)*C, dt*A=dV+12*(.5-u)*C.
Window weights are expanded locally before any Fourier contraction. The
absolute Fourier phase and live endpoints remain; A is not d(w*V)/dt.
"""
    t = np.asarray(time, dtype=float)
    B = np.asarray(position, dtype=float)
    V = np.asarray(velocity, dtype=float)
    f = np.atleast_1d(np.asarray(frequency, dtype=float))
    truncation_bounds(t, f, order)
    if B.ndim != 2 or B.shape != V.shape or len(B) != len(t):
        raise ValueError('matching trajectory dimensions required')
    if not np.isfinite(B).all() or not np.isfinite(V).all():
        raise ValueError('finite trajectory required')
    if any(int(b) != b or b < 1 for b in (frequency_block, segment_block)):
        raise ValueError('positive integer blocks required')
    result = [np.zeros((len(f), V.shape[1]), complex) for _ in range(2)]
    steps = np.diff(t)
    for first in range(0, len(f), frequency_block):
        hz = f[first:first+frequency_block]
        total = [np.zeros((len(hz), V.shape[1]), complex) for _ in range(2)]
        compensation = [np.zeros_like(x) for x in total]
        for start in range(0, len(steps), segment_block):
            stop = min(start+segment_block, len(steps))
            dt = steps[start:stop]
            theta = np.pi*(t[start:stop]-t[0])/(t[-1]-t[0])
            alpha = 2*np.pi*dt/(t[-1]-t[0])
            weights = [np.sin(theta)**2]
            for k in range(1, order+1):
                trig = np.sin(2*theta) if k % 2 else np.cos(2*theta)
                sign = 1 if k % 4 in (1, 2) else -1
                weights.append(sign*.5*alpha**k/math.factorial(k)*trig)
            z = -2j*np.pi*hz[:, None]*dt[None, :]
            moments = np.zeros((*z.shape, order+3), complex)
            term = np.ones_like(z)
            powers = np.arange(order+3)
            for k in range(28):
                if k:
                    term *= z/k
                moments += term[:, :, None]/(k+powers+1)
            local = [sum(w[None, :]*moments[:, :, p+k]
                         for k, w in enumerate(weights)) for p in range(3)]
            phase = np.exp(-2j*np.pi*hz[:, None]*t[None, start:stop])
            v0 = V[start:stop]
            dv = V[start+1:stop+1]-v0
            c = ((B[start+1:stop+1]-B[start:stop])/dt[:, None]
                 -(v0+V[start+1:stop+1])/2)
            def contract(weight, values):
                return weight.real@values+1j*(weight.imag@values)
            k0, k1, k2 = local
            pieces = [contract(phase*dt*k0, v0)+contract(phase*dt*k1, dv)
                      +contract(phase*dt*6*(k1-k2), c),
                      contract(phase*k0, dv)+contract(phase*12*(.5*k0-k1), c)]
            for i, piece in enumerate(pieces):
                corrected = piece-compensation[i]
                updated = total[i]+corrected
                compensation[i] = (updated-total[i])-corrected
                total[i] = updated
        for i in range(2):
            result[i][first:first+len(hz)] = total[i]
    return tuple(result)
