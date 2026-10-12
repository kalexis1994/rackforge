"""Render a cold musical opening with shared physical states and continuous pedals.

Original timing/velocity/controllers, one explicit final sustain return and
bounded capsule pressure/WAV streaming. No samples, EQ, normalization or fades.
This is a listening diagnostic of assumed physics, not a full-piano certificate.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import time

import numpy as np
from concert_grand_wave_cache import digest
from concert_grand_register import make_register
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_shared_strings import SharedDampers
from concert_grand_packed_air import PackedResidues, PackedCoast
from concert_grand_pedal_action import PedalControl, ShiftedHammers, ShiftedContact
from concert_grand_performance import play
from concert_grand_pressure_stream import PhysicalWavStream
from concert_grand_midi_excerpt import excerpt

spec = importlib.util.spec_from_file_location('musical_audio_reference',
    Path(__file__).with_name('render-concert-grand-shared-phrase.py'))
frozen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(frozen)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plan', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--wav', type=Path, required=True)
    parser.add_argument('--cut-seconds', type=float, default=.72)
    parser.add_argument('--tail-seconds', type=float, default=.24)
    parser.add_argument('--allow-frozen-program', action='store_true')
    parser.add_argument('--retry-failed', action='store_true')
    args = parser.parse_args()
    failed_attempt = None
    if args.report.exists() or args.wav.exists():
        if not args.retry_failed or not args.report.exists() or not args.wav.exists():
            raise ValueError('preserve existing report and audio')
        failed = json.loads(args.report.read_text())
        if failed.get('stage') != 'failed partial render' or Path(failed.get('partial_audio', '')).resolve() != args.wav.resolve():
            raise ValueError('retry only this failed partial render')
        failed_attempt = dict(report_sha256=digest(args.report), partial_audio_sha256=digest(args.wav),
            partial_audio_bytes=args.wav.stat().st_size, failure=failed['failure'],
            last_progress=failed.get('progress'), source_sha256=failed['source_sha256'])
    planning = json.loads(args.plan.read_text(encoding='utf-8'))
    selected = next(case for case in planning['cases'] if 'campanella' in Path(case['plan']['midi']).stem)
    plan, keys = selected['plan'], selected['demand']['prefix_keys']
    if plan['contact_rate_hz'] != 256000:
        raise ValueError('frozen contact/capture rates required')
    if digest(plan['midi']) != plan['midi_sha256']:
        raise ValueError('original MIDI changed')
    ticks = []
    for seconds in [args.cut_seconds, args.tail_seconds]:
        if not np.isfinite(seconds) or seconds <= 0:
            raise ValueError('positive duration')
        tick = round(seconds * 256000)
        if abs(tick / 256000 - seconds) > 1e-14:
            raise ValueError('requested duration must align to integration ticks')
        ticks.append(tick)
    window = excerpt(plan, *ticks, bank_keys=keys)
    control = PedalControl(keys, origin_s=-window['cold_silent_pre_roll_s'],
                           allow_frozen_program=args.allow_frozen_program)
    control.validate(window['original_pre_attack_events'] + window['simulated_events'])
    for event in window['original_pre_attack_events']:
        control.event(event, (event['tick'] - window['original_first_attack_tick']) / 256000)
    names = [Path(__file__).name, 'concert_grand_midi_excerpt.py', 'concert_grand_pedal_action.py',
        'concert_grand_performance.py', 'concert_grand_register.py', 'concert_grand_shared_strings.py',
        'concert_grand_shared_kernels.py', 'concert_grand_packed_air.py',
        'concert_grand_exponential_contact.py', 'concert_grand_pressure_stream.py',
        'render-concert-grand-shared-phrase.py']
    begin = time.perf_counter()
    report = dict(stage='preparation', preceding_stage_commit='daba4ff5',
        source_sha256={name: digest(Path(__file__).with_name(name)) for name in names},
        plan_sha256=digest(args.plan), midi=plan['midi'], midi_sha256=plan['midi_sha256'],
        excerpt=window, contact_rate_hz=256000, coast_rate_hz=128000, capture_rate_hz=128000,
        diagnostic_listening=True, sampled_audio_used=False, audio_files=[], new_disk_cache=False,
        all_gates_passed=False, complete_temporal_convergence=False, complete_energy_certificate=False,
        physics_parameters_identified=False, faithful_complete_midi_render=False,
        inherited_precision_failures_preserved=True, criteria=dict(mechanical_work_relative=1e-8),
        scope='Original cold opening events and pre-strike controllers; explicit synthetic sustain return after all excerpt key releases. Same assumed four-key piano, no timbre or full-phrase convergence claim.')
    if failed_attempt:
        # Only this producer's named failed output is replaced, in place.
        # No successful audio, source or cache is removed or duplicated.
        report['failed_attempt'] = failed_attempt
        report['restart_reason'] = 'Use original uniform B/V knots at shared boundaries; remove redundant spline evaluation, keep dynamics and pressure continuity criterion unchanged.'
        args.wav.unlink()
    args.report.parent.mkdir(parents=True, exist_ok=True)
    def save():
        report['seconds'] = time.perf_counter() - begin
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False) + '\n', encoding='utf-8')
    save()
    modal, geometry, air, kernel, paths = frozen.prepare(report, save)
    original, metadata = make_register(modal, geometry, keys)
    system = ShiftedHammers(BatchedStrings(original.string), original.felt)
    del original
    report['register'] = metadata
    residues = PackedResidues(air.wave)
    contact, coast = ShiftedContact(system, air, residues), PackedCoast(system, air, residues)
    dampers = SharedDampers(system.string, system.nh)
    writer = PhysicalWavStream(args.wav, kernel, block_intervals=1024)
    report['stage'] = 'rendering original opening and physical release'
    save()
    def progress(value):
        report['progress'] = value
        save()
        print('MUSICAL RENDER', value, flush=True)
    try:
        final, metrics = play(system, contact, coast, dampers, window['simulated_events'],
            window['end_tick'], writer.push, block_s=.008, control=control, progress=progress)
        audio = writer.finish()
    except BaseException as error:
        if not writer.closed:
            writer.abort()
        report.update(stage='failed partial render', failure=repr(error),
                      partial_audio=str(args.wav.resolve()))
        save()
        raise
    denominator = abs(metrics['external_head_work_j']) + abs(metrics['pedal_parameter_work_j'])
    relative = float(abs(metrics['mechanical_work_residual_j']) / denominator)
    report.update(metrics=metrics, audio=dict(audio, path=str(args.wav.resolve()),
        sha256=digest(args.wav), bytes=args.wav.stat().st_size),
        audio_files=[str(args.wav.resolve())], final_states_finite=all(np.isfinite(x).all() for x in final),
        final_damper_levels=control.levels(window['end_tick'] / 256000),
        final_lateral_shift=system.shift, final_held_counts=dict(control.held),
        mechanical_work_relative=relative, mechanical_work_gate_passed=relative <= 1e-8,
        source_and_cache_unchanged=all(digest(Path(__file__).with_name(name)) == expected
            for name, expected in report['source_sha256'].items()) and digest(paths.board) == report['board_sha256']
            and digest(paths.cache / 'R.npy') == report['cache_R_sha256'],
        stage='original opening and physical release rendered')
    save()
    print('MUSICAL RENDER COMPLETED', audio, relative, flush=True)


if __name__ == '__main__':
    main()
