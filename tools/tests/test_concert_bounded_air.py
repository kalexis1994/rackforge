"""Independent mass/resistance limits, ODE, stop and coupled-work checks."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from scipy.integrate import solve_ivp
from concert_grand_bounded_air import RelaxingMass,BoundedAir,LoadedBoundedBoard
from concert_grand_rational_air import RationalAir


class BoundedAirTests(unittest.TestCase):
    def fixture(self):
        rng=np.random.default_rng(614)
        h=rng.normal(size=(7,3,2));h[-1]=-h[:-1].sum(axis=0)
        factor=np.concatenate((h,-h[::-1]),axis=1)
        smoothed=np.zeros((len(factor)+1,)+factor.shape[1:]);smoothed[:-1]+=factor/2;smoothed[1:]+=factor/2
        wave=RationalAir(smoothed,16000)
        return BoundedAir(factor,16000,wave.static_mass()+np.array([[.004,.001],[.001,.006]]),np.array([[5.,.7],[.7,7.]]))

    def test_limits_passivity_and_rejected_residuals(self):
        air=self.fixture();r=air.relaxation
        np.testing.assert_allclose(r.B@np.diag(r.tau)@r.B.T,r.mass,atol=1e-17)
        np.testing.assert_allclose(r.B@r.B.T,r.resistance,atol=3e-15)
        # Extrapolate two finite Laplace points: Z near zero subtracts large
        # wave residues, so a single extremely small s tests cancellation.
        extrapolated=2*air.impedance(.005)/.005-air.impedance(.01)/.01
        self.assertLess(np.linalg.norm(extrapolated-air.static_mass())/np.linalg.norm(air.static_mass()),2e-8)
        self.assertLess(np.linalg.norm(air.impedance(1e13)-air.high_limit())/np.linalg.norm(air.high_limit()),2e-8)
        self.assertLess(np.linalg.norm(air.wave_limit),1e-13)
        for s in [3+200j,100+8000j,1e6+1e7j]:
            z=air.impedance(s);self.assertGreater(np.linalg.eigvalsh((z+z.conj().T)/2).min(),0.)
        for mass,resistance in [(-np.eye(2),np.eye(2)),(np.eye(2),np.zeros((2,2))),(np.eye(2),np.array([[1.,2.],[2.,1.]]))]:
            with self.assertRaises(ValueError):RelaxingMass(mass,resistance)
        with self.assertRaises(ValueError):BoundedAir(air.wave.H,air.wave.rate,-np.eye(2),air.high_limit(),smooth=False)

    def test_storage_identity_with_arbitrary_live_states_and_steps(self):
        air=self.fixture();rng=np.random.default_rng(722)
        for dt in [1/16000,1/48000,1/768000,.003]:
            state=(rng.normal(size=air.wave.zero_state().shape),rng.normal(size=2));v=rng.normal(size=2)
            updated,force,loss=air.midpoint(state,v,dt)
            delta=air.energy(updated)-air.energy(state)
            self.assertGreaterEqual(loss,0.)
            self.assertAlmostEqual(delta,dt*(v@force-loss),delta=3e-12*max(air.energy(state),dt*abs(v@force)))

    def test_relaxation_linear_drive_and_exact_stop_against_ode(self):
        r=RelaxingMass(np.array([[.002,.0007],[.0007,.003]]),np.array([[20.,3.],[3.,30.]]))
        dt=1/128000;v=np.array([[0.,0.],[.03,-.02],[.01,.04],[-.01,.02]])
        actual=r.linear_drive_force(v,dt,50);state=np.zeros(2);expected=[np.zeros(2)]
        for i in range(len(actual)-1):
            driven=i<len(v)-1;left=v[i] if driven else np.zeros(2);right=v[i+1] if driven else np.zeros(2)
            def ode(t,q):return (r.B.T@(left+t/dt*(right-left))-q)/r.tau
            state=solve_ivp(ode,(0,dt),state,method='DOP853',rtol=1e-12,atol=1e-15).y[:,-1]
            expected.append(r.B@(r.B.T@right-state))
        np.testing.assert_allclose(actual,expected,rtol=3e-10,atol=1e-13)

    def test_coupled_board_against_independent_physical_ode_and_work(self):
        air=self.fixture();K=np.array([[800.,70.],[70.,1400.]]);D=np.array([[.4,.03],[.03,.7]])
        board=LoadedBoundedBoard(K,D,air);rng=np.random.default_rng(883)
        initial=(rng.normal(size=2)*.001,rng.normal(size=2)*.1,(rng.normal(size=air.wave.zero_state().shape)*.01,rng.normal(size=2)*.01))
        force=np.array([.3,-.2]);duration=.0001;nwave=air.wave.states
        def flatten(state):return np.r_[state[0],state[1],state[2][0].ravel(),state[2][1]]
        def ode(t,y):
            q=y[:2];v=y[2:4];rows=y[4:4+nwave].reshape(air.wave.zero_state().shape);z=y[4+nwave:]
            u=v.copy();derivative=[];load=air.wave.D@u
            for row,residue in zip(rows,air.wave.R[1:]):
                derivative.append(air.wave.a*(u-row));u=2*row-u;load+=residue@u
            difference=air.relaxation.B.T@v-z;load+=air.relaxation.B@difference
            return np.r_[v,force-K@q-D@v-load,np.array(derivative).ravel(),difference/air.relaxation.tau]
        reference=solve_ivp(ode,(0,duration),flatten(initial),method='DOP853',rtol=1e-12,atol=1e-15).y[:,-1]
        errors=[]
        for steps in [32,64]:
            state=initial;work=material=radiation=port=0.
            for _ in range(steps):
                state,stats=board.step(state,duration/steps,force)
                work+=stats['input_work_j'];material+=stats['material_loss_j'];radiation+=stats['radiation_loss_j'];port+=stats['air_port_work_j']
            self.assertAlmostEqual(board.energy(state)-board.energy(initial),work-material-radiation,delta=2e-14)
            self.assertAlmostEqual(air.energy(state[2])-air.energy(initial[2]),port-radiation,delta=2e-14)
            errors.append(np.linalg.norm(flatten(state)-reference))
        self.assertLess(errors[1],.27*errors[0])


if __name__=='__main__':unittest.main()
