import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]));sys.path.insert(0,str(Path(__file__).resolve().parent))
import numpy as np
from scipy.linalg import expm
from scipy.integrate import solve_ivp
import test_concert_exponential_coast as reference
from concert_grand_damped_coast import StringDamper,DampedCoast


class DampedCoastTests(unittest.TestCase):
    def fixture(self):
        old,_,state=reference.ExponentialCoastTests().fixture()
        return old,DampedCoast(old.system,old.air),state

    def test_distributed_drag_against_full_matrix_exponential_and_heat(self):
        old,pilot,state=self.fixture();drag=pilot.damper;v=state[1]
        actual,heat=drag.dissipate(v,.00037,.63)
        expected=expm(-.00037*.63*(drag.rows.T@drag.rows))@v
        np.testing.assert_allclose(actual,expected,rtol=2e-13,atol=1e-15)
        self.assertGreater(heat,0)
        self.assertAlmostEqual(heat,.5*(v@v-actual@actual),places=16)

    def test_zero_engagement_exactly_preserves_control(self):
        old,pilot,state=self.fixture()
        from concert_grand_polynomial_coast import PolynomialCoast
        ref,_=PolynomialCoast(old.system,old.air).step(state,1/64000,True)
        actual,stats=pilot.release_step(state,1/64000,0,True)
        for a,b in zip(actual,ref):np.testing.assert_array_equal(a,b)
        self.assertEqual(stats['damper_heat_j'],0)

    def test_independent_full_energy_ledger_live_air(self):
        _,pilot,state=self.fixture();initial=pilot.energy(state)['total_j'];loss=heat=0
        initial_wave=state[2].copy()
        for _ in range(100):
            state,s=pilot.release_step(state,1/64000,.8)
            loss+=s['loss_j'];heat+=s['damper_heat_j']
        self.assertGreater(heat,0)
        self.assertGreater(np.linalg.norm(state[2]),0)
        self.assertFalse(np.array_equal(state[2],initial_wave))
        self.assertLess(abs(pilot.energy(state)['total_j']+loss-initial)/initial,1e-9)

    def test_second_order_against_independent_damped_ode(self):
        old,pilot,initial=self.fixture();shape=[x.shape for x in initial];cuts=np.cumsum([0]+[x.size for x in initial])
        pack=lambda s:np.concatenate([x.ravel() for x in s])
        unpack=lambda y:tuple(y[cuts[i]:cuts[i+1]].reshape(shape[i]) for i in range(4))
        D=pilot.damper.rows.T@pilot.damper.rows
        def derivative(t,y):
            s=unpack(y);ds=list(old.derivative(s));ds[1]-=.8*D@s[1];return pack(ds)
        ref=solve_ivp(derivative,(0,.0005),pack(initial),method='DOP853',rtol=1e-11,atol=1e-15,max_step=1/1000000)
        self.assertTrue(ref.success);errors=[]
        for rate in [64000,128000,256000]:
            state=tuple(x.copy() for x in initial)
            for _ in range(round(rate*.0005)):state,_=pilot.release_step(state,1/rate,.8,True)
            errors.append(np.linalg.norm(pack(state)-ref.y[:,-1])/np.linalg.norm(ref.y[:,-1]))
        self.assertTrue(all(errors[i+1]<.3*errors[i] for i in [0,1]),errors)

if __name__=='__main__':unittest.main()
