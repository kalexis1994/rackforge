import sys, unittest
from pathlib import Path
from types import SimpleNamespace
import numpy as np
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from concert_grand_contact_observer import ContactObserver


class ObserverTests(unittest.TestCase):
    def fixture(self):
        system = SimpleNamespace(contacts=[(72, np.array([1., 0.]), 2.),
                                          (72, np.array([0., 1.]), 3.)],
                                 felt=SimpleNamespace(exponent=2., rate_damping_s_per_m=.1))
        return system

    def test_individual_brackets_recontact_and_no_initial_false_separation(self):
        state = (np.zeros(2), np.ones(2))
        observer = ContactObserver(self.fixture(), 72, state)
        self.assertEqual(observer.first_separation, [None, None])
        observer.append(.1, (np.ones(2), np.zeros(2)))
        observer.append(.2, (np.array([0., 1.]), np.zeros(2)))
        observer.append(.3, (-np.ones(2), np.zeros(2)))
        observer.append(.4, (np.array([1., -1.]), np.zeros(2)))
        self.assertEqual(observer.first_separation, [[.1, .2], [.2, .3]])
        self.assertEqual(observer.recontacts.tolist(), [1, 0])
        np.testing.assert_array_equal(state[0], np.zeros(2))

    def test_force_is_continuous_rate_law_not_discrete_step_force(self):
        state = (np.array([2., -1.]), np.array([3., 8.]))
        before = tuple(x.copy() for x in state)
        observer = ContactObserver(self.fixture(), 72, state)
        np.testing.assert_allclose(observer.forces[0], [8*np.exp(.3), 0.])
        for a, b in zip(state, before):
            np.testing.assert_array_equal(a, b)
        observer.append(.1, (np.zeros(2), np.zeros(2)))
        np.testing.assert_allclose(observer.curve([.05])[0], [4*np.exp(.3), 0.])
        summary = observer.summary()
        np.testing.assert_allclose(summary['impulse_per_wire_ns'], [.4*np.exp(.3), 0.])
        self.assertEqual(summary['first_separation_bracket_s'], [[0., .1], None])


if __name__ == '__main__': unittest.main()
