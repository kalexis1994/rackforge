"""Directed refinement must evaluate both axes without hiding a failed band."""
import importlib.util
import sys
import unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
spec=importlib.util.spec_from_file_location('directed_audit',Path(__file__).resolve().parents[1]/'audit-concert-grand-string-resolution-directed.py')
audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)


class DirectedResolutionTests(unittest.TestCase):
    def test_both_axes_use_the_same_fine_reference_at_each_fixed_setting(self):
        frequency=np.array([60.,480.,1980.,6000.])
        observations={}
        for nt in audit.MODES:
            for rate in audit.RATES:
                value=nt/48+rate/audit.RATES[0]
                observations[audit.case_key(nt,rate)]=tuple(np.full(shape,value)
                    for shape in [(4,2),(10,2),(10,3),(10,3)])
        rows=audit.comparisons(observations,frequency)
        self.assertEqual(len(rows),4)
        for row in rows:
            actual=observations[row['case']][0][0,0];fine=observations[row['reference']][0][0,0]
            expected=abs(actual-fine)/fine
            self.assertAlmostEqual(row['temporal_pressure_relative'],expected)
            for value in row['complex_pressure_relative'].values():self.assertAlmostEqual(value,expected)
            if row['kind']=='spatial':
                self.assertEqual(row['case'].split('-r')[1],row['reference'].split('-r')[1])
            else:self.assertEqual(row['case'].split('-r')[0],row['reference'].split('-r')[0])
        del observations[audit.case_key(96,audit.RATES[1])]
        with self.assertRaisesRegex(ValueError,'complete'):audit.comparisons(observations,frequency)

    def test_one_failed_high_band_at_either_rate_cannot_be_averaged_away(self):
        frequency=np.array([60.,480.,1980.,6000.])
        observations={audit.case_key(nt,rate):tuple(np.ones(shape)
            for shape in [(4,2),(10,2),(10,3),(10,3)]) for nt in audit.MODES for rate in audit.RATES}
        rows=audit.comparisons(observations,frequency)
        criteria=dict(spatial_pressure_relative=.01,temporal_pressure_relative=.001,spatial_board_relative=.01)
        self.assertTrue(all(audit.comparison_gates(rows,criteria).values()))
        for i,row in enumerate(rows):
            original=row['complex_pressure_relative']['1980-6000']
            row['complex_pressure_relative']['1980-6000']=.01001 if row['kind']=='spatial' else .001001
            gates=audit.comparison_gates(rows,criteria)
            self.assertEqual(sum(not value for value in gates.values()),1)
            self.assertFalse(gates[row['kind']+'_'+row['case']+'_pressure'])
            row['complex_pressure_relative']['1980-6000']=original


if __name__=='__main__':unittest.main()
