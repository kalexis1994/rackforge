"""Independent positive storage and radiation in one angular factor pass.

All modes, sections and output channels stay. This batches observables,
never infers radiation/storage from work or changes the acoustic dynamics.
"""
import numpy as np
from scipy.fft import rfft, irfft, next_fast_len


def certificate_batch(provider, states, inputs, progress=None, input_block=16):
    states = np.asarray(states, dtype=float)
    if (states.ndim != 4 or states.shape[1:] != (provider.taps-1, 2, provider.modes)
            or not np.isfinite(states).all()):
        raise ValueError('finite batch of two-state wave memories')
    if input_block < 1:
        raise ValueError('positive input block')
    for item in inputs:
        if (item.ndim != 3 or item.shape[1:] != (provider.taps, provider.modes)
                or not np.isfinite(item).all()):
            raise ValueError('finite batches of centered section inputs')
    order = provider.taps-1
    size = next_fast_len(2*order)
    # Each section coordinate has its own positive storage weight.
    memory = states.transpose(0, 2, 1, 3).reshape(-1, order, provider.modes)
    S = rfft(memory[:, ::-1], n=size, axis=1)
    energies = np.zeros(len(memory))
    losses = [np.zeros(len(item)) for item in inputs]
    for block, H in enumerate(provider.blocks()):
        F = rfft(H, n=size, axis=0)
        field = np.einsum('fom,bfm->bfo', F, S, optimize=True)
        tails = irfft(field, n=size, axis=1)[:, order:2*order]
        energies += np.sum(tails*tails, axis=(1, 2))/(2*provider.rate)
        observer = np.ascontiguousarray(H.transpose(1,0,2)).reshape(H.shape[1],-1)
        for item, loss in zip(inputs, losses):
            for first in range(0, len(item), input_block):
                part = item[first:first+input_block]
                field = part.reshape(len(part),-1) @ observer.T
                loss[first:first+input_block] += np.sum(field*field, axis=1)/2
        if progress is not None:
            progress(block+1)
    energies = energies.reshape(len(states), 2) @ np.array([144., 12.])
    return energies, losses
