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
from concert_grand_cell_factor_projection import projected_cell_provider


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

    def test_cell_projection_matches_fields_storage_flux_and_residual_bounds(self):
        reference,wave,states,segments,flux = self.fixture()
        for Q in [np.linalg.qr(np.random.default_rng(94).normal(size=(3,3)))[0],np.eye(3)[:,:2]]:
            direct,power,bounds = certificate_observer(wave,states,segments,Q,input_block=4,
                                                      store_projected_inputs=True)
            cells,cpower,cbounds = certificate_observer(wave,states,segments,Q,input_block=4,
                store_projected_inputs=True,factor_projection='cells',projection_probe_blocks=(0,2,5))
            np.testing.assert_allclose(cells,direct,rtol=1e-12,atol=1e-18)
            for a,b in zip(power,cpower):np.testing.assert_allclose(a,b,rtol=1e-12,atol=1e-18)
            for exact,interval in zip(flux,cbounds['radiation_intervals']):
                self.assertLessEqual(interval['lower_j'],exact)
                self.assertGreaterEqual(interval['upper_j'],exact)
            for state,interval in zip(states,cbounds['storage_intervals']):
                self.assertLessEqual(interval['lower_j'],reference.energy(state))
                self.assertGreaterEqual(interval['upper_j'],reference.energy(state))
            for key in ['full_endpoint_replay_relative','residual_input_integral','factor_norm_relative_error']:
                self.assertEqual(bounds[key],cbounds[key])
            self.assertLess(max(p['relative'] for p in cbounds['factor_projection']['probe_results']),1e-12)

    def test_cell_projection_preserves_folded_geometry_and_all_derivative_variants(self):
        rng=np.random.default_rng(19)
        shapes=rng.normal(size=(6,4,3));shapes=(shapes+shapes[:,::-1])/2
        Q=np.linalg.qr(rng.normal(size=(3,3)))[0][:,:2]
        for fold in [False,True]:
            for derivative in ['raw','smooth','fourth']:
                base=SurfaceFactorBlocks(RectangularLoad(.7,.4,shapes),16000,4,8,3,
                                         smooth=derivative!='raw',fold_y=fold)
                provider=FourthFactorBlocks(base) if derivative=='fourth' else base
                original=base.shapes.copy();view=projected_cell_provider(provider,Q)
                for H,hq in zip(provider.blocks(),view.blocks(),strict=True):
                    expected=(H.reshape(-1,3)@Q).reshape(hq.shape)
                    np.testing.assert_allclose(hq,expected,rtol=1e-10,atol=1e-13)
                    np.testing.assert_array_equal(hq[:,1::2],-hq[::-1,::2])
                np.testing.assert_array_equal(base.shapes,original)
                self.assertEqual(base.modes,3)
                self.assertIs(view.directions,provider.directions)
                self.assertIs(view.weights,provider.weights)
                self.assertIs(view.positions,provider.positions)
                self.assertEqual(view.taps,provider.taps)
                self.assertEqual(view.outputs,provider.outputs)

    def test_invalid_projection_method_and_probe_are_rejected(self):
        _,wave,states,segments,_=self.fixture()
        with self.assertRaisesRegex(ValueError,'factor projection'):
            certificate_observer(wave,states,segments,np.eye(3),factor_projection='unknown')
        with self.assertRaisesRegex(ValueError,'out of range'):
            certificate_observer(wave,states,segments,np.eye(3),factor_projection='cells',projection_probe_blocks=(999,))


if __name__=='__main__':
    unittest.main()
