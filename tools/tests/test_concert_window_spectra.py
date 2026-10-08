"""Independent quadrature and finite-window controls, including live endpoints."""
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
import numpy as np
from numpy.polynomial.legendre import leggauss
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from concert_grand_window_spectra import (velocity_spectrum,
    hann_acceleration_spectrum, band_norms, projected_pressure)
from concert_grand_cell_observer import CellObserver


class WindowSpectraTests(unittest.TestCase):
    def test_live_cubic_against_independent_quadrature_and_phase(self):
        t = np.array([.003, .00313, .0035])
        x = t-.003
        # Deliberately nonzero position, velocity, acceleration at both ends.
        B = (2 + 3*x + 4e4*x*x + 5e7*x**3)[:, None]
        V = (3 + 8e4*x + 15e7*x*x)[:, None]
        f = np.array([0., 2000., 6000., 24000., -8000.])
        g, w = leggauss(220)
        nodes = .003 + (g+1)*.0005/2
        z = nodes-.003
        window = .5-.5*np.cos(2*np.pi*z/.0005)
        phase = np.exp(-2j*np.pi*f[:, None]*nodes)
        reference_v = phase @ (w*.0005/2*window*(3+8e4*z+15e7*z*z))
        reference_a = phase @ (w*.0005/2*window*(8e4+3e8*z))
        np.testing.assert_allclose(velocity_spectrum(t,B,V,f,True,64)[:,0],
                                   reference_v, rtol=1e-9, atol=1e-13)
        np.testing.assert_allclose(hann_acceleration_spectrum(t,B,V,f)[:,0],
                                   reference_a, rtol=1e-8, atol=1e-9)
        shifted = hann_acceleration_spectrum(t+.137,B,V,f)
        np.testing.assert_allclose(shifted[:,0], reference_a*np.exp(-2j*np.pi*f*.137),
                                   rtol=1e-8, atol=1e-9)

    def test_constant_velocity_edges_are_diagnostic_not_physics(self):
        t = np.linspace(0,.0005,129)
        V = np.ones((len(t),1))
        B = t[:,None].copy()
        f = np.array([0.,6000.,7000.,20000.])
        # Tapering the displacement would create spurious acceleration here.
        np.testing.assert_allclose(hann_acceleration_spectrum(t,B,V,f),0.,atol=1e-12)
        raw = velocity_spectrum(t,B,V,f)
        soft = velocity_spectrum(t,B,V,f,True)
        self.assertAlmostEqual(raw[0,0].real,.0005)
        self.assertAlmostEqual(soft[0,0].real,.00025)
        self.assertGreater(abs(raw[2,0]),abs(soft[2,0])*10)

    def test_projected_pressure_matches_full_transfer(self):
        rng = np.random.default_rng(7)
        surface = SimpleNamespace(nx=3,ny=2,hx=.1,hy=.13,modes=4,
            density=1.2,speed=343.,shapes=rng.normal(size=(3,2,4)))
        observer = CellObserver(surface,[[.1,.2,.4],[.25,.1,.35]],4)
        f = np.array([60.,6000.,24000.])
        A = rng.normal(size=(3,4))+1j*rng.normal(size=(3,4))
        expected = np.einsum('fmn,fn->fm',observer.acceleration_transfer(f),A)
        np.testing.assert_allclose(projected_pressure(observer,f,A),expected,rtol=1e-13,atol=1e-14)

    def test_band_integration_requires_boundaries(self):
        f = np.arange(0.,24001.,250.)
        result = band_norms(f,np.ones((len(f),2)),[(0,6000),(6000,24000)])
        self.assertEqual(result,{'0-6000':12000.,'6000-24000':36000.})
        with self.assertRaises(ValueError):band_norms(f,np.ones((len(f),2)),[(60,6000)])


if __name__ == '__main__':
    unittest.main()
