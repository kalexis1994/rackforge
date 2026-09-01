//! The host's MIDI vocabulary, shaped like MIDI 2.0, fed by MIDI 1.0.
//!
//! Every plug-in used to parse status bytes for itself, and so did the
//! host's own live state, and so did the sequencer when it wrote a note-off.
//! Three parsers agreed by luck. The day one of them read a byte the others
//! ignored -- release velocity, on 2026-09-01 -- the sequencer's `0x40` and
//! the test helper's `0` turned out to be two different "neutrals" that had
//! never disagreed only because nothing looked. This module is the one place
//! where a MIDI message MEANS something.
//!
//! It is modelled on MIDI 2.0's semantics because they are a superset of
//! MIDI 1.0's and cleaner: velocity is 16 bits, controllers and pressure and
//! pitch bend are 32, and a note-off may or may not carry a measurement. MIDI
//! 1.0 is a PRODUCER into it: seven-bit values are scaled up by the rule the
//! MIDI 2.0 specification gives for protocol translation, and a message that
//! never had a byte to give -- a Note On at velocity 0, the running-status
//! note-off -- says so with `None` instead of pretending a value. The reverse
//! direction, back to the three bytes the V1 plug-in ABI carries, is exact:
//! `from_midi1` followed by `to_midi1` is the identity on every message the
//! host accepts, and that is proved exhaustively below, byte by byte, rather
//! than argued. Nothing a plug-in receives has changed; what changed is that
//! there is now one vocabulary it was derived from.
//!
//! What this is NOT yet: Universal MIDI Packets. There are no groups here, no
//! per-note controllers, no 16-bit velocity from a real source. Those arrive
//! with the transport and the V2 plug-in extension. This is the floor they
//! stand on, and its only job today is to be exactly right about 1.0.

use rackforge_midi_api::MidiPacket;
use rackforge_plugin_api::abi::{
    MIDI_FAMILY_BEND, MIDI_FAMILY_CONTROL, MIDI_FAMILY_NOTE, MIDI_FAMILY_PRESSURE,
    MIDI_FAMILY_PROGRAM, MIDI2_FLAG_ORIGIN_7BIT, MIDI2_FLAG_RELEASE_MEASURED,
    MIDI2_KIND_CHANNEL_PRESSURE, MIDI2_KIND_CONTROL_CHANGE, MIDI2_KIND_NOTE_OFF,
    MIDI2_KIND_NOTE_ON, MIDI2_KIND_PITCH_BEND, MIDI2_KIND_POLY_PRESSURE, MIDI2_KIND_PROGRAM_CHANGE,
    MidiEventV1, MidiEventV2,
};

/// One channel-voice message, with MIDI 2.0's widths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Midi2Message {
    NoteOff {
        note: u8,
        /// `None` when the message had no byte to carry it: a Note On at
        /// velocity 0. `Some` for a real Note Off, whatever its byte was --
        /// including 0 and 64, so the byte survives the round trip. Whether a
        /// value is a MEASUREMENT is a different question, answered by
        /// `Midi2Event::release_velocity_measured`, and deliberately not here.
        velocity: Option<u16>,
    },
    NoteOn {
        note: u8,
        velocity: u16,
    },
    PolyPressure {
        note: u8,
        pressure: u32,
    },
    ControlChange {
        controller: u8,
        value: u32,
    },
    ProgramChange {
        program: u8,
    },
    ChannelPressure {
        pressure: u32,
    },
    PitchBend {
        /// Centre is `1 << 31`, as MIDI 2.0 has it.
        value: u32,
    },
    /// Anything that is not channel voice -- system common, real time, a
    /// short or odd message. Carried untouched so nothing the host accepted
    /// is lost on the way through the vocabulary.
    Raw {
        length: u8,
        data: [u8; 3],
    },
}

/// A message at a frame on a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Midi2Event {
    pub frame: u32,
    /// 0..=15. Meaningless for `Raw`, and kept at 0 there.
    pub channel: u8,
    pub message: Midi2Message,
    /// Whether the wide values were scaled up from a seven-bit source. Set by
    /// `from_midi1`, cleared by a producer that measured at full width. It
    /// rides through to `MidiEventV2::flags` so a plug-in can stay
    /// bit-identical for 1.0 input while using every bit a 2.0 source gives.
    pub origin_7bit: bool,
}

/// Scales a value up between bit widths the way the MIDI 2.0 specification's
/// protocol translation does: a plain shift below the centre, and above it
/// the low bits are filled by repeating the source's lower bits, so that the
/// source's full scale lands on the destination's full scale and the centre
/// lands on the centre. The top `src_bits` of the result are always the
/// source, which is what makes `scale_down` an exact inverse.
pub fn scale_up(value: u32, src_bits: u32, dst_bits: u32) -> u32 {
    debug_assert!(src_bits < dst_bits && dst_bits <= 32);
    let scale_bits = dst_bits - src_bits;
    let src_centre = 1u32 << (src_bits - 1);
    if value <= src_centre {
        return value << scale_bits;
    }
    let mut shifted = value << scale_bits;
    let src_centre_mask = src_centre - 1;
    let mut repeat = value & src_centre_mask;
    let repeat_bits = src_bits - 1;
    if scale_bits > repeat_bits {
        repeat <<= scale_bits - repeat_bits;
    } else {
        repeat >>= repeat_bits - scale_bits;
    }
    while repeat != 0 {
        shifted |= repeat;
        repeat >>= repeat_bits;
    }
    shifted
}

/// The exact inverse of `scale_up`: the top `src_bits` of the wide value.
pub fn scale_down(value: u32, src_bits: u32, dst_bits: u32) -> u32 {
    debug_assert!(src_bits < dst_bits && dst_bits <= 32);
    value >> (dst_bits - src_bits)
}

impl Midi2Event {
    /// MIDI 1.0 as a producer: the three-byte message every host path
    /// carries today, read once, here, into the vocabulary.
    pub fn from_midi1(event: &MidiEventV1) -> Self {
        let length = usize::from(event.length).min(3);
        let raw = || Midi2Message::Raw {
            length: event.length,
            data: event.data,
        };
        if length == 0 {
            return Self {
                frame: event.frame,
                channel: 0,
                message: raw(),
                origin_7bit: true,
            };
        }
        let status = event.data[0] & 0xf0;
        let channel = event.data[0] & 0x0f;
        let d1 = event.data[1] & 0x7f;
        let d2 = event.data[2] & 0x7f;
        let message = match (status, length) {
            (0x80, 3) => Midi2Message::NoteOff {
                note: d1,
                velocity: Some(scale_up(u32::from(d2), 7, 16) as u16),
            },
            (0x90, 3) if d2 == 0 => Midi2Message::NoteOff {
                note: d1,
                velocity: None,
            },
            (0x90, 3) => Midi2Message::NoteOn {
                note: d1,
                velocity: scale_up(u32::from(d2), 7, 16) as u16,
            },
            (0xa0, 3) => Midi2Message::PolyPressure {
                note: d1,
                pressure: scale_up(u32::from(d2), 7, 32),
            },
            (0xb0, 3) => Midi2Message::ControlChange {
                controller: d1,
                value: scale_up(u32::from(d2), 7, 32),
            },
            (0xc0, 2) => Midi2Message::ProgramChange { program: d1 },
            (0xd0, 2) => Midi2Message::ChannelPressure {
                pressure: scale_up(u32::from(d1), 7, 32),
            },
            (0xe0, 3) => Midi2Message::PitchBend {
                value: scale_up(u32::from(d1) | (u32::from(d2) << 7), 14, 32),
            },
            _ => {
                return Self {
                    frame: event.frame,
                    channel: 0,
                    message: raw(),
                    origin_7bit: true,
                };
            }
        };
        Self {
            frame: event.frame,
            channel,
            message,
            origin_7bit: true,
        }
    }

    /// Which `MIDI_FAMILY_*` this message belongs to; `None` for `Raw`,
    /// which no plug-in can ask for wide.
    pub fn family(&self) -> Option<u32> {
        Some(match self.message {
            Midi2Message::NoteOff { .. } | Midi2Message::NoteOn { .. } => MIDI_FAMILY_NOTE,
            Midi2Message::PolyPressure { .. } | Midi2Message::ChannelPressure { .. } => {
                MIDI_FAMILY_PRESSURE
            }
            Midi2Message::ControlChange { .. } => MIDI_FAMILY_CONTROL,
            Midi2Message::ProgramChange { .. } => MIDI_FAMILY_PROGRAM,
            Midi2Message::PitchBend { .. } => MIDI_FAMILY_BEND,
            Midi2Message::Raw { .. } => return None,
        })
    }

    /// The wide ABI event, for a plug-in that asked for this family. `Raw`
    /// has no wide form and returns `None`.
    pub fn to_v2(&self) -> Option<MidiEventV2> {
        let mut flags = if self.origin_7bit {
            MIDI2_FLAG_ORIGIN_7BIT
        } else {
            0
        };
        let (kind, index, value) = match self.message {
            Midi2Message::NoteOff { note, velocity } => {
                if self.release_velocity_measured().is_some() {
                    flags |= MIDI2_FLAG_RELEASE_MEASURED;
                }
                (MIDI2_KIND_NOTE_OFF, note, u32::from(velocity.unwrap_or(0)))
            }
            Midi2Message::NoteOn { note, velocity } => {
                (MIDI2_KIND_NOTE_ON, note, u32::from(velocity))
            }
            Midi2Message::PolyPressure { note, pressure } => {
                (MIDI2_KIND_POLY_PRESSURE, note, pressure)
            }
            Midi2Message::ControlChange { controller, value } => {
                (MIDI2_KIND_CONTROL_CHANGE, controller, value)
            }
            Midi2Message::ProgramChange { program } => (MIDI2_KIND_PROGRAM_CHANGE, program, 0),
            Midi2Message::ChannelPressure { pressure } => {
                (MIDI2_KIND_CHANNEL_PRESSURE, 0, pressure)
            }
            Midi2Message::PitchBend { value } => (MIDI2_KIND_PITCH_BEND, 0, value),
            Midi2Message::Raw { .. } => return None,
        };
        Some(MidiEventV2 {
            frame: self.frame,
            kind,
            channel: self.channel,
            index,
            flags,
            value,
            extra: 0,
        })
    }

    /// Back to the three bytes the V1 plug-in ABI carries. Exact for every
    /// event `from_midi1` produced -- see `round_trip_is_the_identity`.
    /// The host's packet, at whatever width it carries: its bytes lifted,
    /// then the wide value put back where the packet had one. A packet from
    /// a byte source is exactly `from_midi1` of its bytes.
    pub fn from_packet(packet: &MidiPacket) -> Self {
        let mut event = Self::from_midi1(&MidiEventV1 {
            frame: packet.frame,
            length: packet.length,
            data: packet.data,
        });
        if let Some(value) = packet.wide {
            event.origin_7bit = false;
            match &mut event.message {
                Midi2Message::NoteOff { velocity, .. } => *velocity = Some(value as u16),
                Midi2Message::NoteOn { velocity, .. } => *velocity = value as u16,
                Midi2Message::PolyPressure { pressure, .. }
                | Midi2Message::ChannelPressure { pressure } => *pressure = value,
                Midi2Message::ControlChange { value: wide, .. }
                | Midi2Message::PitchBend { value: wide } => *wide = value,
                Midi2Message::ProgramChange { .. } | Midi2Message::Raw { .. } => {
                    event.origin_7bit = true;
                }
            }
        }
        event
    }

    /// The host's packet for this event, its width kept: a byte-origin
    /// event is `MidiPacket::new` of its bytes, a wide one carries its value
    /// beside the projection. `None` for a system message, which the packet
    /// layer does not carry; a transport hands those to the clock instead.
    /// `from_packet` of the result is this event again.
    pub fn to_packet(&self) -> Option<MidiPacket> {
        let bytes = self.to_midi1();
        if bytes.data[0] >= 0xf0 {
            return None;
        }
        if self.origin_7bit {
            return MidiPacket::new(self.frame, &bytes.data[..bytes.length as usize]).ok();
        }
        let (index, value) = match self.message {
            Midi2Message::NoteOff {
                note,
                velocity: Some(velocity),
            } => (note, u32::from(velocity)),
            Midi2Message::NoteOff {
                note: _,
                velocity: None,
            }
            | Midi2Message::ProgramChange { .. }
            | Midi2Message::Raw { .. } => {
                return MidiPacket::new(self.frame, &bytes.data[..bytes.length as usize]).ok();
            }
            Midi2Message::NoteOn { note, velocity } => (note, u32::from(velocity)),
            Midi2Message::PolyPressure { note, pressure } => (note, pressure),
            Midi2Message::ControlChange { controller, value } => (controller, value),
            Midi2Message::ChannelPressure { pressure } => (0, pressure),
            Midi2Message::PitchBend { value } => (0, value),
        };
        MidiPacket::wide(self.frame, bytes.data[0], index, value).ok()
    }

    pub fn to_midi1(&self) -> MidiEventV1 {
        let ch = self.channel & 0x0f;
        let (length, data): (u8, [u8; 3]) = match self.message {
            Midi2Message::NoteOff {
                note,
                velocity: Some(v),
            } => (3, [0x80 | ch, note, scale_down(u32::from(v), 7, 16) as u8]),
            Midi2Message::NoteOff {
                note,
                velocity: None,
            } => (3, [0x90 | ch, note, 0]),
            Midi2Message::NoteOn { note, velocity } => (
                3,
                [
                    0x90 | ch,
                    note,
                    // The specification's translation rule: a note-on whose
                    // velocity would become the byte 0 becomes 1, since 0
                    // would make it a note-off. Unreachable from `from_midi1`,
                    // which never lifts a byte below 1; reachable from UMP.
                    (scale_down(u32::from(velocity), 7, 16) as u8).max(1),
                ],
            ),
            Midi2Message::PolyPressure { note, pressure } => {
                (3, [0xa0 | ch, note, scale_down(pressure, 7, 32) as u8])
            }
            Midi2Message::ControlChange { controller, value } => {
                (3, [0xb0 | ch, controller, scale_down(value, 7, 32) as u8])
            }
            Midi2Message::ProgramChange { program } => (2, [0xc0 | ch, program, 0]),
            Midi2Message::ChannelPressure { pressure } => {
                (2, [0xd0 | ch, scale_down(pressure, 7, 32) as u8, 0])
            }
            Midi2Message::PitchBend { value } => {
                let fourteen = scale_down(value, 14, 32);
                (
                    3,
                    [
                        0xe0 | ch,
                        (fourteen & 0x7f) as u8,
                        ((fourteen >> 7) & 0x7f) as u8,
                    ],
                )
            }
            Midi2Message::Raw { length, data } => (length, data),
        };
        MidiEventV1 {
            frame: self.frame,
            length,
            data,
        }
    }

    /// The key came up; was how fast it came up actually measured?
    ///
    /// This is the one home for a rule three pieces of code used to keep
    /// separately. A running-status note-off has no byte. A real Note Off
    /// at 0 is what most keyboards without a release sensor send. 64 is
    /// MIDI's conventional neutral, which the sequencer writes and a sensorless
    /// keyboard may write too. None of those is a gesture; reading them as
    /// one would change an instrument's character with the controller
    /// plugged in. Only another value is a measurement, and only a
    /// measurement may move anything.
    pub fn release_velocity_measured(&self) -> Option<u16> {
        match self.message {
            Midi2Message::NoteOff {
                velocity: Some(v), ..
            } => {
                let byte = scale_down(u32::from(v), 7, 16);
                if byte == 0 || byte == 64 {
                    None
                } else {
                    Some(v)
                }
            }
            _ => None,
        }
    }

    /// A note starting, with the velocity it started at.
    pub fn note_on(&self) -> Option<(u8, u16)> {
        match self.message {
            Midi2Message::NoteOn { note, velocity } => Some((note, velocity)),
            _ => None,
        }
    }

    /// A note ending, however the message spelled it.
    pub fn note_off(&self) -> Option<u8> {
        match self.message {
            Midi2Message::NoteOff { note, .. } => Some(note),
            _ => None,
        }
    }
}

/// The choke point's contract, asserted on every real-time block in debug
/// builds and free in release: every event the host hands a plug-in is one
/// the vocabulary can express and give back unchanged. The proof that it can
/// is exhaustive (`round_trip_is_the_identity`); this keeps the proof honest
/// against whatever a host path does next.
#[inline]
pub fn assert_expressible(events: &[MidiEventV1]) {
    if cfg!(debug_assertions) {
        for event in events {
            let back = Midi2Event::from_midi1(event).to_midi1();
            debug_assert_eq!(
                (back.frame, back.length, back.data),
                (event.frame, event.length, event.data),
                "a MIDI event left the vocabulary changed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(data: [u8; 3], length: u8) -> MidiEventV1 {
        MidiEventV1 {
            frame: 7,
            length,
            data,
        }
    }

    /// `from_midi1` then `to_midi1` is the identity on every three-byte
    /// channel-voice message there is -- every status, every channel, every
    /// pair of data bytes -- and on every two-byte one. Not argued: run.
    #[test]
    fn round_trip_is_the_identity() {
        let mut checked = 0u64;
        for status in 0x80u8..=0xef {
            let kind = status & 0xf0;
            let three_bytes = !matches!(kind, 0xc0 | 0xd0);
            let length = if three_bytes { 3 } else { 2 };
            for d1 in 0u8..=127 {
                let d2_range = if three_bytes { 0..=127u8 } else { 0..=0 };
                for d2 in d2_range {
                    let original = event([status, d1, d2], length);
                    let back = Midi2Event::from_midi1(&original).to_midi1();
                    assert_eq!(
                        (back.frame, back.length, back.data),
                        (original.frame, original.length, original.data),
                        "changed: {original:?} -> {back:?}"
                    );
                    checked += 1;
                }
            }
        }
        // 80 three-byte statuses (0x80..=0xbf, 0xe0..=0xef) x 128 x 128, plus
        // 32 two-byte statuses (0xc0..=0xdf) x 128: every channel-voice
        // message there is, and the exact count says none was skipped.
        assert_eq!(
            checked,
            80 * 128 * 128 + 32 * 128,
            "not every message was checked"
        );
    }

    /// Scaling lands the ends on the ends and the centre on the centre, as
    /// the specification requires, and comes back exactly.
    #[test]
    fn scaling_honours_the_specification() {
        assert_eq!(scale_up(0, 7, 16), 0);
        assert_eq!(scale_up(64, 7, 16), 0x8000);
        assert_eq!(scale_up(127, 7, 16), 0xffff);
        assert_eq!(scale_up(0, 7, 32), 0);
        assert_eq!(scale_up(64, 7, 32), 0x8000_0000);
        assert_eq!(scale_up(127, 7, 32), 0xffff_ffff);
        assert_eq!(scale_up(0x2000, 14, 32), 0x8000_0000);
        assert_eq!(scale_up(0x3fff, 14, 32), 0xffff_ffff);
        for v in 0..=127u32 {
            assert_eq!(scale_down(scale_up(v, 7, 16), 7, 16), v);
            assert_eq!(scale_down(scale_up(v, 7, 32), 7, 32), v);
        }
        for v in 0..=0x3fffu32 {
            assert_eq!(scale_down(scale_up(v, 14, 32), 14, 32), v);
        }
    }

    /// The rule three parsers used to keep separately now lives here, and
    /// says the same thing three ways: no byte, a byte of 0, a byte of 64 --
    /// none is a measurement.
    #[test]
    fn neutral_has_one_home() {
        let none = Midi2Event::from_midi1(&event([0x90, 60, 0], 3));
        let zero = Midi2Event::from_midi1(&event([0x80, 60, 0], 3));
        let sixty_four = Midi2Event::from_midi1(&event([0x80, 60, 64], 3));
        let measured = Midi2Event::from_midi1(&event([0x80, 60, 20], 3));
        assert_eq!(none.note_off(), Some(60));
        assert_eq!(none.release_velocity_measured(), None);
        assert_eq!(zero.release_velocity_measured(), None);
        assert_eq!(sixty_four.release_velocity_measured(), None);
        assert_eq!(
            measured.release_velocity_measured(),
            Some(scale_up(20, 7, 16) as u16)
        );
        // ...while the bytes themselves are preserved, because other plug-ins
        // may read them and they must see exactly what was sent.
        assert_eq!(none.to_midi1().data, [0x90, 60, 0]);
        assert_eq!(zero.to_midi1().data, [0x80, 60, 0]);
        assert_eq!(sixty_four.to_midi1().data, [0x80, 60, 64]);
    }

    /// The wide event carries the same message: for every 1.0 channel-voice
    /// message, going wide and reading the wide event back through the
    /// vocabulary's own downscale gives the original bytes, the origin flag
    /// is set, and the release flag says exactly what
    /// `release_velocity_measured` says.
    /// A packet from bytes is its bytes; a wide packet keeps its value and
    /// projects to exactly the bytes the packet already carried.
    #[test]
    fn a_packet_enters_at_its_width() {
        let bytes = MidiPacket::new(4, &[0x91, 60, 100]).unwrap();
        assert_eq!(
            Midi2Event::from_packet(&bytes),
            Midi2Event::from_midi1(&MidiEventV1 {
                frame: 4,
                length: 3,
                data: [0x91, 60, 100]
            })
        );
        let wide = MidiPacket::wide(4, 0x91, 60, 0x1234).unwrap();
        let event = Midi2Event::from_packet(&wide);
        assert_eq!(
            event.message,
            Midi2Message::NoteOn {
                note: 60,
                velocity: 0x1234
            }
        );
        assert!(!event.origin_7bit);
        assert_eq!(event.to_midi1().data, wide.data);
        let pedal = MidiPacket::wide(0, 0xb0, 64, 0xdead_beef).unwrap();
        let event = Midi2Event::from_packet(&pedal);
        assert_eq!(
            event.message,
            Midi2Message::ControlChange {
                controller: 64,
                value: 0xdead_beef
            }
        );
        assert_eq!(event.to_midi1().data, pedal.data);
        let whisper = Midi2Event::from_packet(&MidiPacket::wide(0, 0x90, 60, 7).unwrap());
        assert_eq!(whisper.to_midi1().data, [0x90, 60, 1]);
    }

    /// Into a packet and back is the identity on every message the packet
    /// layer carries, at either width; a system message has no packet.
    #[test]
    fn a_packet_is_the_event_again() {
        let mut cases = 0;
        for origin_7bit in [true, false] {
            for value in [
                0u32,
                1,
                0x1ff,
                0x200,
                0x7fff,
                0x8000,
                0xffff,
                0x8000_0000,
                u32::MAX,
            ] {
                let velocity = value as u16;
                let messages = [
                    Midi2Message::NoteOff {
                        note: 60,
                        velocity: Some(velocity),
                    },
                    Midi2Message::NoteOn { note: 61, velocity },
                    Midi2Message::PolyPressure {
                        note: 1,
                        pressure: value,
                    },
                    Midi2Message::ControlChange {
                        controller: 64,
                        value,
                    },
                    Midi2Message::ChannelPressure { pressure: value },
                    Midi2Message::PitchBend { value },
                ];
                for message in messages {
                    let event = Midi2Event {
                        frame: 3,
                        channel: 5,
                        message,
                        origin_7bit,
                    };
                    // A byte-origin event must be one a byte can express.
                    let event = if origin_7bit {
                        Midi2Event::from_midi1(&event.to_midi1())
                    } else {
                        event
                    };
                    let packet = event.to_packet().unwrap();
                    assert_eq!(packet.wide.is_none(), event.origin_7bit);
                    assert_eq!(packet.data, event.to_midi1().data);
                    assert_eq!(Midi2Event::from_packet(&packet), event);
                    cases += 1;
                }
            }
        }
        assert_eq!(cases, 2 * 9 * 6);
        let program = Midi2Event::from_midi1(&MidiEventV1 {
            frame: 0,
            length: 2,
            data: [0xc3, 9, 0],
        });
        assert_eq!(
            Midi2Event::from_packet(&program.to_packet().unwrap()),
            program
        );
        let clock = Midi2Event::from_midi1(&MidiEventV1 {
            frame: 0,
            length: 1,
            data: [0xf8, 0, 0],
        });
        assert_eq!(clock.to_packet(), None);
    }

    #[test]
    fn the_wide_event_is_the_same_message() {
        for status in [0x80u8, 0x90, 0xa0, 0xb0, 0xc0, 0xd0, 0xe0] {
            let length = if matches!(status, 0xc0 | 0xd0) { 2 } else { 3 };
            for d1 in [0u8, 1, 60, 64, 127] {
                for d2 in [0u8, 1, 20, 64, 100, 127] {
                    let original = event([status | 0x3, d1, d2], length);
                    let semantic = Midi2Event::from_midi1(&original);
                    let Some(wide) = semantic.to_v2() else {
                        panic!("channel voice has a wide form: {original:?}");
                    };
                    assert_eq!(wide.frame, 7);
                    assert_eq!(wide.channel, 3);
                    assert_ne!(wide.flags & MIDI2_FLAG_ORIGIN_7BIT, 0);
                    let measured = wide.flags & MIDI2_FLAG_RELEASE_MEASURED != 0;
                    assert_eq!(measured, semantic.release_velocity_measured().is_some());
                    // Two u64 words round-trip the ABI struct exactly.
                    let (head, tail) = wide.packed();
                    assert_eq!(MidiEventV2::from_packed(head, tail), wide);
                    // Wide value back to the byte: the origin flag promises this.
                    let back = match wide.kind {
                        MIDI2_KIND_NOTE_ON | MIDI2_KIND_NOTE_OFF => {
                            scale_down(wide.value, 7, 16) as u8
                        }
                        MIDI2_KIND_PITCH_BEND => (scale_down(wide.value, 14, 32) & 0x7f) as u8,
                        MIDI2_KIND_PROGRAM_CHANGE => wide.index,
                        MIDI2_KIND_CHANNEL_PRESSURE => scale_down(wide.value, 7, 32) as u8,
                        _ => scale_down(wide.value, 7, 32) as u8,
                    };
                    let expected = match status {
                        0xc0 => d1,
                        0xd0 => d1,
                        0xe0 => d1 & 0x7f,
                        0x90 if d2 == 0 => 0,
                        _ => d2,
                    };
                    assert_eq!(
                        back, expected,
                        "wide value lost the byte: {original:?} -> {wide:?}"
                    );
                }
            }
        }
    }

    /// What is not channel voice is carried, not dropped, and not reshaped.
    #[test]
    fn everything_else_is_carried_untouched() {
        for (data, length) in [
            ([0xf8, 0, 0], 1u8),
            ([0xfa, 0, 0], 1),
            ([0xf1, 0x33, 0], 2),
            ([0x91, 60, 0], 2),
        ] {
            let original = event(data, length);
            let back = Midi2Event::from_midi1(&original).to_midi1();
            assert_eq!((back.length, back.data), (original.length, original.data));
        }
    }
}
