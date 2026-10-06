"""Two-state accelerated contact/coast with independent work and ODE gates."""
import sys
import unittest
from pathlib import Path

import numpy as np
from scipy.integrate import solve_ivp

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import test_concert_bounded_contact as old_fixture
from concert_grand_rational_air import RationalAir
from concert_grand_pade_air import PadeAir, PadeBoundedAir, fourth_factor
from concert_grand_bounded_contact import BoundedContact
from concert_grand_fast_contact import FastBoundedContact
from concert_grand_exponential_coast import ExponentialCoast


def fixture():
    old = old_fixture.BoundedContactTests().fixture(rate_loss=.1)
    rng = np.random.default_rng(61006)
    half = rng.normal(size=(5, 3, 3))
    half[-1] = -half[:-1].sum(axis=0)
    H = np.zeros((6, 6, 3))
    H[:-1, ::2] += half/2
    H[1:, ::2] += half/2
    H[:, 1::2] = -H[::-1, ::2]
    wave = PadeAir(RationalAir(fourth_factor(H), 16000))
    air = PadeBoundedAir(wave, wave.static_mass()+np.diag([.04, .03, .02]),
                        np.diag([20., 25., 30.]))
    return BoundedContact(old.system, air)


class PadeContactTests(unittest.TestCase):
    def test_fast_contact_live_relaunch_and_loss(self):
        for rate in [128000, 384000, 768000]:
            reference = fixture()
            fast = FastBoundedContact(reference.system, reference.air)
            a = reference.initial_state(5.)
            a[2][:] = np.linspace(-.0002, .0003, a[2].size).reshape(a[2].shape)
            a[3][:] = [.01, -.02, .03]
            b = tuple(x.copy() for x in a)
            initial = reference.energy(a)['total_j']
            loss = 0.
            for _ in range(96):
                a, sa = reference.step(a, 1/rate)
                b, sb = fast.step(b, 1/rate)
                for expected, actual in zip(a, b):
                    self.assertLess(np.linalg.norm(actual-expected)
                                    / max(np.linalg.norm(expected), 1e-30), 2e-9)
                for key in ['force_n', 'felt_loss_j', 'material_loss_j',
                            'radiation_loss_j', 'air_port_work_j']:
                    np.testing.assert_allclose(sb[key], sa[key], rtol=2e-8, atol=2e-15)
                loss += sb['loss_j']
                self.assertGreaterEqual(sb['radiation_loss_j'], 0)
                self.assertLess(abs(fast.energy(b)['total_j']+loss-initial)/initial, 1e-9)
            # Prescribed hammer relaunch after separation keeps both wave coordinates.
            b[0][-1] = -float(fast.system.direction[:-1] @ b[0][:-1]) / fast.system.direction[-1] - .001
            b[1][-1] = -.1
            before = fast.energy(b)['total_j']
            new, work = fast.relaunch(b, 3.)
            np.testing.assert_array_equal(new[2], b[2])
            np.testing.assert_array_equal(new[3], b[3])
            self.assertAlmostEqual(fast.energy(new)['total_j']-before, work, delta=1e-13)
            # Deferred radiation uses the same live states and factor after relaunch.
            a, sa = reference.step(new, 1/rate)
            b, sb = fast.step(new, 1/rate, True)
            for expected, actual in zip(a, b):
                np.testing.assert_allclose(actual, expected, rtol=2e-8, atol=2e-13)
            radiation = sb['radiation_loss_j'] + fast.air.wave.radiation_batch(sb['wave_inputs'][None])[0]/rate
            np.testing.assert_allclose(radiation, sa['radiation_loss_j'], rtol=2e-8, atol=2e-15)

    def test_coast_live_ledger_and_ode_refinement(self):
        reference = fixture()
        coast = ExponentialCoast(reference.system, reference.air)
        rng = np.random.default_rng(20)
        initial = reference.initial_state(0.)
        initial[0][:-1] = rng.normal(size=coast.n-1)*1e-7
        initial[0][-1] = -.001
        initial[1][:-1] = rng.normal(size=coast.n-1)*.005
        initial[1][-1] = -.1
        initial[2][:] = np.linspace(-.0002, .0003, initial[2].size).reshape(initial[2].shape)
        initial[3][:] = [.01, -.02, .03]
        state = tuple(x.copy() for x in initial)
        energy0 = coast.energy(state)
        loss = radiation = work = 0.
        for _ in range(96):
            state, stats = coast.step(state, 1/32000)
            loss += stats['loss_j']
            radiation += stats['radiation_loss_j']
            work += stats['air_port_work_j']
            energy = coast.energy(state)
            self.assertLess(abs(energy['total_j']+loss-energy0['total_j'])/energy0['total_j'], 1e-9)
            self.assertLess(abs(energy['air_j']+radiation-work-energy0['air_j'])/energy0['total_j'], 1e-9)
            self.assertGreaterEqual(stats['radiation_loss_j'], 0)
        shapes = [x.shape for x in initial]
        cuts = np.cumsum([0]+[x.size for x in initial])
        pack = lambda state: np.concatenate([x.ravel() for x in state])
        unpack = lambda y: tuple(y[cuts[i]:cuts[i+1]].reshape(shapes[i]) for i in range(4))
        ode = solve_ivp(lambda t, y: pack(reference.derivative(unpack(y))), (0, .0005), pack(initial),
                        method='DOP853', rtol=1e-11, atol=1e-15, max_step=1/1000000)
        self.assertTrue(ode.success)
        errors = []
        for rate in [64000, 128000, 256000]:
            state = tuple(x.copy() for x in initial)
            for _ in range(round(rate*.0005)):
                state, _ = coast.step(state, 1/rate, True)
            errors.append(np.linalg.norm(pack(state)-ode.y[:, -1])/np.linalg.norm(ode.y[:, -1]))
        self.assertTrue(all(errors[i+1] < .3*errors[i] for i in [0, 1]), errors)
        self.assertLess(errors[-1], .001)


if __name__ == '__main__':
    unittest.main()
