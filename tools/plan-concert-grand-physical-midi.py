"""Preflight the preserved musical performances before an expensive render."""
import argparse
import json
from pathlib import Path

from concert_grand_wave_cache import digest
from concert_grand_performance_plan import plan_midi, summarize


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--midi', type=Path, nargs='+', required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists():
        raise ValueError('preserve previous preflight')
    root = Path(__file__).resolve().parents[1]
    note_path = root / 'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-nota-release-2026-10-10/nota.json'
    note = json.loads(note_path.read_text(encoding='utf-8'))
    wall_per_s = note['trajectory']['seconds'] / note['physical_duration_s']
    results = []
    for path in args.midi:
        plan = plan_midi(path)
        summary = summarize(plan)
        coordinates = len(plan['keys']) * 3 * (2 * 96 + 192) + 606
        # Just ONE dense core matrix; construction needs several, and factors
        # and the air add more. Do not label this as peak resident memory.
        dense = coordinates**2 * 8
        trace = 2 * 606 * 8 * (round(plan['duration_s'] * 128000) + 1)
        results.append(dict(plan=plan, demand=summary,
                            prototype_all_triple_wire_coordinates=coordinates,
                            one_dense_core_matrix_bytes=dense,
                            monolithic_board_trace_bytes=trace,
                            three_key_runtime_extrapolation_s=plan['duration_s'] * wall_per_s,
                            ready_for_faithful_full_render=False))
        print(path.name, dict(seconds=plan['duration_s'], keys=summary['distinct_keys'],
                              strikes=summary['strikes'], unsupported=plan['unsupported_for_faithful_render'],
                              matrix_gib=dense/2**30, trace_gib=trace/2**30), flush=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    report = dict(reference_note_report_sha256=digest(note_path),
                  source_sha256={name:digest(Path(__file__).with_name(name)) for name in
                                 [Path(__file__).name, 'concert_grand_performance_plan.py']},
                  sampled_audio_used=False, audio_files=[],
                  timing_scope='Extrapolation of measured three-key run, not a predicted full-keyboard render time.',
                  current_contact_rate_hz=256000, current_coast_rate_hz=128000,
                  cases=results)
    args.report.write_text(json.dumps(report, indent=2, allow_nan=False), encoding='utf-8')


if __name__ == '__main__':
    main()
