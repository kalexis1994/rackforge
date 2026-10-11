import sys
import unittest
from pathlib import Path

import numpy as np
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import test_concert_performance as performance_tests
from concert_grand_performance import play
from concert_grand_pedal_action import PedalControl, ShiftedHammers, ShiftedContact, Stroke
from concert_grand_packed_air import PackedCoast
from concert_grand_shared_strings import SharedDampers


def event(tick, kind='cc', key=64, value=0):
    return dict(tick=tick, kind=kind, key=key, value=value, channel=0)


def fixture():
    old, contact, _, _ = performance_tests.PerformanceTests().fixture()
    system = ShiftedHammers(old.string, old.felt)
    air, residues = contact.air, contact.packed_residues
    return system, ShiftedContact(system, air, residues), PackedCoast(system, air, residues), SharedDampers(system.string, system.nh)


class PedalActionTests(unittest.TestCase):
    def test_actuator_retarget_is_continuous_and_repeated_cc_does_not_restart(self):
        stroke = Stroke()
        stroke.move(1., 0.)
        self.assertEqual(stroke.position(.015), .5)
        stroke.move(1., .015)
        self.assertEqual(stroke.position(.03), 1.)
        stroke.move(0., .03)
        before = stroke.position(.04)
        stroke.move(.3, .04)
        self.assertEqual(stroke.position(.04), before)
        self.assertEqual(stroke.position(.07), .3)

    def test_continuous_preload_key_release_and_unmatched_off(self):
        control = PedalControl([60, 61])
        control.validate([event(0, value=63), event(1, key=67, value=93)])
        self.assertEqual(control.levels(0.), {60: 1., 61: 1.})
        control.event(event(0, value=63), 0.)
        partial = control.levels(.03)[60]
        self.assertGreater(partial, 0.)
        self.assertLess(partial, 1.)
        other = PedalControl([60])
        other.event(event(0, value=64), 0.)
        self.assertLess(abs(partial - other.levels(.03)[60]), .02)
        control.event(event(0, kind='on', key=60, value=84), .04)
        control.event(event(0, kind='on', key=60, value=84), .05)
        control.event(event(0, kind='off', key=60), .06)
        self.assertEqual(control.levels(.07)[60], 0.)
        control.event(event(0, kind='off', key=60), .08)
        control.event(event(0, kind='off', key=60), .09)
        self.assertEqual(control.changed[60], .08)
        self.assertAlmostEqual(control.levels(.12)[60], partial)
        control.event(event(0, value=127), .13)
        self.assertEqual(control.levels(.16)[60], 0.)
        control.event(event(0, value=0), .17)
        self.assertGreater(control.levels(.185)[60], 0.)
        self.assertEqual(control.levels(.2)[60], 1.)
        with self.assertRaisesRegex(ValueError, 'sostenuto'):
            control.validate([event(0, key=66, value=127)])

    def test_cold_pre_roll_preserves_controller_values_and_rejects_stale_solver(self):
        control = PedalControl([60, 61], origin_s=-1.)
        control.event(event(0, key=67, value=93), -1.)
        control.event(event(0, key=64, value=64), -.5)
        self.assertEqual(control.soft.position(0.), 93 / 127)
        self.assertEqual(control.sustain.position(0.), 64 / 127)
        system, contact, _, _ = performance_tests.PerformanceTests().fixture()
        with self.assertRaisesRegex(ValueError, 'synchronized contact solver'):
            control.bind(system, contact)

    def test_overlap_signed_parameter_work_and_contact_solver_refresh(self):
        system, contact, _, _ = fixture()
        q, v, r, z = contact.initial_state(0.)
        q[system.string.size:] = .0001 * np.sqrt(system.felt.hammer_mass)
        originals = [x.copy() for x in (q, v, r, z)]
        before = system.energy(q, v)
        work = system.set_shift(q, 1.)
        np.testing.assert_array_equal(system.weights(1.), [0., 1., 1., 0., 1., 1.])
        self.assertLess(work, 0.)
        self.assertAlmostEqual(system.energy(q, v) - before, work, places=18)
        a = contact.project_full(q)
        contact.remainder(q, a, a, 1 / 256000)
        np.testing.assert_array_equal(contact.stiffness, [k for _, _, k in system.contacts])
        self.assertEqual(np.count_nonzero(contact.stiffness), 4)
        back = system.set_shift(q, 0.)
        self.assertAlmostEqual(back, -work, places=18)
        np.testing.assert_array_equal(system.weights(.5), [.5, 1., 1., .5, 1., 1.])
        for actual, expected in zip((q, v, r, z), originals):
            np.testing.assert_array_equal(actual, expected)

    def test_inactive_wire_does_not_restrict_hammer_launch(self):
        system, contact, _, _ = fixture()
        state = contact.initial_state(0.)
        system.set_shift(state[0], 1.)
        # The first wire moves toward the resting head; it has no contact area.
        inactive = system.contacts[0][1]
        direction = inactive.copy()
        direction[system.string.size:] = 0.
        state[0][:] += direction * (.02 / (direction @ direction))
        self.assertGreater(inactive @ state[0], 0.)
        self.assertLess(system.compression(state[0]), 0.)
        new, work = system.launch(state, 60, 3.)
        self.assertGreater(work, 0.)
        np.testing.assert_array_equal(new[0][:system.string.size], state[0][:system.string.size])
        for i in (2, 3):
            np.testing.assert_array_equal(new[i], state[i])

    def test_live_half_pedal_and_shift_close_work_and_preserve_block_states(self):
        # Includes changing area during the first hammer contact, half pedal,
        # pedal return and two distinct heads; same trajectory in two buffers.
        events = [event(0, value=93), event(0, kind='on', key=60, value=84),
                  event(32, key=67, value=127), event(512, kind='off', key=60),
                  event(768, kind='on', key=61, value=90), event(1024, value=40),
                  event(1280, key=67, value=0), event(1536, kind='off', key=61),
                  event(1792, value=0)]
        results = []
        for block_s in (.002, .004):
            system, contact, coast, dampers = fixture()
            control = PedalControl(system.keys, travel_s=.001, key_return_s=.001)
            result, metrics = play(system, contact, coast, dampers, events, 2048,
                                   lambda *_: None, block_s=block_s, control=control)
            scale = metrics['external_head_work_j'] + abs(metrics['pedal_parameter_work_j'])
            self.assertLess(abs(metrics['mechanical_work_residual_j']) / scale, 1e-8)
            self.assertGreater(metrics['damper_heat_j'], 0.)
            self.assertNotEqual(metrics['pedal_parameter_work_j'], 0.)
            self.assertTrue(all(np.isfinite(x).all() for x in result))
            results.append((result, metrics))
        for a, b in zip(results[0][0], results[1][0]):
            np.testing.assert_array_equal(a, b)
        for key in ['pedal_parameter_work_j', 'damper_heat_j', 'mechanical_work_residual_j']:
            self.assertEqual(results[0][1][key], results[1][1][key])


if __name__ == '__main__':
    unittest.main()
