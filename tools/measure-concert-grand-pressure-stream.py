"""Measure bounded pressure/audio against the same live four-key trajectory.

One short 6ms probe, not a new piano sound or musical interpretation. Store
small contracted vectors only; the reference B/V trajectory exists in RAM.
No new large R/state cache and no diagnostic click WAV in SHARED.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import time

import numpy as np
from numpy.polynomial.legendre import leggauss
from scipy.signal import resample_poly
from concert_grand_wave_cache import digest
from concert_grand_register import make_register
from concert_grand_shared_strings import SharedHammers, SharedDampers
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_packed_air import PackedResidues, PackedContact, PackedCoast
from concert_grand_performance import play
from concert_grand_pressure_stream import HermitePressureStream, PolyphaseStream, source_coefficients
from concert_grand_cell_audio import pressure_fft, kernel_transfer
from concert_grand_band_diagnostics import frequency_grid, pressure_spectra, compare_spectra


spec = importlib.util.spec_from_file_location('pressure_stream_reference',
    Path(__file__).with_name('render-concert-grand-integrated-phrase.py'))
frozen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(frozen)
ROOT = Path(__file__).resolve().parents[1]
DOC = ROOT / 'docs/concert-grand-physics-roadmap-2026-10-02'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    data_path = args.report.parent / 'presion.npz'
    if args.report.exists() or data_path.exists():
        raise ValueError('preserve prior evidence')
    prior_path = DOC / 'medidas-P6-entrada-MIDI-2026-10-11/registro.json'
    prior = json.loads(prior_path.read_text(encoding='utf-8'))
    names = dict(prior['source_sha256'])
    for name, expected in names.items():
        if digest(Path(__file__).with_name(name)) != expected:
            raise ValueError('changed frozen musical-register source')
    for name in [Path(__file__).name, 'concert_grand_pressure_stream.py', 'concert_grand_cell_audio.py',
                 'concert_grand_band_diagnostics.py', 'concert_grand_uniform_spectra.py']:
        names[name] = digest(Path(__file__).with_name(name))
    start = time.perf_counter()
    report = dict(stage='preparation', all_gates_passed=False, diagnostic_listening=False,
                  sampled_audio_used=False, audio_files=[], new_disk_cache=False,
                  complete_temporal_convergence=False, complete_energy_certificate=False,
                  pressure_source_interpolation_taps=12, source_rate_hz=128000, output_rate_hz=48000,
                  source_sha256=names, reference_report_sha256=digest(prior_path),
                  criteria=dict(band_relative=.01, waveform_relative=.01, moment_transfer_relative=1e-6),
                  scope='Same assumed four-key/12-wire system; 6ms first attack only, frozen felt, sustain fully down, no soft pedal. Numerical observer/audio validation, not timbre identification.')
    def save():
        report['seconds'] = time.perf_counter() - start
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False), encoding='utf-8')
    save()
    modal, geometry, air, kernel, paths = frozen.audit.frozen.prepare(report, save)
    original, metadata = make_register(modal, geometry, prior['keys'])
    system = SharedHammers(BatchedStrings(original.string), original.felt)
    del original
    report['register'] = metadata
    residues = PackedResidues(air.wave)
    contact, coast = PackedContact(system, air, residues), PackedCoast(system, air, residues)
    dampers = SharedDampers(system.string, system.nh)
    pressure = HermitePressureStream(kernel, block_intervals=1024)
    sampler = PolyphaseStream()
    P, A, B, V = [], [], [], []
    observer_seconds = 0.
    def observe(start_s, trace):
        nonlocal observer_seconds
        now = time.perf_counter()
        native = pressure.push(start_s, trace)
        P.append(native)
        A.append(sampler.push(native * .25))
        observer_seconds += time.perf_counter() - now
        take = slice(None) if not B else slice(1, None)
        # Small monolithic oracle window only; not the piece-render sink.
        B.append(trace[1][take].copy())
        V.append(trace[2][take].copy())
    first = prior['first_original_midi_event']
    events = [dict(tick=0, kind='cc', key=64, value=127, channel=0), dict(first, tick=0)]
    report['stage'] = 'same physical attack with streaming observer'
    save()
    final, metrics = play(system, contact, coast, dampers, events, 1536, observe, block_s=.002)
    tail = pressure.finish()
    P.append(tail)
    A.extend([sampler.push(tail * .25), sampler.finish()])
    actual_native, actual_audio = np.concatenate(P), np.concatenate(A)
    report.update(probe=metrics, streaming_pressure=pressure.metadata(),
                  source_callback_seconds=observer_seconds,
                  resampler_maximum_buffer_frames=sampler.maximum_buffer_frames,
                  resampler_retained_frames=len(sampler.buffer),
                  streamed_native_frames=len(actual_native), streamed_audio_frames=len(actual_audio),
                  source_trace_retained_for_short_oracle_only=True,
                  final_states_finite=all(np.isfinite(x).all() for x in final),
                  stage='same-trajectory analytic comparison')
    save()
    trace = (np.arange(769) / 128000, np.concatenate(B), np.concatenate(V))
    f = frequency_grid(6.25)
    ideal = pressure_spectra(trace, f, kernel_transfer(kernel, 128000, f))['rectangular']
    actual = np.exp(-2j * np.pi * f[:, None] * np.arange(len(actual_native)) / 128000) @ actual_native / 128000
    comparison = compare_spectra(f, actual, ideal)
    report['analytic_pressure_bands'] = comparison
    report['nested_pressure_bands'] = compare_spectra(f[::2], actual[::2], ideal[::2])
    reference_native = pressure_fft(*trace, kernel)[0][:len(trace[0]) + len(kernel) - 1]
    reference_audio = resample_poly(reference_native * .25, 3, 8, axis=0)
    common = min(len(actual_audio), len(reference_audio))
    report['legacy_wav_comparison'] = dict(frozen.compare_audio(actual_audio[:common], reference_audio[:common], 48000),
         compared_frames=common, legacy_frames=len(reference_audio), new_frames=len(actual_audio),
         extra_native_frames=len(actual_native) - len(reference_native),
         scope='Same origin/gain/trajectory; common prefix, no alignment or normalization. New finite interpolation tail fully retained.')
    nodes, h0, h1 = source_coefficients()
    x, w = leggauss(32)
    x, w = (x + 1) / 2, w / 2
    report['source_moment_controls'] = []
    for lo, hi in [(60., 12000.), (12000., 24000.)]:
        frequency = np.linspace(lo, hi, 1001)
        phase = np.exp(-2j * np.pi * frequency[:, None] * x / 128000)
        ideal0, ideal1 = phase @ w, phase @ (12 * (.5 - x) * w)
        discrete = np.exp(-2j * np.pi * frequency[:, None] * nodes / 128000)
        report['source_moment_controls'].append(dict(lower_hz=lo, upper_hz=hi,
            zero_relative=float(np.max(abs(discrete @ h0 - ideal0) / abs(ideal0))),
            first_relative=float(np.max(abs(discrete @ h1 - ideal1) / abs(ideal1))),
            independent_quadrature_nodes=32))
    np.savez_compressed(data_path, frequency_hz=f, analytic_pressure=ideal, streamed_pressure=actual,
                        streamed_native=actual_native, legacy_native=reference_native,
                        streamed_audio=actual_audio, legacy_audio=reference_audio)
    report['data'] = dict(path=str(data_path.resolve()), sha256=digest(data_path), bytes=data_path.stat().st_size)
    report['source_and_cache_unchanged'] = all(digest(Path(__file__).with_name(name)) == expected for name, expected in names.items()) and digest(paths.board) == report['board_sha256'] and digest(paths.cache / 'R.npy') == report['cache_R_sha256']
    moment = report['source_moment_controls'][0]
    report['local_observer_controls_passed'] = bool(comparison['all_bands_1_percent_passed']
        and report['legacy_wav_comparison']['waveform_relative'] < .01
        and max(moment['zero_relative'], moment['first_relative']) < 1e-6
        and report['source_and_cache_unchanged'])
    report['stage'] = 'bounded pressure and resampling control completed'
    save()
    print('PRESSURE STREAM COMPLETED', comparison, report['legacy_wav_comparison'], report['source_moment_controls'], flush=True)


if __name__ == '__main__':
    main()
