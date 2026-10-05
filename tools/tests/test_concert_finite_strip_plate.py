import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from scipy.linalg import cholesky
from numpy.polynomial.legendre import leggauss
import concert_grand_orthotropic_plate as plate
import concert_grand_finite_strip_plate as strip

class StripPlateTests(unittest.TestCase):
    def test_zero_footprint_exactly_preserves_delta_control(self):
        panel=plate.Panel();beams=plate.pilot_beams(panel)
        old=plate.assemble(panel,9,7,beams);new=strip.assemble(panel,9,7,beams,spread=0)
        for a,b in zip(old,new):np.testing.assert_array_equal(a,b)

    def test_strip_against_independent_displacement_squared_quadrature(self):
        rng=np.random.default_rng(17);q=rng.normal(size=31);center=.6;width=.02
        gram=strip.strip_gram(1.4,31,'clamped',center,width)
        x,w=leggauss(96);displacement=plate.basis(center+width*x/2,1.4,31,'clamped')[0]@q
        self.assertAlmostEqual(q@gram@q,float(w@(displacement**2)/2),places=12)
        self.assertGreaterEqual(q@gram@q,0)

    def test_finite_footprint_positive_matrices_and_reciprocity(self):
        model=strip.solve(plate.Panel(),12,10,plate.pilot_beams(plate.Panel()))
        cholesky(model['mass']);cholesky(model['stiffness'])
        Y=plate.mobility(model,np.array([70.,261.625565,600.,2200.,5000.]),[(.62,.5),(.85,.5)])
        np.testing.assert_allclose(Y,Y.transpose(0,2,1),rtol=0,atol=1e-15)
        self.assertGreaterEqual(np.linalg.eigvalsh((Y+Y.conj().transpose(0,2,1))/2).min(),-1e-15)

if __name__=='__main__':unittest.main()
