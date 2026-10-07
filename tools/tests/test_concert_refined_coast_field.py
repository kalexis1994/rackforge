"""Refined coast certificate must keep the physical wave immutable."""
import sys
import unittest
import importlib.util
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
sys.path.insert(0,str(Path(__file__).resolve().parent))
from test_concert_bounded_observer import BoundedObserverTests
from concert_grand_bounded_observer import certificate_observer
from concert_grand_window_field_probe import ProbeProvider,observation_wave_view
spec=importlib.util.spec_from_file_location('refined_audit',Path(__file__).resolve().parents[1]/'audit-concert-grand-refined-coast-field.py')
audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)


class RefinedCoastFieldTests(unittest.TestCase):
    def test_probe_view_preserves_storage_flux_bounds_and_authentic_provider(self):
        _,wave,states,segments,_=BoundedObserverTests().fixture()
        Q=np.eye(3)[:,:2];provider=wave.base.provider;R=wave.R
        expected,powers,bounds=certificate_observer(wave,states,segments,Q,input_block=4,store_projected_inputs=True)
        probe=ProbeProvider(provider,wave,states,segments,Q,(0,3),frequency_block=3)
        view=observation_wave_view(wave,probe)
        actual,observed,new_bounds=certificate_observer(view,states,segments,Q,input_block=4,store_projected_inputs=True)
        np.testing.assert_array_equal(actual,expected)
        for a,b in zip(powers,observed):np.testing.assert_array_equal(a,b)
        for name in ['storage_intervals','radiation_intervals','full_endpoint_replay_relative',
                     'factor_norm_relative_error','projected_factor_norm_relative_error']:
            self.assertEqual(new_bounds[name],bounds[name])
        self.assertIs(wave.base.provider,provider);self.assertIs(wave.R,R);self.assertIs(view.R,R)
        self.assertEqual([b['block'] for b in probe.results],[0,3])
        for block in probe.results:
            for segment in block['segments']:
                self.assertLess(segment['replay_relative'],1e-11)
                for trial in segment['trials']:
                    self.assertLess(trial['complex_field_relative'],1e-10)
                    self.assertLess(trial['radiation_relative'],1e-10)

    def test_probe_rejects_reset_endpoint(self):
        _,wave,states,segments,_=BoundedObserverTests().fixture()
        states=states.copy();states[1]+=.001
        probe=ProbeProvider(wave.base.provider,wave,states,segments,np.eye(3),(0,))
        with self.assertRaisesRegex(ValueError,'endpoint'):next(probe.blocks())

    def test_native_comparison_does_not_interpolate_or_accept_shifted_times(self):
        traces=[];rates=[64000,128000,256000]
        for rate in rates:
            t=np.arange(round(.0005*rate)+1)/rate
            exact=1+t[:,None]*np.array([2.,3.])
            error=(rates[0]/rate)**2*.0001
            traces.append((t,exact+error,exact*2+error))
        measured=audit.native_comparison(traces,rates)
        self.assertFalse(measured['interpolation_used']);self.assertEqual(measured['samples'],33)
        self.assertAlmostEqual(measured['ratio'],.2,places=10)
        t,B,V=traces[1];traces[1]=(t+.00001,B,V)
        with self.assertRaisesRegex(ValueError,'identical native'):audit.native_comparison(traces,rates)


if __name__=='__main__':unittest.main()
