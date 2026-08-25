//! Deterministic, portable MIDI traces for reliability tests and diagnostics.
//!
//! A trace addresses inputs by their persistent [`MidiSourceId`] rather than
//! the process-local [`MidiSourceKey`]. Compiling resolves those identities
//! against the host's approved source registry and validates every message
//! before any event is replayed. Replay is deliberately clock-free: callers
//! own pacing, while this type guarantees stable ordering for events sharing a
//! frame and prevents a malformed trace from reaching the real-time ingress.

use rackforge_midi_api::{
    IngressMidiEvent, MidiPacket, MidiRoutingError, MidiSourceId, MidiSourceRegistry,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MIDI_TRACE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiTrace {
    pub schema_version: u32,
    pub events: Vec<MidiTraceEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiTraceEvent {
    /// Absolute frame from the beginning of the trace.
    pub frame: u32,
    /// Persistent input identity, never a transient runtime key or display name.
    pub source_id: MidiSourceId,
    /// One MIDI 1.0 channel-voice message. Two- and three-byte messages are
    /// both supported; SysEx and realtime transport messages are intentionally
    /// outside this reliability trace contract.
    pub message: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledMidiTrace {
    events: Vec<IngressMidiEvent>,
}

impl MidiTrace {
    pub fn compile(
        &self,
        sources: &MidiSourceRegistry,
    ) -> Result<CompiledMidiTrace, MidiTraceError> {
        if self.schema_version != MIDI_TRACE_SCHEMA_VERSION {
            return Err(MidiTraceError::UnsupportedSchema(self.schema_version));
        }
        if self.events.is_empty() {
            return Err(MidiTraceError::Empty);
        }

        let mut previous_frame = None;
        let mut events = Vec::with_capacity(self.events.len());
        for (index, event) in self.events.iter().enumerate() {
            if previous_frame.is_some_and(|previous| event.frame < previous) {
                return Err(MidiTraceError::FramesOutOfOrder {
                    index,
                    previous: previous_frame.expect("checked above"),
                    current: event.frame,
                });
            }
            previous_frame = Some(event.frame);
            let source = sources.resolve(&event.source_id).map_err(|error| {
                MidiTraceError::InvalidEvent {
                    index,
                    source: error,
                }
            })?;
            let packet = MidiPacket::new(event.frame, &event.message).map_err(|error| {
                MidiTraceError::InvalidEvent {
                    index,
                    source: error,
                }
            })?;
            events.push(IngressMidiEvent { source, packet });
        }
        Ok(CompiledMidiTrace { events })
    }
}

impl CompiledMidiTrace {
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn duration_frames(&self) -> u32 {
        self.events.last().map_or(0, |event| event.packet.frame)
    }

    /// Replays every event in deterministic trace order.
    ///
    /// The visitor can enqueue, route, or inspect the event. It is invoked
    /// synchronously and receives no hidden sleeps, allocations, or threads.
    pub fn replay(&self, mut visit: impl FnMut(IngressMidiEvent)) {
        for event in self.events.iter().copied() {
            visit(event);
        }
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum MidiTraceError {
    #[error("unsupported MIDI trace schema version {0}")]
    UnsupportedSchema(u32),
    #[error("MIDI trace must contain at least one event")]
    Empty,
    #[error("MIDI trace frame order decreases at event {index}: {previous} followed by {current}")]
    FramesOutOfOrder {
        index: usize,
        previous: u32,
        current: u32,
    },
    #[error("invalid MIDI trace event {index}: {source}")]
    InvalidEvent {
        index: usize,
        #[source]
        source: MidiRoutingError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_midi_api::{
        MIDI_ROUTING_SCHEMA_VERSION, MidiRoute, MidiRouteId, MidiRouteMatch, MidiRouteTarget,
        MidiRouteTransform, MidiSourceDescriptor, MidiSourceKey, MidiTargetId, PluginChannelModel,
    };

    const SOURCE_ID: &str = "usb.arturia.keylab-essential-mk3";

    fn source_id() -> MidiSourceId {
        MidiSourceId::new(SOURCE_ID).unwrap()
    }

    fn sources() -> MidiSourceRegistry {
        let mut sources = MidiSourceRegistry::default();
        sources
            .register(
                MidiSourceKey::new(7),
                MidiSourceDescriptor {
                    id: source_id(),
                    name: "Arturia KeyLab Essential mk3".into(),
                    primary: true,
                },
            )
            .unwrap();
        sources
    }

    fn event(frame: u32, message: &[u8]) -> MidiTraceEvent {
        MidiTraceEvent {
            frame,
            source_id: source_id(),
            message: message.to_vec(),
        }
    }

    #[test]
    fn mixed_dense_trace_replays_every_message_in_stable_order() {
        let trace = MidiTrace {
            schema_version: MIDI_TRACE_SCHEMA_VERSION,
            events: vec![
                event(0, &[0x90, 60, 110]),
                event(0, &[0x90, 64, 108]),
                event(0, &[0x90, 67, 105]),
                event(1, &[0xb0, 1, 91]),
                event(1, &[0xe0, 0x7f, 0x7f]),
                event(1, &[0xd0, 72]),
                event(1, &[0xa0, 64, 84]),
                event(127, &[0x80, 67, 0]),
                event(127, &[0x80, 64, 0]),
                event(127, &[0x80, 60, 0]),
            ],
        };
        let compiled = trace.compile(&sources()).unwrap();
        let mut replayed = Vec::new();
        compiled.replay(|event| replayed.push(event));

        assert_eq!(compiled.len(), trace.events.len());
        assert_eq!(compiled.duration_frames(), 127);
        assert!(
            replayed
                .iter()
                .all(|event| event.source == MidiSourceKey::new(7))
        );
        assert_eq!(
            replayed
                .iter()
                .map(|event| (event.packet.frame, event.packet.length, event.packet.data))
                .collect::<Vec<_>>(),
            vec![
                (0, 3, [0x90, 60, 110]),
                (0, 3, [0x90, 64, 108]),
                (0, 3, [0x90, 67, 105]),
                (1, 3, [0xb0, 1, 91]),
                (1, 3, [0xe0, 0x7f, 0x7f]),
                (1, 2, [0xd0, 72, 0]),
                (1, 3, [0xa0, 64, 84]),
                (127, 3, [0x80, 67, 0]),
                (127, 3, [0x80, 64, 0]),
                (127, 3, [0x80, 60, 0]),
            ]
        );
    }

    #[test]
    fn dense_replay_delivers_every_note_off_through_the_normal_route() {
        let mut events = Vec::new();
        for chord in 0..2_000_u32 {
            let frame = chord * 2;
            for note in [48, 52, 55, 60, 64, 67] {
                events.push(event(frame, &[0x90, note, 127]));
            }
            for note in [67, 64, 60, 55, 52, 48] {
                events.push(event(frame + 1, &[0x80, note, 0]));
            }
        }
        let trace = MidiTrace {
            schema_version: MIDI_TRACE_SCHEMA_VERSION,
            events,
        };
        let compiled = trace.compile(&sources()).unwrap();
        let route = MidiRoute {
            schema_version: MIDI_ROUTING_SCHEMA_VERSION,
            id: MidiRouteId::new("trace-to-instrument").unwrap(),
            enabled: true,
            matches: MidiRouteMatch::default(),
            transform: MidiRouteTransform::default(),
            target: MidiRouteTarget {
                instance_id: MidiTargetId::new("instrument").unwrap(),
                input_bus_id: rackforge_midi_api::MidiInputBusId::new("main").unwrap(),
            },
        }
        .compile(&sources(), PluginChannelModel::SinglePart)
        .unwrap();
        let mut held = [0_i32; 128];
        let mut delivered = 0;
        compiled.replay(|event| {
            let routed = route.route(event).expect("primary trace event must route");
            let status = routed.packet.data[0] & 0xf0;
            let note = usize::from(routed.packet.data[1]);
            if status == 0x90 && routed.packet.data[2] > 0 {
                held[note] += 1;
            } else if status == 0x80 || (status == 0x90 && routed.packet.data[2] == 0) {
                held[note] -= 1;
            }
            delivered += 1;
        });

        assert_eq!(delivered, 24_000);
        assert!(held.into_iter().all(|count| count == 0));
    }

    #[test]
    fn compile_is_atomic_when_a_later_event_is_invalid() {
        let trace = MidiTrace {
            schema_version: MIDI_TRACE_SCHEMA_VERSION,
            events: vec![event(0, &[0x90, 60, 100]), event(1, &[0xf8])],
        };
        assert!(matches!(
            trace.compile(&sources()),
            Err(MidiTraceError::InvalidEvent { index: 1, .. })
        ));
    }

    #[test]
    fn compile_rejects_time_travel_and_unknown_sources() {
        let backwards = MidiTrace {
            schema_version: MIDI_TRACE_SCHEMA_VERSION,
            events: vec![event(4, &[0x90, 60, 100]), event(3, &[0x80, 60, 0])],
        };
        assert_eq!(
            backwards.compile(&sources()),
            Err(MidiTraceError::FramesOutOfOrder {
                index: 1,
                previous: 4,
                current: 3,
            })
        );

        let unknown = MidiTrace {
            schema_version: MIDI_TRACE_SCHEMA_VERSION,
            events: vec![MidiTraceEvent {
                frame: 0,
                source_id: MidiSourceId::new("usb.unknown").unwrap(),
                message: vec![0x90, 60, 100],
            }],
        };
        assert!(matches!(
            unknown.compile(&sources()),
            Err(MidiTraceError::InvalidEvent { index: 0, .. })
        ));
    }

    #[test]
    fn trace_json_round_trips_without_losing_same_frame_order() {
        let trace = MidiTrace {
            schema_version: MIDI_TRACE_SCHEMA_VERSION,
            events: vec![
                event(12, &[0x90, 60, 100]),
                event(12, &[0x90, 64, 100]),
                event(12, &[0x90, 67, 100]),
            ],
        };
        let encoded = serde_json::to_string(&trace).unwrap();
        assert_eq!(serde_json::from_str::<MidiTrace>(&encoded).unwrap(), trace);
    }
}
