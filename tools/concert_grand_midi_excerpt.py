"""Cold-start MIDI excerpt preserving events, with an explicit pedal-return tail."""
from collections import Counter


def excerpt(plan, cut_tick, tail_ticks, *, bank_keys):
    rate = plan['contact_rate_hz']
    if (not isinstance(cut_tick, int) or not isinstance(tail_ticks, int)
            or cut_tick <= 0 or tail_ticks <= 0 or (cut_tick + tail_ticks) % 2):
        raise ValueError('positive excerpt/tail ticks and 128k aligned endpoint')
    first = next(event for event in plan['events'] if event['kind'] == 'on')
    prefix = plan['events'][:plan['events'].index(first)]
    if any(event['kind'] not in ('cc', 'program') for event in prefix):
        raise ValueError('only cold silence can be reduced before first attack')
    events = [dict(event, tick=event['tick'] - first['tick']) for event in plan['events']
              if first['tick'] <= event['tick'] < first['tick'] + cut_tick]
    held = Counter()
    for event in events:
        if event['kind'] in ('on', 'off'):
            if event['key'] not in bank_keys:
                raise ValueError('original note outside the constructed physical bank')
            if event['kind'] == 'on':
                held[event['key']] += 1
            else:
                held[event['key']] = max(0, held[event['key']] - 1)
    if any(held.values()):
        raise ValueError('choose a cut after original key releases; no synthetic note-off')
    channels = {event['channel'] for event in prefix + events}
    if len(channels) != 1:
        raise ValueError('one physical piano channel required')
    closing = dict(tick=cut_tick, kind='cc', key=64, value=0, channel=next(iter(channels)))
    return dict(original_first_attack_tick=first['tick'],
                cold_silent_pre_roll_s=first['tick'] / rate,
                original_absolute_cut_tick=first['tick'] + cut_tick,
                original_excerpt_duration_s=cut_tick / rate,
                tail_duration_s=tail_ticks / rate, end_tick=cut_tick + tail_ticks,
                original_pre_attack_events=prefix, original_shifted_events=events,
                synthetic_closing_events=[closing], simulated_events=events + [closing],
                original_attacks=sum(event['kind'] == 'on' for event in events),
                original_note_offs=sum(event['kind'] == 'off' for event in events),
                no_pitch_or_velocity_changes=True, cut_after_all_original_key_releases=True)
