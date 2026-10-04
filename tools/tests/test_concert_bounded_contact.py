"""Independent physical ODE, nonlinear ledgers, decoupling and live restrike."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from scipy.integrate import solve_ivp
from scipy.linalg import cho_factor,cho_solve
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
from concert_grand_moving_bridge import MovingBridge,Support,BridgeHammer
from concert_grand_rational_air import RationalAir
from concert_grand_bounded_air import BoundedAir
from concert_grand_bounded_contact import BoundedContact


class BoundedContactTests(unittest.TestCase):
    def fixture(self,rate_loss=0.,coupling=1.):
        core=MovingBridge(Wire(.62,.0081,850),Support((110.,270.,680.),(1.7,-.9,.4),(.03,.06,.04)),transverse=3)
        system=BridgeHammer(core,Felt(.002,7.5e8,2.6,.125,.02,rate_loss))
        rng=np.random.default_rng(415);H=rng.normal(size=(5,3,3));H[-1]=-H[:-1].sum(axis=0)
        H=np.concatenate((H,-H[::-1]),axis=1);filtered=np.zeros((len(H)+1,)+H.shape[1:]);filtered[:-1]+=H/2;filtered[1:]+=H/2
        wave=RationalAir(filtered,16000)
        air=BoundedAir(H,16000,wave.static_mass()+np.array([[.04,.007,-.002],[.007,.03,.003],[-.002,.003,.02]]),np.diag([20.,25.,30.]))
        return BoundedContact(system,air,coupling)

    def test_nonlinear_dynamics_against_independent_physical_ode(self):
        coupled=self.fixture(coupling=.7);system=coupled.system;core=system.string;air=coupled.air;n=coupled.n
        transform=np.eye(n);transform[:-1,:-1]=core.transform;inverse=np.linalg.inv(transform)
        physical_mass=np.eye(n);physical_mass[:-1,:-1]=core.mass;factor=cho_factor(physical_mass)
        damping=np.zeros((n,n));damping[core.sb,core.sb]=np.diag(core.viscous)
        initial=coupled.initial_state(5.);wave_count=air.wave.states
        def ode(t,state):
            position,velocity=state[:n],state[n:2*n];r=state[2*n:2*n+wave_count].reshape(air.wave.zero_state().shape);z=state[2*n+wave_count:]
            gradient,_=system.conservative_force(inverse@position,inverse@velocity)
            boundary=coupled.coupling*velocity[core.sb];u=boundary.copy();load=air.wave.D@u;rd=[]
            for row,residue in zip(r,air.wave.R[1:]):
                rd.append(air.wave.a*(u-row));u=2*row-u;load+=residue@u
            difference=air.relaxation.B.T@boundary-z;load+=air.relaxation.B@difference
            force=inverse.T@gradient+damping@velocity;force[core.sb]+=coupled.coupling*load
            return np.r_[velocity,cho_solve(factor,-force),np.array(rd).ravel(),difference/air.relaxation.tau]
        start=np.r_[transform@initial[0],transform@initial[1],initial[2].ravel(),initial[3]]
        reference=solve_ivp(ode,(0,.0015),start,method='DOP853',rtol=1e-11,atol=1e-15,
            max_step=1/(10*core.nl*np.sqrt(core.wire.young/core.wire.density)/(2*core.wire.length)))
        self.assertTrue(reference.success);q=inverse@reference.y[:n,-1];v=inverse@reference.y[n:2*n,-1];errors=[]
        for rate in [384000,768000,1536000]:
            state=coupled.initial_state(5.)
            for _ in range(round(.0015*rate)):state,_=coupled.step(state,1/rate)
            errors.append(max(np.linalg.norm(state[0]-q)/np.linalg.norm(q),np.linalg.norm(state[1]-v)/np.linalg.norm(v)))
        self.assertLess(errors[-1],.002);self.assertTrue(all(errors[i+1]<.3*errors[i] for i in [0,1]),errors)
        interface=coupled.interface(state)
        self.assertLess(interface['force_residual'],1e-10);self.assertLess(abs(interface['power_residual_w']),1e-10)

    def test_felt_material_air_and_board_ledgers_with_live_states(self):
        coupled=self.fixture(rate_loss=.1);state=coupled.initial_state(5.)
        state[2][:]=np.linspace(-.0002,.0003,state[2].size).reshape(state[2].shape);state[3][:]=[.01,-.02,.03]
        initial=coupled.energy(state);board0=coupled.system.string.components(state[0][:-1],state[1][:-1])['support_energy_j']
        lost=port=radiation=work=material=0.
        for _ in range(1200):
            state,stats=coupled.step(state,1/768000);lost+=stats['loss_j'];port+=stats['air_port_work_j'];radiation+=stats['radiation_loss_j']
            work+=stats['work_into_board_j'];material+=stats['material_loss_j']
            for key in ['felt_loss_j','material_loss_j','radiation_loss_j']:self.assertGreaterEqual(stats[key],-1e-18)
            self.assertLess(abs(coupled.energy(state)['total_j']+lost-initial['total_j'])/initial['total_j'],1e-9)
        final=coupled.energy(state);board=coupled.system.string.components(state[0][:-1],state[1][:-1])['support_energy_j']
        self.assertLess(abs(final['air_j']-initial['air_j']+radiation-port)/initial['total_j'],1e-9)
        self.assertLess(abs(board-board0+material+port-work)/initial['total_j'],1e-9)

    def test_decoupling_recovers_original_contact(self):
        coupled=self.fixture(coupling=0.);state=coupled.initial_state(5.);q,v=coupled.system.initial_state(5.)
        for _ in range(500):state,_=coupled.step(state,1/768000);q,v,_=coupled.system.step(q,v,1/768000)
        np.testing.assert_allclose(state[0],q,rtol=1e-10,atol=1e-15);np.testing.assert_allclose(state[1],v,rtol=1e-10,atol=1e-13)
        np.testing.assert_array_equal(state[2],0.);np.testing.assert_array_equal(state[3],0.)

    def test_batched_observer_preserves_live_dynamics_and_radiation(self):
        coupled=self.fixture();scalar=coupled.initial_state(5.)
        scalar[2][:]=np.linspace(-.0002,.0003,scalar[2].size).reshape(scalar[2].shape)
        scalar[3][:]=[.01,-.02,.03];batched=tuple(s.copy() for s in scalar)
        inputs=[];expected=[];relaxation=[]
        for _ in range(120):
            scalar,a=coupled.step(scalar,1/768000)
            batched,b=coupled.step(batched,1/768000,defer_radiation=True)
            for s,t in zip(scalar,batched):np.testing.assert_array_equal(s,t)
            inputs.append(b['wave_inputs']);expected.append(a['radiation_loss_j']);relaxation.append(b['radiation_loss_j'])
        measured=coupled.air.wave.radiation_batch(np.array(inputs))/768000+relaxation
        np.testing.assert_allclose(measured,expected,rtol=2e-13,atol=1e-22)

    def test_relaunch_preserves_living_coordinates_and_records_external_work(self):
        coupled=self.fixture();state=coupled.initial_state(5.)
        state[0][:-1]=np.linspace(-1e-6,1e-6,coupled.n-1)
        state[0][-1]=-float(coupled.system.direction[:-1]@state[0][:-1])/coupled.system.direction[-1]-.001
        state[1][:-1]=np.linspace(-.01,.02,coupled.n-1);state[1][-1]=-.1
        state[2][:]=np.linspace(-.0002,.0003,state[2].size).reshape(state[2].shape);state[3][:]=[.01,-.02,.03]
        updated,work=coupled.relaunch(state,4.)
        np.testing.assert_array_equal(updated[0][:-1],state[0][:-1]);np.testing.assert_array_equal(updated[1][:-1],state[1][:-1])
        for a,b in zip(updated[2:],state[2:]):np.testing.assert_array_equal(a,b)
        self.assertAlmostEqual(coupled.system.compression(updated[0]),0.,places=14)
        self.assertAlmostEqual(coupled.energy(updated)['total_j']-coupled.energy(state)['total_j'],work,places=14)
        state[0][-1]=.001
        with self.assertRaises(ValueError):coupled.relaunch(state,5.)
        with self.assertRaises(ValueError):coupled.step(state,0.)
        with self.assertRaises(ValueError):BoundedContact(coupled.system,coupled.air,float('nan'))


if __name__=='__main__':unittest.main()
