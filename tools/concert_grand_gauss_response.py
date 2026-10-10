"""Harmonic response of Gauss air under constant input within an interval.

This is not Gauss applied to a continuously sinusoidal stage input. It
matches the actual displacement-average port used by the coupled pilot.
The two continuous resolvents are derived from its discrete state equations.
"""
import numpy as np


def harmonic_impedance(impedance,omega,dt):
    if not np.isfinite(omega) or not np.isfinite(dt) or omega<=0 or dt<=0 or omega*dt>=np.pi:
        raise ValueError('positive frequency below the step Nyquist')
    s=2j/dt*np.tan(omega*dt/2)
    rho=np.sqrt(1-dt*dt*s*s/3)
    p1=2*s/(1+rho);p2=6*(1+rho)/(dt*dt*s)
    c1=(1+1/rho)/2;c2=(1-1/rho)/2
    return c1*impedance(p1)+c2*impedance(p2)
