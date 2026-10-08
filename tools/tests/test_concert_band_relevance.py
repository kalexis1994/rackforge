import sys
import unittest
from pathlib import Path
import math
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from concert_grand_band_relevance import band_error_contributions


class BandRelevanceTests(unittest.TestCase):
    def test_preserves_large_relative_small_absolute_error(self):
        norms = dict(zip(['0-6000', '6000-12000', '12000-24000', '0-24000'], [1., 1e-6, 1e-8, 1.00000101]))
        errors = dict(zip(norms, [1e-5, .004, .02, math.sqrt((1e-10+16e-12+4e-12)/norms['0-24000'])]))
        result = band_error_contributions(norms, errors)
        self.assertEqual(result['bands']['12000-24000']['band_relative_error'], .02)
        self.assertAlmostEqual(result['bands']['12000-24000']['error_relative_to_total'], 2e-6, places=11)
        self.assertAlmostEqual(result['reconstructed_total_relative_error'], errors['0-24000'], places=15)

    def test_reject_mismatched_weights_or_total(self):
        n = dict(zip(['0-6000', '6000-12000', '12000-24000', '0-24000'], [1., 2., 3., 6.]))
        for norms, errors in [(dict(n, **{'0-24000':7.}), dict.fromkeys(n, .01)),
                              (n, dict.fromkeys(n, float('nan'))),
                              (n, dict(dict.fromkeys(n, .01), **{'0-24000':.02}))]:
            with self.assertRaises(ValueError):
                band_error_contributions(norms, errors)


if __name__ == '__main__':
    unittest.main()
