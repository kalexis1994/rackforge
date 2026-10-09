import unittest
from concert_grand_damper_control import DamperControl
from concert_grand_damped_coast import engagement


class DamperControlTests(unittest.TestCase):
    def test_rest_and_note_release(self):
        c = DamperControl([60, 67])
        self.assertEqual(c.levels(0.), {60: 1., 67: 1.})
        c.event('on', 60, 0.)
        self.assertEqual(c.levels(.1), {60: 0., 67: 1.})
        c.event('off', 60, .14)
        self.assertAlmostEqual(c.levels(.155)[60], .5)
        self.assertEqual(c.levels(.18)[60], 1.)

    def test_redundant_pedal_up_does_not_open_dampers(self):
        c = DamperControl([60, 67])
        c.event('on', 60, 0.)
        c.event('off', 60, .14)
        dates = c.changed.copy()
        c.event('pedal_off', 0, 1.05)
        self.assertEqual(c.changed, dates)
        self.assertEqual(c.levels(1.05), {60: 1., 67: 1.})

    def test_real_pedal_release_keeps_held_key_open(self):
        c = DamperControl([60, 67], True)
        c.event('on', 60, 0.)
        self.assertEqual(c.levels(.1), {60: 0., 67: 0.})
        c.event('pedal_off', 0, 1.)
        self.assertEqual(c.levels(1.), {60: 0., 67: 0.})
        self.assertAlmostEqual(c.levels(1.015)[67], .5)
        self.assertEqual(c.levels(1.04), {60: 0., 67: 1.})

    def test_accepted_pedal_schedule_is_unchanged(self):
        c = DamperControl([60, 67, 72], True)
        keys = {k: False for k in c.keys}
        dates = {k: 0. for k in keys}
        pedal = True
        schedule = [(0., 'on', 60), (.14, 'off', 60), (.25, 'on', 67),
                    (.39, 'off', 67), (.5, 'on', 72), (.64, 'off', 72),
                    (.75, 'on', 60), (.89, 'off', 60), (1.05, 'pedal_off', 0)]
        cursor = 0
        for i in range(2001):
            now = i / 1000
            while cursor < len(schedule) and schedule[cursor][0] <= now:
                t, kind, key = schedule[cursor]
                c.event(kind, key, t)
                if kind in ('on', 'off'):
                    keys[key] = kind == 'on'
                    dates[key] = t
                else:
                    pedal = False
                    for k in keys:
                        if not keys[k]: dates[k] = t
                cursor += 1
            old = {k: 0. if pedal or keys[k] else engagement(now-dates[k]) for k in keys}
            self.assertEqual(c.levels(now), old)


if __name__ == '__main__':
    unittest.main()
