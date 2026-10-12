import unittest
import numpy as np
from scipy.linalg import null_space
from concert_grand_variable_projection import VariablePressureProblem
from concert_grand_joint_pressure_fit import responses,readout_transfer,objective_gradient


class VariableProjectionTests(unittest.TestCase):
    def problem(self):
        rng = np.random.default_rng(49)
        omega = np.array([130.,430.,810.,1450.])
        ports = rng.normal(size=(4,2))
        base = np.diag([5.,10.,20.,35.])+np.ones((4,4))*2
        basis = null_space((ports/omega[:,None]).T)
        frequency = np.geomspace(9.,700.,31)
        delays = np.array([[.001,.002],[.0015,.003]])
        readout = rng.normal(0,.1,(2,2,4))
        _,_,y,p = responses(np.diag(omega**2),base,omega,ports,frequency,readout_transfer(frequency,delays,readout))
        problem = VariablePressureProblem(omega,ports,base,basis,frequency,y,p,delays,rank=2,readout_ridge=1e-5)
        x = rng.normal(0,.06,basis.shape[1]+4+8)
        problem.prepare(np.zeros_like(x))
        return problem,x

    def test_envelope_gradient_against_refitted_finite_differences(self):
        problem,x = self.problem()
        _,gradient,parts,_ = problem.evaluate(x)
        self.assertLess(parts['inner_stationarity_relative'],1e-10)
        measured = []
        for i in range(len(x)):
            d = np.zeros_like(x)
            d[i] = 1e-6
            measured.append((problem.evaluate(x+d)[0]-problem.evaluate(x-d)[0])/2e-6)
        np.testing.assert_allclose(gradient,measured,rtol=2e-5,atol=2e-8)

    def test_cached_lu_adjoint_matches_fixed_readout_gradient(self):
        problem,x = self.problem()
        value,gradient,parts,readout = problem.evaluate(x)
        expected,g,_ = objective_gradient(x,problem.omega,problem.ports,problem.base_loss,problem.basis,
            problem.frequency,problem.target_y,problem.target_p,readout_transfer(problem.frequency,problem.delays,readout),2)
        np.testing.assert_allclose(value,expected+parts['readout_penalty'],rtol=1e-11,atol=1e-13)
        np.testing.assert_allclose(gradient,g,rtol=1e-9,atol=1e-12)


if __name__=='__main__':
    unittest.main()
