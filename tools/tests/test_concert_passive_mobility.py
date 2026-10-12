import unittest
import numpy as np
from concert_grand_passive_mobility import MobilityBank


class PassiveMobilityTests(unittest.TestCase):
    def test_shared_real_modes_keep_positive_power_and_physical_work(self):
        bank = MobilityBank([170., 690., 1300.], [2., 6., 17.], [[.8, -.2], [.3, .7], [-.4, .2]])
        for h in bank.response([1., 20., 90., 210., 6000.]):
            np.testing.assert_allclose(h, h.T, atol=1e-15)
            self.assertGreaterEqual(np.linalg.eigvalsh(h.real).min(), -1e-14)
        for rate in [44100, 48000]:
            coefficients = bank.coefficients(rate)
            x = np.zeros((3, 2))
            for frame in range(300):
                force = np.array([.8, -.3]) if frame < 110 else np.zeros(2)
                new, average = bank.step(x, force, rate, coefficients)
                change = (np.sum(new**2)-np.sum(x**2))/2
                self.assertLessEqual(change-force @ average/rate, 1e-14)
                x = new

    def test_endpoint_velocity_is_not_the_power_port(self):
        bank = MobilityBank([7000.], [18.], [[1.]])
        new, average = bank.step(np.zeros((1, 2)), [2.], 44100, bank.coefficients(44100))
        self.assertGreater(abs(new[0, 1]-average[0]), 1e-7)
        energy = np.sum(new**2)/2
        self.assertLessEqual(energy, 2*average[0]/44100+1e-18)


if __name__ == '__main__':
    unittest.main()
