"""Batch certificate against independent stored-factor quadratics and cache."""
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_streaming_air import SurfaceFactorBlocks, StreamingRationalAir
from concert_grand_rational_air import RationalAir
from concert_grand_pade_air import FourthFactorBlocks, PadeAir
from concert_grand_pade_certificate import certificate_batch
from concert_grand_wave_cache import save_wave_cache, load_wave_cache


class CertificateTests(unittest.TestCase):
    def fixture(self):
        rng = np.random.default_rng(61006)
        surface = RectangularLoad(.7, .4, rng.normal(size=(7, 5, 3)))
        return FourthFactorBlocks(SurfaceFactorBlocks(surface, 16000, 4, 8, 3)), rng

    def test_storage_radiation_and_work_in_one_factor_pass(self):
        provider, rng = self.fixture()
        H = np.concatenate(list(provider.blocks()), axis=1)
        air = PadeAir(RationalAir(H, provider.rate))
        states = [air.zero_state(), rng.normal(size=air.zero_state().shape)*.01]
        histories = []
        work = []
        for initial in states[:]:
            state = initial.copy()
            inputs = []
            w = 0.
            for _ in range(7):
                v = rng.normal(size=provider.modes)*.01
                state, f, u = air.midpoint_deferred(state, v, 1/192000)
                inputs.append(u)
                w += v@f/192000
            states.append(state)
            histories.append(np.array(inputs))
            work.append(w)
        energies, losses = certificate_batch(provider, np.array(states), histories, input_block=3)
        for state, energy in zip(states, energies):
            self.assertAlmostEqual(energy, air.energy(state), delta=1e-11*max(energy, 1e-10))
        for i, (u, loss) in enumerate(zip(histories, losses)):
            expected = np.array([np.linalg.norm(np.einsum('tom,tm->o', H, item))**2/2 for item in u])
            np.testing.assert_allclose(loss, expected, rtol=1e-11, atol=1e-15)
            self.assertTrue(np.all(loss >= 0))
            balance = energies[i+2]-energies[i]+loss.sum()/192000-work[i]
            self.assertLess(abs(balance)/max(energies[i], energies[i+2], abs(work[i])), 1e-11)

    def test_cache_preserves_full_live_wave_and_rejects_changed_data(self):
        provider, rng = self.fixture()
        wave = StreamingRationalAir(provider, 100_000_000, real_gram=True)
        identity = {'board': 'fixture', 'rate': provider.rate, 'angular': [4, 8]}
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp)/'cache'
            save_wave_cache(path, wave, identity)
            copy, _ = load_wave_cache(path, provider, identity)
            np.testing.assert_array_equal(copy.R, wave.R)
            self.assertFalse(copy.R.flags.writeable)
            a, b = PadeAir(wave), PadeAir(copy)
            state = rng.normal(size=a.zero_state().shape)*.01
            v = rng.normal(size=a.modes)*.01
            np.testing.assert_array_equal(a.impedance(100+4000j), b.impedance(100+4000j))
            ar, af, au = a.midpoint_deferred(state, v, 1/192000)
            br, bf, bu = b.midpoint_deferred(state, v, 1/192000)
            np.testing.assert_array_equal(ar, br)
            np.testing.assert_array_equal(af, bf)
            np.testing.assert_array_equal(au, bu)
            self.assertEqual(a.energy(state), b.energy(state))
            del copy, b
            with self.assertRaises(ValueError):
                load_wave_cache(path, provider, dict(identity, board='changed'))
            # Corruption is rejected rather than silently rebuilt or repaired.
            with (path/'R.npy').open('r+b') as f:
                f.seek(-8, 2)
                f.write(b'corrupt!')
            with self.assertRaises(ValueError):
                load_wave_cache(path, provider, identity)


if __name__ == '__main__':
    unittest.main()
