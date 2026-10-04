"""Independent angular, spatial, static, Laplace and causal-memory checks."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
import math
from scipy.integrate import quad
from numpy.polynomial.legendre import leggauss
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_geometric_memory import GeometricMemory,ring_overlap


class GeometricMemoryTests(unittest.TestCase):
    def test_ring_against_independent_adaptive_angular_integral(self):
        hx=.08;hy=.05
        for dx,dy,r in [(0.,0.,0.),(0.,0.,.03),(.08,0.,.07),(.16,.10,.18),(.24,.15,.27),(.24,.15,.4)]:
            def integrand(theta):return max(hx-abs(r*np.cos(theta)-dx),0)*max(hy-abs(r*np.sin(theta)-dy),0)
            # QUADPACK's error estimate can miss a narrow overlap cusp.
            # Supply scalar geometric crossings; integrate the original
            # absolute-value function, not the implementation's primitive.
            cuts={0.,2*math.pi}
            if r:
                for offset in (-hx,0.,hx):
                    value=(dx+offset)/r
                    if abs(value)<=1:
                        a=math.acos(value);cuts.update((a,2*math.pi-a))
                for offset in (-hy,0.,hy):
                    value=(dy+offset)/r
                    if abs(value)<=1:
                        a=math.asin(value);cuts.update((a%(2*math.pi),(math.pi-a)%(2*math.pi)))
            cuts=sorted(cuts)
            reference=sum(quad(integrand,a,b,epsabs=1e-16,epsrel=1e-12)[0] for a,b in zip(cuts[:-1],cuts[1:]))
            self.assertAlmostEqual(float(ring_overlap(hx,hy,dx,dy,r)),reference,delta=2e-13)

    def test_modal_correlations_against_dense_pairs_and_fft_projection(self):
        rng=np.random.default_rng(375);surface=RectangularLoad(.4,.3,rng.normal(size=(4,3,3)));memory=GeometricMemory(surface)
        kernel=rng.normal(size=(4,3));packed=kernel.ravel()@memory.correlation
        actual=memory.unpack(packed[None,:])[0]
        dense=np.zeros((3,3))
        for x in range(4):
            for y in range(3):
                for a in range(4):
                    for b in range(3):dense+=kernel[abs(x-a),abs(y-b)]*np.outer(surface.shapes[x,y],surface.shapes[a,b])
        np.testing.assert_allclose(actual,dense,atol=2e-13)
        np.testing.assert_allclose(actual,surface.project_kernel(kernel).real,atol=2e-13)

    def test_uniform_panel_partition_and_high_frequency_limit(self):
        surface=RectangularLoad(.4,.3,np.ones((5,4,1)));memory=GeometricMemory(surface)
        times=np.r_[0,np.linspace(.00001,memory.max_delay,50),memory.max_delay*1.01]
        expected=surface.density*surface.speed/(2*np.pi)*ring_overlap(.4,.3,0.,0.,0.)
        self.assertAlmostEqual(memory.kernel([0])[0,0,0],expected,delta=1e-12)
        exact=np.array([surface.density*surface.speed/(2*np.pi)*ring_overlap(.4,.3,0.,0.,t*surface.speed) for t in times])
        np.testing.assert_allclose(memory.kernel(times)[:,0,0],exact,atol=3e-12)
        self.assertEqual(memory.kernel([memory.max_delay*1.01])[0,0,0],0.)

    def test_time_integral_static_and_laplace_against_duffy_kernel(self):
        rng=np.random.default_rng(573);surface=RectangularLoad(.4,.3,rng.normal(size=(4,3,2)));memory=GeometricMemory(surface)
        dt=memory.max_delay/512;nodes,weights=leggauss(8)
        t=(np.arange(512)[:,None]+(nodes+1)/2)*dt
        kernel=memory.kernel(t.ravel()).reshape(512,8,2,2)
        mass=np.einsum('q,tqmn->mn',weights*dt/2,kernel)
        reference=surface.added_mass_static(24)
        self.assertLess(np.linalg.norm(mass-reference)/np.linalg.norm(reference),2e-6)
        for s in [2j*np.pi*800,150+2j*np.pi*1200]:
            packed=kernel[:,:,memory.upper[0],memory.upper[1]]
            transform=np.einsum('tq,q,tqm->m',np.exp(-s*t),weights*dt/2,packed)
            actual=s*memory.unpack(transform[None,:])[0]
            reference=surface.impedance(s,24)
            self.assertLess(np.linalg.norm(actual-reference)/np.linalg.norm(reference),3e-6)

    def test_linear_drive_independent_time_integration_and_support(self):
        surface=RectangularLoad(.2,.15,np.ones((3,2,1)));memory=GeometricMemory(surface)
        dt=1/128000;velocity=np.array([[0.],[.01],[.03],[.02]])
        coast=int(np.ceil(memory.max_delay/dt))+20
        force=memory.linear_drive_force(velocity,dt,coast,8)[:,0]
        slope=np.diff(velocity[:,0])/dt;stop=(len(velocity)-1)*dt
        def scalar_kernel(t):return surface.density*surface.speed/(2*np.pi)*float(ring_overlap(.2,.15,0.,0.,t*surface.speed))
        for n in [1,3,10,40]:
            t=n*dt;expected=0.
            for i,a in enumerate(slope):
                left=max(0,t-(i+1)*dt);right=max(0,t-i*dt)
                expected+=a*quad(scalar_kernel,left,right,epsabs=1e-13)[0]
            if t>stop:expected-=scalar_kernel(t-stop)*velocity[-1,0]
            self.assertAlmostEqual(force[n],expected,delta=2e-10)
        self.assertLess(abs(force[-10:]).max(),1e-13)


if __name__=='__main__':unittest.main()
