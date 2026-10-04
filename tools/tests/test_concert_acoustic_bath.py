"""Independent dense dynamics, spectral sphere and reciprocal energy checks."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from scipy.linalg import expm
from concert_grand_acoustic_bath import AcousticBath,LoadedBoard,positive_factor


def dense_generator(board):
    sizes=[board.bath.modes]*2+[board.bath.ports]*2;cuts=np.cumsum([0]+sizes)
    columns=[]
    for i in range(cuts[-1]):
        e=np.eye(1,cuts[-1],i)[0];state=tuple(e[a:b] for a,b in zip(cuts[:-1],cuts[1:]))
        columns.append(np.concatenate(board.derivative(state)))
    return np.array(columns).T,cuts


class BathTests(unittest.TestCase):
    def small(self):
        bath=AcousticBath(2*np.pi*np.array([60.,120.,250.]),2*np.pi*3,
            [[12.,3.],[-8.,7.],[4.,-9.]],[[.08,.01],[.01,.03]])
        return LoadedBoard(np.diag((2*np.pi*np.array([40.,85.]))**2),np.diag([1.2,2.]),bath)

    def test_transfer_against_independent_oscillator_state_solve(self):
        bath=self.small().bath;n=bath.ports
        A=np.block([[np.zeros((n,n)),np.eye(n)],[-np.diag(bath.omega**2),-np.diag(2*bath.gamma)]])
        B=np.r_[np.zeros_like(bath.projection),bath.projection];C=np.c_[np.zeros_like(bath.projection.T),bath.projection.T]
        for s in (0.,2j*np.pi*80,20+2j*np.pi*261):
            expected=C@np.linalg.solve(s*np.eye(2*n)-A,B)+s*bath.tail_mass
            np.testing.assert_allclose(bath.impedance(s),expected,atol=1e-12,rtol=1e-12)
            np.testing.assert_allclose(bath.impedance(s),bath.impedance(s).T,atol=1e-12)
            if s!=0:self.assertGreater(np.linalg.eigvalsh(bath.impedance(s).real).min(),0.)

    def test_midpoint_work_and_air_return_close_energy(self):
        board=self.small();state=board.zero_state();state[2][:]=[.002,-.001,.0007]
        initial=board.energy(state)['total_j'];material=absorption=work=port=0.;returned=0
        for step in range(600):
            before_air=board.energy(state)['air_j']
            state,stats=board.step(state,1/12000)
            after_air=board.energy(state)['air_j']
            self.assertAlmostEqual(stats['air_port_work_j'],after_air-before_air+stats['bath_absorption_j'],places=13)
            material+=stats['material_loss_j'];absorption+=stats['bath_absorption_j'];port+=stats['air_port_work_j']
            self.assertGreaterEqual(stats['bath_absorption_j'],0.)
            returned+=stats['air_port_work_j']<0
        self.assertGreater(returned,0)
        self.assertLess(abs(board.energy(state)['total_j']+material+absorption-initial)/initial,1e-11)

    def test_dynamics_against_exponential_and_second_order(self):
        board=self.small();A,cuts=dense_generator(board)
        H=np.zeros_like(A);loss=np.zeros_like(A)
        for block,matrix in enumerate((board.K,board.M,np.diag(board.bath.omega**2),np.eye(board.bath.ports))):
            H[cuts[block]:cuts[block+1],cuts[block]:cuts[block+1]]=matrix
        loss[cuts[1]:cuts[2],cuts[1]:cuts[2]]=board.D
        loss[cuts[3]:cuts[4],cuts[3]:cuts[4]]=np.diag(2*board.bath.gamma)
        np.testing.assert_allclose(A.T@H+H@A,-2*loss,rtol=1e-12,atol=1e-9)
        self.assertLess(np.linalg.eigvals(A).real.max(),0.)
        state,_=board.impulse(board.zero_state(),[.02,-.01]);initial=np.concatenate(state)
        expected=expm(A*.02)@initial;errors=[]
        def norm(vector):
            state=tuple(vector[a:b] for a,b in zip(cuts[:-1],cuts[1:]))
            return np.sqrt(2*board.energy(state)['total_j'])
        for rate in (4000,8000,16000):
            actual=tuple(x.copy() for x in state)
            for _ in range(round(.02*rate)):actual,_=board.step(actual,1/rate)
            errors.append(norm(np.concatenate(actual)-expected)/norm(expected))
        self.assertLess(errors[-1],.002)
        self.assertLess(errors[1]/errors[0],.3);self.assertLess(errors[2]/errors[1],.3)

    def test_restrike_preserves_global_air_state_and_accounts_work(self):
        board=self.small();state,first=board.impulse(board.zero_state(),[.02,-.01])
        for _ in range(60):state,_=board.step(state,1/12000)
        before=board.energy(state)['total_j'];air=tuple(x.copy() for x in state[2:])
        next_state,work=board.impulse(state,[-.01,.025])
        for a,b in zip(next_state[2:],air):np.testing.assert_array_equal(a,b)
        self.assertAlmostEqual(board.energy(next_state)['total_j']-before,work,places=15)

    def test_spectral_sphere_and_static_tail(self):
        # Exact sphere: Z=R*s*tau/(1+s*tau). The spectral integral has an
        # independent closed target, including the tail's missing static mass.
        frequency=np.r_[.01,np.arange(2.,12002.,2.)];tau=.18/343;R=1.2*343*4*np.pi*.18**2
        omega=2*np.pi*frequency;r=R*(omega*tau)**2/(1+(omega*tau)**2)
        edges=np.r_[np.arange(0.,1200.25,.25),np.arange(1205.,12000.1,5.)]
        bath=AcousticBath.from_spectrum(frequency,r[:,None,None],[[R*tau]],edges,.5)
        np.testing.assert_allclose(bath.static_mass(),[[R*tau]],rtol=1e-12)
        for hz in (60.,261.,800.):
            s=2j*np.pi*hz;exact=R*s*tau/(1+s*tau);actual=bath.impedance(s)[0,0]
            self.assertLess(abs(actual-exact)/abs(exact),.005)

    def test_materially_negative_weights_or_tail_are_rejected(self):
        with self.assertRaises(ValueError):positive_factor([[1.,0.],[0.,-.01]])
        with self.assertRaises(ValueError):AcousticBath([100.],1.,[[1.,1.]],[[.1,0.],[0.,-.1]])
        with self.assertRaises(ValueError):self.small().step(self.small().zero_state(),0.)


if __name__=='__main__':unittest.main()
