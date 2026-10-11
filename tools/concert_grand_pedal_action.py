"""Assumed continuous damper support and grand-piano lateral hammer shift.

These are explicit geometric/actuator hypotheses, not identified Steinway
parameters. CC67 changes contact area, not incident speed or output gain.
Changing area during compression transfers signed parameter work. No living
string, head, board or acoustic state is reset when either pedal moves.
"""
from collections import Counter

import numpy as np
from concert_grand_damped_coast import engagement
from concert_grand_shared_strings import SharedHammers
from concert_grand_packed_air import PackedContact


class Stroke:
    """Prescribed 30ms smoothstep travel, continuous position on retarget.

    Each segment has zero endpoint slopes. Interrupting a moving segment
    resets the prescribed velocity; actuator inertia is not modeled here.
    """
    def __init__(self, duration=.03, origin_s=0.):
        if not np.isfinite(duration) or duration <= 0 or not np.isfinite(origin_s):
            raise ValueError('positive actuator travel time')
        self.duration = duration
        self.start, self.origin, self.target = origin_s, 0., 0.

    def position(self, now):
        if not np.isfinite(now) or now < self.start:
            raise ValueError('actuator time before last target')
        return self.origin + (self.target - self.origin) * engagement(now - self.start, self.duration)

    def move(self, target, now):
        if not np.isfinite(target) or not 0 <= target <= 1:
            raise ValueError('normalized actuator target')
        current = self.position(now)
        if target != self.target:
            self.origin, self.target, self.start = current, target, now


class ShiftedHammers(SharedHammers):
    """Three unit-pitch felt strips covered by a translating three-pitch head.

At zero shift the head covers [-.5,2.5]; at full shift it covers [.5,3.5].
The strips centered at 0,1,2 keep fractional overlap [1-u,1,1]. Stiffness
per strip scales by this area; the local felt law and all three wires stay.
Longitudinal contact directions, strike fraction and head mass are unchanged.
Only a three-wire register is supported; bass/other lateral geometry pending.
"""
    def __init__(self, core, felt):
        super().__init__(core, felt)
        if any(sum(key == k for k, _, _ in self.contacts) != 3 for key in self.keys):
            raise ValueError('lateral shift requires an explicit three-wire choir')
        self.base_contacts = tuple(self.contacts)
        self.shift = 0.
        self.shift_revision = 0

    def weights(self, shift):
        if not np.isfinite(shift) or not 0 <= shift <= 1:
            raise ValueError('normalized lateral shift')
        counts, weights = Counter(), []
        left, right = -.5 + shift, 2.5 + shift
        for key, _, _ in self.base_contacts:
            center = counts[key]
            counts[key] += 1
            weights.append(max(0., min(right, center + .5) - max(left, center - .5)))
        return np.array(weights)

    def set_shift(self, q, shift):
        if shift == self.shift:
            return 0.
        weights = self.weights(shift)
        updated = [(key, d, k * weight) for (key, d, k), weight in zip(self.base_contacts, weights)]
        # Work of prescribed lateral motion at fixed mechanical coordinates.
        # It can have either sign; this is not damper heat or hammer launch.
        work = sum((new_k - old_k) * max(float(d @ q), 0.)**(self.felt.exponent + 1)
                   / (self.felt.exponent + 1)
                   for (_, d, old_k), (_, _, new_k) in zip(self.contacts, updated))
        self.contacts = updated
        self.shift = float(shift)
        self.shift_revision += 1
        return float(work)

    def compression(self, q):
        return max(float(d @ q) for _, d, k in self.contacts if k > 0)

    def launch(self, state, key, speed):
        if key not in self.keys or not np.isfinite(speed) or speed < 0:
            raise ValueError('valid key and incident speed')
        q, v, r, z = state
        contacts = [d for kk, d, k in self.contacts if kk == key and k > 0]
        if max(d @ q for d in contacts) > 1e-12:
            raise ValueError('cannot relaunch a contacting hammer')
        n = self.string.size
        head = n + self.keys.index(key)
        new_q, new_v = q.copy(), v.copy()
        new_q[head] = min(-float(d[:n] @ q[:n]) / d[head] for d in contacts)
        new_v[head] = np.sqrt(self.felt.hammer_mass) * speed
        return (new_q, new_v, r, z), float(self.energy(new_q, new_v) - self.energy(q, v))


class ShiftedContact(PackedContact):
    """Refresh the cached contact coefficients when prescribed area changes."""
    def __init__(self, system, air, residues=None):
        if not isinstance(system, ShiftedHammers):
            raise ValueError('shifted hammer system required')
        super().__init__(system, air, residues)
        self.shift_revision = system.shift_revision

    def remainder(self, q, a, candidate, dt):
        if self.shift_revision != self.system.shift_revision:
            self.stiffness = np.array([k for _, _, k in self.system.contacts])
            self.shift_revision = self.system.shift_revision
        return super().remainder(q, a, candidate, dt)


class PedalControl:
    def __init__(self, keys, *, travel_s=.03, key_return_s=.03,
                 preload_exponent=1.5, allow_frozen_program=False, origin_s=0.):
        if (not np.isfinite(key_return_s) or key_return_s <= 0
                or not np.isfinite(preload_exponent) or preload_exponent <= 0):
            raise ValueError('positive key return time and preload exponent')
        self.held = Counter({key: 0 for key in keys})
        self.changed = {key: origin_s - key_return_s for key in keys}
        self.sustain, self.soft = Stroke(travel_s, origin_s), Stroke(travel_s, origin_s)
        self.key_return_s, self.exponent = key_return_s, preload_exponent
        self.allow_frozen_program = allow_frozen_program

    def bind(self, system, contact):
        if not isinstance(system, ShiftedHammers) or not isinstance(contact, ShiftedContact) or contact.system is not system:
            raise ValueError('pedal action requires shifted mechanics and synchronized contact solver')

    def validate(self, events):
        if len({e['channel'] for e in events}) > 1:
            raise ValueError('one physical piano channel required')
        for event in events:
            kind, key, value = (event[k] for k in ('kind', 'key', 'value'))
            if not isinstance(value, int) or not 0 <= value <= 127:
                raise ValueError('MIDI value range')
            if kind in ('on', 'off'):
                if key not in self.held or (kind == 'on' and value == 0):
                    raise ValueError('constructed physical bank and positive attack required')
            elif kind == 'cc':
                if key not in (64, 66, 67) or (key == 66 and value):
                    raise ValueError('controller/sostenuto mechanism not implemented')
            elif kind == 'program':
                if not self.allow_frozen_program:
                    raise ValueError('program requires explicit frozen-program selection')
            else:
                raise ValueError('unsupported planned event')

    def event(self, event, now):
        kind, key, value = (event[k] for k in ('kind', 'key', 'value'))
        if kind == 'on':
            self.held[key] += 1
            self.changed[key] = now
        elif kind == 'off':
            was_held = self.held[key] > 0
            self.held[key] = max(0, self.held[key] - 1)
            if was_held and not self.held[key]:
                self.changed[key] = now
        elif kind == 'cc' and key in (64, 67):
            (self.sustain if key == 64 else self.soft).move(value / 127, now)

    def levels(self, now):
        # The rack supports a released damper. Prescribed pedal lift and key
        # return set normalized felt penetration; nonnegative preload scales
        # the existing passive viscous patch. No conservative damper spring,
        # vertical damper mass, mechanical noise or identified threshold here.
        lift = self.sustain.position(now)
        return {key: 0. if count else max(0., engagement(now - self.changed[key], self.key_return_s) - lift)**self.exponent
                for key, count in self.held.items()}

    def advance(self, system, state, now):
        if not isinstance(system, ShiftedHammers):
            raise ValueError('pedal action requires shifted hammer mechanics')
        return system.set_shift(state[0], self.soft.position(now))

    def metadata(self):
        return dict(model='prescribed damper lift and lateral three-wire felt overlap',
            travel_s=self.sustain.duration, key_return_s=self.key_return_s,
            preload_exponent=self.exponent, lateral_pitch_units=1., hammer_width_pitch_units=3.,
            pedal_values_preserved=True, parameters_identified=False,
            damper_spring_or_mass_modeled=False, lateral_inertia_or_felt_variation_modeled=False,
            interrupted_stroke_velocity_reset=True,
            sostenuto_implemented=False, final_sustain_target=self.sustain.target,
            final_soft_target=self.soft.target, frozen_program_diagnostic=self.allow_frozen_program)
