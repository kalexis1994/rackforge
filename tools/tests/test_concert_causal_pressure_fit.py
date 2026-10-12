import unittest
import numpy as np
from concert_grand_causal_pressure_fit import (support_delays, acceleration_response,
                                             pressure_response, fit_readout)


class CausalPressureFitTests(unittest.TestCase):
    def test_support_follows_each_original_microphone(self):
        kernel = np.zeros((12, 2, 3))
        kernel[3:9, 0] = 1.
        kernel[5:11, 1] = -1.
        actual = support_delays(kernel, 1000., 3)
        np.testing.assert_allclose(actual, [[.003, .0055, .008], [.005, .0075, .010]])

    def test_real_acceleration_readout_recovers_unseen_frequencies(self):
        omega = 2*np.pi*np.array([80., 170., 330.])
        damping = np.diag([20., 28., 40.])
        ports = np.array([[1., .2], [.3, 1.], [.6, -.4]])
        delays = np.array([[.001, .002], [.0015, .003]])
        readout = np.array([[[.2, -.1, .3], [.1, .15, -.05]],
                            [[-.1, .3, .1], [.2, .05, .1]]])
        training = np.linspace(20., 900., 96)
        validation = np.linspace(26., 883., 37)
        a = acceleration_response(omega, damping, ports, training)
        target = pressure_response(a, training, delays, readout)
        fitted, _ = fit_readout(a, training, target, delays, ridge=1e-10)
        b = acceleration_response(omega, damping, ports, validation)
        actual = pressure_response(b, validation, delays, fitted)
        expected = pressure_response(b, validation, delays, readout)
        self.assertLess(np.linalg.norm(actual-expected)/np.linalg.norm(expected), 1e-7)
        self.assertFalse(np.iscomplexobj(fitted))


if __name__ == '__main__':
    unittest.main()
