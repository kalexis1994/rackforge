"""Construct the four real opening pitches and measure a short living attack.

Cost/mechanics diagnostic only: frozen felt, no soft pedal, no output WAV.
The MIDI's soft pedal remains a blocker for faithful performance rendering.
Reuses the existing single board/R/reduction; stores no additional state cache.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import time

import numpy as np
from concert_grand_wave_cache import digest
from concert_grand_register import make_register
from concert_grand_shared_strings import SharedHammers, SharedDampers
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_packed_air import PackedResidues, PackedContact, PackedCoast
from concert_grand_performance import play


spec = importlib.util.spec_from_file_location('musical_register_reference',
    Path(__file__).with_name('render-concert-grand-shared-phrase.py'))
frozen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(frozen)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plan', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists():
        raise ValueError('preserve prior evidence')
    plan_report = json.loads(args.plan.read_text(encoding='utf-8'))
    for name, expected in plan_report['source_sha256'].items():
        if digest(Path(__file__).with_name(name)) != expected:
            raise ValueError('changed preflight source')
    candidate = next(case for case in plan_report['cases'] if 'campanella' in Path(case['plan']['midi']).stem)
    plan = candidate['plan']
    if digest(plan['midi']) != plan['midi_sha256']:
        raise ValueError('changed preserved MIDI')
    keys = candidate['demand']['prefix_keys']
    first = next(event for event in plan['events'] if event['kind'] == 'on')
    start = time.perf_counter()
    names = [Path(__file__).name, 'concert_grand_register.py', 'concert_grand_performance.py',
             'concert_grand_shared_strings.py', 'concert_grand_shared_kernels.py',
             'concert_grand_packed_air.py', 'render-concert-grand-shared-phrase.py']
    report = dict(stage='preparation', plan_report_sha256=digest(args.plan),
                  source_sha256={name:digest(Path(__file__).with_name(name)) for name in names},
                  keys=keys, sampled_audio_used=False, new_disk_cache=False, audio_files=[],
                  faithful_midi_render=False, soft_pedal_implemented=False,
                  scope='Four real opening pitches, one 6ms cost/attack probe with original first velocity, frozen felt and fully lifted sustain. No soft pedal or complete musical phrase.',
                  all_gates_passed=False)
    def save():
        report['seconds'] = time.perf_counter() - start
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False), encoding='utf-8')
    save()
    modal, geometry, air, _kernel, paths = frozen.prepare(report, save)
    report['stage'] = 'four-key mechanical construction'
    save()
    begin = time.perf_counter()
    original, metadata = make_register(modal, geometry, keys)
    system = SharedHammers(BatchedStrings(original.string), original.felt)
    del original
    report['register'] = dict(metadata, construction_seconds=time.perf_counter() - begin)
    report['stage'] = 'contact/coast/damper factors'
    save()
    begin = time.perf_counter()
    residues = PackedResidues(air.wave)
    contact = PackedContact(system, air, residues)
    coast = PackedCoast(system, air, residues)
    dampers = SharedDampers(system.string, system.nh)
    report['factor_seconds'] = time.perf_counter() - begin
    report['packed_residue_ram_bytes'] = residues.matrix.nbytes
    report['stage'] = '6ms attack with bounded capture'
    save()
    events = [dict(tick=0, kind='cc', key=64, value=127, channel=0), dict(first, tick=0)]
    captured = dict(chunks=0, intervals=0, maximum_board_displacement_norm=0., maximum_board_velocity_norm=0.)
    def observe(_start, trace):
        t, b, v = trace
        if not all(np.isfinite(x).all() for x in trace):
            raise ValueError('nonfinite board capture')
        captured['chunks'] += 1
        captured['intervals'] += len(t) - 1
        captured['maximum_board_displacement_norm'] = max(captured['maximum_board_displacement_norm'], float(np.linalg.norm(b, axis=1).max()))
        captured['maximum_board_velocity_norm'] = max(captured['maximum_board_velocity_norm'], float(np.linalg.norm(v, axis=1).max()))
    final, metrics = play(system, contact, coast, dampers, events, 1536, observe, block_s=.002)
    if captured['intervals'] != 768 or len(metrics['launches']) != 1:
        raise ValueError('missed opening attack or capture duration')
    report.update(probe=metrics, capture=captured, first_original_midi_event=first,
                  final_states_finite=all(np.isfinite(x).all() for x in final),
                  source_and_cache_unchanged=all(digest(Path(__file__).with_name(name)) == expected for name, expected in report['source_sha256'].items())
                      and digest(paths.board) == report['board_sha256']
                      and digest(paths.cache / 'R.npy') == report['cache_R_sha256'],
                  stage='four-key bank and event/capture probe completed')
    save()
    print('MUSICAL REGISTER COMPLETED', report['register'], metrics, captured, flush=True)


if __name__ == '__main__':
    main()
