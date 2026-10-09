import sys
import unittest
from pathlib import Path
import numpy as np
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from concert_grand_audition_air import project_air, MappedPlan, project_air_preserving_relaxation
from concert_grand_rational_air import RationalAir
from concert_grand_pade_air import PadeAir, PadeBoundedAir
from concert_grand_pade_midpoint import PadeMidpoint


class AuditionAirTests(unittest.TestCase):
    def reference(self):
        rng = np.random.default_rng(63)
        part = rng.normal(size=(7, 3, 3))*.01
        part -= part.mean(axis=0)
        H = np.empty((7, 6, 3))
        H[:, ::2] = part
        H[:, 1::2] = -part[::-1]
        wave = PadeAir(RationalAir(H, 128000))
        return PadeBoundedAir(wave, wave.static_mass()+np.eye(3)*.001, np.eye(3))

    def test_full_rank_preserves_load_and_living_wave_force(self):
        full = self.reference()
        reduced = project_air(full, np.eye(3))
        for s in [60j, 12000j, 50+6000j]:
            np.testing.assert_allclose(full.impedance(s), reduced.impedance(s), atol=3e-14, rtol=2e-12)
        a = PadeMidpoint(full.wave, 1/64000)
        b = MappedPlan(reduced.wave, 1/64000)
        x, y = full.wave.zero_state(), reduced.wave.zero_state()
        rng = np.random.default_rng(3)
        for _ in range(20):
            v = rng.normal(size=3)
            fa, oa = a.constant(x)
            fb, ob = b.constant(y)
            np.testing.assert_allclose(fa, fb, atol=1e-15)
            x, _ = a.update(x, v, oa)
            y, _ = b.update(y, v, ob)

    def test_low_rank_retains_static_mass_limit_and_positive_realness(self):
        full = self.reference()
        air = project_air(full, np.eye(3)[:, :1])
        np.testing.assert_allclose(air.static_mass(), full.static_mass(), atol=1e-15)
        np.testing.assert_allclose(air.high_limit(), full.high_limit(), atol=1e-15)
        for s in [60j, 12000j, 50+6000j]:
            Z = air.impedance(s)
            self.assertGreater(np.linalg.eigvalsh((Z+Z.conj().T)/2).min(), -1e-14)

    def test_parallel_completion_limits_full_rank_and_storage_work(self):
        full = self.reference()
        for rank in [1, 3]:
            air = project_air_preserving_relaxation(full, np.eye(3)[:, :rank])
            np.testing.assert_allclose(air.static_mass(), full.static_mass(), atol=1e-15)
            np.testing.assert_allclose(air.high_limit(), full.high_limit(), atol=1e-15)
            for s in [60j, 12000j, 50+6000j]:
                Z = air.impedance(s)
                self.assertGreater(np.linalg.eigvalsh((Z+Z.conj().T)/2).min(), -1e-14)
                if rank == 3:
                    np.testing.assert_allclose(Z, full.impedance(s), atol=3e-14, rtol=2e-12)
            relax = air.relaxation
            state = air.zero_state()[1]
            rng = np.random.default_rng(32)
            for _ in range(20):
                velocity = rng.normal(size=3)
                new, force, heat = relax.midpoint(state, velocity, 1/64000)
                self.assertAlmostEqual(relax.energy(new)-relax.energy(state),
                    (velocity@force-heat)/64000, places=14)
                self.assertGreaterEqual(heat, 0.)
                state = new


if __name__ == '__main__':
    unittest.main()
