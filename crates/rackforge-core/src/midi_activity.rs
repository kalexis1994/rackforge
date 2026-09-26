//! The channel messages a host has received lately, numbered, for the
//! Controllers editor: what lights a control as the player moves it, and
//! what a new controller's controls are learnt from.
//!
//! A bounded log rather than a stream: the editor asks every tenth of a
//! second for what is newer than what it has, and a host that nobody asks
//! keeps only the last few hundred messages.

use rackforge_control_api::MidiActivityEvent;
use rackforge_midi_api::MidiSourceDescriptor;
use std::collections::VecDeque;

/// Enough for a hand on every fader of a large controller between two polls.
pub const MIDI_ACTIVITY_CAPACITY: usize = 256;

#[derive(Debug)]
pub struct MidiActivityLog {
    events: VecDeque<MidiActivityEvent>,
    next_sequence: u64,
}

impl Default for MidiActivityLog {
    fn default() -> Self {
        Self {
            events: VecDeque::with_capacity(MIDI_ACTIVITY_CAPACITY),
            next_sequence: 1,
        }
    }
}

impl MidiActivityLog {
    /// Records a channel voice message, or the Start, Continue and Stop some
    /// keyboards' transport buttons send. Clock, sensing, SysEx and every
    /// other system message are nobody's control and are left out, as are
    /// malformed bytes.
    pub fn record(&mut self, source: &MidiSourceDescriptor, bytes: &[u8]) -> bool {
        let Some(&status) = bytes.first() else {
            return false;
        };
        if matches!(status, 0xfa..=0xfc) {
            self.push(source, status, 0, 0);
            return true;
        }
        if !(0x80..0xf0).contains(&status) {
            return false;
        }
        // Program change and channel pressure carry one data byte, the rest
        // two.
        let data_bytes = if matches!(status & 0xf0, 0xc0 | 0xd0) {
            1
        } else {
            2
        };
        if bytes.len() < 1 + data_bytes || bytes[1..=data_bytes].iter().any(|byte| *byte > 0x7f) {
            return false;
        }
        let data2 = if data_bytes == 2 { bytes[2] } else { 0 };
        self.push(source, status, bytes[1], data2);
        true
    }

    fn push(&mut self, source: &MidiSourceDescriptor, status: u8, data1: u8, data2: u8) {
        if self.events.len() == MIDI_ACTIVITY_CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(MidiActivityEvent {
            sequence: self.next_sequence,
            source: source.clone(),
            status,
            data1,
            data2,
        });
        self.next_sequence += 1;
    }

    /// Everything numbered after `after`, and the cursor to ask from next
    /// time. A client further behind than the log reaches gets what is left.
    pub fn since(&self, after: u64) -> (u64, Vec<MidiActivityEvent>) {
        let events = self
            .events
            .iter()
            .filter(|event| event.sequence > after)
            .cloned()
            .collect();
        (self.next_sequence - 1, events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_midi_api::MidiSourceId;

    fn source() -> MidiSourceDescriptor {
        MidiSourceDescriptor {
            id: MidiSourceId::new("alsa.oxygen-49").unwrap(),
            name: "Oxygen 49".into(),
            primary: false,
        }
    }

    #[test]
    fn a_client_gets_only_what_is_newer() {
        let mut log = MidiActivityLog::default();
        assert_eq!(log.since(0), (0, Vec::new()));
        assert!(log.record(&source(), &[0xb0, 74, 100]));
        assert!(log.record(&source(), &[0x99, 36, 90]));
        let (cursor, events) = log.since(0);
        assert_eq!(cursor, 2);
        assert_eq!(events.len(), 2);
        assert_eq!(
            (events[1].status, events[1].data1, events[1].data2),
            (0x99, 36, 90)
        );
        assert!(log.since(cursor).1.is_empty());
        log.record(&source(), &[0xd0, 64]);
        let (_, newer) = log.since(cursor);
        assert_eq!(
            (newer[0].status, newer[0].data1, newer[0].data2),
            (0xd0, 64, 0)
        );
    }

    #[test]
    fn system_and_malformed_messages_are_nobodys_control() {
        let mut log = MidiActivityLog::default();
        assert!(!log.record(&source(), &[0xf8]));
        assert!(!log.record(&source(), &[0xf0, 0x7e, 0xf7]));
        assert!(!log.record(&source(), &[0xb0, 74]));
        assert!(!log.record(&source(), &[0xb0, 0x80, 1]));
        assert!(!log.record(&source(), &[]));
        assert_eq!(log.since(0).0, 0);
    }

    /// A Launchkey MK4's Play and Stop send MIDI Start and Stop: the editor
    /// has to see them to show the buttons working.
    #[test]
    fn transport_real_time_messages_are_a_buttons() {
        let mut log = MidiActivityLog::default();
        assert!(log.record(&source(), &[0xfa]));
        assert!(log.record(&source(), &[0xfc]));
        assert!(!log.record(&source(), &[0xf8]));
        assert!(!log.record(&source(), &[0xfe]));
        let (_, events) = log.since(0);
        assert_eq!(
            events
                .iter()
                .map(|event| (event.status, event.data1, event.data2))
                .collect::<Vec<_>>(),
            vec![(0xfa, 0, 0), (0xfc, 0, 0)]
        );
    }

    #[test]
    fn a_log_nobody_reads_keeps_only_the_last_messages() {
        let mut log = MidiActivityLog::default();
        for value in 0..(MIDI_ACTIVITY_CAPACITY as u64 + 10) {
            log.record(&source(), &[0xb0, 1, (value % 128) as u8]);
        }
        let (cursor, events) = log.since(0);
        assert_eq!(events.len(), MIDI_ACTIVITY_CAPACITY);
        assert_eq!(cursor, MIDI_ACTIVITY_CAPACITY as u64 + 10);
        assert_eq!(events[0].sequence, 11);
    }
}
