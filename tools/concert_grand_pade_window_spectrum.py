"""Exact finite DFT of centered Padé section inputs, including both endpoints.

Observer-only prototype. No wave/contact dynamics change, modal reduction,
periodic-window assumption or acoustic energy inferred from mechanical work.
Endpoint replay must be checked separately before using a field certificate.
"""
import numpy as np


def section_input_spectrum(wave,dt,initial,final,port_spectrum,steps,fft_size,first,last):
    """Return (frequency, section input, mode) for one bounded frequency block.

    port_spectrum is the unnormalized rfft of the N-step midpoint port
    history, optionally zero padded. No section-by-time history is needed.
    Section x[k+1]=M x[k]+B u[k], y[k]=C x[k]+D u[k] gives
    Y=(D+z*C*(I-z*M)^-1*B)*U+C*(I-z*M)^-1*(x[0]-z^N*x[N]).
    Retain z^N even when fft_size exceeds the physical history length.
    """
    if (not np.isfinite(dt) or dt<=0 or not isinstance(steps,int) or steps<1
            or not isinstance(fft_size,int) or fft_size<steps
            or not isinstance(first,int) or not isinstance(last,int)
            or not 0<=first<last<=fft_size//2+1):
        raise ValueError('positive step, finite-window length and frequency block required')
    if np.iscomplexobj(initial) or np.iscomplexobj(final):raise ValueError('real endpoint states required')
    initial=wave.validate(initial);final=wave.validate(final)
    port=np.asarray(port_spectrum)
    if port.shape!=(fft_size//2+1,wave.modes) or not np.isfinite(port).all():
        raise ValueError('finite rfft port spectrum with all physical modes required')
    h=dt*wave.rate/2;den=1+6*h+12*h*h
    inverse=np.array([[1+6*h,h],[-12*h,1.]])/den
    M=2*inverse-np.eye(2);B=np.array([2*h*h,2*h])/den
    C=np.array([144*h,-12.])/den;D=(1-6*h+12*h*h)/den
    angle=-2j*np.pi*np.arange(first,last)/fft_size
    z=np.exp(angle);boundary_phase=np.exp(angle*steps)
    matrix=np.eye(2)[None,:,:]-z[:,None,None]*M
    # C*(I-z*M)^-1, evaluated without explicit inverses.
    w=np.linalg.solve(matrix.transpose(0,2,1),np.broadcast_to(C,(len(z),2))[...,None])[...,0]
    transfer=D+z*(w@B)
    out=np.empty((len(z),wave.order+1,wave.modes),complex)
    out[:,0]=port[first:last]
    for section in range(wave.order):
        boundary=initial[section][None,:,:]-boundary_phase[:,None,None]*final[section][None,:,:]
        source=np.einsum('fa,fam->fm',w,boundary)
        out[:,section+1]=transfer[:,None]*out[:,section]+source
    return out
