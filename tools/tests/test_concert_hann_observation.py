import copy
import sys
import unittest
from pathlib import Path
import numpy as np
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from concert_grand_hann_observation import hann_observation_spectra, profile_controls


class HannObservationTests(unittest.TestCase):
    def reports(self):
        row = dict(duration_s=.003, criteria=dict(analytic_oracle_relative=1e-10),
            oracle_frequency_hz=[0., 60., 6000., 8000., 10000., 12000., 16000., 20000., 24000.],
            local_oracle_relative=dict(velocity=[2e-11]*9, acceleration=[1.06e-10]*9),
            original_global_oracle_relative=dict(acceleration=[6e-12]*9))
        return dict(copy.deepcopy(row), rate=1536000), dict(copy.deepcopy(row), rate=3072000)

    def test_preserves_local_failure_and_failed_velocity(self):
        coarse, fine = self.reports()
        result = profile_controls(coarse, fine)
        self.assertTrue(all(r['velocity_local_passed'] and r['acceleration_original_passed'] for r in result))
        self.assertFalse(result[0]['experimental_acceleration_local_passed'])
        coarse['local_oracle_relative']['velocity'][-1] = 2e-10
        self.assertFalse(profile_controls(coarse, fine)[0]['velocity_local_passed'])

    def test_reject_changed_threshold_or_unmatched_observations(self):
        coarse, fine = self.reports()
        coarse['criteria']['analytic_oracle_relative'] = 2e-10
        fine['criteria']['analytic_oracle_relative'] = 2e-10
        with self.assertRaises(ValueError):
            profile_controls(coarse, fine)
        coarse, fine = self.reports()
        fine['oracle_frequency_hz'][-1] = 25000.
        with self.assertRaises(ValueError):
            profile_controls(coarse, fine)

    def test_live_constant_velocity_has_no_window_force(self):
        t = .007+np.arange(1153)/384000
        B = t[:, None]
        V = np.ones_like(B)
        v, a = hann_observation_spectra(t, B, V, [0., 24017.])
        self.assertEqual(float(abs(a).max()), 0.)
        self.assertAlmostEqual(float(v[0, 0].real), (t[-1]-t[0])/2, places=15)
        # Absolute source phase on a nonzero start; closed constant-V integral.
        f = 24017.
        T = t[-1]-t[0]
        def integral(hz):
            return np.expm1(-2j*np.pi*hz*T)/(-2j*np.pi*hz)
        exact = np.exp(-2j*np.pi*f*t[0])*(.5*integral(f)-.25*integral(f-1/T)-.25*integral(f+1/T))
        np.testing.assert_allclose(v[1, 0], exact, rtol=2e-9, atol=1e-18)


if __name__ == '__main__':
    unittest.main()
