//! Universal MIDI Packets, read into the host's vocabulary and written from it.
//!
//! A UMP is one to four 32-bit words; the top nibble of the first word says
//! which kind, and the kind fixes the length. Two kinds carry channel-voice
//! messages: MIDI 1.0 Channel Voice (type 2, one word: the three bytes with a
//! group in front) and MIDI 2.0 Channel Voice (type 4, two words: 16-bit
//! velocity, 32-bit controllers, and messages MIDI 1.0 never had). This
//! module turns both into [`Midi2Event`]s -- a type-2 packet reads EXACTLY
//! as its bytes would, and that is proved below over every byte -- and
//! writes a [`Midi2Event`] back as a type-4 packet.
//!
//! What has no byte form is named, not dropped silently: per-note
//! controllers, per-note bend and per-note management come back as
//! [`Unread::NoByteForm`] with their opcode, so a transport can count them
//! and a later stage can give them a home. Registered and assignable
//! parameters DO have a byte form -- the controller sequence 101/100/6/38
//! (99/98 for assignable) with the top fourteen bits of the data -- and are
//! read as that sequence, which is the translation the specification
//! prescribes. A program change with a bank becomes the two bank
//! controllers and the program, in that order.
//!
//! Groups are carried beside the event, not inside it: a transport decides
//! what a group is (a port, a channel space, a cable) and the vocabulary
//! stays what it is.

use crate::midi2::{Midi2Event, Midi2Message};
use rackforge_plugin_api::abi::MidiEventV1;

pub const MT_UTILITY: u8 = 0x0;
pub const MT_SYSTEM: u8 = 0x1;
pub const MT_MIDI1_VOICE: u8 = 0x2;
pub const MT_SYSEX7: u8 = 0x3;
pub const MT_MIDI2_VOICE: u8 = 0x4;
pub const MT_DATA128: u8 = 0x5;
pub const MT_FLEX: u8 = 0xD;
pub const MT_STREAM: u8 = 0xF;

/// How many words a packet occupies, from its first word alone. Reserved
/// types have the lengths the specification assigns them, so a stream with
/// a message this host does not know still stays in step.
pub const fn word_count(first: u32) -> usize {
    match first >> 28 {
        0x0..=0x2 | 0x6 | 0x7 => 1,
        0x3 | 0x4 | 0x8..=0xA => 2,
        0xB | 0xC => 3,
        _ => 4,
    }
}

/// Why a packet produced no event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unread {
    /// NOOP, jitter-reduction clocks and timestamps: the transport's business.
    Utility,
    /// SysEx7, SysEx8 and mixed data sets: the control plane's business.
    Data,
    /// Flex data (lyrics, tempo, key) and stream messages (endpoint discovery).
    FlexOrStream,
    /// A message type the specification reserves.
    Reserved(u8),
    /// A SysEx7 continuation or end with no start before it, or a message
    /// longer than [`SYSEX7_MAX_BYTES`]; whatever was gathered is dropped.
    SysExOutOfOrder,
    /// A MIDI 2.0 channel-voice opcode with no MIDI 1.0 form: per-note
    /// controllers (0x0, 0x1), relative parameters (0x4, 0x5), per-note
    /// pitch bend (0x6) and per-note management (0xF).
    NoByteForm(u8),
    /// A status or opcode the specification does not define.
    Malformed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UmpError {
    /// The packet needs more words than the buffer holds.
    Truncated { needed: usize, available: usize },
}

/// What reading one packet did: how many words it took, and, when it
/// produced nothing, why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Read {
    pub consumed: usize,
    pub unread: Option<Unread>,
}

fn byte_length(status: u8) -> u8 {
    match status & 0xf0 {
        0xc0 | 0xd0 => 2,
        _ => 3,
    }
}

fn lifted(frame: u32, data: [u8; 3], length: u8) -> Midi2Event {
    Midi2Event::from_midi1(&MidiEventV1 {
        frame,
        length,
        data,
    })
}

/// Reads the packet at the head of `words`, handing each event it carries
/// to `emit` with the packet's group. Every event lands at `frame`.
pub fn read_packet(
    words: &[u32],
    frame: u32,
    emit: &mut impl FnMut(u8, Midi2Event),
) -> Result<Read, UmpError> {
    let Some(&first) = words.first() else {
        return Err(UmpError::Truncated {
            needed: 1,
            available: 0,
        });
    };
    let consumed = word_count(first);
    if words.len() < consumed {
        return Err(UmpError::Truncated {
            needed: consumed,
            available: words.len(),
        });
    }
    let message_type = (first >> 28) as u8;
    let group = ((first >> 24) & 0xf) as u8;
    let done = |unread| {
        Ok(Read {
            consumed,
            unread: Some(unread),
        })
    };
    let read = Ok(Read {
        consumed,
        unread: None,
    });
    match message_type {
        MT_UTILITY => done(Unread::Utility),
        MT_SYSTEM => {
            let status = (first >> 16) as u8;
            let length = match status {
                0xf1 | 0xf3 => 2,
                0xf2 => 3,
                0xf6 | 0xf8 | 0xfa..=0xfc | 0xfe | 0xff => 1,
                _ => return done(Unread::Malformed),
            };
            let mut data = [status, 0, 0];
            if length >= 2 {
                data[1] = (first >> 8) as u8 & 0x7f;
            }
            if length == 3 {
                data[2] = first as u8 & 0x7f;
            }
            emit(
                group,
                Midi2Event {
                    frame,
                    channel: 0,
                    message: Midi2Message::Raw { length, data },
                    origin_7bit: true,
                },
            );
            read
        }
        MT_MIDI1_VOICE => {
            let status = (first >> 16) as u8;
            if !(0x80..0xf0).contains(&status) {
                return done(Unread::Malformed);
            }
            let data = [status, (first >> 8) as u8 & 0x7f, first as u8 & 0x7f];
            emit(group, lifted(frame, data, byte_length(status)));
            read
        }
        MT_MIDI2_VOICE => {
            let second = words[1];
            let opcode = ((first >> 20) & 0xf) as u8;
            let channel = ((first >> 16) & 0xf) as u8;
            let index = (first >> 8) as u8 & 0x7f;
            let low = first as u8;
            let wide = |message| Midi2Event {
                frame,
                channel,
                message,
                origin_7bit: false,
            };
            let controller = |controller: u8, byte: u8| {
                lifted(frame, [0xb0 | channel, controller, byte & 0x7f], 3)
            };
            match opcode {
                0x8 => emit(
                    group,
                    wide(Midi2Message::NoteOff {
                        note: index,
                        velocity: Some((second >> 16) as u16),
                    }),
                ),
                0x9 => emit(
                    group,
                    wide(Midi2Message::NoteOn {
                        note: index,
                        velocity: (second >> 16) as u16,
                    }),
                ),
                0xA => emit(
                    group,
                    wide(Midi2Message::PolyPressure {
                        note: index,
                        pressure: second,
                    }),
                ),
                0xB => emit(
                    group,
                    wide(Midi2Message::ControlChange {
                        controller: index,
                        value: second,
                    }),
                ),
                0xC => {
                    if low & 1 != 0 {
                        emit(group, controller(0, (second >> 8) as u8));
                        emit(group, controller(32, second as u8));
                    }
                    emit(
                        group,
                        lifted(frame, [0xc0 | channel, (second >> 24) as u8 & 0x7f, 0], 2),
                    );
                }
                0xD => emit(
                    group,
                    wide(Midi2Message::ChannelPressure { pressure: second }),
                ),
                0xE => emit(group, wide(Midi2Message::PitchBend { value: second })),
                0x2 | 0x3 => {
                    // The parameter's address, then its top fourteen bits.
                    let (msb, lsb) = if opcode == 0x2 { (101, 100) } else { (99, 98) };
                    emit(group, controller(msb, index));
                    emit(group, controller(lsb, low));
                    emit(group, controller(6, (second >> 25) as u8));
                    emit(group, controller(38, (second >> 18) as u8));
                }
                0x0 | 0x1 | 0x4 | 0x5 | 0x6 | 0xF => return done(Unread::NoByteForm(opcode)),
                _ => return done(Unread::Malformed),
            }
            read
        }
        MT_SYSEX7 | MT_DATA128 => done(Unread::Data),
        MT_FLEX | MT_STREAM => done(Unread::FlexOrStream),
        other => done(Unread::Reserved(other)),
    }
}

/// The longest system-exclusive message the assembler will gather. Real
/// dumps are kilobytes; a stream that keeps continuing past this is a
/// stream that lost its end, and the memory is given back.
pub const SYSEX7_MAX_BYTES: usize = 64 * 1024;

const SYSEX7_COMPLETE: u32 = 0;
const SYSEX7_START: u32 = 1;
const SYSEX7_CONTINUE: u32 = 2;
const SYSEX7_END: u32 = 3;

/// Gathers system-exclusive messages out of SysEx7 packets (type 3).
///
/// A type-3 packet carries up to six data bytes and a status: the whole
/// message in this one packet, or its start, a continuation, or its end.
/// The packets leave out the `F0` and `F7` that frame the message on a
/// byte transport; the assembler puts them back, so what it hands over is
/// exactly what `midir` hands over for the same message, and the same
/// parsers read both. One assembler per connection: a message's packets
/// arrive in order on one endpoint, and other groups' messages between
/// them are not this assembler's business.
#[derive(Debug, Default)]
pub struct SysEx7Assembler {
    buffer: Vec<u8>,
    open: bool,
}

impl SysEx7Assembler {
    /// Feeds one type-3 packet. `Ok(Some(message))` when a message is
    /// complete, `F0` to `F7` inclusive; `Ok(None)` while one is still
    /// being gathered; `Err` for a packet that cannot belong to one, after
    /// which the assembler is empty again.
    pub fn push(&mut self, first: u32, second: u32) -> Result<Option<&[u8]>, Unread> {
        let status = (first >> 20) & 0xf;
        let count = ((first >> 16) & 0xf) as usize;
        if count > 6 || status > SYSEX7_END {
            self.reset();
            return Err(Unread::Malformed);
        }
        let bytes = [
            (first >> 8) as u8 & 0x7f,
            first as u8 & 0x7f,
            (second >> 24) as u8 & 0x7f,
            (second >> 16) as u8 & 0x7f,
            (second >> 8) as u8 & 0x7f,
            second as u8 & 0x7f,
        ];
        if matches!(status, SYSEX7_COMPLETE | SYSEX7_START) {
            self.reset();
            self.buffer.push(0xf0);
            self.open = true;
        } else if !self.open {
            self.reset();
            return Err(Unread::SysExOutOfOrder);
        }
        if self.buffer.len() + count > SYSEX7_MAX_BYTES {
            self.reset();
            return Err(Unread::SysExOutOfOrder);
        }
        self.buffer.extend_from_slice(&bytes[..count]);
        if matches!(status, SYSEX7_COMPLETE | SYSEX7_END) {
            self.buffer.push(0xf7);
            self.open = false;
            return Ok(Some(&self.buffer));
        }
        Ok(None)
    }

    fn reset(&mut self) {
        self.buffer.clear();
        self.open = false;
    }
}

/// Writes a system-exclusive message as SysEx7 packets, six bytes per
/// packet, appending two words per packet to `out`. `message` may carry
/// its `F0`/`F7` framing or not; the packets never do.
pub fn write_sysex7(group: u8, message: &[u8], out: &mut Vec<u32>) {
    let payload = message
        .strip_prefix(&[0xf0])
        .unwrap_or(message)
        .strip_suffix(&[0xf7])
        .unwrap_or_else(|| message.strip_prefix(&[0xf0]).unwrap_or(message));
    let head = (u32::from(MT_SYSEX7) << 28) | (u32::from(group & 0xf) << 24);
    let chunks: Vec<&[u8]> = if payload.is_empty() {
        vec![&[][..]]
    } else {
        payload.chunks(6).collect()
    };
    let last = chunks.len() - 1;
    for (index, chunk) in chunks.iter().enumerate() {
        let status = match (index == 0, index == last) {
            (true, true) => SYSEX7_COMPLETE,
            (true, false) => SYSEX7_START,
            (false, false) => SYSEX7_CONTINUE,
            (false, true) => SYSEX7_END,
        };
        let mut bytes = [0u8; 6];
        bytes[..chunk.len()].copy_from_slice(chunk);
        out.push(
            head | (status << 20)
                | ((chunk.len() as u32) << 16)
                | (u32::from(bytes[0] & 0x7f) << 8)
                | u32::from(bytes[1] & 0x7f),
        );
        out.push(
            (u32::from(bytes[2] & 0x7f) << 24)
                | (u32::from(bytes[3] & 0x7f) << 16)
                | (u32::from(bytes[4] & 0x7f) << 8)
                | u32::from(bytes[5] & 0x7f),
        );
    }
}

/// A transport's reader: [`read_stream`] plus the state a system-exclusive
/// message needs to be gathered across packets, and across calls.
#[derive(Debug, Default)]
pub struct UmpReader {
    sysex: SysEx7Assembler,
}

impl UmpReader {
    /// Reads a buffer of packets: channel-voice messages out as host
    /// packets with their group and width, system messages as bytes, and
    /// each complete system-exclusive message as its bytes, `F0` to `F7`.
    /// Everything that produced no event is reported to `unread`.
    pub fn read(
        &mut self,
        words: &[u32],
        frame: u32,
        packets: &mut impl FnMut(u8, rackforge_midi_api::MidiPacket),
        system: &mut impl FnMut(u8, MidiEventV1),
        sysex: &mut impl FnMut(u8, &[u8]),
        unread: &mut impl FnMut(Unread),
    ) -> Result<(), UmpError> {
        let mut at = 0;
        while at < words.len() {
            let first = words[at];
            if (first >> 28) as u8 == MT_SYSEX7 {
                if words.len() < at + 2 {
                    return Err(UmpError::Truncated {
                        needed: 2,
                        available: words.len() - at,
                    });
                }
                let group = ((first >> 24) & 0xf) as u8;
                match self.sysex.push(first, words[at + 1]) {
                    Ok(Some(message)) => sysex(group, message),
                    Ok(None) => {}
                    Err(why) => unread(why),
                }
                at += 2;
                continue;
            }
            let read = read_packet(
                &words[at..],
                frame,
                &mut |group, event| match event.to_packet() {
                    Some(packet) => packets(group, packet),
                    None => system(group, event.to_midi1()),
                },
            )?;
            if let Some(why) = read.unread {
                unread(why);
            }
            at += read.consumed;
        }
        Ok(())
    }
}

/// Reads a whole buffer of packets, the way a transport receives them:
/// channel-voice messages become host packets with their width and go to
/// `packets` with their group; system messages (clock, start, stop, song
/// position) go to `system` as their bytes; whatever produced no event is
/// reported to `unread`. Every event lands at `frame`. Stops at the first
/// truncated packet, which a transport should treat as a framing fault.
/// Stateless: system-exclusive packets are reported as [`Unread::Data`];
/// a transport that wants those messages keeps an [`UmpReader`].
pub fn read_stream(
    words: &[u32],
    frame: u32,
    packets: &mut impl FnMut(u8, rackforge_midi_api::MidiPacket),
    system: &mut impl FnMut(u8, MidiEventV1),
    unread: &mut impl FnMut(Unread),
) -> Result<(), UmpError> {
    let mut at = 0;
    while at < words.len() {
        let read = read_packet(
            &words[at..],
            frame,
            &mut |group, event| match event.to_packet() {
                Some(packet) => packets(group, packet),
                None => system(group, event.to_midi1()),
            },
        )?;
        if let Some(why) = read.unread {
            unread(why);
        }
        at += read.consumed;
    }
    Ok(())
}

/// Writes one event as a packet into `out`, returning how many words it
/// took: two for a channel-voice message (type 4), one for a system message
/// (type 1). A note-off without a measured velocity is written with
/// velocity 0, as the specification translates a velocity-0 note-on.
pub fn write_packet(group: u8, event: &Midi2Event, out: &mut [u32; 4]) -> usize {
    let group = u32::from(group & 0xf) << 24;
    let channel = u32::from(event.channel & 0xf) << 16;
    let voice = |opcode: u32, index: u8, second: u32| {
        (
            (u32::from(MT_MIDI2_VOICE) << 28)
                | group
                | (opcode << 20)
                | channel
                | (u32::from(index & 0x7f) << 8),
            second,
        )
    };
    let (first, second) = match event.message {
        Midi2Message::NoteOff { note, velocity } => {
            voice(0x8, note, u32::from(velocity.unwrap_or(0)) << 16)
        }
        Midi2Message::NoteOn { note, velocity } => voice(0x9, note, u32::from(velocity) << 16),
        Midi2Message::PolyPressure { note, pressure } => voice(0xA, note, pressure),
        Midi2Message::ControlChange { controller, value } => voice(0xB, controller, value),
        Midi2Message::ProgramChange { program } => voice(0xC, 0, u32::from(program & 0x7f) << 24),
        Midi2Message::ChannelPressure { pressure } => voice(0xD, 0, pressure),
        Midi2Message::PitchBend { value } => voice(0xE, 0, value),
        Midi2Message::Raw { length, data } => {
            let mut word = (u32::from(MT_SYSTEM) << 28) | group | (u32::from(data[0]) << 16);
            if length >= 2 {
                word |= u32::from(data[1] & 0x7f) << 8;
            }
            if length >= 3 {
                word |= u32::from(data[2] & 0x7f);
            }
            out[0] = word;
            return 1;
        }
    };
    out[0] = first;
    out[1] = second;
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_one(words: &[u32]) -> (Read, Vec<(u8, Midi2Event)>) {
        let mut events = Vec::new();
        let read = read_packet(words, 9, &mut |group, event| events.push((group, event))).unwrap();
        (read, events)
    }

    fn event(channel: u8, message: Midi2Message) -> Midi2Event {
        Midi2Event {
            frame: 9,
            channel,
            message,
            origin_7bit: false,
        }
    }

    /// The table the specification gives, for every type including the
    /// reserved ones.
    #[test]
    fn word_counts_follow_the_message_type() {
        let expected = [1, 1, 1, 2, 2, 4, 1, 1, 2, 2, 2, 3, 3, 4, 4, 4];
        for (message_type, expected) in expected.into_iter().enumerate() {
            assert_eq!(
                word_count((message_type as u32) << 28),
                expected,
                "type {message_type:#x}"
            );
        }
    }

    /// A MIDI 1.0 channel-voice packet is its three bytes, lifted: proved
    /// over every status and every data byte pair, against the same
    /// `from_midi1` the byte transports use.
    #[test]
    fn midi1_packets_read_exactly_like_bytes() {
        let mut cases = 0u32;
        for status in 0x80u32..0xf0 {
            let length = byte_length(status as u8);
            for d1 in 0u32..128 {
                for d2 in 0u32..128 {
                    let word = (u32::from(MT_MIDI1_VOICE) << 28)
                        | (5 << 24)
                        | (status << 16)
                        | (d1 << 8)
                        | d2;
                    let mut events = Vec::new();
                    let read = read_packet(&[word], 9, &mut |group, event| {
                        events.push((group, event));
                    })
                    .unwrap();
                    assert_eq!(read.consumed, 1);
                    let data = [
                        status as u8,
                        d1 as u8,
                        if length == 3 { d2 as u8 } else { 0 },
                    ];
                    assert_eq!(events, [(5, lifted(9, data, length))]);
                    cases += 1;
                }
            }
        }
        assert_eq!(cases, 112 * 128 * 128);
    }

    /// Every MIDI 2.0 channel-voice opcode with a byte form goes out and
    /// comes back as the same words and the same event.
    #[test]
    fn midi2_voice_packets_round_trip() {
        let values = [
            0u32,
            1,
            0x7fff,
            0x8000,
            0xffff,
            0x1_0000,
            0x8000_0000,
            u32::MAX,
        ];
        let mut cases = 0;
        for channel in [0u8, 7, 15] {
            for value in values {
                let velocity = value as u16;
                let messages = [
                    Midi2Message::NoteOff {
                        note: 60,
                        velocity: Some(velocity),
                    },
                    Midi2Message::NoteOn {
                        note: 127,
                        velocity,
                    },
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
                    let original = event(channel, message);
                    let mut words = [0u32; 4];
                    assert_eq!(write_packet(3, &original, &mut words), 2);
                    assert_eq!(word_count(words[0]), 2);
                    let (read, events) = read_one(&words[..2]);
                    assert_eq!(
                        read,
                        Read {
                            consumed: 2,
                            unread: None
                        }
                    );
                    assert_eq!(events, [(3, original)]);
                    let mut again = [0u32; 4];
                    write_packet(3, &events[0].1, &mut again);
                    assert_eq!(again[..2], words[..2]);
                    cases += 1;
                }
            }
        }
        assert_eq!(cases, 3 * 8 * 6);

        // A program change is bytes on both sides, and says so.
        let program = Midi2Event {
            origin_7bit: true,
            ..event(2, Midi2Message::ProgramChange { program: 9 })
        };
        let mut words = [0u32; 4];
        assert_eq!(write_packet(0, &program, &mut words), 2);
        assert_eq!(read_one(&words[..2]).1, [(0, program)]);

        // A system message is one word, and comes back as the raw bytes.
        let clock = Midi2Event {
            channel: 0,
            origin_7bit: true,
            ..event(
                0,
                Midi2Message::Raw {
                    length: 1,
                    data: [0xf8, 0, 0],
                },
            )
        };
        assert_eq!(write_packet(1, &clock, &mut words), 1);
        assert_eq!(words[0], 0x11f8_0000);
        assert_eq!(read_one(&words[..1]).1, [(1, clock)]);
        let position = Midi2Event {
            channel: 0,
            origin_7bit: true,
            ..event(
                0,
                Midi2Message::Raw {
                    length: 3,
                    data: [0xf2, 0x12, 0x34],
                },
            )
        };
        assert_eq!(write_packet(1, &position, &mut words), 1);
        assert_eq!(read_one(&words[..1]).1, [(1, position)]);
    }

    /// A transport's buffer -- a timestamp, a byte note, a wide note, a
    /// clock tick, a per-note controller -- becomes two packets (one wide),
    /// one system message and one named leftover, in order.
    #[test]
    fn a_stream_becomes_packets_system_messages_and_leftovers() {
        let words = [
            0x0020_1234,
            0x2390_3c64,
            0x4491_3d00,
            0xabcd_0000,
            0x15f8_0000,
            0x4600_3c01,
            0x1234_5678,
        ];
        let mut packets = Vec::new();
        let mut system = Vec::new();
        let mut leftovers = Vec::new();
        read_stream(
            &words,
            11,
            &mut |group, packet| packets.push((group, packet)),
            &mut |group, event| system.push((group, event.data[0], event.length)),
            &mut |why| leftovers.push(why),
        )
        .unwrap();
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].0, 3);
        assert_eq!(
            (packets[0].1.frame, packets[0].1.data, packets[0].1.wide),
            (11, [0x90, 60, 100], None)
        );
        assert_eq!(packets[1].0, 4);
        assert_eq!(
            (packets[1].1.data, packets[1].1.wide),
            ([0x91, 61, 0x55], Some(0xabcd))
        );
        assert_eq!(system, [(5, 0xf8, 1)]);
        assert_eq!(leftovers, [Unread::Utility, Unread::NoByteForm(0)]);
        assert_eq!(
            read_stream(&words[..2], 0, &mut |_, _| {}, &mut |_, _| {}, &mut |_| {}),
            Ok(())
        );
        assert_eq!(
            read_stream(&words[..3], 0, &mut |_, _| {}, &mut |_, _| {}, &mut |_| {}),
            Err(UmpError::Truncated {
                needed: 2,
                available: 1
            })
        );
    }

    /// Every message length from empty to several packets goes into SysEx7
    /// packets and comes back with its framing; the KeyLab's real messages
    /// among them, which is the transport the display speaks.
    #[test]
    fn system_exclusive_messages_survive_being_packetised() {
        let mut reader = UmpReader::default();
        let mut cases = 0;
        for length in 0..=20usize {
            let payload: Vec<u8> = (0..length).map(|i| (i * 37 % 128) as u8).collect();
            let mut framed = vec![0xf0];
            framed.extend_from_slice(&payload);
            framed.push(0xf7);
            for message in [&payload[..], &framed[..]] {
                let mut words = Vec::new();
                write_sysex7(4, message, &mut words);
                assert_eq!(words.len(), 2 * length.div_ceil(6).max(1));
                let mut received = Vec::new();
                let mut leftovers = Vec::new();
                reader
                    .read(
                        &words,
                        0,
                        &mut |_, _| panic!("no packets"),
                        &mut |_, _| panic!("no system messages"),
                        &mut |group, bytes| received.push((group, bytes.to_vec())),
                        &mut |why| leftovers.push(why),
                    )
                    .unwrap();
                assert_eq!(received, [(4, framed.clone())]);
                assert!(leftovers.is_empty());
                cases += 1;
            }
        }
        assert_eq!(cases, 42);

        // A KeyLab display command: 12 bytes, two packets, byte-exact.
        let keylab = [
            0xf0, 0x00, 0x20, 0x6b, 0x7f, 0x42, 0x02, 0x0f, 0x40, 0x5a, 0x01, 0xf7,
        ];
        let mut words = Vec::new();
        write_sysex7(0, &keylab, &mut words);
        assert_eq!(words.len(), 4);
        assert_eq!(words[0] >> 20, 0x301);
        assert_eq!(words[2] >> 20, 0x303);
        let mut received = Vec::new();
        reader
            .read(
                &words,
                0,
                &mut |_, _| {},
                &mut |_, _| {},
                &mut |_, b| received.push(b.to_vec()),
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(received, [keylab.to_vec()]);
    }

    /// A message gathered across two deliveries is still one message; a
    /// continuation with nothing before it is named and dropped; a new
    /// start abandons the message in progress; a note between the packets
    /// of a message is still a note.
    #[test]
    fn the_assembler_keeps_state_across_calls_and_names_what_it_drops() {
        let mut reader = UmpReader::default();
        let mut words = Vec::new();
        write_sysex7(1, &(0..10u8).collect::<Vec<_>>(), &mut words);
        assert_eq!(words.len(), 4);
        let mut received = Vec::new();
        let mut packets = Vec::new();
        let mut leftovers = Vec::new();
        fn feed(
            reader: &mut UmpReader,
            chunk: &[u32],
            packets: &mut Vec<[u8; 3]>,
            received: &mut Vec<Vec<u8>>,
            leftovers: &mut Vec<Unread>,
        ) {
            reader
                .read(
                    chunk,
                    0,
                    &mut |_, packet| packets.push(packet.data),
                    &mut |_, _| {},
                    &mut |_, bytes| received.push(bytes.to_vec()),
                    &mut |why| leftovers.push(why),
                )
                .unwrap();
        }
        feed(
            &mut reader,
            &words[..2],
            &mut packets,
            &mut received,
            &mut leftovers,
        );
        feed(
            &mut reader,
            &[0x2190_3c64],
            &mut packets,
            &mut received,
            &mut leftovers,
        );
        feed(
            &mut reader,
            &words[2..],
            &mut packets,
            &mut received,
            &mut leftovers,
        );
        assert_eq!(packets, [[0x90, 60, 100]]);
        let mut expected = vec![0xf0];
        expected.extend(0..10u8);
        expected.push(0xf7);
        assert_eq!(received, [expected]);
        assert!(leftovers.is_empty());

        // An end with no start.
        feed(
            &mut reader,
            &words[2..],
            &mut packets,
            &mut received,
            &mut leftovers,
        );
        assert_eq!(leftovers, [Unread::SysExOutOfOrder]);
        assert_eq!(received.len(), 1);

        // Start, then a fresh start: only the second completes.
        let mut second = Vec::new();
        write_sysex7(1, &[0x11, 0x22], &mut second);
        feed(
            &mut reader,
            &words[..2],
            &mut packets,
            &mut received,
            &mut leftovers,
        );
        feed(
            &mut reader,
            &second,
            &mut packets,
            &mut received,
            &mut leftovers,
        );
        assert_eq!(received.last().unwrap(), &vec![0xf0, 0x11, 0x22, 0xf7]);

        // A lone type-3 word is a truncated packet.
        assert_eq!(
            reader.read(
                &[0x3001_0000],
                0,
                &mut |_, _| {},
                &mut |_, _| {},
                &mut |_, _| {},
                &mut |_| {}
            ),
            Err(UmpError::Truncated {
                needed: 2,
                available: 1
            })
        );
    }

    /// A note-on the byte scale cannot whisper is still a note-on, and its
    /// projection is the byte 1 the specification prescribes.
    #[test]
    fn a_whisper_note_on_projects_to_one() {
        let words = [0x4091_3c00, 0x0001_0000];
        let (_, events) = read_one(&words);
        assert_eq!(
            events[0].1.message,
            Midi2Message::NoteOn {
                note: 60,
                velocity: 1
            }
        );
        assert_eq!(events[0].1.to_midi1().data, [0x91, 60, 1]);
    }

    /// The bank rides in front of the program, as the two controllers a
    /// byte instrument expects, only when the packet says the bank is valid.
    #[test]
    fn a_program_with_a_bank_becomes_three_messages() {
        let with_bank = [0x40c2_0001, 0x0900_0102];
        let (read, events) = read_one(&with_bank);
        assert_eq!(read.consumed, 2);
        let bytes: Vec<[u8; 3]> = events.iter().map(|(_, e)| e.to_midi1().data).collect();
        assert_eq!(bytes, [[0xb2, 0, 1], [0xb2, 32, 2], [0xc2, 9, 0]]);
        assert!(events.iter().all(|(_, e)| e.origin_7bit));
        let without_bank = [0x40c2_0000, 0x0900_0102];
        assert_eq!(read_one(&without_bank).1.len(), 1);
    }

    /// Registered and assignable parameters are the controller sequence,
    /// carrying the data's top fourteen bits, exactly as a byte instrument
    /// would have received them.
    #[test]
    fn parameters_become_the_controller_sequence() {
        let rpn = [0x4020_0102, 0x8000_0000];
        let bytes: Vec<[u8; 3]> = read_one(&rpn)
            .1
            .iter()
            .map(|(_, e)| e.to_midi1().data)
            .collect();
        assert_eq!(
            bytes,
            [[0xb0, 101, 1], [0xb0, 100, 2], [0xb0, 6, 64], [0xb0, 38, 0]]
        );
        let nrpn = [0x4030_7f7f, 0xffff_ffff];
        let bytes: Vec<[u8; 3]> = read_one(&nrpn)
            .1
            .iter()
            .map(|(_, e)| e.to_midi1().data)
            .collect();
        assert_eq!(
            bytes,
            [
                [0xb0, 99, 127],
                [0xb0, 98, 127],
                [0xb0, 6, 127],
                [0xb0, 38, 127]
            ]
        );
    }

    /// What has no byte form is named with its opcode; what is not ours is
    /// stepped over at its own length; a short buffer is an error, not a
    /// misread.
    #[test]
    fn what_cannot_be_read_is_named_and_stepped_over() {
        let per_note = [0x4000_3c01, 0x1234_5678];
        assert_eq!(
            read_one(&per_note).0,
            Read {
                consumed: 2,
                unread: Some(Unread::NoByteForm(0))
            }
        );
        let timestamp = [0x0020_1234];
        assert_eq!(
            read_one(&timestamp).0,
            Read {
                consumed: 1,
                unread: Some(Unread::Utility)
            }
        );
        let sysex8 = [0x5000_0000, 0, 0, 0];
        assert_eq!(
            read_one(&sysex8).0,
            Read {
                consumed: 4,
                unread: Some(Unread::Data)
            }
        );
        let stream = [0xf000_0000, 0, 0, 0];
        assert_eq!(
            read_one(&stream).0,
            Read {
                consumed: 4,
                unread: Some(Unread::FlexOrStream)
            }
        );
        let reserved = [0x8000_0000, 0];
        assert_eq!(
            read_one(&reserved).0,
            Read {
                consumed: 2,
                unread: Some(Unread::Reserved(8))
            }
        );
        let mut sink = |_: u8, _: Midi2Event| {};
        assert_eq!(
            read_packet(&[0x4091_3c00], 0, &mut sink),
            Err(UmpError::Truncated {
                needed: 2,
                available: 1
            })
        );
        assert_eq!(
            read_packet(&[], 0, &mut sink),
            Err(UmpError::Truncated {
                needed: 1,
                available: 0
            })
        );
        // A jitter-reduction timestamp in front of a note is two packets.
        let stream = [0x0020_1234, 0x2090_3c64];
        let first = read_packet(&stream, 0, &mut sink).unwrap();
        assert_eq!(first.consumed, 1);
        let mut events = Vec::new();
        read_packet(&stream[first.consumed..], 0, &mut |group, event| {
            events.push((group, event))
        })
        .unwrap();
        assert_eq!(events[0].1.to_midi1().data, [0x90, 60, 100]);
    }
}
