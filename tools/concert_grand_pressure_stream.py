"""Bounded Hermite-pressure convolution and continuous polyphase audio.

The source's continuous acceleration moments use integrated Lagrange delay
polynomials. This is a numerical approximation to the frozen analytic moments,
not an extra acoustic mechanism or tonal filter. Green cell kernel, Pa scale,
all board coordinates and the source trajectory remain unchanged. Measure its
error against the analytic observer before using a listening result.
"""
from fractions import Fraction
from math import gcd
from pathlib import Path

import numpy as np
from scipy.fft import rfft, irfft, rfftfreq, next_fast_len
from scipy.signal import firwin
import soundfile as sf


def source_coefficients(taps=12):
    """Exact rational integrals of L_n(x) and 12(.5-x)L_n(x), x in [0,1]."""
    if not isinstance(taps, int) or taps < 4 or taps > 24 or taps % 2:
        raise ValueError('even interpolation size between 4 and 24')
    left = taps // 2 - 1
    nodes = np.arange(-left, taps - left)
    zero, first = [], []
    for node in nodes:
        polynomial = [Fraction(1)]
        for other in nodes:
            if other == node:
                continue
            updated = [Fraction(0)] * (len(polynomial) + 1)
            for i, coefficient in enumerate(polynomial):
                updated[i] -= coefficient * int(other) / int(node - other)
                updated[i + 1] += coefficient / int(node - other)
            polynomial = updated
        integral = sum(c / (i + 1) for i, c in enumerate(polynomial))
        moment = sum(c / (i + 2) for i, c in enumerate(polynomial))
        zero.append(float(integral))
        first.append(float(12 * (integral / 2 - moment)))
    return nodes, np.array(zero), np.array(first)


class HermitePressureStream:
    def __init__(self, kernel, rate=128000, taps=12, block_intervals=4096):
        kernel = np.asarray(kernel, dtype=float)
        if (kernel.ndim != 3 or len(kernel) < 2 or not np.isfinite(kernel).all()
                or not isinstance(rate, int) or rate <= 0
                or not isinstance(block_intervals, int) or block_intervals <= 0):
            raise ValueError('finite Green kernel and positive integer rate/block')
        self.nodes, self.zero, self.first = source_coefficients(taps)
        left = -int(self.nodes[0])
        if len(kernel) <= left or np.any(kernel[:left]):
            raise ValueError('propagation must exceed source interpolation lookahead')
        self.kernel_length, self.channels, self.modes = kernel.shape
        self.rate, self.block = rate, block_intervals
        # Actual source delays run from -left to +right. The Green kernel
        # has enough leading zeros that their product is strictly causal.
        self.filter_length = len(kernel) + int(self.nodes[-1])
        self.fft_size = next_fast_len(block_intervals + self.filter_length - 1)
        self.frequency = rfftfreq(self.fft_size, 1 / rate)
        phase = np.exp(2j * np.pi * self.frequency * left / rate)
        self.F0 = rfft(self.zero, n=self.fft_size) * phase
        self.F1 = rfft(self.first, n=self.fft_size) * phase
        self.H = rfft(kernel, n=self.fft_size, axis=0)
        self.tail = np.zeros((self.filter_length - 1, self.channels))
        self.intervals = self.chunks = 0
        self.previous = None
        self.closed = False

    def push(self, start, trace):
        if self.closed:
            raise ValueError('pressure stream already finished')
        t, position, velocity = (np.asarray(x, dtype=float) for x in trace)
        if (t.ndim != 1 or len(t) < 2 or t[0] != 0 or position.shape != (len(t), self.modes)
                or velocity.shape != position.shape or not all(np.isfinite(x).all() for x in (t, position, velocity))
                or not np.isfinite(start) or abs(start * self.rate - self.intervals) > 1e-7
                or not np.allclose(np.diff(t), 1 / self.rate, rtol=1e-9, atol=1e-15)):
            raise ValueError('contiguous finite uniform Hermite block required')
        if self.previous is not None:
            for current, previous in zip((position[0], velocity[0]), self.previous):
                tolerance = 64 * np.finfo(float).eps * max(np.linalg.norm(previous), 1e-20)
                if np.linalg.norm(current - previous) > tolerance:
                    raise ValueError('changed physical boundary between pressure blocks')
        result = []
        for begin in range(0, len(t) - 1, self.block):
            n = min(self.block, len(t) - 1 - begin)
            b = position[begin:begin + n + 1]
            v = velocity[begin:begin + n + 1]
            change = v[1:] - v[:-1]
            curvature = (b[1:] - b[:-1]) * self.rate - (v[1:] + v[:-1]) / 2
            spectrum = rfft(change, n=self.fft_size, axis=0) * self.F0[:, None]
            spectrum += rfft(curvature, n=self.fft_size, axis=0) * self.F1[:, None]
            pressure = irfft(np.einsum('fcm,fm->fc', self.H, spectrum) * self.rate,
                             n=self.fft_size, axis=0)[:n + self.filter_length - 1]
            pressure[:len(self.tail)] += self.tail
            result.append(pressure[:n].copy())
            self.tail = pressure[n:].copy()
            self.intervals += n
            self.chunks += 1
        self.previous = (position[-1].copy(), velocity[-1].copy())
        return np.concatenate(result)

    def finish(self):
        if self.closed or not self.intervals:
            raise ValueError('finished or empty pressure stream')
        self.closed = True
        return self.tail.copy()

    def metadata(self):
        return dict(source_rate_hz=self.rate, source_interpolation_taps=len(self.nodes),
                    source_delay_nodes=self.nodes.tolist(), fft_size=self.fft_size,
                    maximum_internal_intervals=self.block, source_intervals=self.intervals,
                    internal_chunks=self.chunks, retained_pressure_frames=len(self.tail),
                    filter_length=self.filter_length,
                    cached_transfer_bytes=self.H.nbytes + self.F0.nbytes + self.F1.nbytes,
                    source_approximation_validated_globally=False)


class PolyphaseStream:
    """SciPy's default constant-pad resample_poly FIR, with persistent phase.

Compensates the same FIR group delay once. Zero-padding happens only at
the beginning/end of the whole stream, never at a chunk boundary.
"""
    def __init__(self, source_rate=128000, output_rate=48000, channels=2):
        if any(not isinstance(x, int) or x <= 0 for x in (source_rate, output_rate, channels)):
            raise ValueError('positive integer rates/channels')
        divisor = gcd(source_rate, output_rate)
        self.up, self.down = output_rate // divisor, source_rate // divisor
        self.channels = channels
        maximum = max(self.up, self.down)
        half = 10 * maximum
        if maximum == 1:
            self.remove = 0
            self.h = np.ones(1)
        else:
            h = firwin(2 * half + 1, 1 / maximum, window=('kaiser', 5.)) * self.up
            padding = self.down - half % self.down
            self.remove = (half + padding) // self.down
            self.h = np.r_[np.zeros(padding), h]
        self.lags = np.arange((len(self.h) - 1) // self.up + 1)
        self.buffer = np.empty((0, channels))
        self.buffer_start = self.inputs = self.next_output = self.outputs = 0
        self.maximum_buffer_frames = 0
        self.closed = False

    def _emit(self, end, zero_right=False):
        result = []
        # Limit gather temporaries even when a caller supplies a long block.
        for begin in range(self.next_output, end, 4096):
            output = np.arange(begin, min(begin + 4096, end))
            scaled = output * self.down
            source = scaled[:, None] // self.up - self.lags
            coefficient = scaled[:, None] % self.up + self.lags * self.up
            in_filter = coefficient < len(self.h)
            valid = in_filter & (source >= 0) & (source < self.inputs)
            if np.any(valid & (source < self.buffer_start)):
                raise AssertionError('resampling history discarded prematurely')
            if not zero_right and np.any(in_filter & (source >= self.inputs)):
                raise AssertionError('resampling output needs future input')
            values = np.zeros((*source.shape, self.channels))
            values[valid] = self.buffer[source[valid] - self.buffer_start]
            weights = np.zeros_like(source, dtype=float)
            weights[in_filter] = self.h[coefficient[in_filter]]
            chunk = np.einsum('ij,ijc->ic', weights, values)
            chunk = chunk[output >= self.remove]
            if len(chunk):
                result.append(chunk)
                self.outputs += len(chunk)
        self.next_output = end
        return np.concatenate(result) if result else np.empty((0, self.channels))

    def push(self, samples):
        samples = np.asarray(samples, dtype=float)
        if self.closed or samples.ndim != 2 or samples.shape[1] != self.channels or not np.isfinite(samples).all():
            raise ValueError('finite open resampling stream required')
        if not len(samples):
            return np.empty((0, self.channels))
        self.buffer = np.vstack([self.buffer, samples])
        self.inputs += len(samples)
        self.maximum_buffer_frames = max(self.maximum_buffer_frames, len(self.buffer))
        result = self._emit((self.inputs * self.up + self.down - 1) // self.down)
        earliest = max(0, self.next_output * self.down // self.up - int(self.lags[-1]))
        drop = max(0, earliest - self.buffer_start)
        self.buffer = self.buffer[drop:].copy()
        self.buffer_start += drop
        return result

    def finish(self):
        if self.closed or not self.inputs:
            raise ValueError('finished or empty resampling stream')
        desired = (self.inputs * self.up + self.down - 1) // self.down
        result = self._emit(self.remove + desired, zero_right=True)
        self.closed = True
        if self.outputs != desired:
            raise AssertionError('incorrect final resampled duration')
        return result


class PhysicalWavStream:
    def __init__(self, path, kernel, source_rate=128000, output_rate=48000,
                 gain=.25, lead_s=.1, taps=12, block_intervals=4096):
        path = Path(path)
        if path.exists() or not np.isfinite(gain) or gain <= 0 or not np.isfinite(lead_s) or lead_s < 0:
            raise ValueError('new WAV and positive fixed pressure gain required')
        self.pressure = HermitePressureStream(kernel, source_rate, taps, block_intervals)
        self.resampler = PolyphaseStream(source_rate, output_rate, self.pressure.channels)
        self.gain, self.output_rate = gain, output_rate
        self.frames = 0
        self.peak = 0.
        self.last = np.empty((0, self.pressure.channels))
        self.closed = False
        path.parent.mkdir(parents=True, exist_ok=True)
        self.file = sf.SoundFile(path, mode='x', samplerate=output_rate,
                                 channels=self.pressure.channels, subtype='FLOAT', format='WAV')
        self._write(np.zeros((round(lead_s * output_rate), self.pressure.channels)))

    def _write(self, samples):
        if not np.isfinite(samples).all() or np.max(abs(samples), initial=0.) >= .98:
            raise ValueError('fixed pressure gain clips or produces nonfinite WAV')
        self.file.write(samples.astype(np.float32))
        self.frames += len(samples)
        self.peak = max(self.peak, float(np.max(abs(samples), initial=0.)))
        self.last = np.vstack([self.last, samples])[-960:]

    def push(self, start, trace):
        if self.closed:
            raise ValueError('WAV stream already finished')
        self._write(self.resampler.push(self.pressure.push(start, trace) * self.gain))

    def finish(self):
        if self.closed:
            raise ValueError('WAV stream already finished')
        self._write(self.resampler.push(self.pressure.finish() * self.gain))
        self._write(self.resampler.finish())
        self.file.close()
        self.closed = True
        return dict(frames=self.frames, seconds=self.frames / self.output_rate,
                    peak_before_float32=self.peak, output_rate_hz=self.output_rate,
                    format='FLOAT32', pressure_gain_per_pa=self.gain,
                    last20ms_rms=float(np.sqrt(np.mean(self.last**2))),
                    pressure=self.pressure.metadata(),
                    resampling_retained_frames=len(self.resampler.buffer),
                    output_fade_used=False)

    def abort(self):
        # Close/repair the header of the same partial file. Never flush a
        # fabricated physical tail after a failed trajectory.
        self.file.close()
        self.closed = True
