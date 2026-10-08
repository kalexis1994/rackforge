"""Analytic blocking must reproduce the existing Hermite and quadrature oracles."""
import sys
import unittest
from pathlib import Path
import numpy as np
from numpy.polynomial.legendre import leggauss
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_uniform_spectra import uniform_spectra,uniform_window_spectra,_moments
from concert_grand_cell_observer import acceleration_spectrum
from concert_grand_window_spectra import velocity_spectrum,hann_acceleration_spectrum


class UniformSpectraTests(unittest.TestCase):
    def test_moments_against_independent_quadrature(self):
        z = -2j*np.pi*np.array([0.,.00001,.05,.2,1.,3.,-1.])
        g,w = leggauss(180)
        u = (g+1)/2
        expected = np.column_stack([np.exp(z[:,None]*u)@(w/2*u**p) for p in range(3)])
        np.testing.assert_allclose(_moments(z),expected,rtol=2e-12,atol=2e-14)

    def test_random_c1_source_oracles_and_block_independence(self):
        rng = np.random.default_rng(23)
        t = .003+np.arange(129)/256000
        B = rng.normal(size=(len(t),7))*1e-7
        V = rng.normal(size=B.shape)*.2
        f = np.array([0.,60.,6000.,24000.,-12000.,24000.])
        actual = uniform_window_spectra(t,B,V,f,1)
        batched = uniform_window_spectra(t,B,V,f,5)
        for label,taper in [('rectangular',False),('hann',True)]:
            np.testing.assert_allclose(actual[label][0],velocity_spectrum(t,B,V,f,taper,16),rtol=2e-10,atol=1e-16)
            oracle = (hann_acceleration_spectrum if taper else acceleration_spectrum)(t,B,V,f)
            np.testing.assert_allclose(actual[label][1],oracle,rtol=2e-10,atol=1e-12)
            for a,b in zip(actual[label],batched[label]):
                np.testing.assert_allclose(a,b,rtol=1e-12,atol=1e-13)

    def test_polynomial_live_endpoints_and_absolute_phase(self):
        t = .017+np.arange(65)/128000
        x = t-.017
        B = (2e-5+.3*x+200*x*x+2e5*x**3)[:,None]
        V = (.3+400*x+6e5*x*x)[:,None]
        f = np.array([0.,6000.,24000.])
        base = uniform_window_spectra(t,B,V,f)
        moved = uniform_window_spectra(t+.013,B,V,f)
        for label in base:
            for a,b in zip(base[label],moved[label]):
                np.testing.assert_allclose(b,a*np.exp(-2j*np.pi*f[:,None]*.013),rtol=2e-9,atol=1e-12)
        self.assertAlmostEqual(base['rectangular'][1][0,0].real,(V[-1]-V[0])[0],places=12)

    def test_constant_velocity_does_not_create_acceleration(self):
        t = np.arange(9217)/3072000
        B = .25*t[:,None]
        V = np.full(B.shape,.25)
        result = uniform_window_spectra(t,B,V,[0.,6000.,24000.])
        np.testing.assert_allclose(result['hann'][1],0.,atol=1e-12)
        self.assertAlmostEqual(result['hann'][0][0,0].real,.25*.003/2,places=14)

    def test_rejects_nonuniform_invalid_sources_and_blocks(self):
        t = np.array([0.,.001,.003])
        B = np.zeros((3,1));V = B.copy()
        with self.assertRaises(ValueError):uniform_spectra(t,B,V,[0.])
        for block in [0,1.5]:
            with self.assertRaises(ValueError):uniform_spectra([0.,1.,2.],B,V,[0.],block)
        with self.assertRaises(ValueError):uniform_spectra([0.,1.,2.],B,V,[float('nan')])


if __name__=='__main__':
    unittest.main()
