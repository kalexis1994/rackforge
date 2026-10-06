"""Observer intervals against independent full-factor storage and fields."""
import sys
import unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_streaming_air import SurfaceFactorBlocks,StreamingRationalAir
from concert_grand_pade_air import FourthFactorBlocks,PadeAir
from concert_grand_rational_air import RationalAir
from concert_grand_bounded_observer import certificate_observer,observation_basis


class BoundedObserverTests(unittest.TestCase):
    def fixture(self):
        rng = np.random.default_rng(61008)
        provider = FourthFactorBlocks(SurfaceFactorBlocks(
            RectangularLoad(.7,.4,rng.normal(size=(7,5,3))),16000,4,8,3))
        H = np.concatenate(list(provider.blocks()),axis=1)
        reference = PadeAir(RationalAir(H,provider.rate))
        wave = PadeAir(StreamingRationalAir(provider,100_000_000,real_gram=True))
        states,segments,flux = [],[],[]
        for initial in [reference.zero_state(),rng.normal(size=wave.zero_state().shape)*.01]:
            states.append(initial)
            first = len(states)-1
            v = rng.normal(size=(9,wave.modes))*.01
            state,total = initial.copy(),0.
            for velocity in v:
                state,_,u = reference.midpoint_deferred(state,velocity,1/192000)
                field = np.einsum('tom,tm->o',H,u)
                total += float(field@field/2/192000)
            states.append(state)
            segments.append(dict(initial_index=first,final_index=first+1,dt=1/192000,velocity=v))
            flux.append(total)
        return reference,wave,np.array(states),segments,flux

    def test_omitted_directions_are_bounded_against_full_nonzero_fields(self):
        reference,wave,states,segments,flux = self.fixture()
        _,_,bounds = certificate_observer(wave,states,segments,np.eye(3)[:,:1],input_block=4)
        for state,interval in zip(states,bounds['storage_intervals']):
            exact = reference.energy(state)
            self.assertLessEqual(interval['lower_j'],exact)
            self.assertGreaterEqual(interval['upper_j'],exact)
        for exact,interval in zip(flux,bounds['radiation_intervals']):
            self.assertGreater(exact,0.)
            self.assertLessEqual(interval['lower_j'],exact)
            self.assertGreaterEqual(interval['upper_j'],exact)
        self.assertLess(max(bounds['full_endpoint_replay_relative']),1e-11)

    def test_full_basis_matches_independent_convolution_and_basis_selection(self):
        reference,wave,states,segments,flux = self.fixture()
        Q,metadata = observation_basis(wave,states,segments)
        self.assertEqual(metadata['rank'],wave.modes)
        energy,loss,bounds = certificate_observer(wave,states,segments,Q,input_block=3)
        expected = [reference.energy(state) for state in states]
        np.testing.assert_allclose(energy,expected,rtol=1e-11,atol=1e-15)
        for power,exact,interval in zip(loss,flux,bounds['radiation_intervals']):
            self.assertAlmostEqual(power.sum()/192000,exact,delta=1e-11*max(exact,1e-10))
            self.assertLessEqual(interval['lower_j'],exact)
            self.assertGreaterEqual(interval['upper_j'],exact)
        for exact,interval in zip(expected,bounds['storage_intervals']):
            self.assertLessEqual(interval['lower_j'],exact)
            self.assertGreaterEqual(interval['upper_j'],exact)

    def test_bad_basis_and_mismatched_memory_are_rejected(self):
        _,wave,states,segments,_ = self.fixture()
        with self.assertRaisesRegex(ValueError,'orthonormal'):
            certificate_observer(wave,states,segments,np.eye(3)*2)
        wrong = states.copy()
        wrong[1] += .001
        with self.assertRaisesRegex(ValueError,'endpoint memory'):
            certificate_observer(wave,wrong,segments,np.eye(3))

    def test_cached_observation_inputs_match_replay_with_living_memory(self):
        _,wave,states,segments,_ = self.fixture()
        Q = np.eye(3)[:,:2]
        energy,power,bounds = certificate_observer(wave,states,segments,Q,input_block=4)
        stored,stored_power,stored_bounds = certificate_observer(
            wave,states,segments,Q,input_block=4,store_projected_inputs=True)
        np.testing.assert_array_equal(stored,energy)
        for a,b in zip(stored_power,power):
            np.testing.assert_array_equal(a,b)
        self.assertEqual(stored_bounds['storage_intervals'],bounds['storage_intervals'])
        self.assertEqual(stored_bounds['radiation_intervals'],bounds['radiation_intervals'])
        self.assertEqual(stored_bounds['full_endpoint_replay_relative'],bounds['full_endpoint_replay_relative'])
        self.assertGreater(stored_bounds['projected_input_history_bytes'],0)
        self.assertEqual(bounds['projected_input_history_bytes'],0)


if __name__=='__main__':
    unittest.main()
