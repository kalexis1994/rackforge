"""Diagnostics for H_linear(full) + V_nonlinear(P_low q), no acoustic model."""
from math import gcd
import numpy as np
from scipy.signal import resample_poly
from concert_grand_modal_viability import omega, common_signal


def high_signal(source, low, rate=512000):
    """Exact linear high-mode mechanical velocity sampled before anti-aliasing.

    Diagnostic offline synthesis; this is NOT a measured realtime audio mixer.
    All high state coordinates survive, including inaudible axial coordinates;
    this particular observation reads only the vertical transverse coordinates.
    """
    nt=source['transverse'];nl=source['axial'];width=nl+2*nt
    count=round(source['times'][2]*rate);result=np.zeros((count,9));w=omega(source)
    n=np.arange(low+1,nt+1)
    weights=np.sin(np.pi*n[:,None]*np.array([.125,.31,.73]))
    weights*=np.exp(-1.2*(n*.02)**2)[:,None]/np.sqrt(source['parameters'][0]*source['parameters'][1]/2)
    for i in range(3):
        section=slice(i*width+nl+low,i*width+nl+nt);frequency=w[section]
        for start in range(0,count,4096):
            t=np.arange(start,min(start+4096,count))/rate
            phase=t[:,None]*frequency
            velocity=np.cos(phase)*source['v'][section]-np.sin(phase)*frequency*source['q'][section]
            result[start:start+len(t),i*3:(i+1)*3]=velocity @ weights
    return result


def hybrid_signal(source,low,trace,rate,output_rate=48000,dense_rate=512000):
    # Apply the same final anti-alias filter as the full reference. Low data get
    # bandlimited interpolation to the dense observation clock first. Boundaries
    # remain in the untrimmed diagnostic and are never gain/phase fitted away.
    rows=np.asarray(trace['rows']);divisor=gcd(rate,dense_rate)
    low_signal=resample_poly(rows[:-1,1:],dense_rate//divisor,rate//divisor,axis=0)
    full=low_signal+high_signal(source,low,dense_rate)
    divisor=gcd(dense_rate,output_rate)
    return resample_poly(full,output_rate//divisor,dense_rate//divisor,axis=0)
