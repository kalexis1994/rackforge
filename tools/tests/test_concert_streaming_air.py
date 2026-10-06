"""Streaming construction against complete factor and live energy oracle."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_rational_air import surface_factor_sparse,RationalAir
from concert_grand_bounded_air import BoundedAir
from concert_grand_streaming_air import SurfaceFactorBlocks,StreamingRationalAir,StreamingBoundedAir


class StreamingAirTests(unittest.TestCase):
    def test_folded_symmetric_surface_keeps_correlations_impedance_and_live_energy(self):
        rng=np.random.default_rng(435);half=rng.normal(size=(7,3,3))
        surface=RectangularLoad(.7,.4,np.concatenate((half,half[:,::-1]),axis=1))
        full=StreamingRationalAir(SurfaceFactorBlocks(surface,16000,4,16),100_000_000)
        provider=SurfaceFactorBlocks(surface,16000,4,16,fold_y=True)
        folded=StreamingRationalAir(provider,100_000_000)
        self.assertEqual(folded.outputs,full.outputs//2)
        self.assertEqual(folded.states,full.states)
        np.testing.assert_allclose(folded.R,full.R,rtol=1e-11,atol=1e-11)
        for s in [3+500j,2j*np.pi*6000]:
            np.testing.assert_allclose(folded.impedance(s),full.impedance(s),rtol=1e-11,atol=1e-11)
        state=rng.normal(size=full.zero_state().shape)*.01;v=rng.normal(size=3)*.1
        self.assertAlmostEqual(folded.energy(state),full.energy(state),delta=1e-11*full.energy(state))
        a,f,l=folded.midpoint(state,v,1/192000);b,g,k=full.midpoint(state,v,1/192000)
        np.testing.assert_array_equal(a,b);np.testing.assert_allclose(f,g,rtol=1e-11,atol=1e-11)
        self.assertAlmostEqual(l,k,delta=1e-11*max(k,1e-20))

    def test_fold_rejects_asymmetric_shapes_and_unpaired_angular_grid(self):
        rng=np.random.default_rng(431);surface=RectangularLoad(.7,.4,rng.normal(size=(7,6,3)))
        with self.assertRaises(ValueError):SurfaceFactorBlocks(surface,16000,4,16,fold_y=True)
        surface=RectangularLoad(.7,.4,np.ones((7,6,3)))
        with self.assertRaises(ValueError):SurfaceFactorBlocks(surface,16000,4,10,fold_y=True)

    def fixture(self,block=3):
        rng=np.random.default_rng(325);shapes=rng.normal(size=(7,5,3))
        surface=RectangularLoad(.7,.4,shapes)
        provider=SurfaceFactorBlocks(surface,16000,4,8,block)
        raw=surface_factor_sparse(surface,16000,4,8)
        H=np.zeros((len(raw)+1,)+raw.shape[1:]);H[:-1]+=raw/2;H[1:]+=raw/2
        full=RationalAir(H,16000);stream=StreamingRationalAir(provider,100_000_000,5,2)
        return provider,full,stream

    def test_factor_correlations_impedance_and_block_invariance(self):
        provider,full,stream=self.fixture()
        np.testing.assert_allclose(np.concatenate(list(provider.blocks()),axis=1),full.H,rtol=1e-13,atol=1e-13)
        np.testing.assert_allclose(stream.R,full.R,rtol=1e-12,atol=1e-12)
        for s in [1.,2j*np.pi*60,2j*np.pi*6000,100+15000j,1e8]:
            np.testing.assert_allclose(stream.impedance(s),full.impedance(s),rtol=1e-11,atol=1e-11)
        _,_,other=self.fixture(1)
        np.testing.assert_allclose(other.R,stream.R,rtol=1e-12,atol=1e-12)
        with self.assertRaises(MemoryError):StreamingRationalAir(provider,1)

    def test_arbitrary_living_states_midpoint_loss_storage_and_batch(self):
        _,full,stream=self.fixture();rng=np.random.default_rng(424)
        state=rng.normal(size=stream.zero_state().shape)*.01;v=rng.normal(size=3)*.1
        self.assertAlmostEqual(stream.energy(state),full.energy(state),delta=1e-12*full.energy(state))
        for dt in [1/16000,1/192000,.003]:
            a,f,loss=full.midpoint(state,v,dt);b,g,other=stream.midpoint(state,v,dt)
            np.testing.assert_array_equal(a,b);np.testing.assert_allclose(f,g,rtol=1e-12,atol=1e-12)
            self.assertAlmostEqual(loss,other,delta=1e-12*max(loss,1e-20))
            balance=stream.energy(b)-stream.energy(state)-dt*(v@g-other)
            self.assertLess(abs(balance)/max(stream.energy(state),abs(dt*v@g)),1e-11)
        inputs=rng.normal(size=(4,stream.order+1,stream.modes))
        np.testing.assert_allclose(stream.radiation_batch(inputs),full.radiation_batch(inputs),rtol=1e-12)

    def test_bounded_completion_matches_full_factor_and_rejects_active_residual(self):
        provider,full,stream=self.fixture()
        mass=full.static_mass()+np.diag([.04,.03,.02]);limit=np.diag([20.,25.,30.])
        old=BoundedAir(full.H,full.rate,mass,limit,smooth=False)
        new=StreamingBoundedAir(stream,mass,limit)
        for s in [2+400j,2j*np.pi*5000,1e8]:
            np.testing.assert_allclose(new.impedance(s),old.impedance(s),rtol=1e-11,atol=1e-11)
        with self.assertRaises(ValueError):StreamingBoundedAir(stream,-np.eye(3),limit)


if __name__=='__main__':unittest.main()
