"""Lossless MIDI event plan for the shared physical piano prototype.

Times use the original tempo map and an explicit nearest integration tick.
Controller values are retained; unsupported mechanisms are reported rather
than silently replaced. This module does not render or transpose notes.
"""
from collections import Counter
from fractions import Fraction
from pathlib import Path
import hashlib

import mido


def plan_midi(path, contact_rate=256000):
    path = Path(path)
    midi = mido.MidiFile(path)
    if midi.type == 2 or midi.ticks_per_beat <= 0:
        raise ValueError('synchronous PPQN MIDI required')
    if not isinstance(contact_rate, int) or contact_rate <= 0:
        raise ValueError('positive integer integration rate')
    elapsed = Fraction(0)
    tempo = 500000
    events, counts, controllers, channels, keys = [], Counter(), {}, set(), set()
    max_error = 0.
    unsupported = set()
    for message in mido.merge_tracks(midi.tracks):
        elapsed += Fraction(message.time * tempo, midi.ticks_per_beat * 1000000)
        counts[message.type] += 1
        if message.type == 'set_tempo':
            tempo = message.tempo
            continue
        if message.is_meta:
            continue
        scaled = elapsed * contact_rate
        tick = (2 * scaled.numerator + scaled.denominator) // (2 * scaled.denominator)
        max_error = max(max_error, float(abs(Fraction(tick, contact_rate) - elapsed)))
        if hasattr(message, 'channel'):
            channels.add(message.channel)
        if message.type in ('note_on', 'note_off'):
            on = message.type == 'note_on' and message.velocity > 0
            events.append(dict(tick=tick, kind='on' if on else 'off', key=message.note,
                               value=message.velocity, channel=message.channel))
            if on:
                keys.add(message.note)
        elif message.type == 'control_change':
            controllers.setdefault(str(message.control), Counter())[message.value] += 1
            events.append(dict(tick=tick, kind='cc', key=message.control,
                               value=message.value, channel=message.channel))
            if message.control == 64 and message.value not in (0, 127):
                unsupported.add('continuous_sustain')
            elif message.control == 66 and message.value:
                unsupported.add('sostenuto')
            elif message.control == 67 and message.value:
                unsupported.add('soft_pedal')
            elif message.control not in (64, 66, 67):
                unsupported.add('controller_' + str(message.control))
        elif message.type == 'program_change':
            # The physical prototype has one frozen parameter set. Preserve
            # the program request in the plan so a renderer must choose explicitly.
            events.append(dict(tick=tick, kind='program', key=message.program,
                               value=0, channel=message.channel))
            unsupported.add('program_selection')
        else:
            unsupported.add(message.type)
    if not keys:
        raise ValueError('MIDI contains no strikes')
    if len(channels) > 1:
        unsupported.add('multiple_channels')
    return dict(midi=str(path.resolve()), midi_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                contact_rate_hz=contact_rate, duration_s=float(elapsed),
                ticks_per_beat=midi.ticks_per_beat, keys=sorted(keys), channels=sorted(channels),
                message_counts=dict(counts), controller_values={c: dict(v) for c, v in controllers.items()},
                maximum_quantization_error_s=max_error, events=events,
                unsupported_for_faithful_render=sorted(unsupported))


def summarize(plan, current_keys=(60, 67, 72), prefix_s=10.):
    """Event/keyboard demand, plus binary-pedal occupancy as a cost proxy only."""
    held, ringing = Counter(), set()
    pedal = False
    strikes = peak_held = peak_ringing = simultaneous = 0
    batch = Counter()
    first_keys = set()
    for event in plan['events']:
        tick, kind, key, value = (event[k] for k in ('tick', 'kind', 'key', 'value'))
        if kind == 'on':
            held[key] += 1
            ringing.add(key)
            strikes += 1
            batch[tick] += 1
            if tick / plan['contact_rate_hz'] <= prefix_s:
                first_keys.add(key)
        elif kind == 'off':
            held[key] = max(0, held[key] - 1)
            if not held[key] and not pedal:
                ringing.discard(key)
        elif kind == 'cc' and key == 64:
            pedal = value >= 64
            if not pedal:
                ringing.intersection_update(k for k, v in held.items() if v)
        peak_held = max(peak_held, sum(v > 0 for v in held.values()))
        peak_ringing = max(peak_ringing, len(ringing))
    simultaneous = max(batch.values(), default=0)
    return dict(strikes=strikes, distinct_keys=len(plan['keys']),
                missing_current_keys=sorted(set(plan['keys']) - set(current_keys)),
                pitch_range_midi=[min(plan['keys']), max(plan['keys'])],
                prefix_s=prefix_s, prefix_keys=sorted(first_keys),
                peak_held_keys=peak_held, peak_ringing_keys_binary_proxy=peak_ringing,
                maximum_attacks_same_integration_tick=simultaneous,
                occupancy_scope='Single piano, aggregate held counts, binary CC64 proxy; not physical decay or half-pedal identification.')
