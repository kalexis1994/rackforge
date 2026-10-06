"""Fast cascade against a dense independent midpoint ODE and live work."""
import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from concert_grand_rational_air import RationalAir
from concert_grand_pade_air import PadeAir, fourth_factor
from concert_grand_pade_midpoint import PadeMidpoint


class PadeMidpointTests(unittest.TestCase):
    def test_dense_midpoint_and_live_work(self):
        rng = np.random.default_rng(61006)
        factor = rng.normal(size=(32, 4, 3))
        factor[:, 1::2] = -factor[::-1, ::2]
        # Enforce DC by constructing a difference before adjacent averaging.
        raw = np.diff(np.concatenate((np.zeros_like(factor[:1]), factor,
                                      np.zeros_like(factor[:1]))), axis=0)
        smooth = np.zeros((len(raw)+1,) + raw.shape[1:])
        smooth[:-1] += raw/2
        smooth[1:] += raw/2
        smooth[:, 1::2] = -smooth[::-1, ::2]
        wave = PadeAir(RationalAir(fourth_factor(smooth), 128000))
        k, m = wave.order, wave.modes
        # Assemble the continuous ODE using u_j=u_0-12*sum(x2_l,l<j).
        # This dense solve has no scalar cascade or lfilter recurrence.
        A = np.zeros((2*k, 2*k))
        U = np.zeros(2*k)
        for j in range(k):
            A[2*j, 2*j+1] = wave.rate
            A[2*j+1, 2*j] = -12*wave.rate
            A[2*j+1, 2*j+1] = -6*wave.rate
            A[2*j+1, 1:2*j:2] = -12*wave.rate
            U[2*j+1] = wave.rate
        for dt in [1/(10*wave.rate), 1/wave.rate, 10/wave.rate]:
            state = rng.normal(size=(k, 2, m)) * .01
            velocity = rng.normal(size=m)
            fast = PadeMidpoint(wave, dt)
            new, force, inputs = fast.midpoint_deferred(state, velocity)
            dense = np.linalg.solve(np.eye(2*k)-dt*A/2,
                                    (np.eye(2*k)+dt*A/2) @ state.reshape(2*k, m)
                                    + dt*U[:, None]*velocity).reshape(k, 2, m)
            np.testing.assert_allclose(new, dense, rtol=2e-11, atol=2e-12)
            center = (state + dense)/2
            dense_inputs = np.vstack((velocity, velocity-12*np.cumsum(center[:, 1], axis=0)))
            dense_force = wave.D @ velocity + np.einsum('kmn,kn->m', wave.R[1:], dense_inputs[1:])
            np.testing.assert_allclose(inputs, dense_inputs, rtol=2e-11, atol=2e-12)
            np.testing.assert_allclose(force, dense_force, rtol=2e-11, atol=2e-11)
            scalar, scalar_force, scalar_inputs = wave.midpoint_deferred(state, velocity, dt)
            np.testing.assert_allclose(new, scalar, rtol=2e-11, atol=2e-12)
            np.testing.assert_allclose(force, scalar_force, rtol=2e-11, atol=2e-11)
            np.testing.assert_allclose(inputs, scalar_inputs, rtol=2e-11, atol=2e-12)
            energy0, energy1 = wave.energy(state), wave.energy(new)
            loss = dt * wave.radiation_batch(inputs[None])[0]
            work = dt * velocity @ force
            self.assertGreaterEqual(loss, 0)
            self.assertLess(abs(energy1-energy0+loss-work)
                            / max(energy0, energy1, loss, abs(work)), 2e-11)

    def test_residue_kernels_keep_live_force_and_memory(self):
        rng = np.random.default_rng(61106)
        H = rng.normal(size=(18, 6, 5))
        H = np.diff(np.concatenate((np.zeros_like(H[:1]), H, np.zeros_like(H[:1]))), axis=0)
        H[:, 1::2] = -H[::-1, ::2]
        wave = PadeAir(RationalAir(H, 128000))
        for rate in [64000, 384000, 1536000]:
            ref = PadeMidpoint(wave, 1/rate, 'reference')
            batched = PadeMidpoint(wave, 1/rate, 'batched')
            state = rng.normal(size=(wave.order, 2, wave.modes))*.01
            for _ in range(48):
                velocity = rng.normal(size=wave.modes)
                a, fa, ia = ref.midpoint_deferred(state, velocity)
                b, fb, ib = batched.midpoint_deferred(state, velocity)
                np.testing.assert_array_equal(a, b)
                np.testing.assert_array_equal(ia, ib)
                self.assertLess(np.linalg.norm(fa-fb)/max(np.linalg.norm(fa), 1e-30), 3e-14)
                state = a
        with self.assertRaises(ValueError):
            PadeMidpoint(wave, 1/64000, 'unknown')


if __name__ == '__main__':
    unittest.main()
