"""Physical mixed-mass mapping, independent ODE and nonlinear work tests."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from scipy.integrate import solve_ivp
from scipy.linalg import cho_factor,cho_solve
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
from concert_grand_moving_bridge import MovingBridge,Support,BridgeHammer
from concert_grand_acoustic_bath import AcousticBath
from concert_grand_bath_contact import BathContact


class ContactBathTests(unittest.TestCase):
    def fixture(self,rate_loss=0.,coupling=1.):
        core=MovingBridge(Wire(.62,.0081,850),Support((110.,270.,680.),(1.7,-.9,.4),(.03,.06,.04)),transverse=3)
        system=BridgeHammer(core,Felt(.002,7.5e8,2.6,.125,.02,rate_loss))
        bath=AcousticBath(2*np.pi*np.array([80.,350.,850.]),2*np.pi*.5,
            [[30.,8.,-4.],[-12.,25.,6.],[9.,-8.,32.]],[[.05,.007,-.002],[.007,.03,.003],[-.002,.003,.02]])
        return BathContact(system,bath,coupling)

    def test_tail_mass_in_physical_mixed_coordinates(self):
        coupled=self.fixture();core=coupled.system.string;n=coupled.n
        transform=np.eye(n);transform[:-1,:-1]=core.transform
        physical_mass=np.eye(n);physical_mass[:-1,:-1]=core.mass
        physical_mass[core.sb,core.sb]+=coupled.bath.tail_mass
        np.testing.assert_allclose(coupled.M,transform.T@physical_mass@transform,atol=1e-14)
        self.assertGreater(np.linalg.eigvalsh(coupled.M).min(),0.)
        rng=np.random.default_rng(8);velocity=rng.normal(size=n)*.03
        z=transform@velocity;board=z[core.sb]
        extra=.5*board@coupled.bath.tail_mass@board
        self.assertAlmostEqual(.5*velocity@coupled.M@velocity,.5*velocity@velocity+extra,places=14)

    def test_nonlinear_dynamics_against_independent_physical_ode(self):
        coupled=self.fixture();system=coupled.system;core=system.string;bath=coupled.bath;n=coupled.n;m=bath.ports
        transform=np.eye(n);transform[:-1,:-1]=core.transform;inverse=np.linalg.inv(transform)
        physical_mass=np.eye(n);physical_mass[:-1,:-1]=core.mass;physical_mass[core.sb,core.sb]+=bath.tail_mass
        factor=cho_factor(physical_mass);damping=np.zeros((n,n));damping[core.sb,core.sb]=np.diag(core.viscous)
        initial=coupled.initial_state(5.);z=transform@initial[0];velocity=transform@initial[1]
        def ode(t,state):
            position,velocity=state[:n],state[n:2*n];a=state[2*n:2*n+m];p=state[2*n+m:]
            gradient,_=system.conservative_force(inverse@position,inverse@velocity)
            force=inverse.T@gradient+damping@velocity;force[core.sb]+=bath.projection.T@p
            acceleration=cho_solve(factor,-force)
            return np.r_[velocity,acceleration,p,bath.projection@velocity[core.sb]-bath.omega**2*a-2*bath.gamma*p]
        reference=solve_ivp(ode,(0,.0015),np.r_[z,velocity,initial[2],initial[3]],method='DOP853',rtol=1e-11,atol=1e-15,
            max_step=1/(10*core.nl*np.sqrt(core.wire.young/core.wire.density)/(2*core.wire.length)))
        self.assertTrue(reference.success)
        q=inverse@reference.y[:n,-1];v=inverse@reference.y[n:2*n,-1];errors=[]
        for rate in (384000,768000,1536000):
            state=coupled.initial_state(5.)
            for _ in range(round(.0015*rate)):state,_=coupled.step(state,1/rate)
            error=max(np.linalg.norm(state[0]-q)/np.linalg.norm(q),np.linalg.norm(state[1]-v)/np.linalg.norm(v));errors.append(error)
        self.assertLess(errors[-1],.002)
        self.assertTrue(all(errors[i+1]<.3*errors[i] for i in (0,1)),errors)
        interface=coupled.interface(state)
        self.assertLess(interface['force_residual'],1e-10);self.assertLess(abs(interface['power_residual_w']),1e-10)

    def test_felt_material_air_work_close_shared_energy(self):
        coupled=self.fixture(rate_loss=.1);state=coupled.initial_state(5.)
        state[2][:]=[.0003,-.0002,.0001];initial=coupled.energy(state)['total_j']
        lost=0.;air_initial=coupled.energy(state)['air_j'];air_work=absorption=0.
        board_initial=coupled.system.string.components(state[0][:-1],state[1][:-1])['support_energy_j']
        board_work=material=0.
        for _ in range(1200):
            state,stats=coupled.step(state,1/768000);lost+=stats['loss_j'];air_work+=stats['air_port_work_j'];absorption+=stats['numerical_bath_absorption_j']
            board_work+=stats['work_into_board_j'];material+=stats['material_loss_j']
            for key in ('felt_loss_j','material_loss_j','numerical_bath_absorption_j'):self.assertGreaterEqual(stats[key],-1e-18)
            self.assertLess(abs(coupled.energy(state)['total_j']+lost-initial)/initial,1e-9)
        self.assertLess(abs(coupled.energy(state)['air_j']-air_initial+absorption-air_work)/initial,1e-9)
        board=coupled.system.string.components(state[0][:-1],state[1][:-1])['support_energy_j']
        self.assertLess(abs(board-board_initial+material+air_work-board_work)/initial,1e-9)

    def test_decoupling_recovers_original_contact(self):
        coupled=self.fixture(coupling=0.);state=coupled.initial_state(5.);q,v=coupled.system.initial_state(5.)
        for _ in range(500):
            state,_=coupled.step(state,1/768000);q,v,_=coupled.system.step(q,v,1/768000)
        np.testing.assert_allclose(state[0],q,rtol=1e-10,atol=1e-15)
        np.testing.assert_allclose(state[1],v,rtol=1e-10,atol=1e-13)
        np.testing.assert_array_equal(state[2],0.);np.testing.assert_array_equal(state[3],0.)

    def test_relaunch_preserves_living_string_board_and_air(self):
        coupled=self.fixture();state=coupled.initial_state(5.)
        # Separated action fixture; all living coordinates remain nonzero.
        state[0][:-1]=np.linspace(-1e-6,1e-6,coupled.n-1)
        state[0][-1]=-float(coupled.system.direction[:-1]@state[0][:-1])/coupled.system.direction[-1]-.001
        state[1][:-1]=np.linspace(-.01,.02,coupled.n-1);state[1][-1]=-.1
        state[2][:]=[.0002,-.0001,.0003];state[3][:]=[.01,-.02,.03]
        relaunched,work=coupled.relaunch(state,4.)
        np.testing.assert_array_equal(relaunched[0][:-1],state[0][:-1]);np.testing.assert_array_equal(relaunched[1][:-1],state[1][:-1])
        for a,b in zip(relaunched[2:],state[2:]):np.testing.assert_array_equal(a,b)
        self.assertAlmostEqual(coupled.system.compression(relaunched[0]),0.,places=14)
        self.assertAlmostEqual(coupled.energy(relaunched)['total_j']-coupled.energy(state)['total_j'],work,places=14)

    def test_invalid_mapping_and_contact_reset(self):
        coupled=self.fixture();state=coupled.initial_state(5.);state[0][-1]=.001
        with self.assertRaises(ValueError):coupled.relaunch(state,5.)
        with self.assertRaises(ValueError):coupled.step(state,0.)
        with self.assertRaises(ValueError):BathContact(coupled.system,coupled.bath,float('nan'))


if __name__=='__main__':unittest.main()
