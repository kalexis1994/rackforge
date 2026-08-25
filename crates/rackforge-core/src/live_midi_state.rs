#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use rackforge_midi_api::{CompiledMidiRoute, IngressMidiEvent, MidiPacket, MidiSourceKey};
use rackforge_plugin_api::abi::MidiEventV1;
use rackforge_session_api::{
    ButtonPhase, HostActionBinding, HostActionTarget, HostControlBinding, MidiButtonBinding,
};

const MIDI_CHANNELS: usize = 16;
const CONTINUOUS_CONTROLLERS: usize = 120;

pub(super) struct ReservedMidiControls {
    control_changes: [[bool; 120]; MIDI_CHANNELS],
    keyboard_parts: Option<MidiButtonBinding>,
    sources: Vec<ReservedMidiSourceState>,
}

struct ReservedMidiSourceState {
    keyboard_parts_held: bool,
    suppressed_notes: [[bool; 128]; MIDI_CHANNELS],
}

impl Default for ReservedMidiControls {
    fn default() -> Self {
        Self::with_sources(1)
    }
}

impl ReservedMidiControls {
    pub(super) fn with_sources(source_count: usize) -> Self {
        Self {
            control_changes: [[false; 120]; MIDI_CHANNELS],
            keyboard_parts: None,
            sources: (0..source_count)
                .map(|_| ReservedMidiSourceState {
                    keyboard_parts_held: false,
                    suppressed_notes: [[false; 128]; MIDI_CHANNELS],
                })
                .collect(),
        }
    }

    pub(super) fn replace(
        &mut self,
        controls: &[HostControlBinding],
        actions: &[HostActionBinding],
    ) {
        self.control_changes = [[false; CONTINUOUS_CONTROLLERS]; MIDI_CHANNELS];
        self.keyboard_parts = None;
        for source in &mut self.sources {
            source.keyboard_parts_held = false;
            source.suppressed_notes = [[false; 128]; MIDI_CHANNELS];
        }
        for binding in controls {
            self.control_changes[binding.midi_cc.channel as usize]
                [binding.midi_cc.controller as usize] = true;
        }
        for binding in actions {
            self.control_changes[binding.midi_cc.channel as usize]
                [binding.midi_cc.controller as usize] = true;
            if binding.target == HostActionTarget::KeyboardParts {
                self.keyboard_parts = Some(binding.midi_cc);
            }
        }
    }

    pub(super) fn consume(&mut self, source: MidiSourceKey, event: MidiEventV1) -> bool {
        let message = &event.data[..usize::from(event.length.min(3))];
        if let Some(binding) = self.keyboard_parts
            && let Some(phase) = binding.phase(message)
        {
            if let Some(state) = self.sources.get_mut(source.get() as usize) {
                state.keyboard_parts_held = phase == ButtonPhase::Press;
            }
            return true;
        }
        if event.length == 3
            && event.data[0] & 0xf0 == 0xb0
            && event.data[1] <= 119
            && self.control_changes[(event.data[0] & 0x0f) as usize][event.data[1] as usize]
        {
            return true;
        }
        let Some(state) = self.sources.get_mut(source.get() as usize) else {
            return false;
        };
        if event.length < 2 {
            return false;
        }
        let channel = (event.data[0] & 0x0f) as usize;
        let note = event.data[1] as usize;
        let status = event.data[0] & 0xf0;
        if status == 0x90 && event.length == 3 && event.data[2] > 0 && state.keyboard_parts_held {
            state.suppressed_notes[channel][note] = true;
            return true;
        }
        let release = status == 0x80 || (status == 0x90 && event.length == 3 && event.data[2] == 0);
        if release && state.suppressed_notes[channel][note] {
            state.suppressed_notes[channel][note] = false;
            return true;
        }
        if status == 0xa0 && state.suppressed_notes[channel][note] {
            return true;
        }
        false
    }
}

pub(super) struct MidiControllerState {
    continuous_controllers: [[Option<u8>; CONTINUOUS_CONTROLLERS]; MIDI_CHANNELS],
    pitch_bend: [Option<(u8, u8)>; MIDI_CHANNELS],
    channel_pressure: [Option<u8>; MIDI_CHANNELS],
}

pub(super) struct MidiControllerStates {
    sources: Vec<MidiControllerState>,
}

impl MidiControllerStates {
    pub(super) fn new(source_count: usize) -> Self {
        Self {
            sources: (0..source_count)
                .map(|_| MidiControllerState::default())
                .collect(),
        }
    }

    pub(super) fn observe(&mut self, source: MidiSourceKey, event: MidiEventV1) {
        if let Some(state) = self.sources.get_mut(source.get() as usize) {
            state.observe(event);
        }
    }

    pub(super) fn replay_routed_into(
        &self,
        route: &CompiledMidiRoute,
        input_channel: Option<u8>,
        events: &mut Vec<MidiEventV1>,
        maximum_events: usize,
    ) -> usize {
        let mut omitted = 0;
        for (source_index, state) in self.sources.iter().enumerate() {
            state.visit_replay(|event| {
                let ingress = IngressMidiEvent {
                    source: MidiSourceKey::new(source_index as u32),
                    packet: MidiPacket {
                        frame: event.frame,
                        length: event.length,
                        data: event.data,
                    },
                };
                if !matches_midi_input_channel(ingress.packet, input_channel) {
                    return;
                }
                if let Some(routed) = route.route(ingress) {
                    push_replay_event(
                        events,
                        maximum_events,
                        plugin_midi_event(routed.packet),
                        &mut omitted,
                    );
                }
            });
        }
        omitted
    }

    #[cfg(test)]
    pub(super) fn replay_source_into(
        &self,
        source: MidiSourceKey,
        events: &mut Vec<MidiEventV1>,
        maximum_events: usize,
    ) -> Option<usize> {
        self.sources
            .get(source.get() as usize)
            .map(|state| state.replay_into(events, maximum_events))
    }
}

impl Default for MidiControllerState {
    fn default() -> Self {
        Self {
            continuous_controllers: [[None; CONTINUOUS_CONTROLLERS]; MIDI_CHANNELS],
            pitch_bend: [None; MIDI_CHANNELS],
            channel_pressure: [None; MIDI_CHANNELS],
        }
    }
}

impl MidiControllerState {
    pub(super) fn observe(&mut self, event: MidiEventV1) {
        if event.length == 0 {
            return;
        }
        let status = event.data[0] & 0xf0;
        let channel = usize::from(event.data[0] & 0x0f);
        match status {
            0xb0 if event.length >= 3 => {
                let controller = usize::from(event.data[1] & 0x7f);
                if controller < CONTINUOUS_CONTROLLERS {
                    self.continuous_controllers[channel][controller] = Some(event.data[2] & 0x7f);
                } else if controller == 121 {
                    self.continuous_controllers[channel].fill(None);
                    self.pitch_bend[channel] = None;
                    self.channel_pressure[channel] = None;
                }
            }
            0xd0 if event.length >= 2 => {
                self.channel_pressure[channel] = Some(event.data[1] & 0x7f);
            }
            0xe0 if event.length >= 3 => {
                self.pitch_bend[channel] = Some((event.data[1] & 0x7f, event.data[2] & 0x7f));
            }
            _ => {}
        }
    }

    fn visit_replay(&self, mut visit: impl FnMut(MidiEventV1)) {
        for channel in 0..MIDI_CHANNELS {
            for (controller, value) in self.continuous_controllers[channel].iter().enumerate() {
                if let Some(value) = value {
                    visit(MidiEventV1 {
                        frame: 0,
                        length: 3,
                        data: [0xb0 | channel as u8, controller as u8, *value],
                    });
                }
            }
            if let Some(pressure) = self.channel_pressure[channel] {
                visit(MidiEventV1 {
                    frame: 0,
                    length: 2,
                    data: [0xd0 | channel as u8, pressure, 0],
                });
            }
            if let Some((least_significant, most_significant)) = self.pitch_bend[channel] {
                visit(MidiEventV1 {
                    frame: 0,
                    length: 3,
                    data: [0xe0 | channel as u8, least_significant, most_significant],
                });
            }
        }
    }

    #[cfg(test)]
    pub(super) fn replay_into(
        &self,
        events: &mut Vec<MidiEventV1>,
        maximum_events: usize,
    ) -> usize {
        let mut omitted = 0;
        self.visit_replay(|event| {
            push_replay_event(events, maximum_events, event, &mut omitted);
        });
        omitted
    }
}

fn push_replay_event(
    events: &mut Vec<MidiEventV1>,
    maximum_events: usize,
    event: MidiEventV1,
    omitted: &mut usize,
) {
    if events.len() < maximum_events {
        events.push(event);
    } else {
        *omitted += 1;
    }
}

pub(super) fn plugin_midi_event(packet: MidiPacket) -> MidiEventV1 {
    MidiEventV1 {
        frame: packet.frame,
        length: packet.length,
        data: packet.data,
    }
}

pub(super) fn matches_midi_input_channel(packet: MidiPacket, channel: Option<u8>) -> bool {
    channel.is_none_or(|channel| packet.channel().user_number() == channel)
}
