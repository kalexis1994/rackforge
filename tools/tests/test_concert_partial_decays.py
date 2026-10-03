import importlib.util
from pathlib import Path
import unittest

import numpy as np

spec = importlib.util.spec_from_file_location('partial_decay', Path(__file__).resolve().parents[1]/'measure-concert-grand-partial-decays.py')
decay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(decay)


class PartialDecayTests(unittest.TestCase):
    def test_known_exponential_slope_and_stereo_phase_cancellation(self):
        time = np.arange(round(1.7*44100))/44100
        for f in (65.406, 261.626, 1046.502):
            y = np.exp(-2*time)*np.sin(2*np.pi*f*time)
            stereo = np.column_stack((y, -y))
            t, levels, _ = decay.envelope(stereo, f, f)
            for row in decay.slopes(t, levels):
                self.assertAlmostEqual(row['slope_db_per_s'], -40/np.log(10), delta=.02)
            _, quieter, _ = decay.envelope(stereo*.1, f, f)
            np.testing.assert_allclose(levels-quieter, 20, atol=1e-10)

    def test_adjacent_partial_does_not_impose_its_decay(self):
        t = np.arange(round(1.7*44100))/44100
        f = 261.626
        y = np.exp(-t)*np.sin(2*np.pi*f*t)+.7*np.exp(-8*t)*np.sin(4*np.pi*f*t)
        time, levels, _ = decay.envelope(np.column_stack((y, y)), f, f)
        for row in decay.slopes(time, levels):
            self.assertAlmostEqual(row['slope_db_per_s'], -20/np.log(10), delta=.02)


if __name__ == '__main__':
    unittest.main()
