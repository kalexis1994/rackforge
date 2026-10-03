"""Independent piston, retarded-time and symmetry controls of SI pressure."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from numpy.polynomial.legendre import leggauss
from concert_grand_si_observer import Surface,RayleighObserver


class ObserverTests(unittest.TestCase):
    def test_uniform_disk_against_analytic_baffled_piston(self):
        radius=.23;height=.66;g,w=leggauss(96);r=(g+1)*radius/2
        surface=Surface(np.c_[r,np.zeros(len(r)),np.zeros(len(r))],w*radius/2*2*np.pi*r,np.ones((len(r),1)))
        observer=RayleighObserver(surface,[(0.,0.,height)])
        frequencies=np.array([0.,80.,261.,800.,4000.]);transfer=observer.acceleration_transfer(frequencies)[:,0,0]
        edge=np.sqrt(height*height+radius*radius);k=2*np.pi*frequencies[1:]/observer.speed
        exact=np.r_[observer.density*(edge-height),observer.density*(np.exp(-1j*k*height)-np.exp(-1j*k*edge))/(1j*k)]
        np.testing.assert_allclose(transfer,exact,rtol=1e-12,atol=1e-14)
        self.assertEqual(observer.displacement_transfer([0.])[0,0,0],0.)

    def test_delay_is_causal_with_correct_gain_and_moment(self):
        rate=192000.;height=10.25*343/rate
        observer=RayleighObserver(Surface([[0,0,0]],[.2],[[1.]]),[(0,0,height)])
        impulse=observer.kernel(rate)[:,0,0];gain=1.2*.2/(2*np.pi*height)
        self.assertTrue((impulse[:11]==0).all())
        self.assertAlmostEqual(impulse.sum()/gain,1.,places=14)
        self.assertAlmostEqual(np.dot(np.arange(len(impulse)),impulse)/gain,10.25,places=13)
        frequencies=np.linspace(80,800,31);exact=observer.acceleration_transfer(frequencies)
        error=np.linalg.norm(observer.sampled_transfer(frequencies,rate)-exact)/np.linalg.norm(exact)
        self.assertLess(error,.001)
        source=np.zeros((20,1));source[0]=1.
        response=observer.pressure(source,rate)[:,0]
        np.testing.assert_allclose(response[:len(impulse)],impulse,atol=1e-12)
        self.assertTrue((response[:11]==0).all())

    def test_retarded_field_against_analytic_time_functions(self):
        surface=Surface([[.05,.03,0],[.18,.08,0],[.25,.16,0]],[.04,.05,.03],[[1.,.5],[-.4,.8],[.2,-.3]])
        observer=RayleighObserver(surface,[(.1,.07,.4),(.3,.15,.8)])
        def source(t):
            u=np.maximum(t,0.)
            return np.stack([u*u*np.exp(-600*u)*np.cos(2*np.pi*261*u),u*u*np.exp(-850*u)*np.sin(2*np.pi*523*u)],axis=-1)*(t>=0)[...,None]
        errors=[]
        for rate in (48000,96000,192000):
            t=np.arange(round(.012*rate))/rate;acceleration=source(t)
            actual=observer.pressure(acceleration,rate)[:len(t)]
            exact=np.stack([np.einsum('tnm,nm->t',source(t[:,None]-observer.delay[c]),observer.weight[c]) for c in range(2)],axis=1)
            error=np.linalg.norm(actual-exact)/np.linalg.norm(exact);errors.append(error)
        self.assertLess(errors[-1],.001)
        # Fractional distances change with the rate, so the interpolation
        # error constant need not remain identical at each doubling.
        self.assertTrue(all(errors[i+1]<errors[i] for i in (0,1)),errors)

    def test_signed_modes_and_mirrored_capsules_preserve_superposition(self):
        surface=Surface([[.2,.4,0],[.8,.4,0]],[.1,.1],[[1.,1.],[1.,-1.]])
        observer=RayleighObserver(surface,[(.3,.4,.66),(.7,.4,.66)])
        transfer=observer.acceleration_transfer([80.,261.,800.])
        np.testing.assert_allclose(transfer[:,0,0],transfer[:,1,0],atol=1e-14)
        np.testing.assert_allclose(transfer[:,0,1],-transfer[:,1,1],atol=1e-14)
        rng=np.random.default_rng(6);a=rng.normal(size=(240,2));b=rng.normal(size=(240,2))
        np.testing.assert_allclose(observer.pressure(a+b,96000),observer.pressure(a,96000)+observer.pressure(b,96000),atol=1e-14)
        self.assertEqual(np.max(abs(observer.pressure(np.zeros((240,2)),96000))),0.)

    def test_invalid_geometry_and_nonfinite_source_are_rejected(self):
        surface=Surface([[0,0,0]],[.1],[[1.]])
        for mic in ([(0,0,0)],[(0,0,-1)],[(0,0,float('nan'))]):
            with self.assertRaises(ValueError):RayleighObserver(surface,mic)
        observer=RayleighObserver(surface,[(0,0,.66)])
        with self.assertRaises(ValueError):observer.pressure([[float('nan')]],44100)

    def test_radiated_power_against_point_source_and_positive_reciprocal_load(self):
        point=RayleighObserver(Surface([[0,0,0]],[.2],[[1.]]),[(0,0,.66)])
        frequency=np.array([80.,261.,800.])
        expected=point.density*(2*np.pi*frequency)**2*.2**2/(2*np.pi*point.speed)
        np.testing.assert_allclose(point.radiation_resistance(frequency)[:,0,0],expected,rtol=1e-13)
        surface=Surface([[.05,.03,0],[.18,.08,0],[.25,.16,0]],[.04,.05,.03],[[1.,.5],[-.4,.8],[.2,-.3]])
        observer=RayleighObserver(surface,[(.1,.07,.66)])
        for resistance in observer.radiation_resistance(frequency):
            np.testing.assert_allclose(resistance,resistance.T,atol=1e-14)
            self.assertGreaterEqual(np.linalg.eigvalsh(resistance).min(),-1e-13)


if __name__=='__main__':unittest.main()
