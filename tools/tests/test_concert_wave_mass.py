"""Cumulative Gram identity against independent correlation construction."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from concert_grand_wave_mass import static_wave_mass
from concert_grand_streaming_air import SurfaceFactorBlocks
from concert_grand_rational_air import RationalAir
from concert_grand_surface_impedance import RectangularLoad


class Factor:
    def __init__(self,H,rate):self.H=H;self.rate=rate;self.modes=H.shape[2]
    def blocks(self):
        for first in range(0,self.H.shape[1],2):yield self.H[:,first:first+2]


class WaveMassTests(unittest.TestCase):
    def test_random_paired_factors_match_correlation_derivative(self):
        rng=np.random.default_rng(913)
        for length,modes in [(2,1),(19,4),(47,7)]:
            H=rng.normal(size=(length,5,modes));H[-1]=-H[:-1].sum(axis=0)
            H=np.concatenate((H,-H[::-1]),axis=1)
            expected=RationalAir(H,32000).static_mass()
            np.testing.assert_allclose(static_wave_mass(Factor(H,32000)),expected,rtol=1e-11,atol=1e-13)

    def test_smoothed_surface_and_block_size_keep_same_mass(self):
        rng=np.random.default_rng(714);surface=RectangularLoad(.7,.4,rng.normal(size=(7,5,3)))
        provider=SurfaceFactorBlocks(surface,16000,4,8,3)
        H=np.concatenate(list(provider.blocks()),axis=1)
        expected=RationalAir(H,16000).static_mass()
        actual=static_wave_mass(provider)
        np.testing.assert_allclose(actual,expected,rtol=1e-11,atol=1e-13)
        np.testing.assert_allclose(static_wave_mass(SurfaceFactorBlocks(surface,16000,4,8,1)),actual,rtol=1e-12,atol=1e-13)

    def test_nonzero_dc_is_rejected(self):
        with self.assertRaises(ValueError):static_wave_mass(Factor(np.ones((3,2,1)),32000))


if __name__=='__main__':unittest.main()
