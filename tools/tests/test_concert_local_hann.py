import sys
import unittest
from pathlib import Path
import numpy as np
from numpy.polynomial.legendre import leggauss
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from concert_grand_local_hann import local_hann_spectra, truncation_bounds
from concert_grand_window_spectra import velocity_spectrum, hann_acceleration_spectrum


class LocalHannTests(unittest.TestCase):
    def trajectory(self):
        t = .007+np.arange(1153)/384000
        omega = np.array([270., 1400., 3900.])*2*np.pi
        B = np.sin((t-.007)[:, None]*omega)/omega
        V = np.cos((t-.007)[:, None]*omega)
        return t, B, V

    def test_independent_live_hermite_oracles(self):
        t, B, V = self.trajectory()
        f = np.array([0., 60., 6000., 16000., 24000.])
        actual = local_hann_spectra(t, B, V, f)
        expected = (velocity_spectrum(t, B, V, f, True, 24),
                    hann_acceleration_spectrum(t, B, V, f))
        # The older analytic A recurrence loses absolute precision at this
        # deliberately coarser synthetic grid. Check it at its absolute
        # roundoff scale; the independent node oracle below is stricter.
        for a, b, atol in zip(actual, expected, [2e-15, 1e-13]):
            np.testing.assert_allclose(a, b, rtol=2e-9, atol=atol)
        # Independently integrate actual A at Gauss nodes, including irregular
        # stored spacings; this detects accidental differentiation of w*V.
        g, w = leggauss(24)
        u = (g+1)/2
        dt = np.diff(t)
        c = np.diff(B, axis=0)/dt[:, None]-(V[:-1]+V[1:])/2
        A = (np.diff(V, axis=0)[:, None, :]+12*(.5-u)[None, :, None]*c[:, None, :])/dt[:, None, None]
        nodes = t[:-1, None]+dt[:, None]*u
        weighted = A*(dt[:, None]*w/2*np.sin(np.pi*(nodes-t[0])/(t[-1]-t[0]))**2)[:, :, None]
        oracle = np.array([np.sum(np.exp(-2j*np.pi*hz*nodes)[:, :, None]*weighted, axis=(0, 1)) for hz in f])
        np.testing.assert_allclose(actual[1], oracle, rtol=2e-9, atol=2e-14)

    def test_blocks_and_local_order(self):
        t, B, V = self.trajectory()
        f = [60., 16000., 24000.]
        base = local_hann_spectra(t, B, V, f)
        other = local_hann_spectra(t, B, V, f, order=12, frequency_block=1, segment_block=37)
        for a, b in zip(base, other):
            np.testing.assert_allclose(a, b, rtol=2e-9, atol=2e-15)

    def test_cancellation_tail_and_zero_acceleration(self):
        t = np.arange(9217)/3072000
        B = t[:, None]
        V = np.ones_like(B)
        f = np.array([24017., 23983.])
        v, a = local_hann_spectra(t, B, V, f)
        # Closed integral of Hann times constant velocity, away from zeros.
        T = t[-1]
        def integral(hz):
            return np.expm1(-2j*np.pi*hz*T)/(-2j*np.pi*hz)
        expected = .5*integral(f)-.25*integral(f-1/T)-.25*integral(f+1/T)
        np.testing.assert_allclose(v[:, 0], expected, rtol=1e-9, atol=1e-18)
        self.assertEqual(float(np.max(abs(a))), 0.)
        self.assertLess(truncation_bounds(t, f)['window_weight_absolute'], 1e-30)

    def test_reject_unsafe_expansion_or_inputs(self):
        t, B, V = self.trajectory()
        for kwargs in [dict(order=5), dict(frequency_block=0), dict(segment_block=1.5)]:
            with self.assertRaises(ValueError):
                local_hann_spectra(t, B, V, [24000.], **kwargs)
        with self.assertRaises(ValueError):
            local_hann_spectra(t, B, V, [1e6])
        with self.assertRaises(ValueError):
            local_hann_spectra(t[::-1], B, V, [60.])


if __name__ == '__main__':
    unittest.main()
