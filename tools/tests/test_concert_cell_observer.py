"""Independent SI point/piston, near-field, source-window and refinement checks."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from numpy.polynomial.legendre import leggauss
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_cell_observer import CellObserver,acceleration_spectrum
from concert_grand_si_observer import Surface,RayleighObserver


class CellObserverTests(unittest.TestCase):
    def test_uniform_rectangle_against_independent_surface_quadrature(self):
        surface=RectangularLoad(.31,.21,np.ones((3,2,1)))
        mic=[(.13,.09,.6),(.2,.14,1.2)];f=np.array([0.,261.,1200.,6000.])
        cell=CellObserver(surface,mic,8)
        g,w=leggauss(64);x,y=np.meshgrid((g+1)*surface.length/2,(g+1)*surface.width/2,indexing='ij')
        point=RayleighObserver(Surface(np.c_[x.ravel(),y.ravel(),np.zeros(x.size)],np.outer(w,w).ravel()*surface.length*surface.width/4,np.ones((x.size,1))),mic)
        np.testing.assert_allclose(cell.acceleration_transfer(f),point.acceleration_transfer(f),rtol=2e-9,atol=1e-12)

    def test_far_point_limit_units_distance_and_phase(self):
        surface=RectangularLoad(.001,.001,np.array([[[2.]]]))
        mic=[(.0005,.0005,100.),(.0005,.0005,200.)];f=np.array([0.,261.,1200.])
        observer=CellObserver(surface,mic,4)
        exact=np.array([1.2/(2*np.pi)*2e-6/np.array([100.,200.])*np.exp(-2j*np.pi*hz*np.array([100.,200.])/343) for hz in f])
        np.testing.assert_allclose(observer.acceleration_transfer(f)[:,:,0],exact,rtol=1e-7,atol=1e-20)

    def test_signed_shapes_mirror_and_nested_basis(self):
        shape=np.array([[[1.,1.,.3]],[[1.,-1.,-.2]]]);surface=RectangularLoad(.4,.2,shape)
        f=[0.,261.,1200.,6000.];mic=[(.1,.1,.66),(.3,.1,.66)]
        H=CellObserver(surface,mic,4).acceleration_transfer(f)
        np.testing.assert_allclose(H[:,0,0],H[:,1,0],atol=1e-14)
        np.testing.assert_allclose(H[:,0,1],-H[:,1,1],atol=1e-14)
        small=CellObserver(RectangularLoad(.4,.2,shape[:,:,:2]),mic,4).acceleration_transfer(f)
        np.testing.assert_allclose(H[:,:,:2],small,atol=1e-14)

    def test_cubic_acceleration_integral_and_no_endpoint_impulses(self):
        t=np.linspace(0,.008,17);B=(.01+2*t+3*t*t+200*t**3)[:,None];V=(2+6*t+600*t*t)[:,None]
        f=np.array([0.,261.,1200.,6000.]);actual=acceleration_spectrum(t,B,V,f)
        g,w=leggauss(256);u=(g+1)*.008/2
        exact=np.array([np.sum(w*.008/2*(6+1200*u)*np.exp(-2j*np.pi*hz*u)) for hz in f])[:,None]
        np.testing.assert_allclose(actual,exact,rtol=2e-9,atol=1e-12)
        self.assertAlmostEqual(actual[0,0].real,V[-1,0]-V[0,0],places=13)
        constant=acceleration_spectrum(t,(.01+2*t)[:,None],np.full((len(t),1),2.),f)
        np.testing.assert_allclose(constant,0.,atol=2e-13)

    def test_source_join_preserves_phase_without_restart(self):
        t=np.linspace(0,.008,129);B=(np.sin(2*np.pi*261*t)/1000)[:,None];V=(2*np.pi*261*np.cos(2*np.pi*261*t)/1000)[:,None]
        f=np.array([0.,261.,1200.,6000.]);full=acceleration_spectrum(t,B,V,f)
        left=acceleration_spectrum(t[:65],B[:65],V[:65],f)
        right=acceleration_spectrum(t[64:]-t[64],B[64:],V[64:],f)
        np.testing.assert_allclose(full,left+np.exp(-2j*np.pi*f*t[64])[:,None]*right,rtol=1e-11,atol=1e-12)

    def test_hermite_source_refines_against_analytic_sine(self):
        f=np.array([261.,1200.,6000.]);g,w=leggauss(256);u=(g+1)*.008/2;omega=2*np.pi*523.
        exact=np.array([np.sum(w*.008/2*(-omega**2*np.sin(omega*u))*np.exp(-2j*np.pi*hz*u)) for hz in f])[:,None]
        errors=[]
        for rate in [32000,64000,128000]:
            t=np.arange(round(.008*rate)+1)/rate
            actual=acceleration_spectrum(t,np.sin(omega*t)[:,None],(omega*np.cos(omega*t))[:,None],f)
            errors.append(np.linalg.norm(actual-exact)/np.linalg.norm(exact))
        self.assertLess(errors[-1],1e-5)
        self.assertTrue(all(errors[i+1]<errors[i]/8 for i in [0,1]),errors)

    def test_invalid_inputs_are_rejected(self):
        surface=RectangularLoad(.3,.2,np.ones((2,2,1)))
        for mic in [[(0,0,0)],[(0,0,-1)],[(0,0,float('nan'))]]:
            with self.assertRaises(ValueError):CellObserver(surface,mic)
        with self.assertRaises(ValueError):CellObserver(surface,[(0,0,.66)],1)
        with self.assertRaises(ValueError):acceleration_spectrum([0,0],[[0],[0]],[[0],[0]],[261])


if __name__=='__main__':unittest.main()
