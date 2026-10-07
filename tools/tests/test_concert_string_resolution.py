"""Independent high-order spatial force and full contact work/capture checks."""
import sys
import unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
sys.path.insert(0,str(Path(__file__).resolve().parent))
from concert_grand_nonlinear_string import Wire,String
from concert_grand_contact_study import CosineString
from concert_grand_fast_contact import FastBoundedContact
from concert_grand_moving_bridge import MovingBridge,Support
from concert_grand_string_resolution import capture_contact,join_capture
from concert_grand_exponential_coast import ExponentialCoast
from concert_grand_coast_measurements import run_coast
from test_concert_pade_contact import fixture


class StringResolutionTests(unittest.TestCase):
    def test_higher_order_potential_gradient_and_hessian_against_spatial_gauss(self):
        rng=np.random.default_rng(70107);wire=Wire(.62,.0081,850)
        for nt,nl in [(6,12),(12,24),(24,48),(48,96),(48,192)]:
            fast=CosineString(wire,nt,nl,bending=True)
            dense=String(wire,nt,nl,quadrature=max(240,8*nl),bending=True)
            q=rng.normal(size=fast.size)*1e-6;q[fast.su]*=.05
            expected=dense.potential(q)
            self.assertLess(abs(fast.potential(q)-expected)/expected,2e-11)
            g,H=dense.gradient_hessian(q);fg,fH=fast.gradient_hessian(q)
            for actual,reference in [(fg,g),(fH,H),(fast.gradient_hessian(q,False),g)]:
                self.assertLess(np.linalg.norm(actual-reference)/np.linalg.norm(reference),2e-11)

    def test_capture_matches_direct_dynamics_and_mechanical_work(self):
        ref=fixture();fast=FastBoundedContact(ref.system,ref.air)
        final,row,t,B,V=capture_contact(fast,5.,768000,16/768000)
        state=ref.initial_state(5.);heat=work=0.
        for i in range(16):
            state,stats=ref.step(state,1/768000,True)
            heat+=stats['felt_loss_j']+stats['material_loss_j'];work+=stats['air_port_work_j']
            np.testing.assert_allclose(B[i+1],ref.board_map@state[0],rtol=2e-8,atol=1e-15)
            np.testing.assert_allclose(V[i+1],ref.board_map@state[1],rtol=2e-8,atol=1e-15)
        self.assertLess(row['maximum_mechanical_budget_relative'],1e-9)
        self.assertLess(row['impulse_momentum_relative'],1e-10)
        self.assertAlmostEqual(row['felt_heat_j']+row['material_heat_j'],heat,delta=1e-13)
        self.assertAlmostEqual(row['air_port_work_j'],work,delta=1e-13)
        self.assertEqual(len(t),17)

    def test_high_order_quadrature_with_moving_endpoint_against_independent_gauss(self):
        rng=np.random.default_rng(89);wire=Wire(.62,.0081,850)
        for nt,nl in [(24,48),(48,96),(48,192)]:
            core=MovingBridge(wire,Support((180.,600.),(.2,.1),(.01,.02)),nt,nl)
            q=rng.normal(size=core.size)*1e-6;q[core.su]*=.05
            energy=core.potential(q);g,H=core.gradient_hessian(q)
            original=core.base
            dense=String(wire,nt,nl,quadrature=max(240,8*nl),bending=True)
            dense.ku=original.ku;dense.kv=original.kv;dense.nodes=len(dense.x)
            dense.synthesize=lambda coefficients:np.cos(
                np.pi*dense.x[:,None]/wire.length*np.arange(1,len(coefficients)+1))@coefficients
            core.base=dense
            expected=core.potential(q);dg,dH=core.gradient_hessian(q)
            self.assertLess(abs(energy-expected)/expected,2e-11)
            self.assertLess(np.linalg.norm(g-dg)/np.linalg.norm(dg),2e-11)
            self.assertLess(np.linalg.norm(H-dH)/np.linalg.norm(dH),2e-11)

    def test_axial_modes_above_twice_transverse_are_unforced_in_this_galerkin_model(self):
        rng=np.random.default_rng(41);wire=Wire(.62,.0081,850)
        low=CosineString(wire,12,24);high=CosineString(wire,12,48)
        q=rng.normal(size=low.size)*1e-6;q[low.su]*=.05
        mapped=np.r_[q[low.su],np.zeros(24),q[low.sv],q[low.sw]]
        g=low.gradient_hessian(q,False);gh=high.gradient_hessian(mapped,False)
        self.assertLess(np.linalg.norm(gh[24:48])/np.linalg.norm(g),2e-12)
        active=np.r_[gh[:24],gh[high.sv],gh[high.sw]]
        self.assertLess(np.linalg.norm(active-g)/np.linalg.norm(g),2e-12)
        self.assertLess(abs(low.potential(q)-high.potential(mapped))/low.potential(q),2e-12)

    def test_join_preserves_live_endpoint_and_rejects_reset(self):
        ref=fixture();state=ref.initial_state(0.);state[0][-1]=-.001
        _,_,_,t,B,V=run_coast(ExponentialCoast(ref.system,ref.air),state,128000,4/128000)
        a=(np.array([0.,1/128000]),np.vstack((B[0]*0,B[0])),np.vstack((V[0]*0,V[0])))
        joined=join_capture(a,(t,B,V))
        self.assertEqual(len(joined[0]),6)
        np.testing.assert_array_equal(joined[1][1:],B)
        changed=B.copy();changed[0,0]+=.001
        with self.assertRaisesRegex(ValueError,'living physical endpoint'):join_capture(a,(t,changed,V))
        with self.assertRaises(ValueError):capture_contact(ref,5.,768000,.0000011)


if __name__=='__main__':unittest.main()
