import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np
from scipy.signal import resample_poly
from scipy.io import wavfile
from numpy.polynomial.legendre import leggauss
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from concert_grand_pressure_stream import (source_coefficients, HermitePressureStream,
                                          PolyphaseStream, PhysicalWavStream)
from concert_grand_cell_audio import pressure_fft


class PressureStreamTests(unittest.TestCase):
    def fixture(self, frames=3074):
        t = np.arange(frames) / 128000
        omega = 2 * np.pi * np.array([261., 2100., 11000.])
        b = 1e-7 * np.sin(t[:, None] * omega)
        v = 1e-7 * omega * np.cos(t[:, None] * omega)
        rng = np.random.default_rng(291)
        kernel = np.zeros((45, 2, 3))
        kernel[17:39] = rng.normal(size=(22, 2, 3)) * .0001
        return (t, b, v), kernel

    def blocks(self, trace, sizes=(1, 71, 223, 509, 3)):
        t, b, v = trace
        begin = count = 0
        while begin < len(t) - 1:
            end = min(begin + sizes[count % len(sizes)], len(t) - 1)
            yield begin / 128000, (np.arange(end - begin + 1) / 128000, b[begin:end + 1], v[begin:end + 1])
            begin = end
            count += 1

    def direct(self, trace, kernel):
        _, b, v = trace
        nodes, h0, h1 = source_coefficients()
        left = -int(nodes[0])
        change = np.diff(v, axis=0)
        curvature = np.diff(b, axis=0) * 128000 - (v[:-1] + v[1:]) / 2
        size = len(change) + len(kernel) + int(nodes[-1]) - 1
        pressure = np.zeros((size, kernel.shape[1]))
        for c in range(kernel.shape[1]):
            for mode in range(kernel.shape[2]):
                k0 = np.convolve(kernel[:, c, mode], h0)[left:]
                k1 = np.convolve(kernel[:, c, mode], h1)[left:]
                pressure[:, c] += np.convolve(change[:, mode], k0) * 128000
                pressure[:, c] += np.convolve(curvature[:, mode], k1) * 128000
        return pressure

    def test_integrated_moments_against_independent_continuous_quadrature(self):
        nodes, h0, h1 = source_coefficients()
        for degree in range(6):
            self.assertAlmostEqual(float(h0 @ nodes.astype(float)**degree), 1 / (degree + 1), places=11)
            self.assertAlmostEqual(float(h1 @ nodes.astype(float)**degree), 12 * (.5 / (degree + 1) - 1 / (degree + 2)), places=11)
        f = np.geomspace(60., 12000., 601)
        x, w = leggauss(32)
        x, w = (x + 1) / 2, w / 2
        analytic = np.exp(-2j * np.pi * f[:, None] * x / 128000)
        ideal0, ideal1 = analytic @ w, analytic @ (12 * (.5 - x) * w)
        discrete = np.exp(-2j * np.pi * f[:, None] * nodes / 128000)
        self.assertLess(np.max(abs(discrete @ h0 - ideal0) / abs(ideal0)), 1e-6)
        self.assertLess(np.max(abs(discrete @ h1 - ideal1) / abs(ideal1)), 1e-6)

    def test_overlap_add_matches_direct_full_convolution_at_every_boundary(self):
        trace, kernel = self.fixture()
        stream = HermitePressureStream(kernel, block_intervals=256)
        pieces = [stream.push(start, block) for start, block in self.blocks(trace)]
        pieces.append(stream.finish())
        actual = np.concatenate(pieces)
        expected = self.direct(trace, kernel)
        np.testing.assert_allclose(actual, expected, rtol=2e-12, atol=2e-14)
        self.assertEqual(len(actual), len(trace[0]) - 1 + stream.filter_length - 1)
        with self.assertRaises(ValueError):
            stream.finish()

    def test_resample_phase_and_tail_match_scipy_for_short_and_long_streams(self):
        rng = np.random.default_rng(18)
        for size in (1, 2, 7, 17, 100, 2049, 4097):
            data = rng.normal(size=(size, 2)) * .02
            stream = PolyphaseStream()
            pieces = [stream.push(data[i:i + 23]) for i in range(0, size, 23)]
            pieces.append(stream.finish())
            np.testing.assert_allclose(np.concatenate(pieces), resample_poly(data, 3, 8, axis=0), rtol=1e-13, atol=1e-15)
            self.assertLessEqual(len(stream.buffer), 57)
        identity = PolyphaseStream(48000, 48000)
        np.testing.assert_array_equal(np.r_[identity.push(data), identity.finish()], data)

    def test_wav_writer_has_one_lead_and_complete_convolution_resampling_tail(self):
        trace, kernel = self.fixture(514)
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'physical.wav'
            writer = PhysicalWavStream(path, kernel, block_intervals=128)
            for start, block in self.blocks(trace):
                writer.push(start, block)
            metadata = writer.finish()
            rate, actual = wavfile.read(path)
            expected = np.vstack([np.zeros((4800, 2)), resample_poly(self.direct(trace, kernel) * .25, 3, 8, axis=0)]).astype(np.float32)
            self.assertEqual(rate, 48000)
            self.assertEqual(actual.dtype, np.float32)
            self.assertEqual(actual.shape, expected.shape)
            np.testing.assert_allclose(actual, expected, rtol=1e-6, atol=1e-12)
            self.assertEqual(metadata['frames'], len(actual))
            np.testing.assert_array_equal(actual[:4800], np.zeros((4800, 2), np.float32))

    def test_gaps_changed_boundaries_and_too_short_propagation_fail(self):
        trace, kernel = self.fixture(8)
        stream = HermitePressureStream(kernel)
        stream.push(0., trace)
        with self.assertRaisesRegex(ValueError, 'contiguous'):
            stream.push(8 / 128000, trace)
        with self.assertRaisesRegex(ValueError, 'boundary'):
            stream.push(7 / 128000, trace)
        with self.assertRaisesRegex(ValueError, 'propagation'):
            HermitePressureStream(np.ones((8, 2, 3)))

    def test_analytic_observer_audio_is_preserved_in_the_audible_bands(self):
        trace, kernel = self.fixture(8193)
        stream = HermitePressureStream(kernel)
        actual = np.r_[stream.push(0., trace), stream.finish()]
        exact = pressure_fft(*trace, kernel)[0][:len(trace[0]) + len(kernel) - 1]
        audio = resample_poly(actual, 3, 8, axis=0)
        reference = resample_poly(exact, 3, 8, axis=0)
        common = min(len(audio), len(reference))
        self.assertLess(np.linalg.norm(audio[:common] - reference[:common]) / np.linalg.norm(reference[:common]), .001)

    def test_live_physical_events_feed_the_same_wav_across_mechanical_blocks(self):
        from test_concert_performance import PerformanceTests
        from concert_grand_performance import play
        from concert_grand_integrated_phrase import trajectory
        system, contact, coast, dampers = PerformanceTests().fixture()
        schedule = [(0., 'on', 60, 84), (.002, 'off', 60, 0),
                    (.0030078125, 'on', 61, 90), (.006, 'off', 61, 0),
                    (.007, 'pedal_off', 0, 0)]
        expected_state, trace, _ = trajectory(system, contact, coast, dampers, True,
                                               duration=.008, contact_rate=256000,
                                               coast_rate=128000, schedule=schedule)
        events = [dict(tick=0, kind='cc', key=64, value=127, channel=0)]
        events.extend(dict(tick=round(t * 256000), kind='cc' if kind == 'pedal_off' else kind,
                           key=64 if kind == 'pedal_off' else key, value=value, channel=0)
                      for t, kind, key, value in schedule)
        kernel = self.fixture()[1]
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'live.wav'
            writer = PhysicalWavStream(path, kernel, block_intervals=128)
            state, metrics = play(system, contact, coast, dampers, events, 2048,
                                   writer.push, block_s=.002)
            writer.finish()
            rate, actual = wavfile.read(path)
        for x, y in zip(state, expected_state):
            np.testing.assert_array_equal(x, y)
        expected = np.vstack([np.zeros((4800, 2)), resample_poly(self.direct(trace, kernel) * .25, 3, 8, axis=0)]).astype(np.float32)
        self.assertEqual(rate, 48000)
        self.assertEqual(metrics['chunks'], 4)
        np.testing.assert_allclose(actual, expected, rtol=1e-6, atol=1e-12)


if __name__ == '__main__':
    unittest.main()
