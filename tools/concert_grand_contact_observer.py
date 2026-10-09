"""Read-only endpoint force diagnostics and native separation brackets.

Forces use the continuous felt law at sampled states, not the step's discrete
work force. Brackets resolve endpoint sign changes only: refinement is needed
to detect crossings inside a coarse step. No interpolated event is claimed.
"""
import numpy as np


class ContactObserver:
    def __init__(self, system, key, state):
        self.system = system
        chosen = [c for c in system.contacts if c[0] == key]
        if not chosen:
            raise ValueError('key must have live contacts')
        self.directions = np.array([d for _, d, _ in chosen])
        self.stiffness = np.array([k for _, _, k in chosen])
        self.times = []; self.forces = []
        self.first_contact = [None]*len(chosen)
        self.first_separation = [None]*len(chosen)
        self.recontacts = np.zeros(len(chosen), dtype=int)
        self.previous = None
        self.append(0., state)

    def append(self, time, state):
        if self.times and time <= self.times[-1]:
            raise ValueError('strictly increasing observation time')
        delta = self.directions @ state[0]
        speed = self.directions @ state[1]
        force = self.stiffness*np.maximum(delta, 0.)**self.system.felt.exponent
        force *= np.exp(np.clip(self.system.felt.rate_damping_s_per_m*speed, -40, 40))
        if not np.isfinite(force).all():
            raise ValueError('nonfinite endpoint force')
        if self.previous is not None:
            bracket = [self.times[-1], float(time)]
            for j, (before, after) in enumerate(zip(self.previous, delta)):
                if before <= 0 < after:
                    if self.first_contact[j] is None:
                        self.first_contact[j] = bracket.copy()
                    elif self.first_separation[j] is not None:
                        self.recontacts[j] += 1
                if before > 0 >= after and self.first_separation[j] is None:
                    self.first_separation[j] = bracket.copy()
        else:
            for j, d in enumerate(delta):
                if d > 0:
                    self.first_contact[j] = [0., 0.]
        self.previous = delta
        self.times.append(float(time)); self.forces.append(force)

    def curve(self, time):
        return np.column_stack([np.interp(time, self.times, x)
                                for x in np.asarray(self.forces).T])

    def summary(self):
        force = np.asarray(self.forces)
        return dict(force_definition='Continuous unilateral felt law at native endpoint states; '
                    'not the discrete work/step-average force.',
                    force_peak_per_wire_n=force.max(axis=0).tolist(),
                    force_peak_sum_n=float(force.sum(axis=1).max()),
                    impulse_per_wire_ns=np.trapezoid(force, self.times, axis=0).tolist(),
                    first_contact_bracket_s=self.first_contact,
                    first_separation_bracket_s=self.first_separation,
                    endpoint_recontacts_per_wire=self.recontacts.tolist())


class ObservedContact:
    def __init__(self, pilot, key, state):
        self.pilot = pilot; self.system = pilot.system
        self.observer = ContactObserver(self.system, key, state)
        self.steps = 0; self.dt = None

    def step(self, state, dt, defer_radiation=False):
        if self.dt is not None and self.dt != dt:
            raise ValueError('observer requires a fixed step in this segment')
        self.dt = dt
        result, stats = self.pilot.step(state, dt, defer_radiation)
        self.steps += 1
        self.observer.append(self.steps*dt, result)
        return result, stats
