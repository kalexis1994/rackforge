"""Select a physical prefix of an archived air factor without duplicating it.

This selects the wave factor and physical mass/resistance targets. Callers
must rebuild the passive completion at the selected dimension; slicing its
state coordinates is not equivalent.
"""
import numpy as np


def load_prefix(path,frequency):
    frequency=np.asarray(frequency,dtype=float)
    if frequency.ndim!=1 or not len(frequency) or not np.isfinite(frequency).all() or (np.diff(frequency)<=0).any():
        raise ValueError('ordered positive physical frequency prefix required')
    if (frequency<=0).any():raise ValueError('positive physical frequencies required')
    with np.load(path) as data:
        parent=data['frequency_hz']
        if parent.ndim!=1 or not np.isfinite(parent).all() or (np.diff(parent)<=0).any() or not np.array_equal(parent[:len(frequency)],frequency):
            raise ValueError('coefficient physical modal order does not match requested prefix')
        factor=data['factor'];mass=data['static_mass'];limit=data['high_limit'];rate=float(data['factor_rate'])
    if factor.ndim!=3 or factor.shape[2]!=len(parent) or mass.shape!=(len(parent),len(parent)) or limit.shape!=mass.shape:
        raise ValueError('parent coefficient dimensions')
    if not np.isfinite(rate) or rate<=0:raise ValueError('positive factor rate')
    n=len(frequency)
    return factor[:,:,:n],rate,mass[:n,:n],limit[:n,:n],len(parent)
