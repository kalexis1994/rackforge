"""Render a piano MIDI through an authenticated existing native production build.

Preserves original timing, velocities, channel messages and final pedal state.
The explicit Concert preset replaces only the initial piano program-0 message.
"""
import argparse
from collections import Counter
import importlib.util
import json
from pathlib import Path
import subprocess
import time

import numpy as np
from scipy.io import wavfile


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--midi', type=Path, required=True)
    p.add_argument('--probe-report', type=Path, required=True)
    p.add_argument('--work', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--report', type=Path, required=True)
    p.add_argument('--tail-seconds', type=int, default=8)
    p.add_argument('--tuning', type=Path, help='Explicit native probe tuning applied after loading Concert')
    a = p.parse_args()
    root = Path(__file__).resolve().parents[1]
    helper = root/'tools/render-concert-grand-release-midi.py'
    spec = importlib.util.spec_from_file_location('midi', helper)
    midi = importlib.util.module_from_spec(spec); spec.loader.exec_module(midi)
    assert a.tail_seconds >= 1
    assert not a.work.exists() and not a.output.exists() and not a.report.exists()
    build = json.loads(a.probe_report.read_text(encoding='utf-8'))
    render_record = build.get('full_render')
    if render_record is None:
        render_record = build['pieces'][0]['B_render']
    probe = Path(render_record['command'][0])
    for path in (probe, root/'plugins/concert-grand/src/lib.rs', root/'plugins/concert-grand/src/math.rs',
                 root/'crates/rackforge-plugin-sdk/src/lib.rs', root/'crates/rackforge-plugin-sdk/src/portable.rs',
                 root/'tools/concert-grand-release-probe.rs'):
        assert midi.sha(path) == build['files_sha256'][str(path.resolve())], path
    paths = [Path(__file__), helper, a.midi, a.probe_report, probe,
             root/'plugins/concert-grand/src/lib.rs', root/'plugins/concert-grand/src/math.rs']
    if a.tuning is not None:
        paths.append(a.tuning)
    frozen = {str(path.resolve()): midi.sha(path) for path in paths}
    source, duration = midi.midi_events(a.midi)
    rate, block = 44100, 256

    def frame(seconds):
        value = seconds * rate
        return (2*value.numerator + value.denominator) // (2*value.denominator)

    events, programs, counts, held, controllers = [], [], Counter(), set(), {}
    for seconds, message in source:
        status, key = message[:2]
        kind, channel = status & 0xf0, status & 15
        assert channel == 0, 'This listening tool expects one piano channel'
        if kind == 0xc0:
            assert key == 0 and seconds == 0
            programs.append(dict(seconds=0, program=0, reason='Concert preset explicitly loaded'))
            continue
        assert len(message) == 3 and kind in (0x80, 0x90, 0xb0)
        value = message[2]
        if kind == 0xb0:
            counts[f'cc{key}'] += 1; controllers[key] = value
        elif kind == 0x90 and value:
            counts['strikes'] += 1; held.add(key)
        else:
            counts['releases'] += 1; held.discard(key)
        events.append((frame(seconds), *message))
    assert counts['strikes'] > 0 and counts['strikes'] == counts['releases'] and not held
    assert all(x[0] <= y[0] for x, y in zip(events, events[1:]))
    total = frame(duration) + rate*a.tail_seconds
    a.work.mkdir(parents=True); a.output.parent.mkdir(parents=True, exist_ok=True)
    event_path = a.work/'events.txt'; raw = a.work/'performance.f32'
    event_path.write_text(''.join(' '.join(map(str, e))+'\n' for e in events), encoding='utf-8', newline='\n')
    command = [str(probe), str(event_path.resolve()), str(raw.resolve()), str(rate), str(total), str(block), 'concert']
    if a.tuning is not None:
        command.append(str(a.tuning.resolve()))
    start = time.perf_counter()
    native = json.loads(subprocess.check_output(command, cwd=root, text=True))
    elapsed = time.perf_counter()-start
    audio = np.fromfile(raw, dtype='<f4').reshape(-1, 2)
    assert audio.shape == (total, 2) and np.isfinite(audio).all() and 0 < abs(audio).max() < 1
    end_rms = float(np.sqrt(np.mean(audio[-rate//10:].astype(np.float64)**2)))
    assert end_rms < 1e-5, 'Tail still audible: rerender with a longer tail'
    wavfile.write(a.output, rate, audio)
    check_rate, check_audio = wavfile.read(a.output)
    assert check_rate == rate and np.array_equal(check_audio, audio)
    assert frozen == {str(path.resolve()): midi.sha(path) for path in paths}
    report = dict(scope=__doc__, model=build.get('physical_change', 'Authenticated Concert Grand build described in the probe report'),
                  tuning=None if a.tuning is None else dict(path=str(a.tuning.resolve()), sha256=midi.sha(a.tuning), text=a.tuning.read_text(encoding='utf-8')),
                  git_head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip(),
                  files_sha256=frozen, midi_duration_seconds=float(duration), audio_duration_seconds=total/rate,
                  rate=rate, channels=2, block=block, tail_seconds=a.tail_seconds, export_gain=1,
                  counts=dict(counts), original_events=len(source), rendered_events=len(events), skipped_initial_program=programs,
                  final_original_controllers=controllers, synthetic_events=[], events_sha256=midi.sha(event_path),
                  command=command, native=native, render_process_seconds=elapsed, peak=float(abs(audio).max()),
                  last_100ms_rms=end_rms, output=str(a.output.resolve()), output_sha256=midi.sha(a.output), raw_sha256=midi.sha(raw),
                  validation=['Authenticated current production sources and executable', 'Original MIDI messages and rational timing preserved',
                              'All note releases present; original final pedal state retained', 'One continuous instance; no note samples',
                              'Finite stereo float audio below full scale', 'Exact WAV/raw equality', 'Tail below -100 dBFS RMS'])
    a.report.parent.mkdir(parents=True, exist_ok=True)
    a.report.write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8', newline='\n')
    print(json.dumps({k: report[k] for k in ('audio_duration_seconds', 'render_process_seconds', 'peak', 'last_100ms_rms', 'output')}))


if __name__ == '__main__':
    main()
