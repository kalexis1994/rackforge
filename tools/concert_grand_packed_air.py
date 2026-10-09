"""Exact in-memory residue contraction for the frozen reduced acoustic pilot.

Same full lag/mode sum and persistent Padé states, one contiguous GEMV.
One shared RAM layout, no new disk cache or rank/lag truncation. Deliberately
bounded to the reduced operator; packing a full large R is not implicit.
"""
import numpy as np
from concert_grand_audition_air import MappedWave, MappedPlan
from concert_grand_shared_kernels import BatchedCoast
from concert_grand_exponential_contact import ExponentialContact


class PackedResidues:
    def __init__(self, wave, max_bytes=512*2**20):
        if not isinstance(wave, MappedWave):
            raise ValueError('mapped reduced wave required')
        inner = wave.inner
        required = inner.order*inner.modes*inner.modes*8
        if required > max_bytes:
            raise ValueError('packed residue RAM budget exceeded')
        self.wave = wave
        self.matrix = np.ascontiguousarray(inner.R[1:].transpose(1,0,2)).reshape(inner.modes,-1)
        self.matrix.flags.writeable = False


class PackedMappedPlan(MappedPlan):
    def __init__(self, wave, dt, residues):
        if residues.wave is not wave:
            raise ValueError('packed residues must belong to the same wave')
        super().__init__(wave, dt)
        self.residues = residues

    def constant(self, state):
        offsets = self.plan.offsets(state)
        force = self.residues.matrix @ offsets.ravel()
        return self.port @ force, offsets


class PackedAirMixin:
    def __init__(self, system, air, residues=None):
        super().__init__(system, air)
        self.packed_residues = residues if residues is not None else PackedResidues(air.wave)
        if self.packed_residues.wave is not air.wave:
            raise ValueError('same acoustic operator required')

    def _air_prepare(self, dt):
        cache = super()._air_prepare(dt)
        if not isinstance(cache['wave_plan'], PackedMappedPlan):
            cache['wave_plan'] = PackedMappedPlan(self.air.wave, dt, self.packed_residues)
        return cache


class PackedCoast(PackedAirMixin, BatchedCoast):
    pass


class PackedContact(PackedAirMixin, ExponentialContact):
    pass
