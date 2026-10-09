import sys
import unittest
from pathlib import Path
import numpy as np
from numpy.polynomial.legendre import leggauss
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_shared_strings import (WirePort,SharedStrings,SharedHammers,
    SharedContact,SharedContactReference,SharedCoast,SharedDampers)
from concert_grand_nonlinear_string import Wire
from concert_grand_moving_bridge import Support,MovingBridge,BridgeHammer
from concert_grand_coupled_contact import Felt
from concert_grand_rational_air import RationalAir
from concert_grand_pade_air import PadeAir,PadeBoundedAir


class SharedTests(unittest.TestCase):
    def setUp(self):
        self.frequency=(110.,270.,680.)
        self.phi=np.array([1.7,-.9,.4])
        self.damping=np.diag(.03*2*np.pi*np.array(self.frequency))
        self.felt=Felt(.002,7.5e8,2.6,.125,.02,.1)

    def core(self,count=2,coupling=True,nonlinear=True):
        ports=[WirePort(60+i,Wire(.62*(1+i*.02),.0081),self.phi if coupling or i==0 else self.phi*0) for i in range(count)]
        return SharedStrings(ports,self.frequency,self.damping,4,8,nonlinear)

    def air(self):
        wave=PadeAir(RationalAir(np.zeros((3,6,3)),128000))
        return PadeBoundedAir(wave,np.eye(3)*.001,np.eye(3)*.01)

    def test_single_wire_matches_frozen_moving_bridge_and_contact(self):
        core=self.core(1)
        original=MovingBridge(core.ports[0].wire,Support(self.frequency,tuple(self.phi),(.03,)*3),4,8)
        rng=np.random.default_rng(51);q=rng.normal(size=core.size)*1e-6
        np.testing.assert_allclose(core.mass,original.mass,atol=2e-16)
        np.testing.assert_allclose(core.transform,original.transform,atol=2e-16)
        self.assertAlmostEqual(core.potential(q),original.potential(q),places=15)
        g,H=core.gradient_hessian(q); og,oH=original.gradient_hessian(q)
        np.testing.assert_allclose(g,og,atol=2e-12,rtol=2e-13)
        np.testing.assert_allclose(H,oH,atol=2e-6,rtol=1e-12)
        system=SharedHammers(core,self.felt);old=BridgeHammer(original,self.felt)
        np.testing.assert_allclose(system.contacts[0][1],old.direction,atol=2e-15)
        a=np.r_[q,.0001*np.sqrt(self.felt.hammer_mass)];b=a+rng.normal(size=len(a))*1e-8
        ga,stats=system.avf_gradient(a,b,1/768000);go,_,so=old.avf(a,b,1/768000)
        np.testing.assert_allclose(ga,go,atol=2e-11,rtol=1e-12)
        self.assertAlmostEqual(stats['felt_loss_j'],so['felt_loss_j'],places=17)

    def test_mass_is_one_board_plus_independent_spatial_wire_integrals(self):
        core=self.core();rng=np.random.default_rng(18)
        q=rng.normal(size=core.size)*.01;z=core.physical(q)
        kinetic=.5*z[core.sb]@z[core.sb]
        for e in core.entries:
            wire=e['port'].wire;nodes,w=leggauss(100)
            x=(nodes+1)*wire.length/2;w=w*wire.length/2
            k=np.pi*np.arange(1,5)/wire.length;ku=np.pi*np.arange(1,9)/wire.length
            root=np.sqrt(wire.modal_mass)
            vv=np.sin(x[:,None]*k)@z[e['sv']]/root+x/wire.length*(e['phi']@z[core.sb])
            ww=np.sin(x[:,None]*k)@z[e['sw']]/root
            uu=np.sin(x[:,None]*ku)@z[e['su']]/root
            kinetic+=.5*wire.linear_density*np.dot(w,vv*vv+ww*ww+uu*uu)
        self.assertAlmostEqual(kinetic,.5*q@q,places=14)
        np.testing.assert_allclose(core.transform.T@core.mass@core.transform,np.eye(core.size),atol=1e-14)

    def test_nonlinear_gradient_and_avf_close_work_with_shared_board(self):
        core=self.core();rng=np.random.default_rng(9)
        a=rng.normal(size=core.size)*2e-6;b=a+rng.normal(size=core.size)*3e-7
        direction=rng.normal(size=core.size);direction/=np.linalg.norm(direction)
        g,H=core.gradient_hessian(a);eps=1e-10
        measured=(core.potential(a+eps*direction)-core.potential(a-eps*direction))/(2*eps)
        self.assertLess(abs(measured-g@direction)/abs(measured),1e-7)
        derivative=(core.gradient_hessian(a+eps*direction,False)-core.gradient_hessian(a-eps*direction,False))/(2*eps)
        self.assertLess(np.linalg.norm(derivative-H@direction)/np.linalg.norm(derivative),1e-7)
        avf=core.K@((a+b)/2)+core.nonlinear_avf(a,b)
        self.assertLess(abs(avf@(b-a)-(core.potential(b)-core.potential(a))),1e-14)

    def test_unstruck_wire_moves_through_board_and_zero_port_removes_transfer(self):
        amplitudes=[]
        for coupling in [True,False]:
            core=self.core(coupling=coupling,nonlinear=False)
            omega=2*np.pi*260;drive=np.zeros(core.size)
            physical=np.zeros(core.size);physical[core.entries[0]['sv'].start]=1
            drive=core.pull_gradient(physical)
            response=np.linalg.solve(core.K-omega**2*np.eye(core.size)+1j*omega*core.damping,drive)
            z=core.physical(response);amplitudes.append(float(np.linalg.norm(z[core.entries[1]['sv']])))
        self.assertGreater(amplitudes[0],1e-9)
        self.assertEqual(amplitudes[1],0.)

    def test_head_launch_preserves_live_choir_and_dampers_pay_for_loss(self):
        core=self.core();system=SharedHammers(core,self.felt);air=self.air()
        pilot=SharedContact(system,air);state=pilot.initial_state(0)
        rng=np.random.default_rng(6);state[0][:core.size]=rng.normal(size=core.size)*1e-7
        state[1][:core.size]=rng.normal(size=core.size)*1e-4
        before=system.energy(*state[:2]);new,work=system.launch(state,61,3.)
        np.testing.assert_array_equal(new[0][:core.size],state[0][:core.size])
        np.testing.assert_array_equal(new[1][:core.size],state[1][:core.size])
        self.assertIs(new[2],state[2]);self.assertIs(new[3],state[3])
        self.assertAlmostEqual(system.energy(*new[:2])-before,work,places=16)
        damper=SharedDampers(core,system.nh)
        v,heat=damper.dissipate(new[1],.001,{60:0.,61:1.})
        self.assertGreaterEqual(heat,0.)
        self.assertAlmostEqual(.5*(new[1]@new[1]-v@v),heat,places=15)

    def test_simultaneous_contact_and_coast_preserve_energy_with_positive_losses(self):
        core=self.core();system=SharedHammers(core,self.felt);pilot=SharedContact(system,self.air())
        state=pilot.initial_state(0)
        state,_=system.launch(state,60,3.);state,_=system.launch(state,61,2.)
        initial=pilot.energy(state)['total_j'];heat=0.
        for i in range(300):
            state,stats=pilot.step(state,1/768000)
            heat+=stats['loss_j']
            self.assertGreaterEqual(stats['felt_loss_j'],-1e-18)
        self.assertLess(abs(pilot.energy(state)['total_j']+heat-initial)/initial,1e-9)
        # Coast from a moving choir after explicitly parking both heads.
        state[0][core.size:]=-.01;state[1][core.size:]=0.
        coast=SharedCoast(system,pilot.air);initial=coast.energy(state)['total_j'];heat=0.
        for _ in range(30):
            state,stats=coast.step(state,1/128000)
            heat+=stats['loss_j']
        self.assertLess(abs(coast.energy(state)['total_j']+heat-initial)/initial,1e-9)

    def test_three_uneven_wire_contacts_share_one_head_and_close_felt_work(self):
        ports=[WirePort(60,Wire(.62*(1+i*.001),.0081),self.phi) for i in range(3)]
        core=SharedStrings(ports,self.frequency,self.damping,4,8)
        system=SharedHammers(core,self.felt)
        self.assertEqual(system.nh,1)
        pilot=SharedContact(system,self.air());state=pilot.initial_state(0)
        rng=np.random.default_rng(46);state[0][:core.size]=rng.normal(size=core.size)*2e-7
        state,work=system.launch(state,60,3.)
        self.assertLessEqual(system.compression(state[0]),1e-16)
        initial=pilot.energy(state)['total_j'];heat=0.
        for _ in range(300):
            state,stats=pilot.step(state,1/768000)
            heat+=stats['loss_j']
        self.assertLess(abs(pilot.energy(state)['total_j']+heat-initial)/initial,1e-9)

    def test_block_contact_matches_full_dense_solver_without_removing_modes(self):
        core=self.core();system=SharedHammers(core,self.felt);air=self.air()
        dense=SharedContactReference(system,air);fast=SharedContact(system,air)
        a=dense.initial_state(0.);a,_=system.launch(a,60,3.);a,_=system.launch(a,61,2.)
        b=tuple(x.copy() for x in a)
        for _ in range(100):
            a,sa=dense.step(a,1/768000)
            b,sb=fast.step(b,1/768000)
            for x,y in zip(a,b):np.testing.assert_allclose(x,y,rtol=2e-11,atol=2e-16)
        np.testing.assert_allclose(core.linear_gradient(b[0][:core.size]),core.K@b[0][:core.size],rtol=2e-13,atol=2e-12)


if __name__=='__main__':
    unittest.main()
