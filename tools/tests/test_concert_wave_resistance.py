"""Independent references for the frequency-only resistance diagnosis."""
import importlib.util,sys,unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_streaming_air import SurfaceFactorBlocks
from concert_grand_rational_air import RationalAir
path=Path(__file__).resolve().parents[1]/'audit-concert-grand-wave-resistance.py'
spec=importlib.util.spec_from_file_location('probe',path);probe=importlib.util.module_from_spec(spec);spec.loader.exec_module(probe)


class ResistanceProbeTests(unittest.TestCase):
    def test_cell_fourier_integral_matches_independent_hemisphere_and_fold(self):
        rng=np.random.default_rng(634)
        for symmetric in [False,True]:
            shapes=rng.normal(size=(7,5,3))
            if symmetric:shapes=np.concatenate((shapes,shapes[:,::-1]),axis=1)
            surface=RectangularLoad(.7,.4,shapes)
            provider=SurfaceFactorBlocks(surface,16000,8,16,3,fold_y=symmetric)
            frequencies=[400.,1500.]
            matrices=probe.angular_resistance(provider,2*np.pi*np.array(frequencies))
            for hz,actual in zip(frequencies,matrices):
                np.testing.assert_allclose(actual,surface.angular_resistance(hz,8,16),rtol=1e-11,atol=1e-12)

    def test_single_frequency_wave_gram_matches_complete_causal_impedance(self):
        rng=np.random.default_rng(734);surface=RectangularLoad(.7,.4,rng.normal(size=(7,5,3)))
        provider=SurfaceFactorBlocks(surface,16000,8,16,3)
        wave=RationalAir(np.concatenate(list(provider.blocks()),axis=1),16000)
        for hz in [60.,1500.,6000.]:
            actual=probe.wave_resistance(provider,2*np.pi*hz)
            np.testing.assert_allclose(actual,wave.impedance(2j*np.pi*hz).real,rtol=1e-11,atol=1e-11)


if __name__=='__main__':unittest.main()
