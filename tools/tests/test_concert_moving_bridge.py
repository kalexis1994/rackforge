"""Independent kinetic integrals, potential derivatives and linear dynamics."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from numpy.polynomial.legendre import leggauss
from scipy.linalg import expm
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt,HammerString
from concert_grand_moving_bridge import MovingBridge,Support,BridgeHammer,contact_footprint


class MovingBridgeTests(unittest.TestCase):
    def setUp(self):
        self.wire=Wire(.62,.0081,850)
        self.core=MovingBridge(self.wire,Support.oscillator(),transverse=4)
        self.felt=Felt(.002,7.5e8,2.6,.125,.02,.1)

    def test_mass_matches_independent_velocity_integral(self):
        core=self.core;rng=np.random.default_rng(7);P=rng.normal(size=core.size)*.03
        z=core.physical(P);b=core.phi@z[core.sb]
        x,w=leggauss(240);x=(x+1)*self.wire.length/2;w=w*self.wire.length/2
        k=np.pi*np.arange(1,core.nt+1)/self.wire.length;ku=np.pi*np.arange(1,core.nl+1)/self.wire.length
        v=np.sin(x[:,None]*k)@z[core.sv]/np.sqrt(self.wire.modal_mass)+x/self.wire.length*b
        h=np.sin(x[:,None]*k)@z[core.sw]/np.sqrt(self.wire.modal_mass)
        u=np.sin(x[:,None]*ku)@z[core.su]/np.sqrt(self.wire.modal_mass)
        kinetic=.5*self.wire.linear_density*np.dot(w,u*u+v*v+h*h)+.5*z[core.sb]@z[core.sb]
        self.assertAlmostEqual(kinetic,.5*P@P,places=14)
        self.assertGreater(np.linalg.eigvalsh(core.mass).min(),0.)
        self.assertLess(np.max(abs(core.transform.T@core.mass@core.transform-np.eye(core.size))),1e-14)

    def test_potential_and_derivatives_match_spatial_field(self):
        core=self.core;rng=np.random.default_rng(8);Q=rng.normal(size=core.size)*2e-6;z=core.physical(Q)
        x,w=leggauss(240);x=(x+1)*self.wire.length/2;w=w*self.wire.length/2
        root=np.sqrt(self.wire.modal_mass);k=np.pi*np.arange(1,core.nt+1)/self.wire.length;ku=np.pi*np.arange(1,core.nl+1)/self.wire.length
        b=core.phi@z[core.sb];vx=np.cos(x[:,None]*k)@(k*z[core.sv]/root)+b/self.wire.length
        wx=np.cos(x[:,None]*k)@(k*z[core.sw]/root);ux=np.cos(x[:,None]*ku)@(ku*z[core.su]/root)
        vc=-np.sin(x[:,None]*k)@(k*k*z[core.sv]/root);wc=-np.sin(x[:,None]*k)@(k*k*z[core.sw]/root)
        ei=self.wire.young*self.wire.area**2/(4*np.pi)
        potential=.5*np.dot(w,self.wire.tension*(vx*vx+wx*wx)+ei*(vc*vc+wc*wc)+self.wire.axial_rigidity*(ux+.5*(vx*vx+wx*wx))**2)
        potential+=.5*np.dot(core.omega*z[core.sb],core.omega*z[core.sb])
        self.assertLess(abs(core.potential(Q)-potential)/potential,1e-12)
        direction=rng.normal(size=core.size);direction/=np.linalg.norm(direction);eps=1e-10
        g,H=core.gradient_hessian(Q)
        measured=(core.potential(Q+eps*direction)-core.potential(Q-eps*direction))/(2*eps)
        self.assertLess(abs(measured-g@direction)/max(abs(measured),1e-20),1e-7)
        measured_H=(core.gradient_hessian(Q+eps*direction,False)-core.gradient_hessian(Q-eps*direction,False))/(2*eps)
        self.assertLess(np.linalg.norm(measured_H-H@direction)/np.linalg.norm(measured_H),1e-7)

    def test_contact_footprint_is_converged_and_reciprocal(self):
        shape,mean=contact_footprint(48,.08534,.07877,512)
        fine,fine_mean=contact_footprint(48,.08534,.07877,1024)
        self.assertLess(np.linalg.norm(shape-fine)/np.linalg.norm(fine),1e-12)
        self.assertAlmostEqual(mean,fine_mean,places=13)
        self.assertGreater(mean,.08534)  # the truncated Gaussian is asymmetric
        system=BridgeHammer(self.core,self.felt);rng=np.random.default_rng(9);Q=rng.normal(size=system.size)*1e-6
        z=self.core.physical(Q[:-1]);compression=Q[-1]/np.sqrt(self.felt.hammer_mass)-system.shape@z[self.core.sv]/np.sqrt(self.wire.modal_mass)-system.lift_contact*(self.core.phi@z[self.core.sb])
        self.assertAlmostEqual(system.compression(Q),compression,places=16)

    def test_force_and_power_are_conjugate(self):
        system=BridgeHammer(self.core,self.felt);rng=np.random.default_rng(10)
        q,p=system.initial_state(3.);q[:-1]=rng.normal(size=self.core.size)*2e-6;p[:-1]=rng.normal(size=self.core.size)*.01
        q[-1]=np.sqrt(self.felt.hammer_mass)*.0005
        force,_=system.continuous_force(q,p);interface=system.interface(q,p,-force)
        self.assertLess(interface['force_equation_residual'],1e-12)
        self.assertLess(abs(interface['power_residual_w']),1e-12)

    def test_zero_projection_recovers_fixed_string_contact(self):
        core=MovingBridge(self.wire,Support.oscillator(coupling=0.),transverse=4)
        felt=Felt(.002,7.5e8,2.6,.125,0.,0.)
        coupled=BridgeHammer(core,felt);fixed=HammerString(core.base,felt)
        q,p=coupled.initial_state(3.);a,b=fixed.initial_state(3.)
        for _ in range(800):
            q,p,_=coupled.step(q,p,1/1400000);a,b,_=fixed.step(a,b,1/1400000)
        np.testing.assert_allclose(q[:core.ns],a[:-1],rtol=1e-12,atol=1e-15)
        np.testing.assert_allclose(p[:core.ns],b[:-1],rtol=1e-12,atol=1e-14)
        self.assertAlmostEqual(q[-1],a[-1],places=14);self.assertAlmostEqual(p[-1],b[-1],places=14)
        self.assertEqual(q[core.ns],0.);self.assertEqual(p[core.ns],0.)

    def test_multimode_support_mass_derivatives_and_power(self):
        # Signed modal projections must preserve reciprocal mechanics too.
        core=MovingBridge(self.wire,Support((110.,270.,680.),(1.7,-.9,.4),(.03,.06,.04)),transverse=4)
        system=BridgeHammer(core,self.felt);rng=np.random.default_rng(14)
        q,p=system.initial_state(3.);q[:-1]=rng.normal(size=core.size)*2e-6;p[:-1]=rng.normal(size=core.size)*.01
        q[-1]=np.sqrt(self.felt.hammer_mass)*.0005
        np.testing.assert_allclose(core.transform.T@core.mass@core.transform,np.eye(core.size),atol=1e-14)
        self.assertGreater(np.linalg.eigvalsh(core.mass).min(),0.)
        g,H=core.gradient_hessian(q[:-1]);direction=rng.normal(size=core.size);direction/=np.linalg.norm(direction);eps=1e-10
        measured=(core.gradient_hessian(q[:-1]+eps*direction,False)-core.gradient_hessian(q[:-1]-eps*direction,False))/(2*eps)
        self.assertLess(np.linalg.norm(measured-H@direction)/np.linalg.norm(measured),1e-7)
        force,_=system.continuous_force(q,p);interface=system.interface(q,p,-force)
        self.assertLess(interface['force_equation_residual'],1e-12)
        self.assertLess(abs(interface['power_residual_w']),1e-12)
        self.assertGreaterEqual(np.linalg.eigvalsh(core.damping).min(),-1e-13)

    def test_centered_losses_pay_for_both_support_and_felt(self):
        system=BridgeHammer(self.core,self.felt);q,p=system.initial_state(5.)
        initial=system.energy(q,p);heat=0.
        for _ in range(2000):
            q,p,stats=system.step(q,p,1/1400000);heat+=stats['loss_j']
            self.assertGreaterEqual(stats['support_loss_j'],-1e-18);self.assertGreaterEqual(stats['felt_loss_j'],-1e-18)
            self.assertLess(abs(system.energy(q,p)+heat-initial)/initial,1e-9)

    def test_linear_response_against_matrix_exponential_and_reciprocity(self):
        core=MovingBridge(Wire(2.,.0081,850),Support.oscillator(),transverse=2,nonlinear=False)
        system=BridgeHammer(core,self.felt);q,p=system.initial_state(0.);q[-1]=-.01*np.sqrt(self.felt.hammer_mass)
        q[0]=1e-6;p[core.ns]=.01;n=core.size;K=core.gradient_hessian(np.zeros(n))[1];D=core.damping
        reference=expm(np.block([[np.zeros((n,n)),np.eye(n)],[-K,-D]])*.0005)@np.r_[q[:-1],p[:-1]]
        errors=[]
        for steps in (250,500,1000):
            a,b=q.copy(),p.copy()
            for _ in range(steps):a,b,_=system.step(a,b,.0005/steps)
            error=np.linalg.norm(np.r_[a[:-1]-reference[:n],b[:-1]-reference[n:]])/np.linalg.norm(reference)
            errors.append(error)
        self.assertTrue(all(3.8<errors[i]/errors[i+1]<4.2 for i in (0,1)))
        self.assertLess(errors[-1],1e-4)
        for hz in (90.,260.,1000.):
            omega=2*np.pi*hz;response=1j*omega*np.linalg.inv(K-omega**2*np.eye(n)+1j*omega*D)
            self.assertLess(np.linalg.norm(response-response.T),1e-12)
            self.assertGreaterEqual(np.linalg.eigvalsh((response+response.conj().T)/2).min(),-1e-10)


if __name__=='__main__':unittest.main()
