import unittest
import numpy as np
from concert_grand_realtime_body import Body
from concert_grand_modal_runtime import ModalRuntime


class ModalRuntimeTests(unittest.TestCase):
    def test_modal_update_matches_coupled_loss_matrix_and_average_power(self):
        body = Body.mechanical([110., 115., 630.],
            [[2., .7, .1], [.7, 3., .2], [.1, .2, 5.]],
            [[.8, .1], [.7, -.2], [-.3, .9]])
        modal = ModalRuntime.from_body(body)
        np.testing.assert_allclose(modal.response([10., 50., 110.], [0, 1]),
                                   body.response([10., 50., 110.]), rtol=1e-10, atol=1e-14)
        rng = np.random.default_rng(5)
        for rate in [44100, 48000]:
            dense = body.held_step(rate)
            fast = modal.coefficients(rate)
            x = np.zeros(6)
            w = np.zeros(len(modal.roots), dtype=complex)
            for frame in range(100):
                f = rng.normal(size=2) if frame < 70 else np.zeros(2)
                x, y = dense.step(x, f)
                w, v = fast.step(w, f)
                np.testing.assert_allclose(modal.reconstruct(w), x, rtol=1e-10, atol=1e-14)
                np.testing.assert_allclose(v, y, rtol=1e-10, atol=1e-14)

    def test_overdamped_real_poles_are_not_duplicated(self):
        body = Body.mechanical([3.], [[11.]], [[.5]])
        modal = ModalRuntime.from_body(body)
        self.assertEqual(list(modal.multiplicity), [1., 1.])
        slow, fast = body.held_step(1000), modal.coefficients(1000)
        x, y = slow.step(np.zeros(2), [.7])
        w, v = fast.step(np.zeros(2, dtype=complex), [.7])
        np.testing.assert_allclose(modal.reconstruct(w), x, atol=1e-14)
        np.testing.assert_allclose(y, v, atol=1e-14)


if __name__ == '__main__':
    unittest.main()
