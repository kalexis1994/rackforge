"""Independent transfer, nonnegative fit, static mass and passive energy."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from scipy.integrate import solve_ivp
from scipy.linalg import expm
from concert_grand_acoustic_bath import AcousticBath,LoadedBoard
from concert_grand_bath_reduction import grouped,response,fit,truncate_basis,realize,linear_drive,free_force


class ReductionTests(unittest.TestCase):
    def fixture(self):
        omega=2*np.pi*np.array([70.,190.,480.]);gamma=2*np.pi*np.array([2.,3.,4.])
        vectors=np.array([[.5,.1],[-.1,.3],[.2,-.3]])
        A=np.einsum('jm,jn->jmn',vectors,vectors)
        weights=np.array([.4,1.6,.7]);static=np.eye(2)
        return omega,gamma,A,weights,static

    def independent(self,omega,gamma,A,weights,static,s):
        values=[];tail=static-sum(k*m for k,m in zip(weights,A))
        for value in s:
            z=value*tail
            for w,g,m,k in zip(omega,gamma,A,weights):z=z+k*m*w*w*value/(value*value+2*g*value+w*w)
            values.append(z)
        return np.array(values)

    def test_exact_grouping_unsorted_distinct_loss(self):
        bath=AcousticBath([120,80,120,40,80,120],[.1,.3,.1,.2,.3,.5],
            [[1,2],[.4,-.2],[2,-1],[.3,.7],[-.2,.4],[.1,.5]],np.eye(2)*.02)
        w,g,r=grouped(bath);s=np.array([1+5j,2+130j,20+250j])
        actual=response(w,g,r,bath.tail_mass,s)
        np.testing.assert_allclose(actual,[bath.impedance(value) for value in s],rtol=1e-13,atol=1e-13)

    def test_fit_known_multiport_and_unseen_transfer(self):
        w,g,A,k,M=self.fixture();s=2j*np.pi*np.linspace(30.3,600.3,80)
        target=self.independent(w,g,A,k,M,s);bath,weights,report=fit(w,g,A,M,s,target,regularization=0.)
        np.testing.assert_allclose(weights,k,rtol=1e-6,atol=1e-8)
        for value in [5+40j,90+700j,2j*np.pi*213.7]:
            expected=self.independent(w,g,A,k,M,[value])[0]
            self.assertLess(np.linalg.norm(bath.impedance(value)-expected)/np.linalg.norm(expected),1e-6)
            self.assertGreater(np.linalg.eigvalsh((bath.impedance(value)+bath.impedance(value).conj().T)/2).min(),-1e-12)
        np.testing.assert_allclose(bath.static_mass(),M,atol=1e-13)

    def test_spatial_discard_preserves_static_mass_and_budget(self):
        w,g,A,k,M=self.fixture();A[0]+=np.eye(2)*1e-5
        truncated,error=truncate_basis(A,.001);self.assertLess(error,.001)
        bath=realize(w,g,truncated,M,k);np.testing.assert_allclose(bath.static_mass(),M,atol=1e-13)
        board=LoadedBoard(np.diag([110.**2,370.**2]),np.diag([.3,.7]),bath)
        state,work=board.impulse(board.zero_state(),[.01,-.006]);heat=0.
        for i in range(600):
            state,stats=board.step(state,1/48000);heat+=stats['material_loss_j']+stats['bath_absorption_j']
            self.assertGreaterEqual(stats['material_loss_j'],0.);self.assertGreaterEqual(stats['bath_absorption_j'],0.)
            self.assertLess(abs(board.energy(state)['total_j']+heat-work)/work,1e-10)
        with self.assertRaises(ValueError):realize(w,g,A,M,[-1,1,1])
        with self.assertRaises(ValueError):realize(w,g,A,M,[100,100,100])

    def test_exact_prescribed_drive_and_late_force_against_ode(self):
        bath=AcousticBath([100,200,300],[150,.5,60],[[1,2],[-.4,1.2],[.5,-.3]],np.eye(2)*.01)
        dt=.0002;velocity=np.array([[0,0],[.01,-.02],[.02,.01],[-.01,.015],[0,0]])
        a,p,force=linear_drive(bath,velocity,dt)
        state=np.zeros(6)
        for i in range(len(velocity)-1):
            def ode(t,y):
                v=velocity[i]+t/dt*(velocity[i+1]-velocity[i])
                return np.r_[y[3:],bath.projection@v-bath.omega**2*y[:3]-2*bath.gamma*y[3:]]
            result=solve_ivp(ode,(0,dt),state,method='DOP853',rtol=1e-12,atol=1e-16);state=result.y[:,-1]
            expected_force=bath.projection.T@state[3:]+bath.tail_mass@((velocity[i+1]-velocity[i])/dt)
            np.testing.assert_allclose(force[i+1],expected_force,rtol=1e-10,atol=1e-14)
        np.testing.assert_allclose(np.r_[a,p],state,rtol=1e-10,atol=1e-15)
        late=free_force(bath,a,p,100,.0005)
        matrix=np.block([[np.zeros((3,3)),np.eye(3)],[-np.diag(bath.omega**2),-2*np.diag(bath.gamma)]])
        for index in [0,1,37,100]:
            expected=expm(index*.0005*matrix)@state
            np.testing.assert_allclose(late[index],bath.projection.T@expected[3:],rtol=1e-10,atol=1e-14)


if __name__=='__main__':unittest.main()
