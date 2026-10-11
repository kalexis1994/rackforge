"""Same four-key/606-mode attack with original pre-strike pedals and a motion probe.

Reuses one board/R and finite pressure stream. Silent cold pre-roll is reduced
analytically: pedal strokes advance through original pre-strike events while
all mechanical/acoustic states remain at rest. No note samples or listening WAV.
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
from concert_grand_pressure_stream import HermitePressureStream, PolyphaseStream

spec = importlib.util.spec_from_file_location('pedal_reference',
    Path(__file__).with_name('render-concert-grand-shared-phrase.py'))
frozen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(frozen)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--retry-incomplete', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    base = root / 'docs/concert-grand-physics-roadmap-2026-10-02'
    folder = base / 'medidas-P6-pedales-continuos-2026-10-11'
    report_path = folder / 'pedales.json'
    incomplete_hash = None
    if report_path.exists():
        partial = json.loads(report_path.read_text())
        if (not args.retry_incomplete or partial.get('cases') != []
                or partial.get('stage') != 'original_pre_attack_controllers'
                or 'data' in partial or (folder / 'presion.npz').exists()):
            raise ValueError('preserve completed/partial evidence')
        incomplete_hash = digest(report_path)
    plan_path = base / 'medidas-P6-entrada-MIDI-2026-10-11/plan.json'
    planning = json.loads(plan_path.read_text(encoding='utf-8'))
    case = next(row for row in planning['cases'] if 'campanella' in Path(row['plan']['midi']).stem)
    plan = case['plan']
    if digest(plan['midi']) != plan['midi_sha256']:
        raise ValueError('original MIDI changed')
    first = next(event for event in plan['events'] if event['kind'] == 'on')
    prefix = plan['events'][:plan['events'].index(first)]
    if any(event['kind'] not in ('cc', 'program') for event in prefix):
        raise ValueError('cold silent reduction only before any note')
    keys = case['demand']['prefix_keys']
    begin = time.perf_counter()
    names = [Path(__file__).name, 'concert_grand_pedal_action.py', 'concert_grand_performance.py',
        'concert_grand_register.py', 'concert_grand_shared_strings.py', 'concert_grand_shared_kernels.py',
        'concert_grand_packed_air.py', 'concert_grand_exponential_contact.py',
        'concert_grand_pressure_stream.py', 'render-concert-grand-shared-phrase.py']
    report = dict(stage='preparation', preceding_stage_commit='983552b7',
        source_sha256={name: digest(Path(__file__).with_name(name)) for name in names},
        plan_sha256=digest(plan_path), midi_sha256=plan['midi_sha256'],
        source_rate_hz=128000, output_rate_hz=48000, gain_per_pa=.25,
        sampled_audio_used=False, new_disk_cache=False, audio_files=[], all_gates_passed=False,
        faithful_complete_midi_render=False, parameters_identified=False,
        complete_temporal_convergence=False, criteria=dict(mechanical_work_relative=1e-8),
        scope='6ms first attack with actual pre-strike controller values and a separate synthetic pedal-motion diagnostic; no full phrase, identified pedal geometry or timbre verdict.')
    if incomplete_hash:
        report['incomplete_attempt_sha256'] = incomplete_hash
        report['restart_reason'] = 'First probe finished but a NumPy boolean gate was not JSON serializable; no contracted data or completed case was saved. Repeated after explicit scalar conversion, no physics change.'
    folder.mkdir(parents=True, exist_ok=True)

    def save():
        report['seconds'] = time.perf_counter() - begin
        report_path.write_text(json.dumps(report, indent=2, allow_nan=False) + '\n', encoding='utf-8')

    save()
    modal, geometry, air, kernel, paths = frozen.prepare(report, save)
    original, metadata = make_register(modal, geometry, keys)
    core, felt = BatchedStrings(original.string), original.felt
    del original
    report['register'] = metadata
    residues = PackedResidues(air.wave)
    arrays, results = {}, []
    report['cases'] = results
    for name, motion in [('original_pre_attack_controllers', False), ('moving_controls_diagnostic', True)]:
        system = ShiftedHammers(core, felt)
        control = PedalControl(keys, origin_s=-first['tick'] / 256000, allow_frozen_program=True)
        control.validate(prefix)
        for event in prefix:
            control.event(event, (event['tick'] - first['tick']) / 256000)
        contact, coast = ShiftedContact(system, air, residues), PackedCoast(system, air, residues)
        dampers = SharedDampers(core, system.nh)
        pressure = HermitePressureStream(kernel, block_intervals=1024)
        resampler = PolyphaseStream()
        native, audio = [], []
        def observe(start, trace):
            p = pressure.push(start, trace)
            native.append(p)
            audio.append(resampler.push(p * .25))
        events = [dict(first, tick=0)]
        if motion:
            events += [dict(tick=32, kind='cc', key=67, value=0, channel=0),
                       dict(tick=512, kind='cc', key=64, value=64, channel=0)]
        report['stage'] = name
        save()
        final, metrics = play(system, contact, coast, dampers, events, 1536, observe,
            block_s=.002, control=control, progress=lambda value: print(name, value, flush=True))
        tail = pressure.finish()
        native.append(tail)
        audio.extend([resampler.push(tail * .25), resampler.finish()])
        p, a = np.concatenate(native), np.concatenate(audio)
        denominator = abs(metrics['external_head_work_j']) + abs(metrics['pedal_parameter_work_j'])
        relative = float(abs(metrics['mechanical_work_residual_j']) / denominator)
        row = dict(name=name, original_pre_attack_events=prefix, attack_event=first,
            simulated_events=events, cold_silent_pre_roll_s=first['tick'] / 256000,
            metrics=metrics, final_shift=system.shift, final_damper_levels=control.levels(.006),
            final_states_finite=all(np.isfinite(x).all() for x in final),
            pressure_metadata=pressure.metadata(), native_frames=len(p), audio_frames=len(a),
            audio_peak=float(np.max(abs(a))), audio_rms=float(np.sqrt(np.mean(a*a))),
            mechanical_work_relative=relative, mechanical_work_gate_passed=relative <= 1e-8)
        arrays[name + '_native'], arrays[name + '_audio'] = p, a
        results.append(row)
        save()
    data_path = folder / 'presion.npz'
    np.savez_compressed(data_path, **arrays)
    report['data'] = dict(path=str(data_path), sha256=digest(data_path), bytes=data_path.stat().st_size)
    report['source_and_cache_unchanged'] = all(digest(Path(__file__).with_name(name)) == expected
        for name, expected in report['source_sha256'].items()) and digest(paths.board) == report['board_sha256'] and digest(paths.cache / 'R.npy') == report['cache_R_sha256']
    report['local_controls_passed'] = report['source_and_cache_unchanged'] and all(
        row['mechanical_work_gate_passed'] and row['final_states_finite'] for row in results)
    report['stage'] = 'continuous pedals and signed parameter work measured'
    save()
    print('PEDALS COMPLETED', results, flush=True)


if __name__ == '__main__':
    main()
