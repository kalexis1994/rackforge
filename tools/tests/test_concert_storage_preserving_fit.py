import unittest
import numpy as np
from scipy.linalg import null_space, solve
from concert_grand_storage_preserving_fit import operators, objective_gradient


class StoragePreservingFitTests(unittest.TestCase):
    def setUp(self):
        self.omega = np.array([130., 430., 820., 1400.])
        self.ports = np.array([[.8, -.2], [.3, .7], [-.4, .2], [.1, .3]])
        self.base = np.diag([5., 10., 20., 35.])+np.ones((4,4))*2
        self.basis = null_space((self.ports/self.omega[:, None]).T)
        self.x = np.r_[[.2, -.15], [.1, -.2, .15, .03], np.arange(8)*.05-.1]

    def test_both_storage_moments_and_positive_operators(self):
        k, c, _, _ = operators(self.x, self.omega, self.base, self.basis, 2)
        expected = self.ports.T @ (self.ports/self.omega[:, None]**2)
        np.testing.assert_allclose(self.ports.T @ solve(k, self.ports), expected, rtol=1e-13, atol=1e-18)
        self.assertGreater(np.linalg.eigvalsh(k).min(), 0)
        self.assertGreater(np.linalg.eigvalsh(c).min(), 0)

    def test_stiffness_and_loss_adjoint_against_finite_difference(self):
        frequency = np.array([9., 25., 66., 117., 280.])
        target = np.array([s*self.ports.T @ solve(np.diag(self.omega**2+s*s)+s*self.base, self.ports)
                           for s in 2j*np.pi*frequency])
        arguments = (self.omega, self.ports, self.base, self.basis, frequency, target, 2)
        _, gradient = objective_gradient(self.x, *arguments)
        measured = []
        for i in range(len(self.x)):
            delta = np.zeros_like(self.x)
            delta[i] = 1e-6
            measured.append((objective_gradient(self.x+delta, *arguments)[0]-objective_gradient(self.x-delta, *arguments)[0])/2e-6)
        np.testing.assert_allclose(gradient, measured, rtol=4e-6, atol=1e-9)


if __name__ == '__main__':
    unittest.main()
