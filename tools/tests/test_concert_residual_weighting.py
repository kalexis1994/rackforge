import unittest
import numpy as np
from test_concert_variable_projection import VariableProjectionTests
from concert_grand_residual_weighting import ResidualPressureProblem,residual_importance


class ResidualWeightingTests(unittest.TestCase):
    def test_weighted_envelope_gradient_and_fixed_scale(self):
        original,x = VariableProjectionTests().problem()
        f = original.frequency
        problem = ResidualPressureProblem(original.omega,original.ports,original.base_loss,original.basis,
            f,original.target_y,original.target_p,original.delays,rank=2,readout_ridge=1e-5,
            fixed_scale=original.scale,boost_y=np.linspace(1.,5.,len(f)),boost_p=np.linspace(6.,1.,len(f)))
        scale = problem.scale.copy()
        problem.prepare(x)
        _,gradient,_,_ = problem.evaluate(x)
        measured = []
        for i in range(len(x)):
            d = np.zeros_like(x)
            d[i] = 1e-6
            measured.append((problem.evaluate(x+d)[0]-problem.evaluate(x-d)[0])/2e-6)
        np.testing.assert_allclose(gradient,measured,rtol=3e-5,atol=3e-8)
        np.testing.assert_array_equal(problem.scale,scale)

    def test_training_peaks_enrich_and_importance_is_bounded(self):
        f = np.arange(20.,220.,2.)
        y,p = np.full(len(f),.03),np.full(len(f),.02)
        y[25],p[72] = .2,.16
        selected,by,bp = residual_importance(f,y,p,np.array([0,40,99]),bins=4)
        self.assertTrue({24,25,26,71,72,73,40}.issubset(set(selected)))
        self.assertEqual(by[25],8.)
        self.assertEqual(bp[72],8.)
        self.assertEqual(by[0],1.)


if __name__=='__main__':
    unittest.main()
