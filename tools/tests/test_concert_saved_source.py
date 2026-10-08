import copy
import sys
import unittest
from pathlib import Path
import numpy as np
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from concert_grand_saved_source import saved_source


class SavedSourceTests(unittest.TestCase):
    def report(self):
        return dict(board_sha256='board', source_vector_binding=dict(coordinates=606,
            normalized=False, board_sha256='board', absolute_time_window_s=[0., .003]),
            source_vectors=dict(v=[dict(frequency_hz=f, transform=[[1e-12, -2e-12]]*606)
                for f in [6000., 8000., 10000., 12000., 16000., 20000., 24000.]]))

    def test_retains_scale_phase_and_all_coordinates(self):
        v = saved_source(self.report(), 'v', 16000.)
        np.testing.assert_array_equal(v, np.full(606, 1e-12-2e-12j))

    def test_reject_changed_basis_window_or_normalization(self):
        for key, value in [('coordinates', 605), ('board_sha256', 'other'),
                           ('normalized', True), ('absolute_time_window_s', [.003, .0035])]:
            r = self.report()
            r['source_vector_binding'][key] = value
            with self.assertRaises(ValueError):
                saved_source(r, 'v', 16000.)

    def test_reject_incomplete_nonfinite_or_zero_vector(self):
        base = self.report()
        for pairs in [[[0., 0.]]*606, [[float('nan'), 0.]]*606, [[1., 0.]]*605]:
            r = copy.deepcopy(base)
            r['source_vectors']['v'][4]['transform'] = pairs
            with self.assertRaises(ValueError):
                saved_source(r, 'v', 16000.)
        base['source_vectors']['v'].pop()
        with self.assertRaises(ValueError):
            saved_source(base, 'v', 16000.)


if __name__ == '__main__':
    unittest.main()
