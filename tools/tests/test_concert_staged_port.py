import sys,unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_staged_gauss_air import StagedGaussAirPlan
from concert_grand_staged_port import StagedPortCoast
from concert_grand_gauss_air import GaussAirPlan
from concert_grand_linear_phase import LinearBank
import test_concert_port_avf_coast as parent


class StagedAirTests(unittest.TestCase):
    def test_distinct_inputs_against_independent_real_gauss_system_and_work(self):
        bank=LinearBank(4,8);air=bank.air;na=bank.nr+bank.nz
        def unpack(a):return a[:bank.nr].reshape(bank.shape),a[bank.nr:]
        def rhs(a,u):
            d,f=air.derivative(unpack(a),u)
            return np.r_[d[0].ravel(),d[1]],f
        L=np.column_stack([rhs(e,np.zeros(air.modes))[0] for e in np.eye(na)])
        U=np.column_stack([rhs(np.zeros(na),u)[0] for u in np.eye(air.modes)])
        g=np.sqrt(3)/6;A=np.array([[.25,.25-g],[.25+g,.25]])
        rng=np.random.default_rng(618);a=rng.normal(size=na)*1e-5
        velocities=rng.normal(size=(2,air.modes))*1e-4
        for rate in [16000,32000,128000,512000]:
            dt=1/rate;plan=StagedGaussAirPlan(air,dt,bank.pilots['split'].packed_residues)
            right=np.tile(a,2)+dt*np.kron(A,U)@velocities.ravel()
            Y=np.linalg.solve(np.eye(2*na)-dt*np.kron(A,L),right).reshape(2,na)
            derivatives=[rhs(y,u) for y,u in zip(Y,velocities)]
            expected=a+dt/2*sum(d[0] for d in derivatives)
            loads=np.array([d[1] for d in derivatives])
            r,z=unpack(a);before=(r.copy(),z.copy(),velocities.copy())
            constant,offsets=plan.constant(r,z)
            actual_load=np.einsum('ijab,jb->ia',plan.stage_A,velocities)+constant
            r1,z1,relax,packets=plan.update(r,z,velocities,offsets)
            np.testing.assert_allclose(np.r_[r1.ravel(),z1],expected,rtol=1e-10,atol=1e-15)
            np.testing.assert_allclose(actual_load,loads,rtol=1e-10,atol=1e-15)
            for stage,(y,u) in enumerate(zip(Y,velocities)):
                inputs=[air.wave.port.T@u]
                for row in unpack(y)[0]:inputs.append(inputs[-1]-12*row[1])
                np.testing.assert_allclose(packets[stage],inputs,rtol=1e-10,atol=1e-15)
            heat=dt*(relax+np.mean(air.wave.radiation_batch(packets)))
            closure=air.energy((r1,z1))-air.energy((r,z))+heat-dt/2*float(np.sum(velocities*loads))
            self.assertLess(abs(closure)/air.energy((r,z)),1e-10)
            self.assertGreaterEqual(heat,0.)
            for x,y in zip((r,z,velocities),before):np.testing.assert_array_equal(x,y)

    def test_equal_stage_inputs_recover_measured_constant_plan(self):
        bank=LinearBank(4,8);air=bank.air
        rng=np.random.default_rng(92);r=rng.normal(size=bank.shape)*1e-5
        z=rng.normal(size=bank.nz)*1e-5;u=rng.normal(size=air.modes)*1e-4
        dt=1/32000
        old=GaussAirPlan(air,dt,bank.pilots['split'].packed_residues)
        new=StagedGaussAirPlan(air,dt,old.residues)
        c,o=old.constant(r,z);cs,os=new.constant(r,z)
        loads=np.einsum('ijab,jb->ia',new.stage_A,np.array([u,u]))+cs
        np.testing.assert_allclose(loads.mean(axis=0),old.A@u+c,rtol=1e-12,atol=1e-15)
        for x,y in zip(old.update(r,z,u,o),new.update(r,z,np.array([u,u]),os)):
            np.testing.assert_allclose(x,y,rtol=1e-12,atol=1e-15)
        with self.assertRaises(ValueError):new.update(r,z,np.array([u,u]).astype(complex),os)


class StagedPortTests(parent.PortAVFTests):
    def fixture(self):
        original,state=super().fixture()
        return StagedPortCoast(original.system,original.air,original.packed_residues),state

    def test_full_energy_loss_and_port_displacement_identity(self):
        pilot,state=self.fixture();energy=pilot.energy(state)['total_j'];heat=0.
        for rate in [16000,32000,64000,128000]:
            before=tuple(x.copy() for x in state)
            full,stats=pilot.step(state,1/rate)
            deferred,dstats=pilot.step(state,1/rate,True)
            for x,y in zip(state,before):np.testing.assert_array_equal(x,y)
            for x,y in zip(full,deferred):np.testing.assert_array_equal(x,y)
            self.assertEqual(stats['distinct_port_forces'],2)
            self.assertEqual(dstats['wave_inputs_stages'].shape[0],2)
            extra=np.mean(pilot.air.wave.radiation_batch(dstats['wave_inputs_stages']))/rate
            self.assertAlmostEqual(stats['radiation_loss_j'],dstats['radiation_loss_j']+extra,places=16)
            self.assertGreaterEqual(stats['material_loss_j'],0.)
            self.assertGreaterEqual(stats['radiation_loss_j'],0.)
            state=full;heat+=stats['loss_j']
            self.assertLess(abs(pilot.energy(state)['total_j']+heat-energy)/energy,1e-9)


if __name__=='__main__':unittest.main()
