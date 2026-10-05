"""Exact linear phases, nonlinear Hamiltonian, live dissipative air and ODE."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
sys.path.insert(0,str(Path(__file__).resolve().parent))
import numpy as np
from scipy.linalg import eigh
from scipy.integrate import solve_ivp
import test_concert_bounded_contact as reference
from concert_grand_exponential_coast import ExponentialCoast


class ExponentialCoastTests(unittest.TestCase):
    def fixture(self):
        old=reference.BoundedContactTests().fixture()
        pilot=ExponentialCoast(old.system,old.air)
        rng=np.random.default_rng(20)
        state=old.initial_state(0.)
        state[0][:-1]=rng.normal(size=old.n-1)*1e-7
        state[0][-1]=-.001;state[1][:-1]=rng.normal(size=old.n-1)*.005;state[1][-1]=-.1
        state[2][:]=np.linspace(-.0002,.0003,state[2].size).reshape(state[2].shape)
        state[3][:]=[.01,-.02,.03]
        return old,pilot,state

    def test_remainder_gradient_and_discrete_potential_identity(self):
        old,pilot,state=self.fixture();q=state[0][:-1];v=state[1][:-1]
        full=old.system.string.gradient_hessian(q,False)
        np.testing.assert_allclose(pilot.nonlinear_gradient(q)+pilot.K@q,full,rtol=2e-11,atol=1e-11)
        b=q+1e-6*v
        U=lambda x:old.system.string.potential(x)-.5*float(x@pilot.K@x)
        self.assertLess(abs(U(b)-U(q)-pilot.nonlinear_avf(q,b)@(b-q)),1e-15)

    def test_exact_linear_flow_with_steps_larger_than_fast_period(self):
        old,pilot,state=self.fixture();core=old.system.string;core.nonlinear=False
        values,U=eigh(pilot.K);omega=np.sqrt(values);q0=state[0][:-1];v0=state[1][:-1]
        q,v=state[:2]
        for _ in range(17):q,v,_=pilot.mechanical_step(q,v,1/8000)
        h=17/8000;a=U.T@q0;p=U.T@v0
        expected_q=U@(np.cos(omega*h)*a+np.sin(omega*h)/omega*p)
        expected_v=U@(-omega*np.sin(omega*h)*a+np.cos(omega*h)*p)
        np.testing.assert_allclose(q[:-1],expected_q,rtol=2e-10,atol=1e-14)
        np.testing.assert_allclose(v[:-1],expected_v,rtol=2e-10,atol=1e-12)

    def test_nonlinear_mechanical_energy_is_not_linearized(self):
        old,pilot,state=self.fixture();q,v=state[:2];initial=old.system.energy(q,v)
        for _ in range(200):
            q,v,stats=pilot.mechanical_step(q,v,1/32000)
            self.assertLess(abs(old.system.energy(q,v)-initial)/initial,1e-9)

    def test_live_air_and_total_independent_loss_ledger(self):
        old,pilot,state=self.fixture();initial=pilot.energy(state);loss=work=radiation=0.
        for _ in range(180):
            state,stats=pilot.step(state,1/32000)
            loss+=stats['loss_j'];work+=stats['air_port_work_j'];radiation+=stats['radiation_loss_j']
            final=pilot.energy(state)
            self.assertLess(abs(final['total_j']+loss-initial['total_j'])/initial['total_j'],1e-9)
            self.assertLess(abs(final['air_j']+radiation-work-initial['air_j'])/initial['total_j'],1e-9)
            self.assertGreaterEqual(stats['material_loss_j'],0.)
            self.assertGreaterEqual(stats['radiation_loss_j'],0.)

    def test_second_order_state_convergence_against_independent_ode(self):
        old,pilot,initial=self.fixture();shapes=[a.shape for a in initial];sizes=[a.size for a in initial];cuts=np.cumsum([0]+sizes)
        pack=lambda s:np.concatenate([a.ravel() for a in s])
        unpack=lambda y:tuple(y[cuts[i]:cuts[i+1]].reshape(shapes[i]) for i in range(4))
        ref=solve_ivp(lambda t,y:pack(old.derivative(unpack(y))),(0,.0005),pack(initial),method='DOP853',rtol=1e-11,atol=1e-15,max_step=1/1000000)
        self.assertTrue(ref.success);errors=[]
        for rate in [64000,128000,256000]:
            state=tuple(a.copy() for a in initial)
            for _ in range(round(rate*.0005)):state,_=pilot.step(state,1/rate,True)
            errors.append(np.linalg.norm(pack(state)-ref.y[:,-1])/np.linalg.norm(ref.y[:,-1]))
        self.assertTrue(all(errors[i+1]<.3*errors[i] for i in [0,1]),errors)
        self.assertLess(errors[-1],.001)

    def test_contact_and_invalid_steps_are_rejected(self):
        _,pilot,state=self.fixture()
        with self.assertRaises(ValueError):pilot.step(state,0.)
        state[0][-1]=1.
        with self.assertRaises(ValueError):pilot.step(state,1/64000)


if __name__=='__main__':unittest.main()
