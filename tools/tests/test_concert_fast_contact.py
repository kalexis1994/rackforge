"""Check accelerated equations against live reference and storage ledgers."""
import sys, unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
sys.path.insert(0,str(Path(__file__).resolve().parent))
import numpy as np
import test_concert_bounded_contact as reference_tests
from concert_grand_fast_contact import FastBoundedContact


class FastContactTests(unittest.TestCase):
    def test_full_nonlinear_gradient_matches_reference_avf(self):
        original = reference_tests.BoundedContactTests().fixture(rate_loss=.1)
        fast = FastBoundedContact(original.system,original.air)
        rng = np.random.default_rng(13)
        for scale in [0.,1e-7,1e-5]:
            a = rng.normal(size=fast.n)*scale
            b = a+rng.normal(size=fast.n)*scale*.02
            g, _, stats = original.system.avf(a,b,1/768000)
            fg, fs = fast.avf_gradient(a,b,1/768000)
            np.testing.assert_allclose(fg,g,rtol=3e-14,atol=1e-13)
            self.assertEqual(fs['force_n'],stats['force_n'])
            self.assertEqual(fs['felt_loss_j'],stats['felt_loss_j'])

    def test_live_states_and_independent_energy_against_scalar_reference(self):
        for rate in [128000,384000,768000]:
            original = reference_tests.BoundedContactTests().fixture(rate_loss=.1)
            fast = FastBoundedContact(original.system,original.air)
            a = original.initial_state(5.)
            a[2][:] = np.linspace(-.0002,.0003,a[2].size).reshape(a[2].shape)
            a[3][:] = [.01,-.02,.03]
            b = tuple(x.copy() for x in a)
            E0 = original.energy(a)['total_j']; lost = 0.
            for _ in range(120):
                a, sa = original.step(a,1/rate)
                b, sb = fast.step(b,1/rate)
                for expected,actual in zip(a,b):
                    # A relative vector error remains meaningful at component
                    # zero crossings; the reference Newton stopping tolerance
                    # also introduces tiny differences in those components.
                    error = np.linalg.norm(actual-expected)/max(np.linalg.norm(expected),1e-30)
                    self.assertLess(error,2e-9)
                for key in ['force_n','felt_loss_j','material_loss_j','radiation_loss_j','air_port_work_j']:
                    np.testing.assert_allclose(sb[key],sa[key],rtol=2e-8,atol=2e-15)
                lost += sb['loss_j']
                self.assertLess(abs(fast.energy(b)['total_j']+lost-E0)/E0,1e-9)

    def test_acceleration_keeps_decoupled_air_unforced(self):
        original = reference_tests.BoundedContactTests().fixture(coupling=0.)
        fast = FastBoundedContact(original.system,original.air,coupling=0.)
        state = fast.initial_state(5.)
        for _ in range(20): state, _ = fast.step(state,1/768000,True)
        np.testing.assert_array_equal(state[2],0.)
        np.testing.assert_array_equal(state[3],0.)
        with self.assertRaises(ValueError): fast.step(state,0.)


if __name__ == '__main__': unittest.main()
