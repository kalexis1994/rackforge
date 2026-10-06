"""Vectorized two-state Padé midpoint cascade, with persistent live states.

The lfilter axis is the sequence of acoustic sections, not audio time.
This evaluates the same implicit midpoint equations as PadeAir. It does
not add damping, reset memory, or change the integration step.
"""
import numpy as np
from scipy.signal import lfilter

from concert_grand_pade_air import PadeAir


class PadeMidpoint:
    def __init__(self, wave, dt, residue_kernel='batched'):
        if not isinstance(wave, PadeAir):
            raise ValueError('two-state Pade wave required')
        if not np.isfinite(dt) or dt <= 0:
            raise ValueError('positive finite time step')
        if residue_kernel not in ('batched', 'reference'):
            raise ValueError('known full residue contraction required')
        self.residue_kernel = residue_kernel
        self.wave = wave
        self.dt = float(dt)
        self.h = dt * wave.rate / 2
        self.denominator = 1 + 6*self.h + 12*self.h*self.h
        self.transfer = (1 - 6*self.h + 12*self.h*self.h) / self.denominator
        self.powers = self.transfer**np.arange(1, wave.order + 1)

    def offsets(self, state):
        state = self.wave.validate(state)
        source = (144*self.h*state[:, 0] - 12*state[:, 1]) / self.denominator
        return lfilter([1.], [1., -self.transfer], source, axis=0)

    def constant(self, state):
        offsets = self.offsets(state)
        # No modal-by-(sections*modal) copy of the full correlation tensor.
        if self.residue_kernel == 'reference':
            force = np.einsum('kmn,kn->m', self.wave.R[1:], offsets, optimize=False)
        else:
            # Batched BLAS matrix/vector products read the original contiguous
            # section matrices. Only (sections, modes) outputs are allocated;
            # no transposed modal-by-section copy of R and no modal reduction.
            force = np.matmul(self.wave.R[1:], offsets[:, :, None])[:, :, 0].sum(axis=0)
        return force, offsets

    def update(self, state, velocity, offsets=None):
        state, velocity = self.wave.validate(state, velocity)
        offsets = self.offsets(state) if offsets is None else np.asarray(offsets)
        if offsets.shape != (self.wave.order, self.wave.modes) or not np.isfinite(offsets).all():
            raise ValueError('finite section offsets')
        inputs = np.empty((self.wave.order + 1, self.wave.modes))
        inputs[0] = velocity
        inputs[1:] = offsets + self.powers[:, None]*velocity
        center = np.empty_like(state)
        rhs = state[:, 1] + self.h*inputs[:-1]
        center[:, 0] = ((1 + 6*self.h)*state[:, 0] + self.h*rhs) / self.denominator
        center[:, 1] = (-12*self.h*state[:, 0] + rhs) / self.denominator
        return 2*center - state, inputs

    def midpoint_deferred(self, state, velocity):
        constant, offsets = self.constant(state)
        updated, inputs = self.update(state, velocity, offsets)
        force = constant + self.wave.impedance(2/self.dt).real @ velocity
        return updated, force, inputs
