"""Blocked analytic spectra of uniform C1 Hermite velocity/acceleration.

Exponential moments are shared across uniform segments. Real BLAS contracts
their three coefficient arrays in frequency blocks; no node-expanded source,
FFT periodicity, modal truncation or endpoint delta forces. Uniform spacing
is checked, and floating spacing must be checked against the original oracle.
"""
import numpy as np


def _moments(z):
    z = np.asarray(z,dtype=complex)
    out = np.empty((len(z),3),complex)
    small = abs(z)<.5
    term = np.ones(small.sum(),complex)
    values = np.column_stack([term/(p+1) for p in range(3)])
    for k in range(1,25):
        term *= z[small]/k
        for p in range(3):
            values[:,p] += term/(k+p+1)
    out[small] = values
    large = z[~small]
    delta = np.expm1(large)
    j0 = delta/large
    j1 = (delta*(large-1)+large)/large**2
    j2 = (delta+1-2*j1)/large
    out[~small] = np.column_stack([j0,j1,j2])
    return out


def uniform_spectra(time,position,velocity,frequency,block=32):
    """Return Fourier integrals of actual V and A inside the source window.

For u in [0,1], V=V0+u*dV+6*(u-u*u)*C;
dt*A=dV+12*(.5-u)*C, C=dB/dt-(V0+V1)/2.
Absolute start phase is retained; zero Hz A is exactly V(T)-V(0)
up to summation roundoff. No force from closing the window is added.
"""
    t = np.asarray(time,dtype=float)
    B = np.asarray(position,dtype=float)
    V = np.asarray(velocity,dtype=float)
    f = np.atleast_1d(np.asarray(frequency,dtype=float))
    if (t.ndim!=1 or len(t)<2 or B.ndim!=2 or B.shape!=V.shape
            or len(B)!=len(t) or f.ndim!=1 or not len(f)):
        raise ValueError('time/position/velocity/frequency dimensions')
    if not all(np.isfinite(x).all() for x in (t,B,V,f)):
        raise ValueError('finite trajectory and frequencies required')
    dt = (t[-1]-t[0])/(len(t)-1)
    steps = np.diff(t)
    if dt<=0 or np.max(abs(steps-dt))>2e-11*dt:
        raise ValueError('uniform increasing source grid required')
    if int(block)!=block or block<1:
        raise ValueError('positive frequency block')
    modes = V.shape[1]
    change = np.diff(V,axis=0)
    # Use each stored spacing here to avoid amplifying cancellation in C.
    curvature = np.diff(B,axis=0)/steps[:,None]-(V[:-1]+V[1:])/2
    coefficients = np.ascontiguousarray(np.concatenate([V[:-1],change,curvature],axis=1))
    vel = np.empty((len(f),modes),complex)
    acc = np.empty_like(vel)
    for first in range(0,len(f),block):
        hz = f[first:first+block]
        phase = np.exp(-2j*np.pi*hz[:,None]*t[None,:-1])
        transformed = phase.real@coefficients + 1j*(phase.imag@coefficients)
        v0,dv,c = np.split(transformed,3,axis=1)
        j0,j1,j2 = _moments(-2j*np.pi*hz*dt).T[:,:,None]
        vel[first:first+len(hz)] = dt*(j0*v0+j1*dv+6*(j1-j2)*c)
        acc[first:first+len(hz)] = j0*dv+12*(.5*j0-j1)*c
    return vel,acc


def uniform_window_spectra(time,position,velocity,frequency,block=32):
    """Both rectangular and actual Hann-modulated V/A, same trajectory."""
    t = np.asarray(time,dtype=float)
    f = np.atleast_1d(np.asarray(frequency,dtype=float))
    if t.ndim!=1 or len(t)<2 or not np.isfinite(t).all() or t[-1]<=t[0]:
        raise ValueError('finite increasing source window')
    shift = 1/(t[-1]-t[0])
    # Reuse duplicate requested frequencies without assuming a periodic grid.
    grid,inverse = np.unique(np.r_[f,f-shift,f+shift],return_inverse=True)
    vel,acc = uniform_spectra(t,position,velocity,grid,block)
    center,lower,upper = np.split(inverse,3)
    phase = np.exp(-2j*np.pi*shift*t[0])
    soft = lambda values: (.5*values[center]-.25*phase*values[lower]
                           -.25*phase.conjugate()*values[upper])
    return dict(rectangular=(vel[center],acc[center]),hann=(soft(vel),soft(acc)))
