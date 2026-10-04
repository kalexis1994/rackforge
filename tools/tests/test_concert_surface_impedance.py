"""Analytical self term, independent far field and Galerkin consistency."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from concert_grand_surface_impedance import RectangularLoad,average_basis
from concert_grand_orthotropic_plate import basis
from numpy.polynomial.legendre import leggauss
from scipy.special import j1,struve


class SurfaceLoadTests(unittest.TestCase):
    def test_streamed_hemisphere_matches_dense_exact_cells(self):
        rng=np.random.default_rng(754)
        surface=RectangularLoad(.43,.31,rng.normal(size=(9,7,3)))
        for block in [1,17,400]:
            np.testing.assert_allclose(surface.angular_resistance_streamed(1800,8,16,block),surface.angular_resistance(1800,8,16),rtol=2e-13,atol=2e-12)

    def test_disk_complex_load_against_bessel_and_struve_solution(self):
        # A circular aperture supplies an independent finite-frequency
        # reactance check. Partial square cells approximate its boundary.
        radius=.12;n=128;h=2*radius/n
        x,y=np.meshgrid((np.arange(n)+.5)*h-radius,(np.arange(n)+.5)*h-radius,indexing='ij')
        aperture=np.zeros_like(x)
        for dx in (np.arange(8)+.5)/8-.5:
            for dy in (np.arange(8)+.5)/8-.5:
                aperture+=((x+dx*h)**2+(y+dy*h)**2<=radius**2)/64
        load=RectangularLoad(2*radius,2*radius,aperture[:,:,None])
        for hz in (80.,400.,800.):
            ka=2*np.pi*hz*radius/load.speed
            exact=load.density*load.speed*np.pi*radius**2*(1-j1(2*ka)/ka+1j*struve(1,2*ka)/ka)
            actual=load.impedance(2j*np.pi*hz,8)[0,0]
            self.assertLess(abs(actual.real-exact.real)/abs(exact.real),.002)
            self.assertLess(abs(actual.imag-exact.imag)/abs(exact.imag),.002)

    def test_square_self_mass_against_closed_integral(self):
        load=RectangularLoad(.3,.3,np.ones((1,1,1)))
        exact=1.2/(2*np.pi)*4*.3**3*(np.arcsinh(1)+(1-np.sqrt(2))/3)
        self.assertAlmostEqual(load.added_mass_static(32)[0,0]/exact,1.,places=13)
        self.assertEqual(load.impedance(0.)[0,0],0.)
        self.assertGreater(load.impedance(2j*np.pi*80,16)[0,0].imag,0.)

    def test_uniform_panel_survives_cell_partition(self):
        # Identical continuous field under subdivision; off-diagonal and
        # touching-cell interactions must restore the large cell's result.
        whole=RectangularLoad(.4,.3,np.ones((1,1,1)))
        divided=RectangularLoad(.4,.3,np.ones((5,4,1)))
        for s in (0.,2j*np.pi*430,250+2j*np.pi*600):
            a=whole.project_kernel(whole.overlap_kernel(s,32))
            b=divided.project_kernel(divided.overlap_kernel(s,16))
            np.testing.assert_allclose(a,b,rtol=3e-7,atol=1e-12)

    def test_resistance_against_independent_hemisphere(self):
        x,y=np.meshgrid(np.arange(7),np.arange(5),indexing='ij')
        shapes=np.stack([np.ones_like(x),np.cos(x*.6),np.sin(y*.7)+.2*x],axis=-1)
        load=RectangularLoad(.6,.4,shapes)
        for hz in (80.,261.6256,800.):
            z=load.impedance(2j*np.pi*hz,12)
            r=load.angular_resistance(hz,32,64)
            np.testing.assert_allclose(z.real,r,rtol=2e-10,atol=1e-11)
            np.testing.assert_allclose(z,z.T,rtol=1e-13,atol=1e-12)
            self.assertGreaterEqual(np.linalg.eigvalsh(z.real).min(),-1e-12)
            self.assertGreater(np.linalg.eigvalsh(z.imag).min(),0.)

    def test_positive_real_complex_half_plane_and_linear_projection(self):
        rng=np.random.default_rng(6);shapes=rng.normal(size=(8,6,3))
        load=RectangularLoad(.45,.35,shapes)
        transform=np.array([[1.,.2],[-.4,1.],[.3,-.6]])
        changed=RectangularLoad(.45,.35,shapes@transform)
        for s in (10+80j,300+1600j,1000+5000j):
            z=load.impedance(s,12)
            self.assertGreater(np.linalg.eigvalsh((z+z.conj().T)/2).min(),0.)
            np.testing.assert_allclose(changed.impedance(s,12),transform.T@z@transform,rtol=1e-12,atol=1e-13)

    def test_cell_averages_integrate_the_actual_ritz_basis(self):
        g,w=leggauss(96);length=1.4;edges=np.array([0.,.17,.49,1.4])
        for boundary in ('clamped','simply-supported'):
            actual=average_basis(length,52,edges,boundary)
            exact=np.array([w@basis((g+1)*(b-a)/2+a,length,52,boundary)[0]/2 for a,b in zip(edges[:-1],edges[1:])])
            np.testing.assert_allclose(actual,exact,atol=1e-14)

    def test_invalid_inputs(self):
        for length,shape in ((0.,np.ones((1,1,1))),(.3,np.ones((0,1,1))),(.3,np.array([[[np.nan]]]))):
            with self.assertRaises(ValueError):RectangularLoad(length,.4,shape)
        with self.assertRaises(ValueError):RectangularLoad(.3,.4,np.ones((1,1,1))).impedance(np.nan)


if __name__=='__main__':unittest.main()
