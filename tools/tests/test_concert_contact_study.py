"""Independent spatial integrals and motion-preserving contact initialization."""
import sys,unittest
from types import SimpleNamespace
from importlib.util import spec_from_file_location,module_from_spec
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from concert_grand_nonlinear_string import Wire,String
from concert_grand_coupled_contact import Felt,HammerString
from concert_grand_contact_study import CosineString,from_motion

spec=spec_from_file_location('contact_recoil_test',Path(__file__).resolve().parents[1]/'audit-concert-grand-contact-recoil.py')
recoil=module_from_spec(spec);spec.loader.exec_module(recoil)


class ContactStudy(unittest.TestCase):
    def test_exact_cosine_kernel_against_spatial_gauss_integrals(self):
        wire=Wire(.62,.0081,850);rng=np.random.default_rng(67)
        for bending in (False,True):
            fast=CosineString(wire,12,24,bending=bending)
            reference=String(wire,12,24,quadrature=240,bending=bending)
            q=rng.normal(size=fast.size)*2e-6; q[fast.su]*=.08
            energy=reference.potential(q)
            self.assertLess(abs(fast.potential(q)-energy)/energy,2e-12)
            gradient,hessian=reference.gradient_hessian(q)
            fg,fh=fast.gradient_hessian(q)
            self.assertLess(np.linalg.norm(fg-gradient)/np.linalg.norm(gradient),2e-12)
            self.assertLess(np.linalg.norm(fast.gradient_hessian(q,False)-gradient)/np.linalg.norm(gradient),2e-12)
            self.assertLess(np.linalg.norm(fh-hessian)/np.linalg.norm(hessian),2e-12)
        with self.assertRaises(ValueError):CosineString(wire,12,24,nodes=24)

    def test_end_reaction_includes_bending_and_nonlinear_tension(self):
        wire=Wire(.4,.005,850);core=CosineString(wire,4,8,bending=True)
        q=np.zeros(core.size);harmonic=3;amplitude=.0002
        q[core.nl+harmonic-1]=np.sqrt(wire.modal_mass)*amplitude
        k=np.pi*harmonic/wire.length;slope=(-1)**harmonic*k*amplitude
        ei=wire.young*wire.area**2/(4*np.pi)
        expected=(wire.tension+.5*wire.axial_rigidity*slope*slope)*slope+ei*k*k*slope
        self.assertAlmostEqual(core.end_force(q)[0],expected,places=12)
        self.assertEqual(core.end_force(q)[1],0.)

    def test_relaunch_preserves_motion_and_counts_external_hammer_energy(self):
        core=CosineString(Wire(.62,.0081,850),4,8)
        system=HammerString(core,Felt(.002,7.5e8,2.6,.125,.02,.1))
        rng=np.random.default_rng(88);qs=rng.normal(size=core.size)*1e-6;ps=rng.normal(size=core.size)*.01
        qs_copy,ps_copy=qs.copy(),ps.copy();q,p=from_motion(system,qs,ps,2.)
        np.testing.assert_array_equal(q[:-1],qs);np.testing.assert_array_equal(p[:-1],ps)
        self.assertLess(abs(system.compression(q)),1e-18)
        self.assertAlmostEqual(system.energy(q,p),core.energy(qs,ps)+.5*.002*2**2,places=14)
        initial=system.energy(q,p);heat=0.
        for _ in range(800):
            q,p,stats=system.step(q,p,1/1400000);heat+=stats['loss_j']
            self.assertGreaterEqual(stats['loss_j'],-1e-18)
            self.assertLess(abs(system.energy(q,p)+heat-initial)/initial,1e-9)
        np.testing.assert_array_equal(qs,qs_copy);np.testing.assert_array_equal(ps,ps_copy)

    def test_existing_velocity_changes_contact_without_resetting_string(self):
        core=CosineString(Wire(.62,.0081,850),4,8)
        system=HammerString(core,Felt(.002,7.5e8,2.6,.125,.02))
        shape=-system.direction[:-1];ps=shape*.5/np.dot(shape,shape)
        peaks=[]
        for direction in (-1,1):
            q,p=from_motion(system,np.zeros(core.size),direction*ps,1.)
            peak=0.
            for _ in range(20):
                q,p,stats=system.step(q,p,1/1400000);peak=max(peak,stats['force_n'])
            peaks.append(peak)
        self.assertGreater(peaks[0],2*peaks[1])

    def test_returning_separation_skips_initial_zero_and_forward_going_hammer(self):
        system=HammerString(CosineString(Wire(.62,.0081,850),4,8),Felt(.002,7.5e8,2.6,.125,.02))
        states=np.zeros((3,2*system.size+1));states[1,2*system.size-1]=.01;states[2,2*system.size-1]=-.02
        events=SimpleNamespace(t_events=[np.array([0.,.001,.002])],y_events=[states])
        index,time,state=recoil.recoiling_separation(events,system)
        self.assertEqual(index,2);self.assertEqual(time,.002)
        self.assertEqual(state[2*system.size-1],-.02)
        events.y_events[0][:,2*system.size-1]=.01
        with self.assertRaises(RuntimeError):recoil.recoiling_separation(events,system)


if __name__=='__main__':unittest.main()
