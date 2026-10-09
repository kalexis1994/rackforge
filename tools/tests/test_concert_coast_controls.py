import sys, unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_shared_strings import SharedHammers
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_packed_air import PackedCoast,PackedResidues
from concert_grand_audition_air import project_air_preserving_relaxation
from concert_grand_coast_controls import (quadratic_system,quadratic_energy,
    MechanicalOnlyCoast,AirSubcycledCoast,WorkMeter)
import test_concert_shared_kernels as kernel_tests


class ControlTests(unittest.TestCase):
    def fixture(self):
        core,full,felt=kernel_tests.KernelTests().fixture()
        air=project_air_preserving_relaxation(full,np.eye(3)[:,:2])
        reference=SharedHammers(BatchedStrings(core),felt)
        system=quadratic_system(reference); residues=PackedResidues(air.wave)
        coast=PackedCoast(system,air,residues)
        state=coast.initial_state(0.)
        rng=np.random.default_rng(361)
        state[0][:core.size]=rng.normal(size=core.size)*1e-8
        state[1][:core.size]=rng.normal(size=core.size)*1e-4
        state[2][:]=rng.normal(size=state[2].shape)*1e-5
        state[3][:]=rng.normal(size=state[3].shape)*1e-5
        return reference,coast,residues,state

    def test_quadratic_ablation_keeps_matrices_and_exact_free_flow(self):
        reference,coast,residues,state=self.fixture()
        self.assertTrue(reference.string.nonlinear)
        self.assertFalse(coast.system.string.nonlinear)
        self.assertIs(reference.string.K,coast.system.string.K)
        free=MechanicalOnlyCoast(coast.system,coast.air,residues)
        a=tuple(x.copy() for x in state); b=tuple(x.copy() for x in state)
        before=quadratic_energy(free,a)
        a,_=free.step(a,8/16000)
        for _ in range(8): b,_=free.step(b,1/16000)
        for x,y in zip(a,b): np.testing.assert_allclose(x,y,rtol=1e-10,atol=2e-15)
        np.testing.assert_array_equal(a[2],state[2]); np.testing.assert_array_equal(a[3],state[3])
        self.assertLess(abs(quadratic_energy(free,a)/before-1),1e-11)

    def test_subcycling_matches_explicit_passive_subflows_and_work(self):
        _,coast,residues,state=self.fixture()
        sub=AirSubcycledCoast(coast.system,coast.air,residues,substeps=4)
        dt=1/16000; before=tuple(x.copy() for x in state)
        q,v,_=coast.mechanical_step(state[0],state[1],dt/2)
        manual=(q,v,state[2],state[3]); work=heat=0.
        for _ in range(4):
            manual,stats=coast.air_step(manual,dt/4,True)
            work+=stats['air_port_work_j']; heat+=stats['material_loss_j']
        q,v,_=coast.mechanical_step(manual[0],manual[1],dt/2)
        manual=(q,v,manual[2],manual[3])
        actual,stats=sub.step(state,dt,True)
        for x,y in zip(actual,manual): np.testing.assert_array_equal(x,y)
        for x,y in zip(state,before): np.testing.assert_array_equal(x,y)
        self.assertEqual(stats['wave_inputs_substeps'].shape[0],4)
        self.assertAlmostEqual(stats['air_port_work_j'],work,places=16)
        self.assertAlmostEqual(stats['material_loss_j'],heat,places=16)
        meter=WorkMeter(sub); final=state; initial_energy=quadratic_energy(sub,state)
        for _ in range(8): final,_=meter.step(final,dt,True)
        balance=quadratic_energy(sub,final)-initial_energy+meter.air_work_j+meter.material_heat_j
        self.assertLess(abs(balance)/initial_energy,1e-10)
        self.assertGreaterEqual(meter.material_heat_j,0.)
        with self.assertRaises(ValueError): AirSubcycledCoast(coast.system,coast.air,residues,substeps=1.5)


if __name__=='__main__': unittest.main()
