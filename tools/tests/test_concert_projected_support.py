"""Dense losses: coordinate invariance, independent physical ODE and heat."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
sys.path.insert(0,str(Path(__file__).resolve().parent))
import numpy as np
from scipy.linalg import cho_factor,cho_solve
from scipy.integrate import solve_ivp
from concert_grand_moving_bridge import Support,MovingBridge,BridgeHammer
from concert_grand_projected_support import ModalSupportMap
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
from concert_grand_bounded_contact import BoundedContact
from concert_grand_exponential_coast import ExponentialCoast
from concert_grand_damped_coast import DampedCoast
import test_concert_bounded_contact as bounded_fixture
import test_concert_bath_contact as bath_fixture
from concert_grand_bath_contact import BathContact


class ProjectedSupportTests(unittest.TestCase):
    def fixture(self):
        # Strong mutual losses: a diagonal-only heat diagnostic must fail.
        C=np.array([[25.,12.,-5.],[12.,75.,20.],[-5.,20.,180.]])
        support=Support((110.,270.,680.),(1.7,-.9,.4),(0.,0.,0.),tuple(map(tuple,C)))
        core=MovingBridge(Wire(.62,.0081,850),support,transverse=3)
        system=BridgeHammer(core,Felt(.002,7.5e8,2.6,.125,.02,.1))
        return BoundedContact(system,bounded_fixture.BoundedContactTests().fixture().air)

    def test_generalized_coordinates_preserve_ports_heat_and_energy(self):
        M=np.array([[1.2,.15,-.1],[.15,.8,.07],[-.1,.07,1.4]])
        K=np.array([[2.,.4,-.1],[.4,3.,.6],[-.1,.6,6.]])*1e6
        C=np.array([[25.,12.,-5.],[12.,75.,20.],[-5.,20.,180.]])
        B=np.array([[1.7,-.2],[-.9,.4],[.4,1.2]])
        modal=ModalSupportMap.from_matrices(M,K,C);V=modal.basis
        np.testing.assert_allclose(V.T@M@V,np.eye(3),atol=1e-14)
        np.testing.assert_allclose(V.T@K@V,np.diag((2*np.pi*modal.frequency)**2),rtol=1e-13,atol=1e-8)
        for hz in [0.,60.,270.,1100.]:
            s=2j*np.pi*hz;rhs=V.T@B
            q=np.linalg.solve(np.diag((2*np.pi*modal.frequency)**2)+s*modal.damping+s*s*np.eye(3),rhs)
            reference=np.linalg.solve(K+s*C+s*s*M,B)
            np.testing.assert_allclose(V@q,reference,rtol=1e-12,atol=1e-18)
        v=np.array([.2,-.1,.3]);physical=V@v
        self.assertAlmostEqual(v@modal.damping@v,physical@C@physical,places=12)
        self.assertGreater(np.linalg.norm(modal.damping-np.diag(np.diag(modal.damping))),1.)
        np.testing.assert_allclose(modal.support(B[:,0]).projection,V.T@B[:,0])

    def test_nonpassive_asymmetric_complex_or_conflicting_losses_rejected(self):
        for D in [[[1.,2.],[0.,1.]],[[1.,2.],[2.,1.]],np.eye(2)*1j]:
            with self.assertRaises(ValueError):Support((100.,200.),(1.,1.),(0.,0.),D)
            with self.assertRaises(ValueError):ModalSupportMap.from_matrices(np.eye(2),np.eye(2),D)
        with self.assertRaises(ValueError):Support((100.,200.),(1.,1.),(.03,.04),np.eye(2))
        with self.assertRaises((ValueError,np.linalg.LinAlgError)):ModalSupportMap.from_matrices(np.diag([-1.,1.]),np.eye(2),np.eye(2))

    def test_complete_material_force_heat_and_both_air_interfaces(self):
        coupled=self.fixture();system=coupled.system;core=system.string
        rng=np.random.default_rng(819);q,v=system.initial_state(3.)
        q[:-1]=rng.normal(size=core.size)*1e-6;v[:-1]=rng.normal(size=core.size)*.01
        q[-1]=np.sqrt(system.felt.hammer_mass)*.0002
        physical_v=core.physical(v[:-1]);b=physical_v[core.sb]
        expected=float(b@core.board_damping@b)
        self.assertGreater(abs(expected-np.diag(core.board_damping)@(b*b)),1e-5)
        self.assertAlmostEqual(core.components(q[:-1],v[:-1])['support_loss_w'],expected,places=13)
        force,_=system.continuous_force(q,v);interface=system.interface(q,v,-force)
        self.assertLess(interface['force_equation_residual'],1e-11)
        self.assertLess(abs(interface['power_residual_w']),1e-11)
        for pilot in [coupled,BathContact(system,bath_fixture.ContactBathTests().fixture().bath)]:
            state=pilot.initial_state(3.);state[0][:]=q;state[1][:]=v
            state[2][:]=.0002;state[3][:]=.01
            interface=pilot.interface(state)
            self.assertLess(interface['force_residual'],1e-10)
            self.assertLess(abs(interface['power_residual_w']),1e-10)

    def test_dense_loss_contact_against_independent_physical_ode(self):
        pilot=self.fixture();system=pilot.system;core=system.string;air=pilot.air;n=pilot.n
        T=np.eye(n);T[:-1,:-1]=core.transform;inverse=np.linalg.inv(T)
        M=np.eye(n);M[:-1,:-1]=core.mass;factor=cho_factor(M)
        D=np.zeros((n,n));D[core.sb,core.sb]=core.board_damping
        initial=pilot.initial_state(5.);nw=initial[2].size
        def ode(t,y):
            q,v=y[:n],y[n:2*n];r=y[2*n:2*n+nw].reshape(initial[2].shape);z=y[2*n+nw:]
            g,_=system.conservative_force(inverse@q,inverse@v)
            boundary=v[core.sb];u=boundary.copy();load=air.wave.D@u;rd=[]
            for row,residue in zip(r,air.wave.R[1:]):
                rd.append(air.wave.a*(u-row));u=2*row-u;load+=residue@u
            difference=air.relaxation.B.T@boundary-z;load+=air.relaxation.B@difference
            force=inverse.T@g+D@v;force[core.sb]+=load
            return np.r_[v,cho_solve(factor,-force),np.array(rd).ravel(),difference/air.relaxation.tau]
        start=np.r_[T@initial[0],T@initial[1],initial[2].ravel(),initial[3]];duration=.0005
        ref=solve_ivp(ode,(0,duration),start,method='DOP853',rtol=1e-11,atol=1e-15,max_step=1/1500000)
        self.assertTrue(ref.success);errors=[]
        for rate in [512000,1024000,2048000]:
            state=pilot.initial_state(5.)
            for _ in range(round(duration*rate)):state,_=pilot.step(state,1/rate)
            final=np.r_[T@state[0],T@state[1],state[2].ravel(),state[3]]
            errors.append(np.linalg.norm(final-ref.y[:,-1])/np.linalg.norm(ref.y[:,-1]))
        self.assertTrue(all(errors[i+1]<.3*errors[i] for i in [0,1]),errors)
        self.assertLess(errors[-1],.0005)

    def test_dense_losses_contact_coast_and_damper_live_energy_ledgers(self):
        contact=self.fixture();state=contact.initial_state(5.)
        state[2][:]=np.linspace(-.0002,.0003,state[2].size).reshape(state[2].shape);state[3][:]=[.01,-.02,.03]
        initial=contact.energy(state)['total_j'];heat=0.
        for _ in range(500):
            state,stats=contact.step(state,1/768000);heat+=stats['loss_j']
            self.assertGreaterEqual(stats['material_loss_j'],0.)
            self.assertLess(abs(contact.energy(state)['total_j']+heat-initial)/initial,1e-9)
        for cls in [ExponentialCoast,DampedCoast]:
            pilot=cls(contact.system,contact.air);state=pilot.initial_state(0.)
            rng=np.random.default_rng(827);state[0][:-1]=rng.normal(size=pilot.n-1)*1e-7
            state[0][-1]=-.001;state[1][:-1]=rng.normal(size=pilot.n-1)*.005;state[1][-1]=-.1
            state[2][:]=.0002;state[3][:]=[.01,-.02,.03]
            initial=pilot.energy(state);heat=air_work=radiated=0.
            for _ in range(60):
                if cls is DampedCoast:state,stats=pilot.release_step(state,1/64000,.6)
                else:state,stats=pilot.step(state,1/64000)
                heat+=stats['loss_j'];air_work+=stats['air_port_work_j'];radiated+=stats['radiation_loss_j']
                current=pilot.energy(state)
                self.assertGreaterEqual(stats['material_loss_j'],0.)
                self.assertLess(abs(current['total_j']+heat-initial['total_j'])/initial['total_j'],1e-9)
                self.assertLess(abs(current['air_j']+radiated-air_work-initial['air_j'])/initial['total_j'],1e-9)


if __name__=='__main__':unittest.main()
