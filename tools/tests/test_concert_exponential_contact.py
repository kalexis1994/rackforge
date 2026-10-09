import sys,unittest
from pathlib import Path
import numpy as np
from scipy.integrate import solve_ivp
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_shared_strings import WirePort,SharedStrings,SharedHammers
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_exponential_contact import ExponentialContact
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
from concert_grand_rational_air import RationalAir
from concert_grand_pade_air import PadeAir,PadeBoundedAir


class ContactTests(unittest.TestCase):
    def fixture(self,rate_loss=.1):
        ports=[WirePort(60+i//3,Wire(.62/(1+i*.04),.0081,850+i),[1.7,-.9,.4]) for i in range(6)]
        core=BatchedStrings(SharedStrings(ports,[110.,270.,680.],np.diag([.01,.02,.03]),4,8))
        system=SharedHammers(core,Felt(.002,7.5e8,2.6,.125,.02,rate_loss))
        rng=np.random.default_rng(42);part=rng.normal(size=(7,3,3))*.001
        part-=part.mean(axis=0);H=np.empty((7,6,3));H[:,::2]=part;H[:,1::2]=-part[::-1]
        wave=PadeAir(RationalAir(H,128000))
        air=PadeBoundedAir(wave,wave.static_mass()+np.eye(3)*.001,np.eye(3))
        return system,ExponentialContact(system,air)

    def test_unequal_unison_and_two_heads_close_mechanical_work(self):
        for loss in [0.,.1]:
            system,pilot=self.fixture(loss);state=pilot.initial_state(0.)
            state[0][:system.string.size]=np.random.default_rng(9).normal(size=system.string.size)*1e-8
            state,_=system.launch(state,60,3.);state,_=system.launch(state,61,2.)
            q,v=state[:2];energy=system.energy(q,v);heat=0.
            for _ in range(160):
                q,v,stats=pilot.mechanical_step(q,v,1/64000)
                heat+=stats['felt_loss_j']
                self.assertGreaterEqual(stats['felt_loss_j'],-1e-18)
            self.assertLess(abs(system.energy(q,v)+heat-energy)/energy,1e-9)

    def test_coupled_live_air_and_restrike_close_all_losses(self):
        system,pilot=self.fixture();state=pilot.initial_state(0.)
        state,work=system.launch(state,60,3.)
        initial=pilot.energy(state)['total_j'];heat=0.
        for _ in range(160):
            state,stats=pilot.step(state,1/64000)
            heat+=stats['loss_j']
        self.assertLess(abs(pilot.energy(state)['total_j']+heat-initial)/initial,1e-9)
        before=tuple(x.copy() for x in state)
        state,work=system.launch(state,60,2.)
        for i in [0,1]:np.testing.assert_array_equal(state[i][:system.string.size],before[i][:system.string.size])
        for i in [2,3]:np.testing.assert_array_equal(state[i],before[i])
        initial=pilot.energy(state)['total_j'];heat=0.
        for _ in range(160):
            state,stats=pilot.step(state,1/64000);heat+=stats['loss_j']
        self.assertLess(abs(pilot.energy(state)['total_j']+heat-initial)/initial,1e-9)

    def test_contact_converges_to_independent_continuous_ode(self):
        system,pilot=self.fixture();state=pilot.initial_state(0.)
        state,_=system.launch(state,60,3.);q,v=state[:2];n=system.size;duration=.002
        def derivative(_t,y):
            a,p=y[:n],y[n:];g=np.r_[system.string.gradient_hessian(a[:system.string.size],False),np.zeros(system.nh)]
            for _,d,k in system.contacts:
                compression=max(float(d@a),0.)
                force=k*compression**system.felt.exponent*np.exp(np.clip(system.felt.rate_damping_s_per_m*(d@p),-40,40))
                g+=d*force
            return np.r_[p,-g]
        ode=solve_ivp(derivative,(0,duration),np.r_[q,v],method='DOP853',rtol=1e-11,atol=1e-14,max_step=1/1536000)
        self.assertTrue(ode.success)
        omega=np.sqrt(pilot.oscillator_functions(1/128000)[3]);nc=system.string.size
        def metric(a,b):return np.r_[omega*pilot.project(a[:nc]),b[:nc],a[nc:],b[nc:]]
        reference=metric(ode.y[:n,-1],ode.y[n:,-1]);errors=[]
        for rate in [64000,128000,256000]:
            a,b=q.copy(),v.copy()
            for _ in range(round(duration*rate)):a,b,_=pilot.mechanical_step(a,b,1/rate)
            errors.append(np.linalg.norm(metric(a,b)-reference)/np.linalg.norm(reference))
        self.assertTrue(all(errors[i]>3*errors[i+1] for i in [0,1]),errors)
        self.assertLess(errors[-1],1e-4)


if __name__=='__main__':unittest.main()
