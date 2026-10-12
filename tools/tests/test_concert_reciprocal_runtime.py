import unittest
import numpy as np
from concert_grand_realtime_body import Body
from concert_grand_modal_runtime import ModalRuntime
from concert_grand_reciprocal_runtime import ReciprocalRuntime


class ReciprocalRuntimeTests(unittest.TestCase):
    def test_shared_row_keeps_coupled_loss_response_states_and_work(self):
        body = Body.mechanical([110., 115., 630.], [[2., .7, .1], [.7, 3., .2], [.1, .2, 5.]],
                               [[.8, .1], [.7, -.2], [-.3, .9]])
        original = ModalRuntime.from_body(body)
        shared = ReciprocalRuntime(original)
        np.testing.assert_allclose(original.response([3., 21., 42., 130.], [0, 1]),
            shared.modal.response([3., 21., 42., 130.], [0, 1]), rtol=1e-11, atol=1e-14)
        first, second = original.coefficients(44100), shared.modal.coefficients(44100)
        a = np.zeros(len(original.roots), dtype=complex)
        b = a.copy()
        for frame in range(150):
            force = np.array([.7, -.2]) if frame < 80 else np.zeros(2)
            a, y = first.step(a, force)
            b, z = second.step(b, force)
            np.testing.assert_allclose(y, z, rtol=1e-10, atol=1e-14)
            np.testing.assert_allclose(original.reconstruct(a), shared.modal.reconstruct(b), atol=1e-14)

    def test_real_overdamped_residues_keep_their_sign(self):
        modal = ModalRuntime.from_body(Body.mechanical([3.], [[11.]], [[.5]]))
        shared = ReciprocalRuntime(modal)
        np.testing.assert_allclose(shared.modal.response([1., 3., 7.], [0]), modal.response([1., 3., 7.], [0]), atol=1e-14)


if __name__ == '__main__':
    unittest.main()
