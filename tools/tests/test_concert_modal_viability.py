import unittest
import numpy as np
from concert_grand_native_strings import fixture
from concert_grand_modal_viability import modal_indices, project, pulse_fixture, omega, state_errors


class ModalViabilityTests(unittest.TestCase):
    def test_projection_keeps_identical_coordinates_frequencies_and_head(self):
        full=fixture(transverse=12,seed=83,duration=.012)
        reduced,indices=project(full,4,32000)
        self.assertEqual(len(indices),48);self.assertEqual(len(set(indices)),48)
        for key in ['q','v','endpoint']:np.testing.assert_array_equal(reduced[key],np.r_[full[key][indices],full[key][-1]])
        np.testing.assert_allclose(omega(reduced),omega(full)[indices],rtol=1e-14)
        expected=state_errors(full,{'q':full['q'],'v':full['v']},{'q':reduced['q'],'v':reduced['v']},indices)
        self.assertEqual(expected['retained_state_relative'],0.)
        self.assertAlmostEqual(expected['full_state_relative']**2,expected['omitted_linear_energy_fraction'])
        with self.assertRaises(ValueError):modal_indices(4,8)

    def test_prescribed_pulse_quadrature_is_resolved_and_projection_has_no_new_random_state(self):
        a=pulse_fixture(order=256);b=pulse_fixture(order=512)
        np.testing.assert_allclose(a['q'],b['q'],rtol=1e-10,atol=1e-18)
        np.testing.assert_allclose(a['v'],b['v'],rtol=1e-10,atol=1e-16)
        reduced,indices=project(a,16,16000)
        np.testing.assert_array_equal(reduced['q'][:-1],a['q'][indices])
        self.assertTrue(np.all(reduced['launch_speeds']==0.))


if __name__=='__main__':unittest.main()
