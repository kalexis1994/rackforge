"""Independent passivity, arbitrary-step storage, ODE and surface controls."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from scipy.integrate import solve_ivp
from concert_grand_rational_air import RationalAir,surface_factor
from concert_grand_surface_impedance import RectangularLoad


class RationalAirTests(unittest.TestCase):
    def fixture(self):
        rng=np.random.default_rng(729)
        h=rng.normal(size=(7,3,2));h[-1]=-h[:-1].sum(axis=0)
        return RationalAir(np.concatenate((h,-h[::-1]),axis=1),16000)

    def test_factor_identity_and_positive_real_off_axis(self):
        air=self.fixture()
        for theta in np.linspace(.013,3.12,31):
            s=1j*air.a*np.tan(theta/2);z=air.impedance(s)
            h=np.einsum('k,kom->om',np.exp(-1j*theta*np.arange(len(air.H))),air.H)
            np.testing.assert_allclose((z+z.conj().T)/2,h.conj().T@h/2,atol=3e-13)
        for s in [1+1j,100+1000j,100000+300000j]:
            z=air.impedance(s);self.assertGreaterEqual(np.linalg.eigvalsh((z+z.conj().T)/2).min(),-1e-13)

    def test_energy_work_identity_arbitrary_steps(self):
        air=self.fixture();rng=np.random.default_rng(146)
        for dt in [1/16000,1/48000,1/192000,.003]:
            state=rng.normal(size=air.zero_state().shape);v=rng.normal(size=2)
            updated,force,loss=air.midpoint(state,v,dt)
            delta=air.energy(updated)-air.energy(state)
            self.assertGreaterEqual(loss,0.)
            self.assertAlmostEqual(delta,dt*(v@force-loss),delta=3e-13*max(air.energy(state),dt*abs(v@force)))

    def test_independent_ode_and_sampled_transfer(self):
        air=self.fixture();rng=np.random.default_rng(241);state=rng.normal(size=air.zero_state().shape);v=np.array([.4,-.3])
        def ode(t,y):
            r=y.reshape(state.shape);u=v.copy();derivative=[]
            for row in r:
                derivative.append(air.a*(u-row));u=2*row-u
            return np.array(derivative).ravel()
        duration=1e-5;reference=solve_ivp(ode,(0,duration),state.ravel(),method='DOP853',rtol=1e-12,atol=1e-13).y[:,-1].reshape(state.shape)
        errors=[]
        for steps in [8,16]:
            actual=state.copy()
            for _ in range(steps):actual,_,_=air.midpoint(actual,v,duration/steps)
            errors.append(np.linalg.norm(actual-reference))
        self.assertLess(errors[1],.26*errors[0])
        drive=np.vstack((np.zeros(2),rng.normal(size=(30,2))));dt=1/48000
        force=air.sampled_force(drive,dt);state=air.zero_state()
        for k in range(1,len(drive)):
            state,f,_=air.midpoint(state,(drive[k]+drive[k-1])/2,dt)
            np.testing.assert_allclose(f,(force[k]+force[k-1])/2,atol=2e-12)

    def test_surface_far_field_and_mass_congruence(self):
        surface=RectangularLoad(.4,.3,np.ones((8,6,1)))
        air=RationalAir(surface_factor(surface,32000,12,24),32000)
        for f in [80.,300.,800.]:
            reference=surface.angular_resistance(f,12,24)
            self.assertLess(np.linalg.norm(air.impedance(2j*np.pi*f).real-reference)/np.linalg.norm(reference),.01)
        calibrated,transform=air.mass_congruent(air.static_mass()*1.1)
        np.testing.assert_allclose(calibrated.static_mass(),air.static_mass()*1.1,rtol=1e-12)
        np.testing.assert_allclose(calibrated.impedance(1+1000j),transform.T@air.impedance(1+1000j)@transform,atol=1e-12)
        with self.assertRaises(ValueError):air.mass_congruent(-np.eye(1))
        with self.assertRaises(ValueError):RationalAir(np.ones((3,1,1)),1000)

    def test_positive_tail_without_wave_transfer_change(self):
        air=self.fixture();tail=np.array([[.01,.002],[.002,.015]])
        loaded=air.with_static_tail(air.static_mass()+tail)
        np.testing.assert_allclose(loaded.static_mass(),air.static_mass()+tail,atol=1e-15)
        s=2j*np.pi*350
        np.testing.assert_allclose(loaded.impedance(s),air.impedance(s)+s*tail,atol=1e-13)
        v=np.array([.2,-.3]);state=air.zero_state()
        self.assertAlmostEqual(loaded.energy(state,v),v@tail@v/2)
        with self.assertRaises(ValueError):loaded.energy(state)
        with self.assertRaises(ValueError):air.with_static_tail(air.static_mass()-tail)

    def test_exact_stop_kernel_against_independent_ode(self):
        raw=self.fixture();air=raw.with_static_tail(raw.static_mass()+np.diag([.001,.002]));dt=1/128000
        velocity=np.array([[0.,0.],[.01,-.02],[.03,.01],[-.02,.025]])
        actual=air.linear_drive_force(velocity,dt,25,8)
        state=air.zero_state()
        def force(r,v):
            u=v.copy();value=air.D@u
            for j,row in enumerate(r):u=2*row-u;value+=air.R[j+1]@u
            return value
        reference=[force(state,velocity[0])]
        for k in range(1,len(actual)):
            if k<len(velocity):v0=velocity[k-1];v1=velocity[k]
            else:v0=v1=np.zeros(2)
            def ode(t,y):
                r=y.reshape(state.shape);u=v0+t/dt*(v1-v0);derivative=[]
                for row in r:derivative.append(air.a*(u-row));u=2*row-u
                return np.array(derivative).ravel()
            state=solve_ivp(ode,(0,dt),state.ravel(),method='DOP853',rtol=1e-12,atol=1e-15).y[:,-1].reshape(state.shape)
            reference.append(force(state,v1)+air.tail_mass@((v1-v0)/dt))
        np.testing.assert_allclose(actual,reference,atol=2e-14,rtol=1e-10)
        # Large t/high order: exponential underflow must not prematurely
        # erase a polynomial-amplified tail; compare a scaled recurrence.
        h=np.zeros((326,2,1));h[0,0,0]=1;h[-1,0,0]=-1;h[:,1]=-h[::-1,0]
        high=RationalAir(h,64000)
        times=np.array([.004,.005,.006,.01])
        self.assertTrue(np.isfinite(high.step_kernel(times)).all())
        # Independent 80-digit polynomial evaluation, with exp(-768) below
        # double precision's representable range before amplification.
        from decimal import Decimal,localcontext
        with localcontext() as context:
            context.prec=80;x=Decimal(1536);old=Decimal(1);current=1-x;total=old
            for j in range(1,325):
                total+=(-1)**j*current
                old,current=current,((2*j+1-x)*current-j*old)/(j+1)
            expected=float(4*total*(-x/2).exp())
        np.testing.assert_allclose(high.step_kernel([.006])[0,0,0],expected,rtol=1e-10,atol=1e-30)


if __name__=='__main__':unittest.main()
