"""Persistent musical events and bounded board capture for the physical pilot.

Uses the existing contact/coast/damper subflows. Chunk boundaries never reset
the strings, heads, board, air or key/pedal history. The sink receives local
uniform Hermite traces; pressure convolution/resampling across chunks remains
a separate integration task. No sampled-note assembly or pitch shifting.
"""
from collections import Counter
import time

import numpy as np
from scipy.interpolate import CubicHermiteSpline
from concert_grand_damped_coast import engagement


class PerformanceControl:
    def __init__(self, keys, *, allow_binary_pedal=False, allow_frozen_program=False):
        self.held = Counter({key: 0 for key in keys})
        self.changed = {key: -.03 for key in keys}
        self.pedal = False
        self.allow_binary_pedal = allow_binary_pedal
        self.allow_frozen_program = allow_frozen_program

    def validate(self, events):
        channels = {event['channel'] for event in events}
        if len(channels) > 1:
            raise ValueError('one physical piano channel required')
        for event in events:
            kind, key, value = (event[k] for k in ('kind', 'key', 'value'))
            if kind in ('on', 'off'):
                if key not in self.held or not 0 <= value <= 127:
                    raise ValueError('note outside the constructed physical bank or MIDI range')
                if kind == 'on' and value == 0:
                    raise ValueError('zero-velocity on must be planned as off')
            elif kind == 'cc':
                if key not in (64, 66, 67) or not 0 <= value <= 127:
                    raise ValueError('unsupported MIDI controller')
                if key == 64 and value not in (0, 127) and not self.allow_binary_pedal:
                    raise ValueError('continuous sustain requires a physical mechanism; binary diagnostic must be explicit')
                if key in (66, 67) and value:
                    raise ValueError('sostenuto/soft pedal mechanism not implemented')
            elif kind == 'program':
                if not self.allow_frozen_program:
                    raise ValueError('program selection requires an explicit frozen-program diagnostic')
            else:
                raise ValueError('unsupported planned event')

    def event(self, event, now):
        kind, key, value = (event[k] for k in ('kind', 'key', 'value'))
        if kind == 'on':
            self.held[key] += 1
            self.changed[key] = now
        elif kind == 'off':
            self.held[key] = max(0, self.held[key] - 1)
            if not self.held[key]:
                self.changed[key] = now
        elif kind == 'cc' and key == 64:
            down = value >= 64
            if self.pedal and not down:
                for k, count in self.held.items():
                    if not count:
                        self.changed[k] = now
            self.pedal = down

    def levels(self, now):
        return {key: 0. if self.pedal or count else engagement(now - self.changed[key])
                for key, count in self.held.items()}


def play(system, contact, coast, dampers, events, end_tick, sink, *,
         contact_rate=256000, coast_rate=128000, capture_rate=128000,
         block_s=.125, allow_binary_pedal=False, allow_frozen_program=False,
         progress=None, control=None):
    """One cold chronological trajectory; memory of board history is bounded.

No clipping/truncation of unsupported MIDI: validate before launch. The
caller constructs every requested string port, and explicitly supplies a
tail and a sink. A supplied control implements pedal travel/parameter work;
the default retains the frozen binary path. This does not certify convergence.
"""
    if (not callable(sink) or contact_rate <= 0 or coast_rate <= 0 or capture_rate <= 0
            or contact_rate % coast_rate or contact_rate % capture_rate):
        raise ValueError('sink and aligned positive rates required')
    stride, capture = contact_rate // coast_rate, contact_rate // capture_rate
    block = round(block_s * contact_rate)
    if (block <= 0 or block % stride or block % capture or end_tick <= 0
            or end_tick % capture or not isinstance(end_tick, int)):
        raise ValueError('block/end must align to capture and coast ticks')
    ticks = [event['tick'] for event in events]
    if (ticks != sorted(ticks) or any(not isinstance(t, int) or not 0 <= t < end_tick for t in ticks)):
        raise ValueError('ordered events strictly inside the trajectory')
    custom_control = control is not None
    control = control if custom_control else PerformanceControl(system.keys,
        allow_binary_pedal=allow_binary_pedal, allow_frozen_program=allow_frozen_program)
    if set(control.held) != set(system.keys):
        raise ValueError('control must cover the constructed bank')
    control.validate(events)
    if custom_control:
        control.bind(system, contact)
    state = contact.initial_state(0.)
    parameter_work = control.advance(system, state, 0.) if custom_control else 0.
    core = system.string
    def board(q):
        return core.board_transform @ q[core.sb]
    T, B, V = [0.], [board(state[0])], [board(state[1])]
    cursor = tick = contact_steps = coast_steps = retries = chunks = 0
    maximum_knots = 1
    next_block = min(block, end_tick)
    felt = material = damper_heat = work = air_work = 0.
    launches = []
    begin = last = time.perf_counter()
    while tick < end_tick:
        now = tick / contact_rate
        while cursor < len(events) and events[cursor]['tick'] == tick:
            event = events[cursor]
            control.event(event, now)
            if event['kind'] == 'on':
                previous = state
                state, external = system.launch(state, event['key'], 5. * (event['value'] / 124)**1.5)
                if any(not np.array_equal(previous[i][:core.size], state[i][:core.size]) for i in (0, 1)):
                    raise AssertionError('launch reset living strings/board')
                if any(not np.array_equal(previous[i], state[i]) for i in (2, 3)):
                    raise AssertionError('launch reset acoustic memory')
                work += external
                launches.append(dict(tick=tick, key=event['key'], external_work_j=external))
            cursor += 1
        in_contact = system.compression(state[0]) >= -1e-12
        step = 1 if in_contact or tick % stride else stride
        if tick + step > next_block or (cursor < len(events) and tick + step > events[cursor]['tick']):
            step = 1
        dt = step / contact_rate
        if custom_control:
            parameter_work += control.advance(system, state, now + dt / 2)
        levels = control.levels(now + dt / 2)
        v, heat0 = dampers.dissipate(state[1], dt / 2, levels)
        # Preserve the frozen event-alignment path when stride>1. Equal
        # contact/coast rates must still use coast after head separation.
        if in_contact or (step == 1 and stride > 1):
            middle, stats = contact.step((state[0], v, state[2], state[3]), dt, True)
            contact_steps += 1
        else:
            try:
                middle, stats = coast.step((state[0], v, state[2], state[3]), dt, True)
                coast_steps += 1
            except ValueError as error:
                if 'recontact' not in str(error):
                    raise
                retries += 1
                step, dt = 1, 1 / contact_rate
                if custom_control:
                    parameter_work += control.advance(system, state, now + dt / 2)
                levels = control.levels(now + dt / 2)
                v, heat0 = dampers.dissipate(state[1], dt / 2, levels)
                middle, stats = contact.step((state[0], v, state[2], state[3]), dt, True)
                contact_steps += 1
        v, heat1 = dampers.dissipate(middle[1], dt / 2, levels)
        state = (middle[0], v, middle[2], middle[3])
        tick += step
        if custom_control:
            parameter_work += control.advance(system, state, tick / contact_rate)
        felt += stats['felt_loss_j']
        material += stats['material_loss_j']
        damper_heat += heat0 + heat1
        air_work += stats['air_port_work_j']
        if tick % capture == 0:
            T.append(tick / contact_rate)
            B.append(board(state[0]))
            V.append(board(state[1]))
            maximum_knots = max(maximum_knots, len(T))
        if tick == next_block:
            if not all(np.isfinite(x).all() for x in state):
                raise ValueError('nonfinite living state')
            start_tick = round(T[0] * contact_rate)
            sample_count = (tick - start_tick) // capture
            local = np.arange(sample_count + 1) / capture_rate
            spline = CubicHermiteSpline(np.asarray(T) - T[0], np.asarray(B), np.asarray(V), axis=0)
            sink(start_tick / contact_rate, (local, spline(local), spline(local, 1)))
            chunks += 1
            T, B, V = [T[-1]], [B[-1]], [V[-1]]
            next_block = min(next_block + block, end_tick)
        if time.perf_counter() - last > 25:
            value = dict(physical_time_s=tick / contact_rate, duration_s=end_tick / contact_rate,
                         seconds=time.perf_counter() - begin, chunks=chunks)
            if progress:
                progress(value)
            last = time.perf_counter()
    if cursor != len(events) or contact_steps + stride * coast_steps != end_tick:
        raise AssertionError('missed event or duration')
    final_energy = system.energy(*state[:2])
    residual = final_energy - work - parameter_work + felt + material + damper_heat + air_work
    result = dict(seconds=time.perf_counter() - begin, launches=launches,
                       contact_steps=contact_steps, coast_steps=coast_steps, recontact_retries=retries,
                       chunks=chunks, maximum_buffered_knots=maximum_knots,
                       nominal_capture_buffer_bytes=maximum_knots * (2 * core.nb * 8 + 8),
                       buffer_scope='Knot arrays only; spline temporaries, mechanics and air are additional.',
                       felt_heat_j=felt, material_heat_j=material, damper_heat_j=damper_heat,
                       external_head_work_j=work, air_port_work_j=air_work,
                       final_mechanical_j=final_energy, mechanical_work_residual_j=residual,
                       binary_pedal_diagnostic=allow_binary_pedal,
                       frozen_program_diagnostic=allow_frozen_program,
                       full_temporal_convergence_established=False)
    if custom_control:
        result.update(pedal_parameter_work_j=parameter_work, pedal_action=control.metadata(),
                      frozen_program_diagnostic=control.allow_frozen_program)
    return state, result
