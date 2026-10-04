"""Analytical spherical field, reciprocal power and independent dynamics."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from scipy.linalg import expm
from scipy.integrate import solve_ivp
from concert_grand_air_load import PassiveAirLoad,SphereLoad,AirCoupledContact,contact_sphere,linear_generator
from concert_grand_moving_bridge import MovingBridge,BridgeHammer,Support
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt


def contact():
    core=MovingBridge(Wire(.62,.0081,850.),Support.oscillator(),transverse=4)
    return BridgeHammer(core,Felt(.002,7.5e8,2.6,.125,.02,.1))


class AirLoadTests(unittest.TestCase):
    def test_sphere_against_outgoing_velocity_potential(self):
        a=.18;rho=1.2;c=343.;area=4*np.pi*a*a;load=SphereLoad([a],[[1.]])
        frequency=np.array([0.,1.,20.,180.,800.,8000.]);s=2j*np.pi*frequency
        # phi(r)=phi(a)*a/r*exp(-s*(r-a)/c), v(a)=(s/c+1/a)*phi(a).
        exact=area*rho*s/(s/c+1/a)
        np.testing.assert_allclose(load.impedance(frequency)[:,0,0],exact,rtol=1e-13,atol=1e-13)
        r=.66;pressure=rho*s/(s/c+1/a)*a/r*np.exp(-s*(r-a)/c)
        np.testing.assert_allclose(load.pressure_transfer(frequency,[r])[:,0],pressure,rtol=1e-13,atol=1e-13)
        low=load.impedance([1e-6])[0,0,0];high=load.impedance([1e9])[0,0,0]
        self.assertAlmostEqual(low.imag/(2*np.pi*1e-6)/load.mass[0],1.,places=12)
        self.assertAlmostEqual(high.real/load.resistance[0],1.,places=12)

    def test_port_power_and_force_are_conjugate_for_signed_projections(self):
        load=PassiveAirLoad([45.,90.],[.02,.04],[[.8,-.3,1.2],[-.4,.9,.2]])
        P=np.array([.03,-.04,.02]);W=np.array([.002,-.003]);response=load.response(P,W)
        self.assertAlmostEqual(P@response['mechanical_force'],response['port_power_w'],places=14)
        self.assertAlmostEqual(response['port_power_w'],response['stored_energy_rate_w']+response['escaped_power_w'],places=14)
        self.assertGreaterEqual(np.linalg.eigvalsh(load.damping).min(),-1e-12)

    def test_positive_real_impedance_and_reciprocity(self):
        load=PassiveAirLoad([45.,90.],[.02,.04],[[.8,-.3],[-.4,.9]])
        for s in (0.,2j*np.pi*180,1+2j*np.pi*523,1000+2j*np.pi*800):
            z=load.laplace_impedance([s])[0]
            np.testing.assert_allclose(z,z.T,atol=1e-13)
            self.assertGreaterEqual(np.linalg.eigvalsh((z+z.conj().T)/2).min(),-1e-12)
        np.testing.assert_allclose(load.impedance([-80.,-261.]),load.impedance([80.,261.]).conj(),atol=1e-13)

    def test_linear_transient_against_exponential_and_energy_identity(self):
        mass=.25;load=SphereLoad([.18],[[1/np.sqrt(mass)]])
        K=np.array([[(2*np.pi*180)**2]]);D=np.array([[.05*2*np.pi*180]])
        A,H,loss=linear_generator(K,D,load)
        np.testing.assert_allclose(A.T@H+H@A+2*loss,0.,atol=1e-11)
        initial=np.array([0.,np.sqrt(mass)*.02,0.]);energy=.5*initial@H@initial
        duration=.008;reference=expm(A*duration)@initial;errors=[]
        for steps in (200,400,800):
            dt=duration/steps;propagator=np.linalg.solve(np.eye(3)-.5*dt*A,np.eye(3)+.5*dt*A)
            state=initial.copy();escaped=0.
            for _ in range(steps):
                following=propagator@state;mid=.5*(state+following);escaped+=dt*mid@loss@mid;state=following
            self.assertLess(abs(.5*state@H@state+escaped-energy)/energy,1e-12)
            errors.append(np.sqrt((state-reference)@H@(state-reference)/energy))
        self.assertTrue(all(3.9<errors[i]/errors[i+1]<4.1 for i in (0,1)),errors)

    def test_stored_air_can_return_energy_without_negative_radiation(self):
        load=SphereLoad([.18],[[2.]])
        response=load.response(np.array([.01]),np.array([.1]))
        self.assertLess(response['port_power_w'],0.)
        self.assertGreater(response['escaped_power_w'],0.)
        self.assertLess(response['stored_energy_rate_w'],0.)

    def test_nonlinear_contact_pays_air_energy_and_escaped_wave(self):
        system=contact();coupled=AirCoupledContact(system,contact_sphere(system))
        q,p=coupled.initial_state(5.);initial=coupled.energy(q,p);lost=0.
        for _ in range(2000):
            q,p,stats=coupled.step(q,p,1/1400000);lost+=stats['loss_j']
            self.assertGreaterEqual(stats['escaped_wave_energy_j'],0.)
            self.assertLess(abs(stats['port_work_j']-stats['escaped_wave_energy_j']-stats['air_energy_delta_j']),1e-16)
            self.assertLess(abs(coupled.energy(q,p)+lost-initial)/initial,1e-9)

    def test_zero_projection_recovers_mechanical_contact(self):
        system=contact();coupled=AirCoupledContact(system,contact_sphere(system,coupling=0.))
        q,p=coupled.initial_state(3.);a,b=system.initial_state(3.)
        for _ in range(1000):
            q,p,_=coupled.step(q,p,1/1400000);a,b,_=system.step(a,b,1/1400000)
        np.testing.assert_allclose(q[:system.size],a,rtol=1e-12,atol=1e-15)
        np.testing.assert_allclose(p[:system.size],b,rtol=1e-12,atol=1e-14)
        self.assertEqual(q[-1],0.);self.assertEqual(p[-1],0.)

    def test_nonlinear_accuracy_against_independent_adaptive_solver(self):
        system=contact();coupled=AirCoupledContact(system,contact_sphere(system));n=coupled.size
        q,p=coupled.initial_state(5.);initial=coupled.energy(q,p);duration=.0026
        def derivative(t,state):
            force,heat,port=coupled.continuous_force(state[:n],state[n:2*n])
            return np.r_[state[n:2*n],-force,heat+port['escaped_power_w']]
        ref=solve_ivp(derivative,(0,duration),np.r_[q,p,0.],method='DOP853',rtol=1e-11,atol=1e-14,max_step=1e-6)
        self.assertTrue(ref.success);target=ref.y[:,-1]
        self.assertLess(abs(coupled.energy(target[:n],target[n:2*n])+target[-1]-initial)/initial,1e-8)
        linear=system.string.gradient_hessian(np.zeros(system.string.size))[1];errors=[]
        for steps in (1300,2600,5200):
            a,b=q.copy(),p.copy()
            for _ in range(steps):a,b,_=coupled.step(a,b,duration/steps)
            dq=a[:system.string.size]-target[:system.string.size];dp=b-target[n:2*n]
            errors.append(np.sqrt((dq@linear@dq+dp@dp)/initial))
        self.assertTrue(all(3.7<errors[i]/errors[i+1]<4.3 for i in (0,1)),errors)
        self.assertLess(errors[-1],.001)


if __name__=='__main__':unittest.main()
