import sys,unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_gauss_air import GaussAirPlan
from concert_grand_gauss_response import harmonic_impedance
from concert_grand_port_gauss import PortGaussCoast
from concert_grand_linear_phase import LinearBank
import test_concert_port_avf_coast as parent


class GaussAirTests(unittest.TestCase):
    def test_harmonic_resolvent_matches_discrete_living_gauss_states(self):
        bank=LinearBank(4,8);air=bank.air;na=bank.nr+bank.nz
        def Z(s):
            b=air.relaxation.B;t=air.relaxation.tau
            return air.wave.impedance(s)+(b*(s*t/(1+s*t)))@b.T
        for rate in [32000,128000,512000]:
            dt=1/rate;plan=GaussAirPlan(air,dt,bank.pilots['split'].packed_residues)
            def discrete(a,u):
                r=a[:bank.nr].reshape(bank.shape);z=a[bank.nr:]
                constant,offsets=plan.constant(r,z)
                r1,z1,_,_=plan.update(r,z,u,offsets)
                return np.r_[r1.ravel(),z1],constant+plan.A@u
            columns=[discrete(e,np.zeros(air.modes)) for e in np.eye(na)]
            T=np.column_stack([x[0] for x in columns]);F=np.column_stack([x[1] for x in columns])
            columns=[discrete(np.zeros(na),u) for u in np.eye(air.modes)]
            U=np.column_stack([x[0] for x in columns]);D=np.column_stack([x[1] for x in columns])
            for hz in [1000.,6000.,12000.]:
                omega=2*np.pi*hz;theta=omega*dt
                old=np.linalg.solve(np.exp(1j*theta)*np.eye(na)-T,U*np.exp(1j*theta/2))
                actual=D+F@old*np.exp(-1j*theta/2)
                np.testing.assert_allclose(actual,harmonic_impedance(Z,omega,dt),rtol=1e-9,atol=1e-12)

    def test_complex_cascade_equals_real_two_stage_air_with_living_memory(self):
        bank=LinearBank(4,8);air=bank.air;na=bank.nr+bank.nz
        def unpack(a):return a[:bank.nr].reshape(bank.shape),a[bank.nr:]
        def rhs(a,u):
            derivative,force=air.derivative(unpack(a),u)
            return np.r_[derivative[0].ravel(),derivative[1]],force
        columns=[rhs(e,np.zeros(air.modes)) for e in np.eye(na)]
        L=np.column_stack([x[0] for x in columns])
        U=np.column_stack([rhs(np.zeros(na),u)[0] for u in np.eye(air.modes)])
        g=np.sqrt(3)/6;A=np.array([[.25,.25-g],[.25+g,.25]])
        rng=np.random.default_rng(62);a=rng.normal(size=na)*1e-5;u=rng.normal(size=air.modes)*1e-4
        for rate in [16000,32000,128000,512000]:
            dt=1/rate;plan=GaussAirPlan(air,dt,bank.pilots['split'].packed_residues)
            right=np.tile(a,2)+dt*np.kron(A,np.eye(na))@np.tile(U@u,2)
            Y=np.linalg.solve(np.eye(2*na)-dt*np.kron(A,L),right).reshape(2,na)
            derivatives=[rhs(y,u) for y in Y]
            expected=a+dt/2*(derivatives[0][0]+derivatives[1][0])
            load=(derivatives[0][1]+derivatives[1][1])/2
            r,z=unpack(a);before=(r.copy(),z.copy())
            constant,offsets=plan.constant(r,z);r1,z1,relax,packets=plan.update(r,z,u,offsets)
            np.testing.assert_allclose(np.r_[r1.ravel(),z1],expected,rtol=1e-10,atol=1e-15)
            np.testing.assert_allclose(plan.A@u+constant,load,rtol=1e-10,atol=1e-15)
            for stage,y in enumerate(Y):
                inputs=[air.wave.port.T@u]
                for row in unpack(y)[0]:inputs.append(inputs[-1]-12*row[1])
                np.testing.assert_allclose(packets[stage],inputs,rtol=1e-10,atol=1e-15)
            heat=dt*(relax+np.mean(air.wave.radiation_batch(packets)))
            closure=air.energy((r1,z1))-air.energy((r,z))+heat-dt*float(u@load)
            self.assertLess(abs(closure)/air.energy((r,z)),1e-10)
            self.assertGreaterEqual(heat,0.)
            np.testing.assert_array_equal(r,before[0]);np.testing.assert_array_equal(z,before[1])


class GaussPortTests(parent.PortAVFTests):
    def fixture(self):
        original,state=super().fixture()
        return PortGaussCoast(original.system,original.air,original.packed_residues),state

    def test_full_energy_loss_and_port_displacement_identity(self):
        pilot,state=self.fixture();energy=pilot.energy(state)['total_j'];heat=0.
        for rate in [16000,32000,64000,128000]:
            before=tuple(x.copy() for x in state)
            full,stats=pilot.step(state,1/rate)
            deferred,dstats=pilot.step(state,1/rate,True)
            for a,b in zip(state,before):np.testing.assert_array_equal(a,b)
            for a,b in zip(full,deferred):np.testing.assert_array_equal(a,b)
            self.assertEqual(dstats['wave_inputs_stages'].shape[0],2)
            extra=np.mean(pilot.air.wave.radiation_batch(dstats['wave_inputs_stages']))/rate
            self.assertAlmostEqual(stats['radiation_loss_j'],dstats['radiation_loss_j']+extra,places=16)
            self.assertGreaterEqual(stats['material_loss_j'],0.)
            self.assertGreaterEqual(stats['radiation_loss_j'],0.)
            state=full;heat+=stats['loss_j']
            self.assertLess(abs(pilot.energy(state)['total_j']+heat-energy)/energy,1e-9)


if __name__=='__main__':unittest.main()
