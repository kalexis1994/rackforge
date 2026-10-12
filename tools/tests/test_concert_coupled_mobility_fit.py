import unittest
import numpy as np
from concert_grand_coupled_mobility_fit import objective_gradient, damping
from concert_grand_realtime_body import Body


class CoupledMobilityFitTests(unittest.TestCase):
    def test_complex_adjoint_gradient_against_finite_difference(self):
        omega = np.array([120., 450., 920.])
        base = np.array([4., 9., 12.])
        ports = np.array([[.8, -.2], [.3, .7], [-.4, .2]])
        frequency = np.array([11., 30., 63., 141., 350.])
        target = Body.mechanical(omega, np.diag(base*1.4), ports).response(frequency)
        parameters = np.array([.1, -.1, .2, .1, -.2, .15, .03, -.1, .08])
        _, gradient = objective_gradient(parameters, omega, base, ports, frequency, target, 2)
        measured = []
        for i in range(len(parameters)):
            delta = np.zeros_like(parameters)
            delta[i] = 1e-6
            plus = objective_gradient(parameters+delta, omega, base, ports, frequency, target, 2)[0]
            minus = objective_gradient(parameters-delta, omega, base, ports, frequency, target, 2)[0]
            measured.append((plus-minus)/2e-6)
        np.testing.assert_allclose(gradient, measured, rtol=3e-6, atol=1e-9)

    def test_arbitrary_cross_terms_keep_positive_loss(self):
        base = np.array([4., 9., 12.])
        parameters = np.r_[[1., -3., .5], np.arange(6)/4]
        c, _, _ = damping(parameters, base, 2)
        np.testing.assert_allclose(c, c.T, atol=0)
        self.assertGreater(np.linalg.eigvalsh(c).min(), 0.)
        self.assertNotEqual(c[0, 1], 0.)


if __name__ == '__main__':
    unittest.main()
