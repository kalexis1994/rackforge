#!/usr/bin/env python3
"""Play a renderer event plan at its original rate and measure the whole journal.

Run on the appliance. This sends real MIDI through the live engine; it does
not compare the ALSA output sample for sample with the offline renderer.
The supplied profile is verified before and after playback. No quality or
audio settings are changed. A private virtual-MIDI client is released at exit.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('events', type=Path)
    parser.add_argument('profile', type=Path)
    parser.add_argument('report', type=Path)
    parser.add_argument('--rate', type=int, default=44100)
    parser.add_argument('--tail', type=float, default=8)
    parser.add_argument('--socket', default=os.path.expanduser('~/rackforge/state/live-control.sock'))
    parser.add_argument('--instance', default='live.main.instrument.1')
    parser.add_argument('--unit', default='rackforge-audio.service')
    args = parser.parse_args()
    assert not args.report.exists(), 'Use a new report path for every measurement'
    events = [tuple(map(int, line.split())) for line in args.events.read_text().splitlines()]
    assert events and all(len(e) == 4 and e[0] >= 0 and 0 <= e[1] <= 255
                          and all(0 <= x <= 127 for x in e[2:]) for e in events)
    assert all(a[0] <= b[0] for a, b in zip(events, events[1:]))
    profile = json.loads(args.profile.read_text(encoding='utf-8-sig'))
    client = f'concert-validation-{os.getpid()}'

    def call(payload):
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
            stream.settimeout(10)
            stream.connect(args.socket)
            stream.sendall((json.dumps(payload) + '\n').encode())
            data = b''
            while b'\n' not in data:
                part = stream.recv(65536)
                if not part:
                    break
                data += part
        response = json.loads(data.split(b'\n')[0])
        assert response.get('status') != 'error', response
        return response

    def verify():
        response = call({'op': 'plugin_parameters', 'instance_id': args.instance})
        actual = {p['index']: p['value'] for p in response['values']}
        mismatch = [p for p in profile['values'] if actual.get(p['index']) != p['value']]
        assert not mismatch, mismatch
        return response['values']

    verify()
    audio = call({'op': 'audio_snapshot'})['snapshot']
    assert audio['active_profile']['sample_rate_hz'] == args.rate
    assert audio['active_profile']['period_frames'] == profile['block']
    cursor_text = subprocess.run(['journalctl', '-u', args.unit, '-n', '1',
                                 '--show-cursor', '--no-pager', '-o', 'cat'],
                                check=True, capture_output=True, text=True).stdout
    cursor = cursor_text.split('-- cursor: ')[-1].strip()
    assert cursor != cursor_text.strip() and cursor
    sent = 0
    lateness = []
    error = None
    start = time.monotonic() + 0.5
    progress = start + 30
    try:
        for frame, status, data1, data2 in events:
            due = start + frame / args.rate
            delay = due - time.monotonic()
            if delay > 0:
                time.sleep(delay)
            lateness.append(max(0, time.monotonic() - due))
            response = call({'op': 'virtual_midi', 'client_id': client,
                             'message': {'status': status, 'data1': data1, 'data2': data2}})
            assert response['status'] == 'virtual_midi_accepted', response
            sent += 1
            if time.monotonic() >= progress:
                print(f'Playback {frame / args.rate:.1f}s; accepted={sent}/{len(events)}', flush=True)
                progress += 30
        time.sleep(args.tail)
    except Exception as exc:
        error = repr(exc)
    finally:
        call({'op': 'release_virtual_midi', 'client_id': client})
    elapsed = time.monotonic() - start
    # Include the final telemetry interval and cleanup, never truncate the attack.
    time.sleep(2)
    journal = subprocess.run(['journalctl', '-u', args.unit, f'--after-cursor={cursor}',
                              '--no-pager', '-o', 'cat'], check=True,
                             capture_output=True, text=True).stdout
    log_path = args.report.with_suffix('.journal.txt')
    log_path.write_text(journal)
    blocks = []
    meters = []
    for line in journal.splitlines():
        fields = {k: float(v) for k, v in re.findall(r'(\w+)=(-?\d+(?:\.\d+)?)', line)}
        if 'AUDIO_RENDER_BLOCK ' in line:
            blocks.append(fields)
        if 'AUDIO_METER ' in line:
            meters.append(fields)
    count = sum(b['blocks'] for b in blocks)
    late_sorted = sorted(lateness)
    report = {
        'events_sha256': hashlib.sha256(args.events.read_bytes()).hexdigest(),
        'profile_sha256': hashlib.sha256(args.profile.read_bytes()).hexdigest(),
        'audio_device': audio['active_device'], 'audio_profile': audio['active_profile'],
        'events': len(events), 'accepted': sent, 'elapsed_seconds': elapsed, 'error': error,
        'send_lateness_ms': {'p99': late_sorted[int(.99 * (len(late_sorted) - 1))] * 1000,
                             'max': max(lateness) * 1000} if lateness else {},
        'telemetry_windows': len(blocks), 'blocks': count,
        'render_avg_us': sum(b['avg_us'] * b['blocks'] for b in blocks) / count if count else None,
        'render_max_us': max((b['max_us'] for b in blocks), default=None),
        'deadline_misses': sum(b['deadline_misses'] for b in blocks),
        'meter_peak': max((m['peak'] for m in meters), default=None),
        'clipped': sum(m.get('clipped', 0) for m in meters),
        'dropped_events': sum(m.get('dropped_events', 0) for m in meters),
        # This host logs only the current block's MIDI count once per second.
        # It establishes routing, not a total of every received MIDI event.
        'meter_observed_midi_events': sum(m.get('midi_events', 0) for m in meters),
        'quality_budget_lines': [l for l in journal.splitlines() if 'AUDIO_QUALITY_BUDGET' in l],
        'error_lines': [l for l in journal.splitlines()
                        if re.search(r'XRUN|UNDERRUN|FAILED|ERROR|PANIC|TIMED_OUT|FAULT|QUARANTINED', l, re.I)],
        'parameters_after': verify(), 'journal': str(log_path),
        'note': 'Virtual MIDI timing includes IPC/scheduler jitter; this is a live deadline test, not an audio identity test.'}
    args.report.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: v for k, v in report.items()
                      if k not in ('parameters_after', 'audio_device')}, indent=2), flush=True)
    assert not error and sent == len(events) and count > 0 and report['meter_observed_midi_events'] > 0
    assert not report['deadline_misses'] and not report['dropped_events'] and not report['clipped']
    assert not report['quality_budget_lines'] and not report['error_lines']


if __name__ == '__main__':
    main()
