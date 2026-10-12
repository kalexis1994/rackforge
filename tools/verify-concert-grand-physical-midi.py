"""Authenticate the original opening, waveform and scalar work after rendering."""
import hashlib
import json
from pathlib import Path

import numpy as np
import soundfile as sf


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    root = Path(__file__).resolve().parent.parent
    folder = root / 'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-motivo-MIDI-2026-10-11'
    report = json.loads((folder / 'render.json').read_text())
    if report['stage'] != 'original opening and physical release rendered':
        raise ValueError('incomplete render')
    if not all(digest(root / 'tools' / name) == expected for name, expected in report['source_sha256'].items()):
        raise ValueError('measured sources changed')
    plan_path = folder.parent / 'medidas-P6-entrada-MIDI-2026-10-11/plan.json'
    if digest(plan_path) != report['plan_sha256'] or digest(report['midi']) != report['midi_sha256']:
        raise ValueError('plan or MIDI changed')
    planning = json.loads(plan_path.read_text())
    plan = next(row['plan'] for row in planning['cases'] if row['plan']['midi_sha256'] == report['midi_sha256'])
    window = report['excerpt']
    start, stop = window['original_first_attack_tick'], window['original_absolute_cut_tick']
    original = [dict(event, tick=event['tick'] - start) for event in plan['events'] if start <= event['tick'] < stop]
    if original != window['original_shifted_events']:
        raise ValueError('original note/controller events changed')
    first_index = next(i for i, event in enumerate(plan['events']) if event['kind'] == 'on')
    if plan['events'][:first_index] != window['original_pre_attack_events']:
        raise ValueError('cold pre-roll controls changed')
    expected_tail = dict(tick=stop - start, kind='cc', key=64, value=0, channel=0)
    if window['synthetic_closing_events'] != [expected_tail] or window['simulated_events'] != original + [expected_tail]:
        raise ValueError('undeclared closing events')
    attacks = [(e['tick'], e['key']) for e in original if e['kind'] == 'on']
    metrics = report['metrics']
    if [(e['tick'], e['key']) for e in metrics['launches']] != attacks:
        raise ValueError('missed or extra physical launches')
    if metrics['contact_steps'] + 2 * metrics['coast_steps'] != window['end_tick']:
        raise ValueError('mechanical duration mismatch')
    audio_report = report['audio']
    path = Path(audio_report['path'])
    if digest(path) != audio_report['sha256'] or path.stat().st_size != audio_report['bytes']:
        raise ValueError('listening audio changed')
    info = sf.info(path)
    samples, rate = sf.read(path, dtype='float64', always_2d=True)
    if rate != 48000 or info.channels != 2 or info.subtype != 'FLOAT' or not np.isfinite(samples).all():
        raise ValueError('WAV format or finiteness failed')
    intervals = window['end_tick'] // 2
    native_frames = intervals + audio_report['pressure']['filter_length'] - 1
    expected_frames = 4800 + (native_frames * 3 + 7) // 8
    if len(samples) != expected_frames or len(samples) != audio_report['frames']:
        raise ValueError('source/convolution/resampling/lead duration mismatch')
    if np.any(samples[:4800]):
        raise ValueError('initial quiet lead changed')
    peak = float(np.max(abs(samples)))
    np.testing.assert_allclose(peak, audio_report['peak_before_float32'], rtol=1e-6, atol=1e-12)
    tail_rms = float(np.sqrt(np.mean(samples[-960:]**2)))
    np.testing.assert_allclose(tail_rms, audio_report['last20ms_rms'], rtol=1e-6, atol=1e-12)
    tail_db = float(20 * np.log10(max(tail_rms, 1e-30) / peak))
    last_sample_relative = float(np.max(abs(samples[-1])) / peak)
    if peak >= .98 or audio_report['output_fade_used']:
        raise ValueError('headroom or physical tail altered')
    if any(metrics[key] < 0 for key in ['felt_heat_j', 'material_heat_j', 'damper_heat_j']):
        raise ValueError('negative physical heat')
    residual = (metrics['final_mechanical_j'] - metrics['external_head_work_j'] - metrics['pedal_parameter_work_j']
                + metrics['felt_heat_j'] + metrics['material_heat_j'] + metrics['damper_heat_j'] + metrics['air_port_work_j'])
    np.testing.assert_allclose(residual, metrics['mechanical_work_residual_j'], rtol=0, atol=1e-18)
    scale = abs(metrics['external_head_work_j']) + abs(metrics['pedal_parameter_work_j'])
    relative = abs(residual) / scale
    if relative > report['criteria']['mechanical_work_relative'] or not report['source_and_cache_unchanged']:
        raise ValueError('work or source/cache integrity failed')
    if any(report['final_held_counts'].values()) or any(level != 1. for level in report['final_damper_levels'].values()):
        raise ValueError('keys or dampers not fully released')
    result = dict(stage='independent musical opening verification', verifier_sha256=digest(__file__),
        sources_plan_midi_and_audio_hashes_passed=True, original_events_and_launches_passed=True,
        clock_and_audio_frames_passed=True, frames=len(samples), seconds=len(samples) / rate,
        format='FLOAT32 stereo 48k', peak=peak, tail20ms_rms=tail_rms, tail20ms_relative_db=tail_db,
        last_sample_relative_to_peak=last_sample_relative, physical_tail_40db_gate_passed=tail_db <= -40,
        mechanical_work_relative=relative, mechanical_work_gate_passed=True, all_keys_and_dampers_released=True,
        diagnostic_listening=True, global_physics_approved=False,
        scope='Event/source integrity, WAV format, frame clock, peak/physical tail and scalar work arithmetic. No independent modal replay or temporal/spatial convergence and no Steinway identification.')
    (folder / 'verificacion.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
