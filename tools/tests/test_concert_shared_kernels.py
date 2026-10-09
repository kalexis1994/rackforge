import sys, unittest
from pathlib import Path
import numpy as np
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from concert_grand_shared_strings import WirePort,SharedStrings,SharedHammers,SharedContact,SharedCoast
from concert_grand_shared_kernels import BatchedStrings,BatchedCoast
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
from concert_grand_rational_air import RationalAir
from concert_grand_pade_air import PadeAir,PadeBoundedAir


class KernelTests(unittest.TestCase):
    def fixture(self, nonlinear=True):
        ports=[WirePort(60+i//3,Wire(.62/(1+i*.07),.0081,850+i),[1.7,-.9,.4]) for i in range(6)]
        core=SharedStrings(ports,[110.,270.,680.],np.diag([.01,.02,.03]),8,16,nonlinear)
        rng=np.random.default_rng(7);part=rng.normal(size=(7,3,3))*.001
        part-=part.mean(axis=0);H=np.empty((7,6,3))
        H[:,::2]=part;H[:,1::2]=-part[::-1]
        wave=PadeAir(RationalAir(H,128000))
        air=PadeBoundedAir(wave,wave.static_mass()+np.eye(3)*.001,np.eye(3))
        felt=Felt(.002,7.5e8,2.6,.125,.02,.1)
        return core,air,felt

    def test_batched_nonlinear_work_and_physical_maps_match_unequal_wires(self):
        old,_,_=self.fixture();new=BatchedStrings(old);rng=np.random.default_rng(72)
        for _ in range(10):
            a=rng.normal(size=old.size)*1e-6;b=a+rng.normal(size=old.size)*1e-7
            np.testing.assert_allclose(new.physical(a),old.physical(a),rtol=1e-13,atol=1e-20)
            np.testing.assert_allclose(new.pull_gradient(a),old.pull_gradient(a),rtol=1e-13,atol=1e-20)
            ga=old.nonlinear_avf(a,b);gb=new.nonlinear_avf(a,b)
            np.testing.assert_allclose(gb,ga,rtol=2e-11,atol=2e-15)
            work=(old.K@((a+b)/2)+gb)@(b-a)
            self.assertLess(abs(work-(old.potential(b)-old.potential(a))),1e-13)

    def test_contact_and_coast_keep_all_living_states_and_losses(self):
        old,air,felt=self.fixture();new=BatchedStrings(old)
        a=SharedContact(SharedHammers(old,felt),air);b=SharedContact(SharedHammers(new,felt),air)
        sa=a.initial_state(0.);sa,_=a.system.launch(sa,60,3.);sb=tuple(x.copy() for x in sa)
        for _ in range(160):
            sa,ta=a.step(sa,1/768000);sb,tb=b.step(sb,1/768000)
            for x,y in zip(sa,sb):np.testing.assert_allclose(x,y,rtol=1e-10,atol=2e-16)
            self.assertAlmostEqual(ta['loss_j'],tb['loss_j'],places=16)
        # Separated heads only; identical moving mechanical and acoustic state.
        sa[0][old.size:]=-.01;sa[1][old.size:]=0.;sb=tuple(x.copy() for x in sa)
        ca=SharedCoast(a.system,air);cb=BatchedCoast(b.system,air)
        for _ in range(64):
            sa,ta=ca.step(sa,1/64000);sb,tb=cb.step(sb,1/64000)
            for x,y in zip(sa,sb):np.testing.assert_allclose(x,y,rtol=1e-9,atol=3e-16)
            self.assertAlmostEqual(ta['loss_j'],tb['loss_j'],places=15)

    def test_diagonal_carrier_permutations_preserve_inverse(self):
        old,air,felt=self.fixture(False)
        system=SharedHammers(BatchedStrings(old),felt);coast=BatchedCoast(system,air)
        rng=np.random.default_rng(90);a=rng.normal(size=old.size)
        np.testing.assert_allclose(coast.unproject(coast.project(a)),a,rtol=1e-12,atol=1e-14)


if __name__=='__main__':unittest.main()
