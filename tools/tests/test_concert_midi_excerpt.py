import unittest
from types import SimpleNamespace
import numpy as np
from concert_grand_midi_excerpt import excerpt
from concert_grand_performance import play


def event(tick, kind='on', key=63, value=28):
    return dict(tick=tick, kind=kind, key=key, value=value, channel=0)


class MidiExcerptTests(unittest.TestCase):
    def test_original_events_and_pre_roll_survive_and_tail_is_declared(self):
        events = [event(0, 'cc', 67, 93), event(100, value=28),
                  event(111, key=75, value=14), event(150, 'off', value=0),
                  event(180, 'off', key=75, value=0), event(300, value=33)]
        plan = dict(contact_rate_hz=256000, events=events)
        result = excerpt(plan, 100, 200, bank_keys=[63, 75, 87, 99])
        self.assertEqual(result['original_pre_attack_events'], events[:1])
        self.assertEqual(result['original_shifted_events'], [dict(e, tick=e['tick'] - 100) for e in events[1:5]])
        self.assertEqual(result['original_attacks'], 2)
        self.assertEqual(result['original_note_offs'], 2)
        self.assertEqual(result['simulated_events'][-1], event(100, 'cc', 64, 0))
        self.assertEqual(result['end_tick'], 300)
        self.assertEqual(plan['events'], events)

    def test_held_cut_missing_key_and_non_cold_prefix_are_rejected(self):
        plan = dict(contact_rate_hz=256000, events=[event(100), event(180, 'off', value=0)])
        with self.assertRaisesRegex(ValueError, 'original key releases'):
            excerpt(plan, 50, 200, bank_keys=[63])
        with self.assertRaisesRegex(ValueError, 'constructed physical bank'):
            excerpt(plan, 100, 200, bank_keys=[75])
        with self.assertRaisesRegex(ValueError, 'cold silence'):
            excerpt(dict(plan, events=[event(0, 'off', value=0)] + plan['events']), 100, 200, bank_keys=[63])

    def test_native_frontiers_are_bit_identical_after_many_absolute_time_blocks(self):
        # Pure clock/capture fixture: no costly physical operator or duplicated
        # mechanics assertion. Curvature exposes endpoint re-evaluation error.
        core = SimpleNamespace(size=1, nb=1, sb=slice(0, 1), board_transform=np.eye(1))
        system = SimpleNamespace(keys=[63], string=core, compression=lambda _: -1.,
                                 energy=lambda q, v: float((q @ q + v @ v) / 2))
        class Carrier:
            def __init__(self):
                self.count = 0
            def initial_state(self, _):
                return np.zeros(1), np.array([990.]), np.zeros(1), np.zeros(1)
            def step(self, state, dt, _):
                self.count += 1
                phase = 990. * self.count / 1000
                return (np.array([np.sin(phase)]), np.array([990. * np.cos(phase)]),
                        state[2], state[3]), dict(felt_loss_j=0., material_loss_j=0., air_port_work_j=0.)
        carrier = Carrier()
        damper = SimpleNamespace(dissipate=lambda v, dt, levels: (v, 0.))
        previous = []
        def collect(start, trace):
            t, b, v = trace
            if previous:
                np.testing.assert_array_equal(b[0], previous[0])
                np.testing.assert_array_equal(v[0], previous[1])
            previous[:] = [b[-1].copy(), v[-1].copy()]
        _, metrics = play(system, carrier, carrier, damper, [], 960, collect,
            contact_rate=1000, coast_rate=1000, capture_rate=1000, block_s=.008)
        self.assertEqual(metrics['chunks'], 120)


if __name__ == '__main__':
    unittest.main()
