"""Nested trapezoid identity must match independently evaluated Simpson."""
import sys
import unittest
from pathlib import Path
import numpy as np
from scipy.integrate import simpson
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_band_quadrature import refined_band_quadrature


class BandQuadratureTests(unittest.TestCase):
    def source(self,fun):
        steps = [125.,62.5,31.25]
        rows = {}
        for step in steps:
            f = np.unique(np.r_[np.arange(0.,24001.,step),60.,480.,1980.])
            y = fun(f)
            values = {f'{lo:g}-{hi:g}':float(np.trapezoid(y[(f>=lo)&(f<=hi)],f[(f>=lo)&(f<=hi)]))
                for lo,hi in [(0,6000),(6000,12000),(12000,24000),(0,24000)]}
            rows[str(step)] = {'pressure':values}
        return f,steps,rows

    def test_exact_weight_identity_with_arbitrary_positive_data(self):
        fun = lambda x: 1+np.sin(x/57)**2+np.cos(x/93)**2
        f,steps,rows = self.source(fun)
        result = refined_band_quadrature(f,steps,rows)
        for step in steps[1:]:
            for lo,hi in [(6000,12000),(12000,24000)]:
                x = np.arange(lo,hi+step/2,step)
                expected = simpson(fun(x),x=x)
                self.assertAlmostEqual(result['integrals_by_step_hz'][str(step)]['pressure'][f'{lo:g}-{hi:g}'],expected,places=9)

    def test_cubic_high_band_integral_and_low_band_untouched(self):
        fun = lambda x: 1+(x/24000)**3
        f,steps,rows = self.source(fun)
        result = refined_band_quadrature(f,steps,rows)
        values = result['integrals_by_step_hz']['31.25']['pressure']
        self.assertEqual(values['0-6000'],rows['31.25']['pressure']['0-6000'])
        for lo,hi in [(6000,12000),(12000,24000)]:
            exact = hi-lo+(hi**4-lo**4)/(4*24000**3)
            self.assertAlmostEqual(values[f'{lo:g}-{hi:g}'],exact,places=9)

    def test_rejects_nonuniform_high_grid_negative_and_unnested_steps(self):
        f,steps,rows = self.source(lambda x:np.ones_like(x))
        with self.assertRaises(ValueError):refined_band_quadrature(np.sort(np.r_[f,7001.]),steps,rows)
        with self.assertRaises(ValueError):refined_band_quadrature(f,[100.,50.,31.25],rows)
        rows['125.0']['pressure']['6000-12000'] = 100000.
        with self.assertRaises(ValueError):refined_band_quadrature(f,steps,rows)


if __name__=='__main__':
    unittest.main()
