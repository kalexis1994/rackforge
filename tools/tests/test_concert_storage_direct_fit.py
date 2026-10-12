import unittest
import numpy as np
from scipy.linalg import null_space
from concert_grand_storage_preserving_fit import objective_gradient as dense
from concert_grand_storage_direct_fit import objective_gradient as direct


class StorageDirectTests(unittest.TestCase):
    def test_direct_stiffness_and_loss_adjoint_matches_dense(self):
        rng = np.random.default_rng(45)
        omega = np.geomspace(100.,1e6,17)
        ports = rng.normal(size=(17,4))
        basis = null_space((ports/omega[:,None]).T)
        base = np.diag(omega*.04)+rng.normal(size=(17,3)) @ rng.normal(size=(3,17))
        base = base @ base.T+np.eye(17)
        x = rng.normal(0,.1,basis.shape[1]+17+17*3)
        frequency = np.geomspace(11.,5900.,13)
        target = rng.normal(0,.003,(13,4,4))+1j*rng.normal(0,.003,(13,4,4))
        target = (target+target.transpose(0,2,1))/2
        arguments = (x,omega,ports,base,basis,frequency,target,3)
        original, actual = dense(*arguments), direct(*arguments)
        np.testing.assert_allclose(actual[0],original[0],rtol=1e-12)
        np.testing.assert_allclose(actual[1],original[1],rtol=1e-9,atol=1e-13)


if __name__ == '__main__':
    unittest.main()
