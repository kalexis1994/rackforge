import unittest
import numpy as np
from concert_grand_rational_air import RationalAir
from concert_grand_pade_air import PadeAir
from concert_grand_loaded_teacher import wave_impedance_blocks, observer_transfer, mobility_and_pressure
from concert_grand_cell_audio import kernel_transfer


class LoadedTeacherTests(unittest.TestCase):
    def test_batch_matches_authenticated_wave_formula_across_blocks(self):
        h = np.zeros((5, 2, 2))
        h[0, 0] = [1., .3]
        h[1, 0] = [-1., -.3]
        h[:, 1] = -h[::-1, 0]
        wave = PadeAir(RationalAir(h, 128000))
        frequencies = [0., 23., 72., 500., 1881.9607122235952, 6000.]
        results = np.concatenate([z for first, f, z in wave_impedance_blocks(wave, frequencies, 2)])
        expected = np.array([wave.impedance(2j*np.pi*f) for f in frequencies])
        np.testing.assert_allclose(results, expected, rtol=2e-13, atol=2e-15)
        for actual in results:
            np.testing.assert_allclose(actual, actual.T, atol=1e-15)
            self.assertGreaterEqual(np.linalg.eigvalsh(actual.real).min(), -1e-14)

    def test_observer_clock_and_phase_match_existing_retarded_kernel(self):
        kernel = np.random.default_rng(4).normal(size=(13, 2, 3))
        f = np.array([0., 23., 800., 6000.])
        np.testing.assert_allclose(observer_transfer(kernel, 128000, f), kernel_transfer(kernel, 128000, f), rtol=1e-13, atol=1e-13)

    def test_known_loaded_scalar_port_and_pressure(self):
        omega, gamma, mass, resistance, hz = 480., 3., .15, 9., 83.
        s = 2j*np.pi*hz
        z = resistance*s*mass/(resistance+s*mass)
        force = np.array([[.4]])
        observer = np.array([[.7], [-.3]])*np.exp(-s*.003)
        mobility, pressure = mobility_and_pressure([omega], np.array([[gamma]]), np.array([[z]]), force, observer, hz)
        velocity = s*.4/(omega**2+s*s+s*gamma+s*z)
        np.testing.assert_allclose(mobility, [[.4*velocity]], rtol=1e-13)
        np.testing.assert_allclose(pressure[:, 0], s*observer[:, 0]*velocity, rtol=1e-13)
        self.assertGreaterEqual(mobility.real[0, 0], 0.)


if __name__ == '__main__':
    unittest.main()
