"""Retarded constant-cell pressure audio from a uniform Hermite trajectory.

Physical SI transfer, fixed gain, no tonal fit. A four-tap fractional delay
approximates each cell quadrature point's Green function; its frequency
error is checked separately against CellObserver. Uniform source transforms
use the same analytic Hermite acceleration moments as the finite audit.
"""
import numpy as np
from scipy.fft import rfft, irfft, rfftfreq, next_fast_len
from scipy.sparse import coo_matrix
from concert_grand_cell_observer import CellObserver


def hermite_fft(time, position, velocity, size):
    t=np.asarray(time,dtype=float);B=np.asarray(position,dtype=float);V=np.asarray(velocity,dtype=float)
    if t.ndim!=1 or len(t)<2 or B.ndim!=2 or B.shape!=V.shape or len(B)!=len(t):
        raise ValueError('time/position/velocity dimensions')
    dt=np.diff(t)
    if not all(np.isfinite(x).all() for x in [t,B,V]) or t[0]!=0 or (dt<=0).any() or not np.allclose(dt,dt[0],rtol=1e-10,atol=1e-15) or size<len(t):
        raise ValueError('finite zero-start uniform Hermite trace and sufficient FFT size')
    f=rfftfreq(size,dt[0]);z=-2j*np.pi*f*dt[0];small=abs(z)<.1
    j0=np.empty_like(z);j1=np.empty_like(z);large=z[~small];delta=np.expm1(large)
    j0[~small]=delta/large;j1[~small]=(delta*(large-1)+large)/large**2
    term=np.ones_like(z[small]);a=term.copy();b=term/2
    for k in range(1,13):
        term=term*z[small]/k;a+=term/(k+1);b+=term/(k+2)
    j0[small]=a;j1[small]=b
    change=np.diff(V,axis=0)
    curvature=np.diff(B,axis=0)/dt[:,None]-(V[:-1]+V[1:])/2
    spectrum=rfft(change,n=size,axis=0)*j0[:,None]+12*rfft(curvature,n=size,axis=0)*(.5*j0-j1)[:,None]
    return f,spectrum


def retarded_kernel(observer,rate,aggregation='sparse'):
    if not np.isfinite(rate) or rate<=0:raise ValueError('positive sample rate')
    if aggregation not in ('sparse','reference'):raise ValueError('known cell aggregation required')
    delay=observer.distance/observer.surface.speed*rate
    if delay.min()<1:raise ValueError('microphones require at least one sample propagation')
    base=np.floor(delay).astype(int);u=delay-base
    # Lagrange interpolation at fractional delay u, nodes -1,0,1,2.
    weights=[-u*(u-1)*(u-2)/6,(u+1)*(u-1)*(u-2)/2,
             -(u+1)*u*(u-2)/2,(u+1)*u*(u-1)/6]
    kernel=np.zeros((int(base.max())+3,len(observer.microphones),observer.surface.modes))
    for mic in range(len(observer.microphones)):
        for shift,w in zip([-1,0,1,2],weights):
            coefficients=observer.area[:,None]*w[:,mic]/observer.distance[:,mic]
            bins=(base[:,mic]+shift).ravel()
            # Aggregate quadrature weights by delay before projecting modes;
            # this is the same signed cell shape integral, not random panning.
            cells=np.tile(np.arange(observer.shapes.shape[0]),len(observer.area))
            if aggregation=='reference':
                histogram=np.zeros((len(kernel),observer.shapes.shape[0]))
                np.add.at(histogram,(bins,cells),coefficients.ravel())
            else:
                # Every quadrature point has exactly one delay bin for this
                # Lagrange tap. CSR keeps all signed weights and merges equal
                # bins; it avoids allocating delay-by-cell zeros. No shape,
                # cell, quadrature point or nonzero coefficient is dropped.
                histogram=coo_matrix((coefficients.ravel(),(bins,cells)),
                    shape=(len(kernel),observer.shapes.shape[0])).tocsr()
            kernel[:,mic]+=observer.surface.density/(2*np.pi)*(histogram@observer.shapes)
    return kernel


def kernel_transfer(kernel,rate,frequency):
    f=np.asarray(frequency,dtype=float)
    return np.einsum('fk,kcm->fcm',np.exp(-2j*np.pi*f[:,None]*np.arange(len(kernel))/rate),kernel)


def pressure_fft(time,position,velocity,kernel):
    rate=1/(time[1]-time[0])
    # An even FFT also permits an exact half-rate capture comparison with
    # identical frequency spacing and padded physical duration.
    size=2*next_fast_len((len(time)+len(kernel)+65)//2)
    f,A=hermite_fft(time,position,velocity,size)
    H=rfft(kernel,n=size,axis=0)
    pressure_spectrum=np.einsum('fcm,fm->fc',H,A)
    # Continuous Fourier integral Pa*s -> sampled DFT Pa.
    pressure=irfft(pressure_spectrum*rate,n=size,axis=0)
    return pressure,f,pressure_spectrum,size
