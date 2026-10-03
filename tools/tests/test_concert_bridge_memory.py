import importlib.util
from pathlib import Path
import unittest

import numpy as np
from scipy.linalg import expm

spec = importlib.util.spec_from_file_location('memory', Path(__file__).resolve().parents[1]/'audit-concert-grand-bridge-memory.py')
memory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(memory)


class PassiveMemoryTests(unittest.TestCase):
    def test_exchange_is_passive_and_matches_independent_matrix_exponential(self):
        modes = dict(omega2=np.array([260., 280.])**2,
                     sigma=np.array([2., 3.]), inverse_mass=np.array([.2, .3]),
                     projection=np.array([.7, -.5]), tension_length_decay_scale=np.array([100., 100.]))
        matrix, _, loss = memory.generator(modes, 42., 3., .1)
        times = np.array([0., .01, .05, .1, .3])
        states, _, _ = memory.evolve(matrix, times)
        for column, time in enumerate(times):
            np.testing.assert_allclose(states[:, column], expm(matrix*time)[:, 0], atol=1e-12)
        # Coupling must store and return energy; it cannot act only as a drain.
        self.assertGreater(abs(states[1:, 2]).max(), .01)
        self.assertLessEqual(np.sum(abs(states)**2, axis=0).max(), 1+1e-12)
        derivative = 2*np.real(np.sum(np.conj(states)*(matrix@states), axis=0))
        np.testing.assert_allclose(derivative, -2*np.sum(loss[:, None]*abs(states)**2, axis=0), atol=1e-12)

    def test_steady_envelope_load_matches_exact_modal_admittance(self):
        modes = dict(omega2=np.array([200., 350.])**2, sigma=np.array([3., 5.]),
                     inverse_mass=np.array([.2, .4]), projection=np.array([.8, -.6]),
                     tension_length_decay_scale=np.array([40., 40.]))
        _, steady, _ = memory.generator(modes, 50., 3., .1)
        omega = 88200*np.tan(np.pi*50/44100)
        detuning = modes['omega2']-omega**2
        denominator = detuning**2+(2*modes['sigma']*omega)**2
        scale = 3*modes['tension_length_decay_scale']*modes['inverse_mass']*modes['projection']**2
        gamma = np.sum(scale*2*modes['sigma']*omega**2/denominator)
        beta = np.sum(scale*omega*detuning/denominator)
        self.assertAlmostEqual(steady.real, gamma, places=12)
        self.assertAlmostEqual(steady.imag, -beta, places=12)


if __name__ == '__main__':
    unittest.main()
