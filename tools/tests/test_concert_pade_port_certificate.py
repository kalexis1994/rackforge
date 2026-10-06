"""Compact certificate against stored-factor convolution and original ledger."""
import sys
import unittest
from pathlib import Path

import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_streaming_air import SurfaceFactorBlocks,StreamingRationalAir
from concert_grand_pade_air import FourthFactorBlocks,PadeAir
from concert_grand_rational_air import RationalAir
from concert_grand_pade_port_certificate import certificate_ports,replay_endpoints


class PortCertificateTests(unittest.TestCase):
    def fixture(self):
        rng = np.random.default_rng(61007)
        surface = RectangularLoad(.7,.4,rng.normal(size=(7,5,3)))
        provider = FourthFactorBlocks(SurfaceFactorBlocks(surface,16000,4,8,3))
        H = np.concatenate(list(provider.blocks()),axis=1)
        reference = PadeAir(RationalAir(H,provider.rate))
        wave = PadeAir(StreamingRationalAir(provider,100_000_000,real_gram=True))
        return rng,H,reference,wave

    def test_compact_ports_match_independent_full_inputs_and_live_energy(self):
        rng,H,reference,wave = self.fixture()
        states,segments,expected_losses,work = [],[],[],[]
        for dt,initial in [(1/192000,reference.zero_state()),
                           (1/384000,rng.normal(size=reference.zero_state().shape)*.01)]:
            states.append(initial)
            first = len(states)-1
            velocities = rng.normal(size=(13,wave.modes))*.01
            state,w,loss = initial.copy(),0.,[]
            for velocity in velocities:
                state,force,inputs = reference.midpoint_deferred(state,velocity,dt)
                field = np.einsum('tom,tm->o',H,inputs)
                loss.append(float(field@field/2))
                w += float(velocity@force*dt)
            states.append(state)
            segments.append(dict(initial_index=first,final_index=first+1,dt=dt,velocity=velocities))
            expected_losses.append(loss)
            work.append(w)
        energies,losses,replay = certificate_ports(wave,np.array(states),segments,input_block=4)
        np.testing.assert_array_equal(replay_endpoints(wave,np.array(states),segments),replay)
        self.assertLess(float(replay.max()),1e-11)
        for state,energy in zip(states,energies):
            self.assertAlmostEqual(energy,reference.energy(state),delta=1e-11*max(energy,1e-10))
        for segment,loss,expected,w in zip(segments,losses,expected_losses,work):
            np.testing.assert_allclose(loss,expected,rtol=1e-11,atol=1e-15)
            self.assertTrue(np.all(loss>=0))
            before,after = segment['initial_index'],segment['final_index']
            residual = energies[after]-energies[before]+loss.sum()*segment['dt']-w
            self.assertLess(abs(residual)/max(energies[before],energies[after],abs(w)),1e-11)
        # A different chunk partition preserves observables and endpoints.
        other,flux,closure = certificate_ports(wave,np.array(states),segments,input_block=7)
        np.testing.assert_array_equal(other,energies)
        for a,b in zip(flux,losses):
            np.testing.assert_allclose(a,b,rtol=1e-13,atol=1e-15)
        np.testing.assert_array_equal(closure,replay)

    def test_wrong_port_history_is_rejected_by_endpoint_closure(self):
        rng,H,reference,wave = self.fixture()
        state = rng.normal(size=wave.zero_state().shape)*.01
        velocity = np.zeros((3,wave.modes))
        wrong_final = state.copy()
        with self.assertRaisesRegex(ValueError,'recorded acoustic endpoint'):
            certificate_ports(wave,np.array([state,wrong_final]),[
                dict(initial_index=0,final_index=1,dt=1/192000,velocity=velocity)])


if __name__=='__main__':
    unittest.main()
