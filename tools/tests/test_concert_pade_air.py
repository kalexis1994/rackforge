"""Lossless lift certificate, independent rational ODE and live board ledger."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from scipy.integrate import solve_ivp
from scipy.signal import tf2ss
from concert_grand_rational_air import RationalAir
from concert_grand_streaming_air import SurfaceFactorBlocks,StreamingRationalAir
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_bounded_air import LoadedBoundedBoard
from concert_grand_wave_mass import static_wave_mass
from concert_grand_pade_air import pade_delay,fourth_factor,FourthFactorBlocks,PadeAir,PadeBoundedAir


class PadeAirTests(unittest.TestCase):
    def fixture(self,rate=16000,modes=2,length=7):
        rng=np.random.default_rng(738)
        H=rng.normal(size=(length,3,modes));H[-1]=-H[:-1].sum(axis=0)
        paired=np.zeros((length,6,modes));paired[:,::2]=H;paired[:,1::2]=-H[::-1]
        return PadeAir(RationalAir(paired,rate))

    def test_delay_phase_stable_schur_and_wave_factor_identity(self):
        air=self.fixture();r=air.rate
        self.assertTrue(np.all((r*np.array([-3+1j*np.sqrt(3),-3-1j*np.sqrt(3)])).real<0))
        for hz in [60.,6000.,20000.,100000.]:
            s=2j*np.pi*hz;t=air.delay(s);self.assertAlmostEqual(abs(t),1.,delta=1e-14)
            z=air.impedance(s);F=np.einsum('k,kom->om',t**np.arange(len(air.base.H)),air.base.H)
            np.testing.assert_allclose((z+z.conj().T)/2,F.conj().T@F/2,atol=1e-12,rtol=1e-12)
        for s in [1+2j,100+3000j,100000+300000j,1e300]:
            self.assertLessEqual(abs(pade_delay(s,r)),1+1e-14)
            z=air.impedance(s);self.assertGreaterEqual(np.linalg.eigvalsh((z+z.conj().T)/2).min(),-1e-11)
        x=.3;pade=-np.angle(pade_delay(1j*x*r,r));cayley=2*np.arctan(x/2)
        self.assertLess(abs(pade-x),abs(cayley-x)/100)
        s=r*1e-5;estimate=2*air.impedance(s)/s-air.impedance(2*s)/(2*s)
        np.testing.assert_allclose(estimate,air.static_mass(),rtol=1e-7,atol=1e-12)
        np.testing.assert_allclose(air.high_limit(),0.,atol=1e-12)

    def test_arbitrary_live_state_work_and_deferred_force(self):
        air=self.fixture();rng=np.random.default_rng(239)
        state=rng.normal(size=air.zero_state().shape)*.01;v=rng.normal(size=air.modes)*.1
        before=air.energy(state)
        self.assertGreater(before,0.)
        for dt in [1/16000,1/192000,.003]:
            updated,force,loss=air.midpoint(state,v,dt)
            balance=air.energy(updated)-before-dt*(v@force-loss)
            self.assertGreaterEqual(loss,0.)
            self.assertLess(abs(balance)/max(before,abs(dt*v@force)),1e-11)
            a,f,inputs=air.midpoint_deferred(state,v,dt)
            np.testing.assert_array_equal(a,updated);np.testing.assert_array_equal(f,force)
            np.testing.assert_allclose(force,air.centered_constant(state,dt)+air.impedance(2/dt).real@v,rtol=1e-11,atol=1e-11)

    def test_zero_start_transfer_against_independent_polynomial_ode(self):
        air=self.fixture(rate=1.,modes=1,length=4);order=air.order
        numerator=np.poly1d([1.,-6.,12.]);denominator=np.poly1d([1.,6.,12.])
        top=(denominator**order)*float(air.D[0,0])
        for j in range(1,order+1):top+=(numerator**j)*(denominator**(order-j))*float(air.R[j,0,0])
        self.assertLess(abs(top.c[0]),1e-12*np.linalg.norm(top.c))
        # Zero feedthrough is an exact consequence of T(infinity)=1 and H(1)=0;
        # remove only that known roundoff coefficient in the independent oracle.
        A,B,C,D=tf2ss(top.c[1:],(denominator**order).c)
        duration=.4;velocity=np.array([.2])
        oracle=solve_ivp(lambda t,x:A@x+B[:,0]*velocity[0],(0,duration),np.zeros(len(A)),method='DOP853',rtol=1e-12,atol=1e-14)
        reference=float((C@oracle.y[:,-1]+D[:,0]*velocity[0])[0]);errors=[]
        for steps in [16,32]:
            state=air.zero_state()
            for _ in range(steps):state,_,_=air.midpoint(state,velocity,duration/steps)
            errors.append(abs(air.derivative(state,velocity)[1][0]-reference))
        self.assertLess(errors[1],.26*errors[0])

    def test_fourth_filter_and_streaming_certificate_preserve_pairs(self):
        rng=np.random.default_rng(939);surface=RectangularLoad(.7,.4,rng.normal(size=(7,5,3)))
        base=SurfaceFactorBlocks(surface,16000,4,8,3);provider=FourthFactorBlocks(base)
        H=np.concatenate(list(provider.blocks()),axis=1)
        legacy=np.concatenate(list(base.blocks()),axis=1)
        np.testing.assert_array_equal(H[:,1::2],-H[::-1,::2]);np.testing.assert_allclose(H.sum(axis=0),0.,atol=1e-12)
        for theta in [.1,.3,2.]:
            a=np.einsum('t,tom->om',np.exp(-1j*theta*np.arange(len(H))),H)
            b=np.einsum('t,tom->om',np.exp(-1j*theta*np.arange(len(legacy))),legacy)
            np.testing.assert_allclose(a,np.exp(-1j*theta)*(4/3-np.cos(theta)/3)*b,rtol=1e-11,atol=1e-11)
        full=PadeAir(RationalAir(H,16000));stream=PadeAir(StreamingRationalAir(provider,100_000_000,real_gram=True))
        np.testing.assert_allclose(static_wave_mass(provider),full.static_mass(),rtol=1e-11,atol=1e-11)
        old=RationalAir(legacy,16000)
        np.testing.assert_allclose(full.static_mass()-old.static_mass(),(7*old.R[0]-old.R[1])/(18*old.rate),rtol=1e-11,atol=1e-11)
        state=rng.normal(size=full.zero_state().shape)*.01;v=rng.normal(size=3)*.1
        self.assertAlmostEqual(stream.energy(state),full.energy(state),delta=1e-11*full.energy(state))
        a,f,l=full.midpoint(state,v,1/192000);b,g,k=stream.midpoint(state,v,1/192000)
        np.testing.assert_array_equal(a,b);np.testing.assert_allclose(f,g,rtol=1e-11,atol=1e-11)
        self.assertAlmostEqual(l,k,delta=1e-11*max(k,1e-20))
        with self.assertRaises(ValueError):FourthFactorBlocks(SurfaceFactorBlocks(surface,16000,4,8,smooth=False))

    def test_bounded_limits_and_coupled_live_board_energy(self):
        wave=self.fixture();mass=wave.static_mass()+np.array([[.02,.001],[.001,.03]])
        limit=np.array([[20.,2.],[2.,30.]])
        air=PadeBoundedAir(wave,mass,limit)
        np.testing.assert_allclose(air.static_mass(),mass,rtol=1e-12,atol=1e-12)
        np.testing.assert_allclose(air.high_limit(),limit,rtol=1e-12,atol=1e-12)
        with self.assertRaises(ValueError):PadeBoundedAir(wave,-np.eye(2),limit)
        rng=np.random.default_rng(351);state=(rng.normal(size=wave.zero_state().shape)*.01,rng.normal(size=2)*.01)
        v=rng.normal(size=2)*.1;dt=1/192000
        updated,force,loss=air.midpoint(state,v,dt)
        self.assertLess(abs(air.energy(updated)-air.energy(state)-dt*(v@force-loss))/air.energy(state),1e-11)
        np.testing.assert_allclose(force,air.centered_constant(state,dt)+air.impedance(2/dt).real@v,atol=1e-11,rtol=1e-11)
        board=LoadedBoundedBoard(np.diag([300.**2,500.**2]),np.array([[5.,.5],[.5,8.]]),air)
        current=(np.zeros(2),np.zeros(2),state);initial=board.energy(current);work=heat=0.
        for _ in range(16):
            current,ledger=board.step(current,dt,np.array([.3,-.1]))
            work+=ledger['input_work_j'];heat+=ledger['material_loss_j']+ledger['radiation_loss_j']
        self.assertLess(abs(board.energy(current)-initial-work+heat)/max(initial,abs(work)),1e-11)


if __name__=='__main__':unittest.main()
