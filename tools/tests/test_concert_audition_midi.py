import importlib.util
from pathlib import Path
import sys
import unittest
import mido
import numpy as np

spec = importlib.util.spec_from_file_location('audition_midi',
    Path(__file__).resolve().parents[1]/'render-concert-grand-audition-midi.py')
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


class MidiArticulationTests(unittest.TestCase):
    def test_transposition_suppresses_frequencies_above_target_nyquist(self):
        t = np.arange(24000)/48000
        y = module.transpose_bandlimited(np.sin(2*np.pi*9000*t)[:, None], 24)
        self.assertLess(float(np.sqrt(np.mean(y[64:-64]**2))), 1e-4)
        passed = module.transpose_bandlimited(np.sin(2*np.pi*1000*t)[:, None], 12)
        self.assertGreater(float(np.sqrt(np.mean(passed[64:-64]**2))), .7)

    def test_overlapping_repeated_keys_pair_fifo_and_wait_for_sustain(self):
        events = [(0., mido.Message('note_on', note=60, velocity=84)),
                  (.1, mido.Message('note_on', note=60, velocity=40)),
                  (.15, mido.Message('control_change', control=64, value=90)),
                  (.2, mido.Message('note_off', note=60)),
                  (.3, mido.Message('note_off', note=60)),
                  (.6, mido.Message('control_change', control=64, value=20))]
        notes, _ = module.performances(events, .7)
        self.assertEqual([n.release for n in notes], [.6, .6])
        self.assertEqual([n.velocity for n in notes], [84, 40])

    def test_sostenuto_only_captures_keys_already_down(self):
        events = [(0., mido.Message('note_on', note=60, velocity=84)),
                  (.1, mido.Message('control_change', control=66, value=127)),
                  (.2, mido.Message('note_on', note=64, velocity=84)),
                  (.3, mido.Message('note_off', note=60)),
                  (.4, mido.Message('note_on', note=64, velocity=0)),
                  (.6, mido.Message('control_change', control=66, value=0))]
        notes, _ = module.performances(events, .7)
        self.assertEqual([n.release for n in notes], [.6, .4])


if __name__ == '__main__':
    unittest.main()
