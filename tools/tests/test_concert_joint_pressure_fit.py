import unittest
import importlib.util
from pathlib import Path
import numpy as np
from scipy.linalg import null_space,solve
from concert_grand_joint_pressure_fit import objective_gradient,responses,readout_transfer,balanced_solve


class JointPressureFitTests(unittest.TestCase):
    def test_split_stiffness_modes_keep_pressure_and_mobility(self):
        spec = importlib.util.spec_from_file_location('joint_producer',Path(__file__).resolve().parents[1]/'fit-concert-grand-joint-pressure.py')
        producer = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(producer)
        omega = np.array([130.,430.,920.,4e7])
        k = np.diag(omega**2)
        k[0,1] = k[1,0] = 2000.
        c = np.diag(omega*.046)+np.ones((4,4))*2
        ports = np.random.default_rng(2).normal(size=(4,2))
        readout = np.random.default_rng(3).normal(size=(2,2,4))
        delay = np.array([[.001,.002],[.0015,.003]])
        f = np.array([23.,97.,611.,5970.])
        _,_,y,p = responses(k,c,omega,ports,f,readout_transfer(f,delay,readout))
        new_omega,v = producer.split_stiffness_modes(k,omega)
        new_readout = np.einsum('mgn,nj->mgj',readout,v)
        _,_,new_y,new_p = responses(np.diag(new_omega**2),v.T @ c @ v,new_omega,v.T @ ports,
                                   f,readout_transfer(f,delay,new_readout))
        np.testing.assert_allclose(new_y,y,rtol=1e-11,atol=1e-16)
        np.testing.assert_allclose(new_p,p,rtol=1e-11,atol=1e-16)
        self.assertEqual(new_omega[-1],omega[-1])

    def test_joint_gradient_with_complex_causal_observation(self):
        rng = np.random.default_rng(47)
        omega = np.array([130.,420.,810.,1450.])
        ports = rng.normal(size=(4,2))
        base = np.diag([5.,10.,20.,35.])+np.ones((4,4))*2
        basis = null_space((ports/omega[:,None]).T)
        frequency = np.array([9.,25.,61.,111.,240.])
        observation = readout_transfer(frequency,np.array([[.001,.002],[.0015,.003]]),rng.normal(0,.1,(2,2,4)))
        _,_,y,p = responses(np.diag(omega**2),base,omega,ports,frequency,observation)
        x = rng.normal(0,.1,basis.shape[1]+4+8)
        arguments = (omega,ports,base,basis,frequency,y*1.04,p*.95,observation,2)
        _,gradient,_ = objective_gradient(x,*arguments)
        measured = []
        for i in range(len(x)):
            delta = np.zeros_like(x)
            delta[i] = 1e-6
            plus = objective_gradient(x+delta,*arguments)[0]
            minus = objective_gradient(x-delta,*arguments)[0]
            measured.append((plus-minus)/2e-6)
        np.testing.assert_allclose(gradient,measured,rtol=5e-6,atol=2e-9)

    def test_energy_scaled_solve_matches_original_with_mhz_completion(self):
        omega = np.geomspace(120.,4e7,12)
        k = np.diag(omega**2)
        c = np.diag(omega*.046)
        c += np.ones((12,12))*4
        rhs = np.random.default_rng(2).normal(size=(12,3))
        for hz in [23.,97.,611.,5970.]:
            s = 2j*np.pi*hz
            actual = balanced_solve(k,c,omega,s,rhs)
            expected = solve(k+s*c+s*s*np.eye(12),rhs,assume_a='sym')
            np.testing.assert_allclose(actual,expected,rtol=1e-11,atol=1e-17)


if __name__ == '__main__':
    unittest.main()
