"""DC wave mass from cumulative factor, before allocating modal correlations.

For zero-sum paired H, -sum(k*R_k)/rate equals the Gram of its cumulative
factor divided by rate. This is the same discrete/Cayley static mass, not
an alternative fit or an approximation of the continuous Rayleigh target.
"""
import numpy as np


def static_wave_mass(provider,progress=None):
    mass=np.zeros((provider.modes,provider.modes))
    for i,H in enumerate(provider.blocks()):
        scale=max(float(np.linalg.norm(H)),1e-30)
        if np.linalg.norm(H.sum(axis=0))>1e-11*scale:raise ValueError('zero-DC factor required')
        # The last cumulative row is zero; omit only that exact endpoint.
        cumulative=np.cumsum(H,axis=0)[:-1].reshape(-1,provider.modes)
        mass+=cumulative.T@cumulative/provider.rate
        if progress is not None:progress(i+1)
    return (mass+mass.T)/2
