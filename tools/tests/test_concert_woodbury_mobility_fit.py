import unittest
import numpy as np
from concert_grand_woodbury_mobility_fit import objective_gradient, response_states
from concert_grand_coupled_mobility_fit import objective_gradient as dense_objective, damping
from scipy.linalg import solve


class WoodburyFitTests(unittest.TestCase):
    def test_dense_and_low_rank_responses_and_adjoint_agree(self):
        rng = np.random.default_rng(61)
        omega = np.geomspace(100.,4e7,19)
        base = omega*.046
        ports = rng.normal(size=(19,4))
        x = np.r_[rng.normal(0,.3,19),rng.normal(0,.2,(19,3)).ravel()]
        frequency = np.geomspace(11.,5900.,17)
        c, _, _ = damping(x,base,3)
        expected = np.array([solve(np.diag(omega**2+s*s)+s*c,ports,assume_a='sym') for s in 2j*np.pi*frequency])
        actual, _, _ = response_states(x,omega,base,ports,frequency,3)
        np.testing.assert_allclose(actual,expected,rtol=1e-10,atol=1e-16)
        target = np.array([s*ports.T @ q for s,q in zip(2j*np.pi*frequency,expected)])*1.03
        measured = objective_gradient(x,omega,base,ports,frequency,target,3)
        original = dense_objective(x,omega,base,ports,frequency,target,3)
        np.testing.assert_allclose(measured[0],original[0],rtol=1e-10,atol=1e-15)
        np.testing.assert_allclose(measured[1],original[1],rtol=1e-9,atol=1e-13)


if __name__ == '__main__':
    unittest.main()
