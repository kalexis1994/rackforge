"""Passive acoustic reduction for an explicitly approximate listening pilot.

No soundboard/string coordinate is removed. Congruence projects the positive
wave factor in the metric of the full acoustic static mass. Completing the
remaining mass and high resistance with positive relaxation retains both
limits, reciprocity and passivity. Rebuild from cold states; old capsules
are not compatible with this changed acoustic operator.
"""
import numpy as np
from concert_grand_pade_air import PadeAir, PadeBoundedAir
from concert_grand_pade_midpoint import PadeMidpoint
from concert_grand_fast_contact import FastBoundedContact
from concert_grand_damped_coast import DampedCoast
from concert_grand_rational_air import RationalAir
from concert_grand_bounded_air import RelaxingMass, positive_root


class ProjectedBase:
    static_mass = RationalAir.static_mass
    def __init__(self, reference, lift):
        self.reference = reference
        self.lift = np.asarray(lift)
        self.modes = self.lift.shape[1]
        self.rate = reference.rate
        self.order = reference.order
        self.states = self.order*self.modes
        self.outputs = reference.outputs
        self.tail_mass = np.zeros((self.modes, self.modes))
        # Stream congruences; never write another large R cache.
        self.R = np.empty((len(reference.R), self.modes, self.modes))
        for first in range(0, len(self.R), 16):
            part = self.lift.T@(reference.R[first:first+16]@self.lift)
            self.R[first:first+16] = (part+part.swapaxes(1, 2))/2
        self.D = self.R[0]/2
    def energy(self, state):
        return self.reference.energy(np.asarray(state)@self.lift.T)
    def radiation_batch(self, inputs):
        return self.reference.radiation_batch(np.asarray(inputs)@self.lift.T)


class MappedWave(PadeAir):
    def __init__(self, reference, lift, port):
        self.inner = PadeAir(ProjectedBase(reference.base, lift))
        self.port = np.asarray(port)
        self.modes = self.port.shape[0]
        self.rate, self.order = self.inner.rate, self.inner.order
        self.states, self.outputs = self.inner.states, self.inner.outputs
    def impedance(self, s):
        return self.port@self.inner.impedance(s)@self.port.T
    def static_mass(self):
        return self.port@self.inner.static_mass()@self.port.T
    def high_limit(self):
        return self.port@self.inner.high_limit()@self.port.T
    def zero_state(self):
        return self.inner.zero_state()
    def derivative(self, state, velocity):
        d, f = self.inner.derivative(state, self.port.T@velocity)
        return d, self.port@f
    def energy(self, state):
        return self.inner.energy(state)
    def radiation_batch(self, inputs):
        return self.inner.radiation_batch(inputs)


class MappedPlan:
    def __init__(self, wave, dt):
        self.plan = PadeMidpoint(wave.inner, dt)
        self.port = wave.port
    def constant(self, state):
        force, offsets = self.plan.constant(state)
        return self.port@force, offsets
    def update(self, state, velocity, offsets):
        return self.plan.update(state, self.port.T@velocity, offsets)


def project_air(reference, basis):
    mass = reference.static_mass()
    values, vectors = np.linalg.eigh((mass+mass.T)/2)
    if values.min() <= 0:
        raise ValueError('positive physical acoustic mass required')
    root = (vectors*np.sqrt(values))@vectors.T
    inverse = (vectors/np.sqrt(values))@vectors.T
    U = np.asarray(basis, float)
    if U.ndim != 2 or U.shape[0] != reference.modes or not np.allclose(U.T@U, np.eye(U.shape[1]), atol=2e-12, rtol=0):
        raise ValueError('orthonormal mass-metric acoustic basis required')
    wave = MappedWave(reference.wave, inverse@U, root@U)
    air = PadeBoundedAir(wave, mass, reference.high_limit())
    return air


class ParallelRelaxation(RelaxingMass):
    """Positive parallel branches; port dimension differs from state count."""
    def __init__(self, original, extra, fraction):
        self.B = np.column_stack([np.sqrt(1-fraction)*original.B, extra.B])
        self.tau = np.r_[original.tau, extra.tau]
        self.modes, self.states = self.B.shape

    def midpoint(self, state, velocity, dt):
        if state.shape != (self.states,) or velocity.shape != (self.modes,) or dt <= 0:
            raise ValueError('parallel relaxation dimensions/time')
        u = self.B.T@velocity
        alpha = dt/(2*self.tau)
        center = (state+alpha*u)/(1+alpha)
        difference = u-center
        return 2*center-state, self.B@difference, float(difference@difference)


class ParallelAir(PadeBoundedAir):
    def zero_state(self):
        return self.wave.zero_state(), np.zeros(self.relaxation.states)


def project_air_preserving_relaxation(reference, basis, fraction=.001):
    """Retain 99.9% of the original completion instead of retuning it all.

    Wave congruence uses its own positive static-mass metric. The omitted
    wave mass is positive semidefinite. A small share of the original positive
    mass/resistance completes it in a parallel branch, so both physical limits
    remain exact. Every branch is passive; nothing is clipped or subtracted
    from its storage. Its midband response must still be measured.
    """
    if not 0 < fraction < 1:
        raise ValueError('positive resistance share')
    mass = reference.wave.static_mass()
    root, inverse = positive_root(mass)
    U = np.asarray(basis, float)
    if U.ndim != 2 or U.shape[0] != reference.modes or not np.allclose(U.T@U, np.eye(U.shape[1]), atol=2e-12, rtol=0):
        raise ValueError('orthonormal wave-mass basis required')
    wave = MappedWave(reference.wave, inverse@U, root@U)
    original = reference.relaxation
    original_mass = (original.B*original.tau)@original.B.T
    original_limit = original.B@original.B.T
    extra = RelaxingMass(mass-wave.static_mass()+fraction*original_mass,
        fraction*original_limit+reference.wave.high_limit()-wave.high_limit())
    air = ParallelAir.__new__(ParallelAir)
    air.wave = wave
    air.wave_limit = wave.high_limit()
    air.relaxation = ParallelRelaxation(original, extra, fraction)
    air.modes = reference.modes
    air.states = wave.states+air.relaxation.states
    return air


class AuditionContact(FastBoundedContact):
    def _prepare(self, dt):
        cache = super()._prepare(dt)
        if not isinstance(cache['wave_plan'], MappedPlan):
            cache['wave_plan'] = MappedPlan(self.air.wave, dt)
        return cache


class AuditionCoast(DampedCoast):
    def _air_prepare(self, dt):
        cache = super()._air_prepare(dt)
        if not isinstance(cache['wave_plan'], MappedPlan):
            cache['wave_plan'] = MappedPlan(self.air.wave, dt)
        return cache
