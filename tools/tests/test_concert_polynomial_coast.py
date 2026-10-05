"""Analytic segment integral vs Gauss and shared independent coast checks."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
sys.path.insert(0,str(Path(__file__).resolve().parent))
import numpy as np
import test_concert_exponential_coast as reference
from concert_grand_exponential_coast import ExponentialCoast
from concert_grand_polynomial_coast import PolynomialCoast


class PolynomialCoastTests(reference.ExponentialCoastTests):
    def fixture(self):
        old,_,state=super().fixture()
        return old,PolynomialCoast(old.system,old.air),state

    def test_polynomial_integral_matches_two_point_gauss_at_various_amplitudes(self):
        old,pilot,_=self.fixture();gauss=ExponentialCoast(old.system,old.air)
        rng=np.random.default_rng(58)
        for scale in [1e-8,1e-7,1e-5]:
            a=rng.normal(size=old.n-1)*scale;b=rng.normal(size=old.n-1)*scale
            expected=gauss.nonlinear_avf(a,b);actual=pilot.nonlinear_avf(a,b)
            self.assertLess(np.linalg.norm(actual-expected)/np.linalg.norm(expected),2e-13)


if __name__=='__main__':unittest.main()
