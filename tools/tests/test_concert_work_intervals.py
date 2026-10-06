"""Prevent observer uncertainty from passing a point-only refinement gate."""
import importlib.util
import unittest
from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
spec = importlib.util.spec_from_file_location('work_intervals',
    Path(__file__).resolve().parents[1]/'audit-concert-grand-prefix-work-refinement.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class WorkIntervalsTests(unittest.TestCase):
    criteria = dict(energy_work_relative=.002,near_zero_j=1e-14)

    def test_equal_centres_do_not_hide_wide_observation_uncertainty(self):
        row = dict(wave_final_j=.025,wave_final_interval=dict(
            estimate_j=.025,lower_j=.024,upper_j=.026))
        change = audit.energy_change(row,row,'wave_final_j',self.criteria)
        self.assertEqual(change['relative'],0.)
        self.assertFalse(change['passed'])
        self.assertGreater(change['bounded_relative'],.08)
        bad = dict(wave_final_j=.025,wave_final_interval=dict(
            estimate_j=.03,lower_j=.024,upper_j=.031))
        with self.assertRaisesRegex(ValueError,'matching'):
            audit.energy_change(bad,row,'wave_final_j',self.criteria)

    def test_near_zero_flux_is_absolute_and_signed_work_keeps_its_scale(self):
        row = dict(wave_radiation_j=0.,wave_radiation_interval=dict(
            estimate_j=0.,lower_j=0.,upper_j=1e-16))
        self.assertFalse(audit.energy_change(row,row,'wave_radiation_j',self.criteria)['passed'])
        change = audit.energy_change(dict(air_port_work_j=-.999),
            dict(air_port_work_j=-1.),'air_port_work_j',self.criteria)
        self.assertTrue(change['passed'])
        self.assertAlmostEqual(change['relative'],.001)
        self.assertEqual(change['relative'],change['bounded_relative'])


if __name__=='__main__':
    unittest.main()
