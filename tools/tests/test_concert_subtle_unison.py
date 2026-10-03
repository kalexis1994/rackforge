import importlib.util
from pathlib import Path
import unittest

import numpy as np

spec = importlib.util.spec_from_file_location('subtle', Path(__file__).resolve().parents[1]/'audit-concert-grand-subtle-unison.py')
subtle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(subtle)


class StereoBeatingTests(unittest.TestCase):
    def test_stereo_energy_retains_beating_and_ignores_mono_null(self):
        time = np.arange(13*subtle.RATE)/subtle.RATE
        y = (np.sin(2*np.pi*1000*time)+.8*np.sin(2*np.pi*1001*time))*np.exp(-.05*time)
        t, levels = subtle.partial_envelope(np.column_stack((y, -y)), 1000.5, 1000)
        events = subtle.notches(t, levels, -90)
        self.assertGreaterEqual(len(events), 10)
        self.assertAlmostEqual(events[0]['time_s'], .5, delta=.03)

    def test_decaying_treble_is_not_aliased_into_recurrent_beats(self):
        time = np.arange(13*subtle.RATE)/subtle.RATE
        y = np.exp(-.4*time)*np.sin(2*np.pi*8500*time)
        t, levels = subtle.partial_envelope(np.column_stack((y, y)), 8500, 1000)
        self.assertEqual(subtle.notches(t, levels, -90), [])


if __name__ == '__main__':
    unittest.main()
