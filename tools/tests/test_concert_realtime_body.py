import unittest
import numpy as np
from scipy.integrate import solve_ivp
from concert_grand_realtime_body import Body, modal_response


class RealtimeBodyTests(unittest.TestCase):
    def setUp(self):
        self.body = Body.mechanical([170., 480., 920.],
            [[2., .3, 0.], [.3, 4., .5], [0., .5, 8.]],
            [[.3, -.2], [.1, .5], [-.4, .1]])

    def test_held_force_work_and_free_decay_are_passive(self):
        step = self.body.held_step(44100)
        rng = np.random.default_rng(8)
        x = rng.normal(size=6)
        for i in range(180):
            force = rng.normal(size=2) if i < 80 else np.zeros(2)
            new, average_velocity = step.step(x, force)
            change = (new @ new - x @ x) / 2
            work = step.dt * force @ average_velocity
            self.assertLessEqual(change - work, 2e-14)
            if i >= 80:
                self.assertLessEqual(new @ new, x @ x + 2e-14)
            x = new

    def test_exact_step_and_average_port_against_independent_ode(self):
        step = self.body.held_step(44100)
        x = np.arange(6) / 10
        force = np.array([2., -1.])
        def derivative(t, z):
            return np.r_[self.body.a @ z[:6] + self.body.b @ force,
                         self.body.b.T @ z[:6]]
        reference = solve_ivp(derivative, (0, step.dt), np.r_[x, [0., 0.]],
            method='DOP853', rtol=1e-12, atol=1e-14, max_step=step.dt / 8).y[:, -1]
        new, average = step.step(x, force)
        np.testing.assert_allclose(new, reference[:6], rtol=1e-12, atol=1e-14)
        np.testing.assert_allclose(average, reference[6:] / step.dt, rtol=1e-12, atol=1e-14)

    def test_projection_preserves_reciprocity_and_passivity(self):
        # Paired energy coordinates, two physical modes, no loss diagonalization.
        rng = np.random.default_rng(4)
        basis, _ = np.linalg.qr(rng.normal(size=(3, 2)))
        reduced = self.body.project_mechanical(basis)
        for response in reduced.response([1., 27., 95., 410.]):
            np.testing.assert_allclose(response, response.T, atol=1e-15)
            self.assertGreaterEqual(np.linalg.eigvalsh((response + response.conj().T)/2).min(), -1e-14)
        np.testing.assert_allclose(self.body.response([27., 95.]),
            modal_response(np.array([170., 480., 920.]), -self.body.a[3:, 3:], self.body.b[3:], [27., 95.]),
            rtol=1e-12, atol=1e-14)

    def test_invalid_active_or_nonorthogonal_model_rejected(self):
        with self.assertRaisesRegex(ValueError, 'passive'):
            Body(np.eye(2), np.ones((2, 1)))
        with self.assertRaisesRegex(ValueError, 'orthonormal'):
            self.body.project(np.ones((6, 2)))
        with self.assertRaisesRegex(ValueError, 'reciprocal'):
            Body.mechanical([1., 2.], [[1., 2.], [0., 1.]], np.ones((2, 1)))


if __name__ == '__main__':
    unittest.main()
