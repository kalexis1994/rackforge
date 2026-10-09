import sys, unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_rational_air import RationalAir
from concert_grand_pade_air import PadeAir, PadeBoundedAir
from concert_grand_audition_air import MappedWave, MappedPlan, project_air_preserving_relaxation
from concert_grand_packed_air import PackedResidues, PackedMappedPlan, PackedCoast, PackedContact
import test_concert_shared_kernels as kernel_tests


class PackedTests(unittest.TestCase):
    def wave(self):
        rng=np.random.default_rng(17); part=rng.normal(size=(9,3,4))*.01
        part-=part.mean(axis=0); H=np.empty((9,6,4))
        H[:,::2]=part; H[:,1::2]=-part[::-1]
        full=PadeAir(RationalAir(H,128000))
        lift=rng.normal(size=(4,2)); port=rng.normal(size=(4,2))
        return MappedWave(full,lift,port)

    def test_full_sum_and_live_update_match_for_multiple_steps(self):
        wave=self.wave(); residues=PackedResidues(wave)
        rng=np.random.default_rng(33)
        for rate in [16000,128000,768000]:
            a=MappedPlan(wave,1/rate); b=PackedMappedPlan(wave,1/rate,residues)
            state=rng.normal(size=wave.zero_state().shape)*.01
            before=state.copy(); velocity=rng.normal(size=wave.modes)*.02
            old,oa=a.constant(state); new,ob=b.constant(state)
            np.testing.assert_allclose(new,old,rtol=1e-12,atol=1e-18)
            np.testing.assert_array_equal(oa,ob)
            for x,y in zip(a.update(state,velocity,oa),b.update(state,velocity,ob)):
                np.testing.assert_array_equal(x,y)
            np.testing.assert_array_equal(before,state)
        self.assertFalse(residues.matrix.flags.writeable)
        with self.assertRaises(ValueError): PackedResidues(wave,max_bytes=1)
        with self.assertRaises(ValueError): PackedMappedPlan(self.wave(),1/128000,residues)

    def test_coupled_contact_coast_and_loss_agree_with_original_air(self):
        core,full,felt=kernel_tests.KernelTests().fixture()
        # This fixture has three ports. Retain two wave coordinates and all
        # positive completion states, as in the actual reduced pilot.
        air=project_air_preserving_relaxation(full,np.eye(3)[:,:2])
        from concert_grand_shared_strings import SharedHammers
        from concert_grand_shared_kernels import BatchedStrings, BatchedCoast
        from concert_grand_exponential_contact import ExponentialContact
        system=SharedHammers(BatchedStrings(core),felt)
        residues=PackedResidues(air.wave)
        old=ExponentialContact(system,air); new=PackedContact(system,air,residues)
        a=old.initial_state(0.); a,_=system.launch(a,60,3.)
        b=tuple(x.copy() for x in a)
        for _ in range(64):
            a,sa=old.step(a,1/128000); b,sb=new.step(b,1/128000)
            for x,y in zip(a,b): np.testing.assert_allclose(x,y,rtol=1e-9,atol=1e-17)
            self.assertAlmostEqual(sa['loss_j'],sb['loss_j'],places=15)
        a[0][core.size:]=-.01; a[1][core.size:]=0.
        b=tuple(x.copy() for x in a)
        old=BatchedCoast(system,air); new=PackedCoast(system,air,residues)
        for _ in range(32):
            a,sa=old.step(a,1/64000); b,sb=new.step(b,1/64000)
            for x,y in zip(a,b): np.testing.assert_allclose(x,y,rtol=1e-9,atol=1e-17)
            self.assertAlmostEqual(sa['loss_j'],sb['loss_j'],places=15)


if __name__=='__main__': unittest.main()
