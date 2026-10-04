"""Continuous retarded SI pressure for the constant-cell P6 surface.

Uses the same mass-normalized cell shapes as the reciprocal air load.
Quadrature integrates exp(-s*r/c)/r inside each cell; no room, EQ, pattern,
sample calibration or modification of the already-coupled source.
"""
import numpy as np
from numpy.polynomial.legendre import leggauss


class CellObserver:
    def __init__(self,surface,microphones,order=4):
        self.surface=surface;self.microphones=np.asarray(microphones,dtype=float)
        if self.microphones.ndim!=2 or self.microphones.shape[1]!=3 or not len(self.microphones) or not np.isfinite(self.microphones).all() or (self.microphones[:,2]<=0).any():
            raise ValueError('finite microphone positions in upper half-space required')
        if int(order)!=order or order<2:raise ValueError('cell quadrature order >=2')
        g,w=leggauss(order);offsets=np.array(np.meshgrid(g*surface.hx/2,g*surface.hy/2,indexing='ij')).reshape(2,-1).T
        self.area=np.outer(w,w).ravel()*surface.hx*surface.hy/4
        x,y=np.meshgrid((np.arange(surface.nx)+.5)*surface.hx,(np.arange(surface.ny)+.5)*surface.hy,indexing='ij')
        centers=np.c_[x.ravel(),y.ravel()]
        points=centers[None,:,:]+offsets[:,None,:]
        self.distance=np.sqrt(np.sum((self.microphones[None,:,None,:2]-points[:,None,:,:])**2,axis=-1)+self.microphones[None,:,None,2]**2)
        self.shapes=surface.shapes.reshape(-1,surface.modes)

    def acceleration_transfer(self,frequency):
        f=np.atleast_1d(np.asarray(frequency,dtype=float))
        if f.ndim!=1 or not np.isfinite(f).all():raise ValueError('finite frequency vector')
        out=[];s=self.surface
        for hz in f:
            green=np.sum(self.area[:,None,None]*np.exp(-2j*np.pi*hz*self.distance/s.speed)/self.distance,axis=0)
            out.append(s.density/(2*np.pi)*(green@self.shapes))
        return np.asarray(out)


def acceleration_spectrum(time,position,velocity,frequency):
    """Transform of acceleration inside a C1 cubic-Hermite source window.

    Includes no endpoint delta impulses. At zero frequency this is V(T)-V(0).
    Cell pressure times this transform has units Pa*s. Analytic exponential
    moments avoid numerical time quadrature; the mechanical interpolation
    still requires a separate capture-rate refinement.
    """
    t=np.asarray(time,dtype=float);B=np.asarray(position,dtype=float);V=np.asarray(velocity,dtype=float)
    f=np.atleast_1d(np.asarray(frequency,dtype=float))
    if t.ndim!=1 or len(t)<2 or B.ndim!=2 or B.shape!=V.shape or len(B)!=len(t) or f.ndim!=1:
        raise ValueError('time/position/velocity/frequency dimensions')
    if not all(np.isfinite(x).all() for x in [t,B,V,f]) or (np.diff(t)<=0).any():raise ValueError('finite source with increasing time')
    dt=np.diff(t);change=np.diff(V,axis=0)
    curvature=np.diff(B,axis=0)/dt[:,None]-(V[:-1]+V[1:])/2
    out=[]
    for hz in f:
        s=-2j*np.pi*hz;z=s*dt;small=abs(z)<.1
        j0=np.empty_like(z);j1=np.empty_like(z);large=z[~small];delta=np.expm1(large)
        j0[~small]=delta/large;j1[~small]=(delta*(large-1)+large)/large**2
        # Convergent moments sum z^k / (k!*(k+1 or k+2)); avoids
        # catastrophic cancellation at zero and small phase increments.
        term=np.ones_like(z[small]);a=term.copy();b=term/2
        for k in range(1,13):
            term=term*z[small]/k;a+=term/(k+1);b+=term/(k+2)
        j0[small]=a;j1[small]=b
        out.append(np.exp(s*t[:-1])@(change*j0[:,None]+12*curvature*(.5*j0-j1)[:,None]))
    return np.asarray(out)


def spectral_relative(frequency,actual,reference,low,high):
    """Trapezoid-weighted complex L2 difference, no per-note gain change."""
    f=np.asarray(frequency);mask=(f>=low)&(f<=high)
    if mask.sum()<2:raise ValueError('at least two frequency points per band')
    a=np.asarray(actual)[mask];b=np.asarray(reference)[mask]
    squared=lambda x:np.sum(abs(x.reshape(len(x),-1))**2,axis=1)
    norm=np.trapezoid(squared(b),f[mask])
    if norm<=0:raise ValueError('nonzero reference spectrum')
    return float(np.sqrt(np.trapezoid(squared(a-b),f[mask])/norm))
