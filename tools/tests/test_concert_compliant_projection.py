import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from scipy.linalg import block_diag
from concert_grand_compliant_projection import build,displacement,mobility


class CompliantProjectionTests(unittest.TestCase):
    def fixture(self):
        f=np.geomspace(80,18000,97);B=np.random.default_rng(34).normal(size=(len(f),3))
        return f,B,build(f,B,2200,shifts=[0,2100,4000,6000])

    def test_congruence_preserves_mass_stiffness_dense_loss_and_ports(self):
        f,B,r=self.fixture();Q=r['projection'];n=r['low_count'];w=2*np.pi*f
        np.testing.assert_allclose(Q.T@Q,block_diag(np.eye(n),r['tail_mass']),atol=1e-14)
        K=block_diag(np.diag((2*np.pi*f[:n])**2),r['tail_stiffness'])
        D=block_diag(np.diag(.046*2*np.pi*f[:n]),r['tail_damping'])
        np.testing.assert_allclose(Q.T@(w[:,None]**2*Q),K,rtol=1e-14,atol=1e-5)
        np.testing.assert_allclose(Q.T@(.046*w[:,None]*Q),D,rtol=1e-14,atol=1e-11)
        self.assertGreater(np.linalg.eigvalsh(D).min(),0)
        self.assertGreater(np.linalg.eigvalsh(K).min(),0)
        self.assertGreater(np.linalg.norm(r['tail_damping']-np.diag(np.diag(r['tail_damping']))),1)
        for test_f in [130.,2600.,5500.]:
            s=2j*np.pi*test_f;actual=displacement(r,[test_f],B)[0]
            expected=np.linalg.solve(Q.T@Q*s*s+D*s+K,Q.T@B)
            np.testing.assert_allclose(actual,expected,rtol=1e-11,atol=1e-17)

    def test_static_compliance_and_shifted_forced_state_match_full_system(self):
        f,B,r=self.fixture();w=2*np.pi*f
        for hz in r['shifts']:
            s=2j*np.pi*hz
            full=B/(w[:,None]**2+s*.046*w[:,None]+s*s)
            actual=r['projection']@displacement(r,[hz],B)[0]
            np.testing.assert_allclose(actual,full,rtol=1e-9,atol=1e-15)

    def test_passive_reciprocal_mechanical_ports_and_power_balance(self):
        _,B,r=self.fixture();f=np.geomspace(30,25000,71)
        Y=mobility(r,f,B)
        np.testing.assert_allclose(Y,Y.transpose(0,2,1),rtol=1e-10,atol=1e-15)
        self.assertGreaterEqual(np.linalg.eigvalsh((Y+Y.conj().transpose(0,2,1))/2).min(),-1e-13)
        force=np.array([1+2j,-.7+.2j,.4-1j]);hz=5300.;s=2j*np.pi*hz
        v=s*(displacement(r,[hz],B)[0]@force);n=r['low_count']
        D=block_diag(np.diag(r['low_damping']),r['tail_damping'])
        incoming=np.vdot(force,(B.T@r['projection'])@v).real
        heat=np.vdot(v,D@v).real
        self.assertAlmostEqual(incoming,heat,places=13)

    def test_invalid_or_unexcited_high_sector_is_rejected(self):
        with self.assertRaises(ValueError):build([80,200],np.ones((2,1)),500)
        with self.assertRaises(ValueError):build([80,200],np.zeros((2,1)),100)
        with self.assertRaises(ValueError):build([200,80],np.ones((2,1)),100)

if __name__=='__main__':unittest.main()
