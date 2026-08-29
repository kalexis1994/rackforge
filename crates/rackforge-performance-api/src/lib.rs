use rackforge_plugin_api::PluginStateReference;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use thiserror::Error;

pub const PERFORMANCE_SCHEMA_VERSION: u32 = 1;
pub const PERFORMANCE_SNAPSHOT_SCHEMA_VERSION: u32 = 4;
/// Version 1 was the implicit, flat Rack Slot layout. Version 2 is the first
/// explicit and portable node graph.
pub const RACK_GRAPH_SCHEMA_VERSION: u32 = 2;
pub const MAX_RACKS: usize = 256;
pub const MAX_RACK_SLOTS: usize = 32;
pub const MAX_RACK_GRAPH_NODES: usize = 128;
pub const MAX_RACK_GRAPH_EDGES: usize = 512;
pub const MAX_RACK_GRAPH_LABELS: usize = 128;
pub const MAX_SONGS: usize = 256;
pub const MAX_SONG_PARTS: usize = 64;
pub const MAX_SETLISTS: usize = 128;
pub const MAX_SETLIST_ENTRIES: usize = 256;
pub const MAX_PATTERNS: usize = 512;
pub const MAX_PATTERN_NOTES: usize = 2_048;
/// Tick resolution of sequencer patterns: 960 divides every common figure —
/// straight, dotted and triplet down to 64ths — so editors never round.
pub const PATTERN_TICKS_PER_BEAT: u32 = 960;
/// Longest pattern, in ticks: 256 beats is 64 bars of four — beyond a live
/// pattern and into a timeline, which a pattern is not.
pub const MAX_PATTERN_TICKS: u32 = 256 * PATTERN_TICKS_PER_BEAT;
/// Sequencer lanes a Part may bind — one per engine lane.
pub const MAX_PART_PATTERN_BINDINGS: usize = 8;

macro_rules! performance_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, PerformanceError> {
                let value = value.into();
                validate_id(&value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

performance_id!(RackId);
performance_id!(RackSlotId);
performance_id!(RackGraphNodeId);
performance_id!(RackGraphEdgeId);
performance_id!(RackGraphLabelId);
performance_id!(SongId);
performance_id!(SongPartId);
performance_id!(SetlistId);
performance_id!(SetlistEntryId);
performance_id!(PatternId);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LibraryRevision(String);

impl LibraryRevision {
    pub fn new(value: impl Into<String>) -> Result<Self, PerformanceError> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(PerformanceError::InvalidLibraryRevision);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn validate(&self) -> Result<(), PerformanceError> {
        Self::new(self.0.clone()).map(|_| ())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackSlot {
    pub id: RackSlotId,
    #[serde(default = "default_slot_name")]
    pub name: String,
    pub plugin_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<PluginStateReference>,
    /// Temporary migration input for Racks created before opaque state snapshots.
    #[serde(
        default,
        alias = "program_id",
        alias = "plugin_state_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub legacy_program_id: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub midi_input_channel: Option<u8>,
    #[serde(default)]
    pub midi_note_low: u8,
    #[serde(default = "default_midi_note_high")]
    pub midi_note_high: u8,
    #[serde(default)]
    pub midi_transpose: i8,
    #[serde(default)]
    pub midi_output: MidiOutputRoute,
    #[serde(default = "default_audio_output_bus")]
    pub audio_output_bus: String,
    #[serde(default = "default_slot_level")]
    pub level_per_mille: u16,
    #[serde(default)]
    pub pan_per_mille: i16,
}

impl RackSlot {
    fn validate(&self) -> Result<(), PerformanceError> {
        validate_name(&self.name)?;
        validate_plugin_id(&self.plugin_id)?;
        if let Some(state) = &self.state {
            state
                .validate()
                .map_err(|_| PerformanceError::InvalidPluginState)?;
            if state.plugin_id != self.plugin_id {
                return Err(PerformanceError::PluginStateMismatch);
            }
        }
        if let Some(program_id) = &self.legacy_program_id {
            validate_reference(program_id, "legacy program id")?;
        }
        if self.state.is_some() && self.legacy_program_id.is_some() {
            return Err(PerformanceError::AmbiguousPluginState);
        }
        if self
            .midi_input_channel
            .is_some_and(|channel| !(1..=16).contains(&channel))
        {
            return Err(PerformanceError::InvalidMidiChannel);
        }
        if self.midi_note_low > self.midi_note_high || self.midi_note_high > 127 {
            return Err(PerformanceError::InvalidMidiNoteRange);
        }
        if !(-48..=48).contains(&self.midi_transpose) {
            return Err(PerformanceError::InvalidMidiTranspose);
        }
        self.midi_output.validate()?;
        validate_reference(&self.audio_output_bus, "audio output bus")?;
        if self.level_per_mille > 1_000 || !(-1_000..=1_000).contains(&self.pan_per_mille) {
            return Err(PerformanceError::InvalidSlotMix);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MidiOutputRoute {
    #[default]
    None,
    Bus {
        bus_id: String,
    },
}

impl MidiOutputRoute {
    fn validate(&self) -> Result<(), PerformanceError> {
        match self {
            Self::None => Ok(()),
            Self::Bus { bus_id } => validate_reference(bus_id, "MIDI output bus"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackKeyboardPart {
    pub midi_channel: u8,
    #[serde(default)]
    pub transpose: i8,
}

impl RackKeyboardPart {
    fn validate(&self) -> Result<(), PerformanceError> {
        if !(1..=16).contains(&self.midi_channel) {
            return Err(PerformanceError::InvalidMidiChannel);
        }
        if !(-48..=48).contains(&self.transpose) {
            return Err(PerformanceError::InvalidMidiTranspose);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackKeyboardParts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_key: Option<u8>,
    pub part_1: RackKeyboardPart,
    pub part_2: RackKeyboardPart,
}

impl Default for RackKeyboardParts {
    fn default() -> Self {
        Self {
            split_key: None,
            part_1: RackKeyboardPart {
                midi_channel: 1,
                transpose: 0,
            },
            part_2: RackKeyboardPart {
                midi_channel: 2,
                transpose: 0,
            },
        }
    }
}

impl RackKeyboardParts {
    pub fn part(self, index: usize) -> RackKeyboardPart {
        if index == 1 { self.part_2 } else { self.part_1 }
    }

    fn validate(&self) -> Result<(), PerformanceError> {
        if self.split_key.is_some_and(|key| !(1..=127).contains(&key)) {
            return Err(PerformanceError::InvalidKeyboardSplit);
        }
        self.part_1.validate()?;
        self.part_2.validate()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackGraphPosition {
    #[serde(deserialize_with = "deserialize_graph_coordinate")]
    pub x: i32,
    #[serde(deserialize_with = "deserialize_graph_coordinate")]
    pub y: i32,
}

fn deserialize_graph_coordinate<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = f64::deserialize(deserializer)?;
    if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
        return Err(D::Error::custom(
            "Rack graph coordinate is outside the i32 range",
        ));
    }
    Ok(value.round() as i32)
}

impl RackGraphPosition {
    fn validate(self) -> Result<(), PerformanceError> {
        const LIMIT: i32 = 1_000_000;
        if !(-LIMIT..=LIMIT).contains(&self.x) || !(-LIMIT..=LIMIT).contains(&self.y) {
            return Err(PerformanceError::InvalidRackGraphPosition);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RackGraphNodeKind {
    MidiInput { bus_id: String },
    AudioInput { bus_id: String },
    Plugin { slot_id: RackSlotId },
    Rack { rack_id: RackId },
    MidiOutput { bus_id: String },
    AudioOutput { bus_id: String },
}

impl RackGraphNodeKind {
    fn validate(&self) -> Result<(), PerformanceError> {
        match self {
            Self::MidiInput { bus_id }
            | Self::AudioInput { bus_id }
            | Self::MidiOutput { bus_id }
            | Self::AudioOutput { bus_id } => validate_reference(bus_id, "Rack graph bus"),
            Self::Plugin { .. } | Self::Rack { .. } => Ok(()),
        }
    }

    fn supports_port(
        &self,
        port_id: &str,
        signal: RackGraphSignal,
        direction: RackGraphPortDirection,
    ) -> bool {
        use RackGraphNodeKind::{AudioInput, AudioOutput, MidiInput, MidiOutput, Plugin, Rack};
        use RackGraphPortDirection::{Input, Output};
        use RackGraphSignal::{Audio, Midi};

        matches!(
            (self, port_id, signal, direction),
            (MidiInput { .. }, "out", Midi, Output)
                | (AudioInput { .. }, "out", Audio, Output)
                | (Plugin { .. } | Rack { .. }, "midi_in", Midi, Input)
                | (Plugin { .. } | Rack { .. }, "audio_in", Audio, Input)
                | (Plugin { .. } | Rack { .. }, "midi_out", Midi, Output)
                | (Plugin { .. } | Rack { .. }, "audio_out", Audio, Output)
                | (MidiOutput { .. }, "in", Midi, Input)
                | (AudioOutput { .. }, "in", Audio, Input)
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackGraphNode {
    pub id: RackGraphNodeId,
    pub kind: RackGraphNodeKind,
    #[serde(default)]
    pub position: RackGraphPosition,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RackGraphSignal {
    Midi,
    Audio,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RackGraphPortDirection {
    Input,
    Output,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackGraphEndpoint {
    pub node_id: RackGraphNodeId,
    pub port_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackGraphEdge {
    pub id: RackGraphEdgeId,
    pub signal: RackGraphSignal,
    pub source: RackGraphEndpoint,
    pub target: RackGraphEndpoint,
    /// MIDI filtering and transformation belongs to the cable, not the
    /// instrument. `None` keeps v2 Rack documents compatible and falls back
    /// to the legacy Slot MIDI fields when the graph is compiled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi_transform: Option<RackMidiTransform>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackMidiTransform {
    /// One-based MIDI channels. An empty list means Omni.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_channels: Vec<u8>,
    /// One-based MIDI channel, or `None` to preserve the source channel.
    #[serde(default)]
    pub target_channel: Option<u8>,
    #[serde(default)]
    pub note_low: u8,
    #[serde(default = "default_midi_note_high")]
    pub note_high: u8,
    #[serde(default)]
    pub transpose: i8,
    #[serde(default)]
    pub notes_only: bool,
    #[serde(default)]
    pub velocity_input_low: u8,
    #[serde(default = "default_midi_note_high")]
    pub velocity_input_high: u8,
    #[serde(default)]
    pub velocity_output_low: u8,
    #[serde(default = "default_midi_note_high")]
    pub velocity_output_high: u8,
}

impl Default for RackMidiTransform {
    fn default() -> Self {
        Self {
            source_channels: Vec::new(),
            target_channel: None,
            note_low: 0,
            note_high: 127,
            transpose: 0,
            notes_only: false,
            velocity_input_low: 0,
            velocity_input_high: 127,
            velocity_output_low: 0,
            velocity_output_high: 127,
        }
    }
}

impl RackMidiTransform {
    pub fn from_slot(slot: &RackSlot) -> Self {
        Self {
            source_channels: slot.midi_input_channel.into_iter().collect(),
            note_low: slot.midi_note_low,
            note_high: slot.midi_note_high,
            transpose: slot.midi_transpose,
            ..Self::default()
        }
    }

    fn validate(&self) -> Result<(), PerformanceError> {
        if self
            .source_channels
            .iter()
            .chain(self.target_channel.iter())
            .any(|channel| !(1..=16).contains(channel))
            || self.source_channels.iter().collect::<BTreeSet<_>>().len()
                != self.source_channels.len()
        {
            return Err(PerformanceError::InvalidMidiChannel);
        }
        if self.note_low > self.note_high || self.note_high > 127 {
            return Err(PerformanceError::InvalidMidiNoteRange);
        }
        if !(-48..=48).contains(&self.transpose) {
            return Err(PerformanceError::InvalidMidiTranspose);
        }
        if self.velocity_input_low >= self.velocity_input_high
            || self.velocity_input_high > 127
            || self.velocity_output_low > self.velocity_output_high
            || self.velocity_output_high > 127
        {
            return Err(PerformanceError::InvalidMidiVelocityCurve);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RackGraphLabelKind {
    #[default]
    Note,
    Section,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RackGraphLabelTone {
    #[default]
    Neutral,
    Cyan,
    Green,
    Amber,
    Violet,
    Red,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackGraphLabel {
    pub id: RackGraphLabelId,
    pub text: String,
    #[serde(default)]
    pub kind: RackGraphLabelKind,
    #[serde(default)]
    pub tone: RackGraphLabelTone,
    pub position: RackGraphPosition,
    pub width: u16,
    pub height: u16,
}

impl RackGraphLabel {
    fn validate(&self) -> Result<(), PerformanceError> {
        if self.text.trim().is_empty()
            || self.text.chars().count() > 512
            || self.text.contains('\0')
        {
            return Err(PerformanceError::InvalidRackGraphLabel);
        }
        self.position.validate()?;
        if !(80..=4_000).contains(&self.width) || !(40..=4_000).contains(&self.height) {
            return Err(PerformanceError::InvalidRackGraphLabel);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackGraph {
    pub schema_version: u32,
    pub nodes: Vec<RackGraphNode>,
    pub edges: Vec<RackGraphEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<RackGraphLabel>,
}

impl RackGraph {
    pub fn from_slots(slots: &[RackSlot]) -> Self {
        let midi_input_id = RackGraphNodeId::new("input.midi").expect("static graph id is valid");
        let mut nodes = vec![RackGraphNode {
            id: midi_input_id.clone(),
            kind: RackGraphNodeKind::MidiInput {
                bus_id: "main".into(),
            },
            position: RackGraphPosition { x: 0, y: 0 },
        }];
        let mut edges = Vec::with_capacity(slots.len() * 2);
        let audio_buses = slots
            .iter()
            .map(|slot| slot.audio_output_bus.as_str())
            .collect::<BTreeSet<_>>();
        let midi_buses = slots
            .iter()
            .filter_map(|slot| match &slot.midi_output {
                MidiOutputRoute::None => None,
                MidiOutputRoute::Bus { bus_id } => Some(bus_id.as_str()),
            })
            .collect::<BTreeSet<_>>();
        let mut audio_outputs = BTreeMap::new();
        for (index, bus_id) in audio_buses.into_iter().enumerate() {
            let id = RackGraphNodeId::new(format!("output.audio.{index:02}"))
                .expect("generated graph id is valid");
            nodes.push(RackGraphNode {
                id: id.clone(),
                kind: RackGraphNodeKind::AudioOutput {
                    bus_id: bus_id.into(),
                },
                position: RackGraphPosition {
                    x: 720,
                    y: index as i32 * 180,
                },
            });
            audio_outputs.insert(bus_id, id);
        }
        let mut midi_outputs = BTreeMap::new();
        for (index, bus_id) in midi_buses.into_iter().enumerate() {
            let id = RackGraphNodeId::new(format!("output.midi.{index:02}"))
                .expect("generated graph id is valid");
            nodes.push(RackGraphNode {
                id: id.clone(),
                kind: RackGraphNodeKind::MidiOutput {
                    bus_id: bus_id.into(),
                },
                position: RackGraphPosition {
                    x: 720,
                    y: 360 + index as i32 * 180,
                },
            });
            midi_outputs.insert(bus_id, id);
        }
        for (index, slot) in slots.iter().enumerate() {
            let number = index + 1;
            let plugin_id = RackGraphNodeId::new(format!("plugin.{number:02}"))
                .expect("generated graph id is valid");
            nodes.push(RackGraphNode {
                id: plugin_id.clone(),
                kind: RackGraphNodeKind::Plugin {
                    slot_id: slot.id.clone(),
                },
                position: RackGraphPosition {
                    x: 360,
                    y: index as i32 * 180,
                },
            });
            edges.push(RackGraphEdge {
                id: RackGraphEdgeId::new(format!("midi.{number:02}"))
                    .expect("generated graph id is valid"),
                signal: RackGraphSignal::Midi,
                source: RackGraphEndpoint {
                    node_id: midi_input_id.clone(),
                    port_id: "out".into(),
                },
                target: RackGraphEndpoint {
                    node_id: plugin_id.clone(),
                    port_id: "midi_in".into(),
                },
                midi_transform: Some(RackMidiTransform::from_slot(slot)),
            });
            edges.push(RackGraphEdge {
                id: RackGraphEdgeId::new(format!("audio.{number:02}"))
                    .expect("generated graph id is valid"),
                signal: RackGraphSignal::Audio,
                source: RackGraphEndpoint {
                    node_id: plugin_id.clone(),
                    port_id: "audio_out".into(),
                },
                target: RackGraphEndpoint {
                    node_id: audio_outputs[slot.audio_output_bus.as_str()].clone(),
                    port_id: "in".into(),
                },
                midi_transform: None,
            });
            if let MidiOutputRoute::Bus { bus_id } = &slot.midi_output {
                edges.push(RackGraphEdge {
                    id: RackGraphEdgeId::new(format!("midi-output.{number:02}"))
                        .expect("generated graph id is valid"),
                    signal: RackGraphSignal::Midi,
                    source: RackGraphEndpoint {
                        node_id: plugin_id,
                        port_id: "midi_out".into(),
                    },
                    target: RackGraphEndpoint {
                        node_id: midi_outputs[bus_id.as_str()].clone(),
                        port_id: "in".into(),
                    },
                    midi_transform: None,
                });
            }
        }
        Self {
            schema_version: RACK_GRAPH_SCHEMA_VERSION,
            nodes,
            edges,
            labels: Vec::new(),
        }
    }

    pub fn validate(&self, slots: &[RackSlot]) -> Result<(), PerformanceError> {
        if self.schema_version != RACK_GRAPH_SCHEMA_VERSION {
            return Err(PerformanceError::UnsupportedRackGraphSchema(
                self.schema_version,
            ));
        }
        validate_count(
            "Rack graph nodes",
            self.nodes.len(),
            1,
            MAX_RACK_GRAPH_NODES,
        )?;
        validate_count(
            "Rack graph edges",
            self.edges.len(),
            1,
            MAX_RACK_GRAPH_EDGES,
        )?;
        validate_count(
            "Rack graph labels",
            self.labels.len(),
            0,
            MAX_RACK_GRAPH_LABELS,
        )?;
        unique(
            self.nodes.iter().map(|node| node.id.as_str()),
            "Rack graph node",
        )?;
        unique(
            self.edges.iter().map(|edge| edge.id.as_str()),
            "Rack graph edge",
        )?;
        unique(
            self.labels.iter().map(|label| label.id.as_str()),
            "Rack graph label",
        )?;

        let nodes = self
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect::<BTreeMap<_, _>>();
        for node in &self.nodes {
            validate_id(node.id.as_str())?;
            node.kind.validate()?;
            node.position.validate()?;
        }
        for edge in &self.edges {
            validate_id(edge.id.as_str())?;
            if let Some(transform) = &edge.midi_transform {
                if edge.signal != RackGraphSignal::Midi {
                    return Err(PerformanceError::MidiTransformOnNonMidiEdge);
                }
                transform.validate()?;
            }
        }
        for label in &self.labels {
            validate_id(label.id.as_str())?;
            label.validate()?;
        }

        let slot_ids = slots
            .iter()
            .map(|slot| slot.id.as_str())
            .collect::<BTreeSet<_>>();
        let mut graph_slot_ids = BTreeSet::new();
        for node in &self.nodes {
            if let RackGraphNodeKind::Plugin { slot_id } = &node.kind {
                if !slot_ids.contains(slot_id.as_str()) {
                    return Err(PerformanceError::MissingRackSlot(slot_id.clone()));
                }
                if !graph_slot_ids.insert(slot_id.as_str()) {
                    return Err(PerformanceError::DuplicateRackGraphSlot(slot_id.clone()));
                }
            }
        }
        if graph_slot_ids != slot_ids {
            let missing = slot_ids
                .difference(&graph_slot_ids)
                .next()
                .expect("different Slot sets contain a missing Slot");
            return Err(PerformanceError::MissingRackGraphSlot(
                RackSlotId::new((*missing).to_owned()).expect("validated Slot id remains valid"),
            ));
        }

        let mut connections = BTreeSet::new();
        for edge in &self.edges {
            let source = nodes.get(edge.source.node_id.as_str()).ok_or_else(|| {
                PerformanceError::MissingRackGraphNode(edge.source.node_id.clone())
            })?;
            let target = nodes.get(edge.target.node_id.as_str()).ok_or_else(|| {
                PerformanceError::MissingRackGraphNode(edge.target.node_id.clone())
            })?;
            if source.id == target.id {
                return Err(PerformanceError::RackGraphCycle);
            }
            if !source.kind.supports_port(
                &edge.source.port_id,
                edge.signal,
                RackGraphPortDirection::Output,
            ) {
                return Err(PerformanceError::InvalidRackGraphPort {
                    node_id: source.id.clone(),
                    port_id: edge.source.port_id.clone(),
                });
            }
            if !target.kind.supports_port(
                &edge.target.port_id,
                edge.signal,
                RackGraphPortDirection::Input,
            ) {
                return Err(PerformanceError::InvalidRackGraphPort {
                    node_id: target.id.clone(),
                    port_id: edge.target.port_id.clone(),
                });
            }
            let connection = (edge.signal, edge.source.clone(), edge.target.clone());
            if !connections.insert(connection) {
                return Err(PerformanceError::DuplicateRackGraphConnection);
            }
        }
        validate_acyclic_graph(&self.nodes, &self.edges)
    }
}

#[deprecated(note = "use RackSlotId")]
pub type RackItemId = RackSlotId;
#[deprecated(note = "use RackSlot")]
pub type RackItem = RackSlot;
#[deprecated(note = "use RackSlotId")]
pub type RackNodeId = RackSlotId;
#[deprecated(note = "use RackSlot")]
pub type RackPluginNode = RackSlot;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackDefinition {
    pub schema_version: u32,
    pub id: RackId,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard_parts: Option<RackKeyboardParts>,
    #[serde(alias = "items", alias = "nodes")]
    pub slots: Vec<RackSlot>,
    /// Explicit routing and visual layout. Missing values are legacy flat Racks
    /// and are upgraded deterministically from `slots`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<RackGraph>,
}

impl RackDefinition {
    pub fn resolved_graph(&self) -> RackGraph {
        self.graph
            .clone()
            .unwrap_or_else(|| RackGraph::from_slots(&self.slots))
    }

    /// Materializes the v2 graph for a legacy flat Rack. Returns true when the
    /// Rack changed and should be persisted.
    pub fn migrate_graph(&mut self) -> bool {
        if self.graph.is_some() {
            return false;
        }
        self.graph = Some(RackGraph::from_slots(&self.slots));
        true
    }

    pub fn validate(&self) -> Result<(), PerformanceError> {
        validate_schema(self.schema_version)?;
        validate_name(&self.name)?;
        if let Some(parts) = &self.keyboard_parts {
            parts.validate()?;
        }
        let graph = self.resolved_graph();
        let has_nested_rack = graph
            .nodes
            .iter()
            .any(|node| matches!(node.kind, RackGraphNodeKind::Rack { .. }));
        validate_count(
            "rack slots",
            self.slots.len(),
            usize::from(!has_nested_rack),
            MAX_RACK_SLOTS,
        )?;
        unique(self.slots.iter().map(|slot| slot.id.as_str()), "rack slot")?;
        for slot in &self.slots {
            slot.validate()?;
        }
        if !has_nested_rack && !self.slots.iter().any(|slot| slot.enabled) {
            return Err(PerformanceError::NoEnabledRackSlot);
        }
        graph.validate(&self.slots)?;
        Ok(())
    }
}

/// One Part-to-sequencer binding: when the Part goes on stage, this pattern
/// is queued on this lane at the next bar. The lane number is the routing —
/// lane N speaks wire channel N, so the Rack's own channel filters decide
/// which Slot hears it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SongPartPatternBinding {
    pub lane: u8,
    pub pattern_id: PatternId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SongPart {
    pub id: SongPartId,
    pub name: String,
    /// Compatibility route used by Song documents written before Parts owned
    /// a graph. New editors materialize `content`; old documents continue to
    /// resolve this Rack without an eager on-disk migration.
    pub rack_id: RackId,
    /// A self-contained playable graph for this Part. It can host direct
    /// instruments and/or reusable child Racks. When absent, `rack_id` is the
    /// complete Part, preserving the original v1 Song contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<SongPartGraph>,
    /// Sequencer patterns this Part carries on stage with it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<SongPartPatternBinding>,
}

const LANE_NAMES: [&str; MAX_PART_PATTERN_BINDINGS] =
    ["lane 1", "lane 2", "lane 3", "lane 4", "lane 5", "lane 6", "lane 7", "lane 8"];

impl SongPart {
    fn validate(&self) -> Result<(), PerformanceError> {
        validate_name(&self.name)?;
        validate_count(
            "part pattern bindings",
            self.patterns.len(),
            0,
            MAX_PART_PATTERN_BINDINGS,
        )?;
        for binding in &self.patterns {
            if usize::from(binding.lane) >= MAX_PART_PATTERN_BINDINGS {
                return Err(PerformanceError::InvalidPatternLane(binding.lane));
            }
        }
        unique(
            self.patterns.iter().map(|binding| {
                // Lanes are the uniqueness key: a Part queues one pattern per
                // lane. The leak-proof str comes from a static table.
                LANE_NAMES[usize::from(binding.lane) % MAX_PART_PATTERN_BINDINGS]
            }),
            "part pattern lane",
        )?;
        if let Some(content) = &self.content {
            content.as_rack(self).validate()?;
        }
        Ok(())
    }

    /// Returns the stable synthetic Rack used by the audio compiler for a
    /// graph-backed Part. SongPartId and RackId intentionally share the same
    /// portable identifier grammar, so no platform-specific ID is invented.
    pub fn playable_rack(&self) -> Option<RackDefinition> {
        self.content.as_ref().map(|content| content.as_rack(self))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SongPartGraph {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard_parts: Option<RackKeyboardParts>,
    #[serde(default)]
    pub slots: Vec<RackSlot>,
    pub graph: RackGraph,
}

impl SongPartGraph {
    pub fn from_rack(rack: &RackDefinition) -> Self {
        Self {
            keyboard_parts: rack.keyboard_parts,
            slots: rack.slots.clone(),
            graph: rack.resolved_graph(),
        }
    }

    pub fn as_rack(&self, part: &SongPart) -> RackDefinition {
        RackDefinition {
            schema_version: PERFORMANCE_SCHEMA_VERSION,
            id: RackId::new(part.id.as_str().to_owned())
                .expect("validated Song Part id is also a valid Rack id"),
            name: part.name.clone(),
            enabled: true,
            keyboard_parts: self.keyboard_parts,
            slots: self.slots.clone(),
            graph: Some(self.graph.clone()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SongDefinition {
    pub schema_version: u32,
    pub id: SongId,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub parts: Vec<SongPart>,
}

impl SongDefinition {
    pub fn validate(&self) -> Result<(), PerformanceError> {
        validate_schema(self.schema_version)?;
        validate_name(&self.name)?;
        validate_count("song parts", self.parts.len(), 1, MAX_SONG_PARTS)?;
        unique(self.parts.iter().map(|part| part.id.as_str()), "song part")?;
        for part in &self.parts {
            part.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetlistEntry {
    pub id: SetlistEntryId,
    pub song_id: SongId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetlistDefinition {
    pub schema_version: u32,
    pub id: SetlistId,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub entries: Vec<SetlistEntry>,
}

impl SetlistDefinition {
    pub fn validate(&self) -> Result<(), PerformanceError> {
        validate_schema(self.schema_version)?;
        validate_name(&self.name)?;
        validate_count(
            "setlist entries",
            self.entries.len(),
            1,
            MAX_SETLIST_ENTRIES,
        )?;
        unique(
            self.entries.iter().map(|entry| entry.id.as_str()),
            "setlist entry",
        )
    }
}

/// When a step actually fires — the Elektron grammar, evaluated
/// deterministically so two passes of the same show are the same show.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrigCondition {
    /// Every pass.
    #[default]
    Always,
    /// Pass `hit` of every `of` — 1:2, 3:4, the cycle conditions.
    Cycle { hit: u8, of: u8 },
    /// Only while the FILL key is held.
    Fill,
    /// Only while it is not.
    NotFill,
    /// Only if the previous conditional trig on this lane fired.
    Pre,
    /// Only if it did not.
    NotPre,
}

impl TrigCondition {
    pub fn validate(self) -> Result<(), PerformanceError> {
        if let Self::Cycle { hit, of } = self
            && (!(2..=8).contains(&of) || hit == 0 || hit > of)
        {
            return Err(PerformanceError::InvalidTrigCondition);
        }
        Ok(())
    }
}

fn default_probability() -> u8 {
    100
}

fn probability_is_default(value: &u8) -> bool {
    *value == 100
}

fn condition_is_default(value: &TrigCondition) -> bool {
    *value == TrigCondition::Always
}

/// Parameter locks a step may carry: the Elektron gesture of a knob frozen
/// into a step. The value is in the plugin's own parameter domain — the
/// editor writes what the schema says, the engine passes it through.
pub const MAX_NOTE_LOCKS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterLockSpec {
    pub parameter: u32,
    pub value: f64,
}

fn locks_are_empty(locks: &[ParameterLockSpec]) -> bool {
    locks.is_empty()
}

/// One note of a sequencer pattern. Positions and durations are in integer
/// ticks from the pattern start, so a saved pattern carries no float in it
/// for two platforms to disagree about (parameter locks, which live in a
/// plugin's own value domain, are the deliberate exception).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternNoteSpec {
    pub tick: u32,
    pub duration_ticks: u32,
    pub key: u8,
    pub velocity: u8,
    #[serde(default)]
    pub channel: u8,
    /// Chance this step fires, 1..=100. Rolled deterministically per pass.
    #[serde(default = "default_probability", skip_serializing_if = "probability_is_default")]
    pub probability: u8,
    #[serde(default, skip_serializing_if = "condition_is_default")]
    pub condition: TrigCondition,
    /// Knobs frozen into this step, fired with its note-on.
    #[serde(default, skip_serializing_if = "locks_are_empty")]
    pub locks: Vec<ParameterLockSpec>,
}

/// A sequencer pattern: a performance-library entity like a Rack or a Song,
/// edited in LIVE and launched quantised against the host transport.
/// Which editor lens a pattern opens with. A hint for surfaces only: under
/// every lens a pattern is the same thing — notes — and the engine never
/// reads this field. Drum is the fixed-row grid; melodic is the step-and-
/// pitch lane of the classic analog sequencers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternView {
    #[default]
    Drum,
    Melodic,
}

/// Straight time. Swing walks 50..=75: the classic MPC range, where the
/// number is how far into its eighth-note pair the off-sixteenth lands —
/// 50 is half-way (straight), 66 is the triplet feel, 75 is dotted.
pub const PATTERN_SWING_STRAIGHT: u8 = 50;
pub const PATTERN_SWING_MAX: u8 = 75;

fn default_swing() -> u8 {
    PATTERN_SWING_STRAIGHT
}

/// What a pattern does when its run is over — the clip's will, executed by
/// the lane at the exact cycle boundary, seamlessly. `Any` rolls the same
/// seeded die as the trig grammar: deterministic across nights and hosts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowAction {
    /// Loop forever: no action.
    #[default]
    None,
    NextSlot,
    PreviousSlot,
    FirstSlot,
    AnySlot,
    Stop,
}

fn follow_action_is_default(value: &FollowAction) -> bool {
    *value == FollowAction::None
}

fn follow_after_is_default(value: &u8) -> bool {
    *value == 0
}

pub const MAX_SEQUENCER_TAB_LANES: usize = 8;
pub const SEQUENCER_TAB_SLOTS: usize = 4;

/// One sequencer of the tabbed deck, keyed by its engine lane: which
/// patterns sit in its A-D variation slots and which slot fronts the
/// editor. Library-owned so every host shows the same deck and a show
/// file carries it. Slot references are soft — a deleted pattern leaves
/// an empty slot, never a broken deck.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequencerTabDefinition {
    pub lane: u8,
    #[serde(default)]
    pub view: PatternView,
    #[serde(default)]
    pub slot_ids: Vec<Option<PatternId>>,
    #[serde(default)]
    pub active_slot: u8,
}

impl SequencerTabDefinition {
    fn validate(&self) -> Result<(), PerformanceError> {
        if usize::from(self.lane) >= MAX_SEQUENCER_TAB_LANES {
            return Err(PerformanceError::InvalidSequencerTab(self.lane));
        }
        if self.slot_ids.len() > SEQUENCER_TAB_SLOTS
            || usize::from(self.active_slot) >= SEQUENCER_TAB_SLOTS
        {
            return Err(PerformanceError::InvalidSequencerTab(self.lane));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternDefinition {
    pub id: PatternId,
    pub name: String,
    pub length_ticks: u32,
    #[serde(default)]
    pub notes: Vec<PatternNoteSpec>,
    #[serde(default)]
    pub view: PatternView,
    /// The pattern's own groove, carried wherever the pattern goes.
    #[serde(default = "default_swing")]
    pub swing_percent: u8,
    /// The key the phrase was written in — what key-follow transposes from.
    #[serde(default = "default_root_key")]
    pub root_key: u8,
    /// Full cycles to play before `follow_action` fires. 0 disables it.
    #[serde(default, skip_serializing_if = "follow_after_is_default")]
    pub follow_after: u8,
    #[serde(default, skip_serializing_if = "follow_action_is_default")]
    pub follow_action: FollowAction,
}

fn default_root_key() -> u8 {
    48 // C2, where basses live
}

impl PatternDefinition {
    pub fn validate(&self) -> Result<(), PerformanceError> {
        validate_name(&self.name)?;
        if self.length_ticks == 0 || self.length_ticks > MAX_PATTERN_TICKS {
            return Err(PerformanceError::InvalidPatternLength(self.length_ticks));
        }
        validate_count("pattern notes", self.notes.len(), 0, MAX_PATTERN_NOTES)?;
        if !(PATTERN_SWING_STRAIGHT..=PATTERN_SWING_MAX).contains(&self.swing_percent) {
            return Err(PerformanceError::InvalidPatternSwing(self.swing_percent));
        }
        if self.root_key > 127 {
            return Err(PerformanceError::InvalidPatternNote);
        }
        if self.follow_after > 64 {
            return Err(PerformanceError::InvalidFollowAfter(self.follow_after));
        }
        for note in &self.notes {
            if note.tick >= self.length_ticks
                || note.duration_ticks == 0
                || note.key > 127
                || note.velocity == 0
                || note.velocity > 127
                || note.channel > 15
                || note.probability == 0
                || note.probability > 100
                || note.locks.len() > MAX_NOTE_LOCKS
                || note.locks.iter().any(|lock| !lock.value.is_finite())
            {
                return Err(PerformanceError::InvalidPatternNote);
            }
            note.condition.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceLibrary {
    pub schema_version: u32,
    #[serde(default)]
    pub racks: Vec<RackDefinition>,
    #[serde(default)]
    pub songs: Vec<SongDefinition>,
    #[serde(default)]
    pub setlists: Vec<SetlistDefinition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<PatternDefinition>,
    /// The tabbed sequencer deck, one entry per engine lane in use.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sequencer_tabs: Vec<SequencerTabDefinition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PerformanceEdit {
    PutRack { rack: RackDefinition },
    DeleteRack { rack_id: RackId },
    PutSong { song: SongDefinition },
    DeleteSong { song_id: SongId },
    PutSetlist { setlist: SetlistDefinition },
    DeleteSetlist { setlist_id: SetlistId },
    PutPattern { pattern: PatternDefinition },
    DeletePattern { pattern_id: PatternId },
    PutSequencerTab { tab: SequencerTabDefinition },
    DeleteSequencerTab { lane: u8 },
}

impl PerformanceEdit {
    pub fn apply_to(&self, library: &mut PerformanceLibrary) -> Result<(), PerformanceError> {
        match self {
            Self::PutRack { rack } => upsert(&mut library.racks, rack.clone(), |item| &item.id),
            Self::DeleteRack { rack_id } => {
                remove(&mut library.racks, rack_id, |item| &item.id)
                    .ok_or_else(|| PerformanceError::MissingRack(rack_id.clone()))?;
            }
            Self::PutSong { song } => upsert(&mut library.songs, song.clone(), |item| &item.id),
            Self::DeleteSong { song_id } => {
                remove(&mut library.songs, song_id, |item| &item.id)
                    .ok_or_else(|| PerformanceError::MissingSong(song_id.clone()))?;
            }
            Self::PutSetlist { setlist } => {
                upsert(&mut library.setlists, setlist.clone(), |item| &item.id)
            }
            Self::DeleteSetlist { setlist_id } => {
                remove(&mut library.setlists, setlist_id, |item| &item.id)
                    .ok_or_else(|| PerformanceError::MissingSetlist(setlist_id.clone()))?;
            }
            Self::PutPattern { pattern } => {
                upsert(&mut library.patterns, pattern.clone(), |item| &item.id)
            }
            Self::DeletePattern { pattern_id } => {
                remove(&mut library.patterns, pattern_id, |item| &item.id)
                    .ok_or_else(|| PerformanceError::MissingPattern(pattern_id.clone()))?;
            }
            Self::PutSequencerTab { tab } => {
                upsert(&mut library.sequencer_tabs, tab.clone(), |item| &item.lane)
            }
            Self::DeleteSequencerTab { lane } => {
                remove(&mut library.sequencer_tabs, lane, |item| &item.lane)
                    .ok_or(PerformanceError::MissingSequencerTab(*lane))?;
            }
        }
        library.validate()
    }
}

fn upsert<T, I: PartialEq>(items: &mut Vec<T>, value: T, id: impl Fn(&T) -> &I) {
    if let Some(index) = items.iter().position(|item| id(item) == id(&value)) {
        items[index] = value;
    } else {
        items.push(value);
    }
}

fn remove<T, I: PartialEq>(items: &mut Vec<T>, target: &I, id: impl Fn(&T) -> &I) -> Option<T> {
    items
        .iter()
        .position(|item| id(item) == target)
        .map(|index| items.remove(index))
}

impl PerformanceLibrary {
    pub fn empty() -> Self {
        Self {
            schema_version: PERFORMANCE_SCHEMA_VERSION,
            racks: Vec::new(),
            songs: Vec::new(),
            setlists: Vec::new(),
            patterns: Vec::new(),
            sequencer_tabs: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), PerformanceError> {
        validate_schema(self.schema_version)?;
        validate_count("racks", self.racks.len(), 0, MAX_RACKS)?;
        validate_count("songs", self.songs.len(), 0, MAX_SONGS)?;
        validate_count("setlists", self.setlists.len(), 0, MAX_SETLISTS)?;
        validate_count("patterns", self.patterns.len(), 0, MAX_PATTERNS)?;
        unique(self.racks.iter().map(|rack| rack.id.as_str()), "rack")?;
        unique(self.songs.iter().map(|song| song.id.as_str()), "song")?;
        unique(
            self.setlists.iter().map(|setlist| setlist.id.as_str()),
            "setlist",
        )?;
        unique(
            self.patterns.iter().map(|pattern| pattern.id.as_str()),
            "pattern",
        )?;
        for pattern in &self.patterns {
            pattern.validate()?;
        }
        validate_count(
            "sequencer tabs",
            self.sequencer_tabs.len(),
            0,
            MAX_SEQUENCER_TAB_LANES,
        )?;
        {
            let mut lanes = std::collections::BTreeSet::new();
            for tab in &self.sequencer_tabs {
                tab.validate()?;
                if !lanes.insert(tab.lane) {
                    return Err(PerformanceError::InvalidSequencerTab(tab.lane));
                }
            }
        }
        for rack in &self.racks {
            rack.validate()?;
            if let Some(graph) = &rack.graph {
                for node in &graph.nodes {
                    if let RackGraphNodeKind::Rack { rack_id } = &node.kind
                        && !self.racks.iter().any(|candidate| candidate.id == *rack_id)
                    {
                        return Err(PerformanceError::MissingRack(rack_id.clone()));
                    }
                }
            }
        }
        validate_acyclic_rack_dependencies(&self.racks)?;
        for song in &self.songs {
            song.validate()?;
            for part in &song.parts {
                for binding in &part.patterns {
                    if !self
                        .patterns
                        .iter()
                        .any(|pattern| pattern.id == binding.pattern_id)
                    {
                        return Err(PerformanceError::MissingPattern(binding.pattern_id.clone()));
                    }
                }
                if part.content.is_none() && !self.racks.iter().any(|rack| rack.id == part.rack_id)
                {
                    return Err(PerformanceError::MissingRack(part.rack_id.clone()));
                }
                if let Some(content) = &part.content {
                    for node in &content.graph.nodes {
                        if let RackGraphNodeKind::Rack { rack_id } = &node.kind
                            && !self.racks.iter().any(|candidate| candidate.id == *rack_id)
                        {
                            return Err(PerformanceError::MissingRack(rack_id.clone()));
                        }
                    }
                }
            }
        }
        for setlist in &self.setlists {
            setlist.validate()?;
            for entry in &setlist.entries {
                if !self.songs.iter().any(|song| song.id == entry.song_id) {
                    return Err(PerformanceError::MissingSong(entry.song_id.clone()));
                }
            }
        }
        Ok(())
    }

    /// Upgrades all legacy flat Racks in memory. The repository uses the
    /// returned count to persist only documents that actually changed.
    pub fn migrate_rack_graphs(&mut self) -> usize {
        self.racks
            .iter_mut()
            .map(|rack| usize::from(rack.migrate_graph()))
            .sum()
    }

    pub fn rack(&self, id: &RackId) -> Option<&RackDefinition> {
        self.racks.iter().find(|rack| &rack.id == id)
    }

    pub fn song(&self, id: &SongId) -> Option<&SongDefinition> {
        self.songs.iter().find(|song| &song.id == id)
    }

    pub fn setlist(&self, id: &SetlistId) -> Option<&SetlistDefinition> {
        self.setlists.iter().find(|setlist| &setlist.id == id)
    }

    pub fn resolve(&self, location: &LiveLocation) -> Result<&RackDefinition, PerformanceError> {
        let rack_id = match location {
            LiveLocation::Rack { rack_id } => rack_id,
            LiveLocation::Song { song_id, part_id } => {
                &self
                    .song(song_id)
                    .ok_or_else(|| PerformanceError::MissingSong(song_id.clone()))?
                    .parts
                    .iter()
                    .find(|part| &part.id == part_id)
                    .ok_or_else(|| PerformanceError::MissingSongPart(part_id.clone()))?
                    .rack_id
            }
            LiveLocation::Setlist {
                setlist_id,
                entry_id,
                part_id,
            } => {
                let setlist = self
                    .setlist(setlist_id)
                    .ok_or_else(|| PerformanceError::MissingSetlist(setlist_id.clone()))?;
                let entry = setlist
                    .entries
                    .iter()
                    .find(|entry| &entry.id == entry_id)
                    .ok_or_else(|| PerformanceError::MissingSetlistEntry(entry_id.clone()))?;
                &self
                    .song(&entry.song_id)
                    .ok_or_else(|| PerformanceError::MissingSong(entry.song_id.clone()))?
                    .parts
                    .iter()
                    .find(|part| &part.id == part_id)
                    .ok_or_else(|| PerformanceError::MissingSongPart(part_id.clone()))?
                    .rack_id
            }
        };
        self.rack(rack_id)
            .ok_or_else(|| PerformanceError::MissingRack(rack_id.clone()))
    }

    /// Resolves the actual playable graph for a LIVE location. Rack locations
    /// are cloned; graph-backed Song Parts are converted to a stable synthetic
    /// Rack understood by the existing cross-platform graph compiler.
    pub fn resolve_playable(
        &self,
        location: &LiveLocation,
    ) -> Result<RackDefinition, PerformanceError> {
        match location {
            LiveLocation::Rack { rack_id } => self
                .rack(rack_id)
                .cloned()
                .ok_or_else(|| PerformanceError::MissingRack(rack_id.clone())),
            LiveLocation::Song { song_id, part_id } => {
                let song = self
                    .song(song_id)
                    .ok_or_else(|| PerformanceError::MissingSong(song_id.clone()))?;
                let part = song
                    .parts
                    .iter()
                    .find(|part| &part.id == part_id)
                    .ok_or_else(|| PerformanceError::MissingSongPart(part_id.clone()))?;
                part.playable_rack().map(Ok).unwrap_or_else(|| {
                    self.rack(&part.rack_id)
                        .cloned()
                        .ok_or_else(|| PerformanceError::MissingRack(part.rack_id.clone()))
                })
            }
            LiveLocation::Setlist {
                setlist_id,
                entry_id,
                part_id,
            } => {
                let setlist = self
                    .setlist(setlist_id)
                    .ok_or_else(|| PerformanceError::MissingSetlist(setlist_id.clone()))?;
                let entry = setlist
                    .entries
                    .iter()
                    .find(|entry| &entry.id == entry_id)
                    .ok_or_else(|| PerformanceError::MissingSetlistEntry(entry_id.clone()))?;
                let song = self
                    .song(&entry.song_id)
                    .ok_or_else(|| PerformanceError::MissingSong(entry.song_id.clone()))?;
                let part = song
                    .parts
                    .iter()
                    .find(|part| &part.id == part_id)
                    .ok_or_else(|| PerformanceError::MissingSongPart(part_id.clone()))?;
                part.playable_rack().map(Ok).unwrap_or_else(|| {
                    self.rack(&part.rack_id)
                        .cloned()
                        .ok_or_else(|| PerformanceError::MissingRack(part.rack_id.clone()))
                })
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveBrowseMode {
    #[default]
    Rack,
    Song,
    Setlist,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LiveLocation {
    Rack {
        rack_id: RackId,
    },
    Song {
        song_id: SongId,
        part_id: SongPartId,
    },
    Setlist {
        setlist_id: SetlistId,
        entry_id: SetlistEntryId,
        part_id: SongPartId,
    },
}

impl PerformanceLibrary {
    /// The Song Part a LIVE location puts on stage, when it names one.
    /// Rack locations carry no Part and resolve to `None`.
    pub fn resolve_part(&self, location: &LiveLocation) -> Option<&SongPart> {
        let (song_id, part_id) = match location {
            LiveLocation::Rack { .. } => return None,
            LiveLocation::Song { song_id, part_id } => (song_id.clone(), part_id),
            LiveLocation::Setlist {
                setlist_id,
                entry_id,
                part_id,
            } => {
                let entry = self
                    .setlist(setlist_id)?
                    .entries
                    .iter()
                    .find(|entry| &entry.id == entry_id)?;
                (entry.song_id.clone(), part_id)
            }
        };
        self.song(&song_id)?
            .parts
            .iter()
            .find(|part| &part.id == part_id)
    }
}

impl LiveLocation {
    pub fn mode(&self) -> LiveBrowseMode {
        match self {
            Self::Rack { .. } => LiveBrowseMode::Rack,
            Self::Song { .. } => LiveBrowseMode::Song,
            Self::Setlist { .. } => LiveBrowseMode::Setlist,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LivePerformanceState {
    #[serde(default)]
    pub mode: LiveBrowseMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rack: Option<LiveLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub song: Option<LiveLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setlist: Option<LiveLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<LiveLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_rack_id: Option<RackId>,
}

impl LivePerformanceState {
    pub fn validate(&self, library: &PerformanceLibrary) -> Result<(), PerformanceError> {
        for (expected, location) in [
            (LiveBrowseMode::Rack, self.rack.as_ref()),
            (LiveBrowseMode::Song, self.song.as_ref()),
            (LiveBrowseMode::Setlist, self.setlist.as_ref()),
        ] {
            if let Some(location) = location {
                if location.mode() != expected {
                    return Err(PerformanceError::MismatchedLiveLocation);
                }
                library.resolve_playable(location)?;
            }
        }
        if let Some(active) = &self.active {
            let rack = library.resolve_playable(active)?;
            if self.active_rack_id.as_ref() != Some(&rack.id) {
                return Err(PerformanceError::MismatchedActiveRack);
            }
        } else if self.active_rack_id.is_some() {
            return Err(PerformanceError::MismatchedActiveRack);
        }
        Ok(())
    }

    pub fn activate(&mut self, location: LiveLocation, rack_id: RackId) {
        self.mode = location.mode();
        match location.mode() {
            LiveBrowseMode::Rack => self.rack = Some(location.clone()),
            LiveBrowseMode::Song => self.song = Some(location.clone()),
            LiveBrowseMode::Setlist => self.setlist = Some(location.clone()),
        }
        self.active = Some(location);
        self.active_rack_id = Some(rack_id);
    }

    /// Stops the current LIVE target without losing the user's browsing
    /// position. Returning to LIVE can therefore reopen the same Rack, Song or
    /// Setlist while every surface correctly reports that nothing is playing.
    pub fn deactivate(&mut self) {
        self.active = None;
        self.active_rack_id = None;
    }

    /// Removes navigation targets invalidated by a library edit while keeping
    /// the Rack that is already sounding as the safest possible fallback.
    pub fn reconcile(&mut self, library: &PerformanceLibrary) {
        self.rack = self
            .rack
            .take()
            .filter(|location| library.resolve_playable(location).is_ok());
        self.song = self
            .song
            .take()
            .filter(|location| library.resolve_playable(location).is_ok());
        self.setlist = self
            .setlist
            .take()
            .filter(|location| library.resolve_playable(location).is_ok());

        let active_still_matches = self
            .active
            .as_ref()
            .and_then(|location| library.resolve_playable(location).ok())
            .is_some_and(|rack| self.active_rack_id.as_ref() == Some(&rack.id));
        if active_still_matches {
            return;
        }

        if let Some(rack_id) = self
            .active_rack_id
            .clone()
            .filter(|rack_id| library.rack(rack_id).is_some())
        {
            let location = LiveLocation::Rack {
                rack_id: rack_id.clone(),
            };
            self.mode = LiveBrowseMode::Rack;
            self.rack = Some(location.clone());
            self.active = Some(location);
            self.active_rack_id = Some(rack_id);
        } else {
            self.active = None;
            self.active_rack_id = None;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceSnapshot {
    pub schema_version: u32,
    pub revision: LibraryRevision,
    pub library: PerformanceLibrary,
    pub live: LivePerformanceState,
}

impl PerformanceSnapshot {
    pub fn validate(&self) -> Result<(), PerformanceError> {
        if self.schema_version != PERFORMANCE_SNAPSHOT_SCHEMA_VERSION {
            return Err(PerformanceError::UnsupportedSnapshotSchema(
                self.schema_version,
            ));
        }
        self.revision.validate()?;
        self.library.validate()?;
        self.live.validate(&self.library)
    }
}

fn validate_acyclic_graph(
    nodes: &[RackGraphNode],
    edges: &[RackGraphEdge],
) -> Result<(), PerformanceError> {
    let mut indegree = nodes
        .iter()
        .map(|node| (node.id.as_str(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = BTreeMap::<&str, Vec<&str>>::new();
    for edge in edges {
        *indegree
            .get_mut(edge.target.node_id.as_str())
            .expect("graph endpoints were validated") += 1;
        outgoing
            .entry(edge.source.node_id.as_str())
            .or_default()
            .push(edge.target.node_id.as_str());
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(*id))
        .collect::<Vec<_>>();
    let mut visited = 0;
    while let Some(id) = ready.pop() {
        visited += 1;
        for target in outgoing.get(id).into_iter().flatten() {
            let count = indegree
                .get_mut(target)
                .expect("graph endpoints were validated");
            *count -= 1;
            if *count == 0 {
                ready.push(target);
            }
        }
    }
    if visited != nodes.len() {
        return Err(PerformanceError::RackGraphCycle);
    }
    Ok(())
}

fn validate_acyclic_rack_dependencies(racks: &[RackDefinition]) -> Result<(), PerformanceError> {
    let mut indegree = racks
        .iter()
        .map(|rack| (rack.id.as_str(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = BTreeMap::<&str, Vec<&str>>::new();
    for rack in racks {
        let Some(graph) = &rack.graph else {
            continue;
        };
        for dependency in graph.nodes.iter().filter_map(|node| match &node.kind {
            RackGraphNodeKind::Rack { rack_id } => Some(rack_id.as_str()),
            _ => None,
        }) {
            *indegree
                .get_mut(dependency)
                .expect("nested Rack references were validated") += 1;
            outgoing
                .entry(rack.id.as_str())
                .or_default()
                .push(dependency);
        }
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(*id))
        .collect::<Vec<_>>();
    let mut visited = 0;
    while let Some(id) = ready.pop() {
        visited += 1;
        for target in outgoing.get(id).into_iter().flatten() {
            let count = indegree
                .get_mut(target)
                .expect("nested Rack references were validated");
            *count -= 1;
            if *count == 0 {
                ready.push(target);
            }
        }
    }
    if visited != racks.len() {
        return Err(PerformanceError::RackDependencyCycle);
    }
    Ok(())
}

fn default_true() -> bool {
    true
}

fn default_slot_name() -> String {
    "Instrument".into()
}

fn default_audio_output_bus() -> String {
    "main".into()
}

fn default_slot_level() -> u16 {
    1_000
}

fn default_midi_note_high() -> u8 {
    127
}

fn validate_schema(schema_version: u32) -> Result<(), PerformanceError> {
    if schema_version != PERFORMANCE_SCHEMA_VERSION {
        return Err(PerformanceError::UnsupportedSchema(schema_version));
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<(), PerformanceError> {
    if value.is_empty()
        || value.len() > 128
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
    {
        return Err(PerformanceError::InvalidId(value.into()));
    }
    Ok(())
}

fn validate_plugin_id(value: &str) -> Result<(), PerformanceError> {
    validate_id(value).map_err(|_| PerformanceError::InvalidPluginId(value.into()))
}

fn validate_reference(value: &str, field: &'static str) -> Result<(), PerformanceError> {
    if value.trim().is_empty() || value.len() > 256 || value.contains('\0') {
        return Err(PerformanceError::InvalidReference(field));
    }
    Ok(())
}

fn validate_name(value: &str) -> Result<(), PerformanceError> {
    if value.trim().is_empty() || value.chars().count() > 64 || value.contains('\0') {
        return Err(PerformanceError::InvalidName);
    }
    Ok(())
}

fn validate_count(
    field: &'static str,
    actual: usize,
    minimum: usize,
    maximum: usize,
) -> Result<(), PerformanceError> {
    if !(minimum..=maximum).contains(&actual) {
        return Err(PerformanceError::InvalidCount {
            field,
            actual,
            minimum,
            maximum,
        });
    }
    Ok(())
}

fn unique<'a>(
    values: impl Iterator<Item = &'a str>,
    kind: &'static str,
) -> Result<(), PerformanceError> {
    let mut found = BTreeSet::new();
    for value in values {
        if !found.insert(value) {
            return Err(PerformanceError::DuplicateId {
                kind,
                id: value.into(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum PerformanceError {
    #[error("unsupported performance schema {0}")]
    UnsupportedSchema(u32),
    #[error("unsupported Rack graph schema {0}")]
    UnsupportedRackGraphSchema(u32),
    #[error("unsupported performance snapshot schema {0}")]
    UnsupportedSnapshotSchema(u32),
    #[error("invalid performance library revision")]
    InvalidLibraryRevision,
    #[error("invalid performance id {0:?}")]
    InvalidId(String),
    #[error("invalid plugin id {0:?}")]
    InvalidPluginId(String),
    #[error("invalid {0}")]
    InvalidReference(&'static str),
    #[error("performance name is empty, too long or contains NUL")]
    InvalidName,
    #[error("pattern length {0} ticks is outside the pattern bounds")]
    InvalidPatternLength(u32),
    #[error("a pattern note is outside its pattern or carries invalid values")]
    InvalidPatternNote,
    #[error("pattern {0} does not exist")]
    MissingPattern(PatternId),
    #[error("pattern lane {0} is outside the lane deck")]
    InvalidPatternLane(u8),
    #[error("pattern swing {0} is outside 50..=75")]
    InvalidPatternSwing(u8),
    #[error("a trig cycle condition is outside hit 1..=of, of 2..=8")]
    InvalidTrigCondition,
    #[error("follow-after {0} is outside 0..=64 cycles")]
    InvalidFollowAfter(u8),
    #[error("{field} count {actual} is outside {minimum}..={maximum}")]
    InvalidCount {
        field: &'static str,
        actual: usize,
        minimum: usize,
        maximum: usize,
    },
    #[error("duplicate {kind} id {id:?}")]
    DuplicateId { kind: &'static str, id: String },
    #[error("Rack graph position is outside the supported canvas")]
    InvalidRackGraphPosition,
    #[error("Rack graph label is empty, too large or has invalid bounds")]
    InvalidRackGraphLabel,
    #[error("Rack graph node {0} does not exist")]
    MissingRackGraphNode(RackGraphNodeId),
    #[error("Rack Slot {0} referenced by the graph does not exist")]
    MissingRackSlot(RackSlotId),
    #[error("Rack Slot {0} appears in more than one graph node")]
    DuplicateRackGraphSlot(RackSlotId),
    #[error("Rack Slot {0} has no graph node")]
    MissingRackGraphSlot(RackSlotId),
    #[error("Rack graph node {node_id} does not support port {port_id:?}")]
    InvalidRackGraphPort {
        node_id: RackGraphNodeId,
        port_id: String,
    },
    #[error("Rack graph contains the same connection more than once")]
    DuplicateRackGraphConnection,
    #[error("Rack graph contains a feedback cycle")]
    RackGraphCycle,
    #[error("nested Racks contain a dependency cycle")]
    RackDependencyCycle,
    #[error("rack must contain at least one enabled Slot")]
    NoEnabledRackSlot,
    #[error("MIDI channel must be between 1 and 16")]
    InvalidMidiChannel,
    #[error("Rack Slot MIDI note range must be ordered within 0..127")]
    InvalidMidiNoteRange,
    #[error("Rack Slot MIDI transpose must be within -48..48 semitones")]
    InvalidMidiTranspose,
    #[error("MIDI velocity curve must be ordered within 0..127")]
    InvalidMidiVelocityCurve,
    #[error("MIDI transformation can only be attached to a MIDI connection")]
    MidiTransformOnNonMidiEdge,
    #[error("keyboard split must start Part 2 on a MIDI note within 1..127")]
    InvalidKeyboardSplit,
    #[error("Rack Slot level or pan is outside its supported range")]
    InvalidSlotMix,
    #[error("sequencer tab for lane {0} does not exist")]
    MissingSequencerTab(u8),
    #[error("sequencer tab for lane {0} is invalid")]
    InvalidSequencerTab(u8),
    #[error("Rack Slot plugin state reference is invalid")]
    InvalidPluginState,
    #[error("Rack Slot state belongs to a different plugin")]
    PluginStateMismatch,
    #[error("Rack Slot cannot contain both an opaque state and a legacy program")]
    AmbiguousPluginState,
    #[error("rack {0} does not exist")]
    MissingRack(RackId),
    #[error("song {0} does not exist")]
    MissingSong(SongId),
    #[error("song part {0} does not exist")]
    MissingSongPart(SongPartId),
    #[error("setlist {0} does not exist")]
    MissingSetlist(SetlistId),
    #[error("setlist entry {0} does not exist")]
    MissingSetlistEntry(SetlistEntryId),
    #[error("saved LIVE location is stored in the wrong mode slot")]
    MismatchedLiveLocation,
    #[error("active LIVE location and rack do not match")]
    MismatchedActiveRack,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> PerformanceLibrary {
        let rack_id = RackId::new("rack.stage-piano").unwrap();
        let song_id = SongId::new("song.opener").unwrap();
        PerformanceLibrary {
            schema_version: PERFORMANCE_SCHEMA_VERSION,
            racks: vec![RackDefinition {
                schema_version: PERFORMANCE_SCHEMA_VERSION,
                id: rack_id.clone(),
                name: "Stage Piano".into(),
                enabled: true,
                keyboard_parts: None,
                graph: None,
                slots: vec![RackSlot {
                    id: RackSlotId::new("instrument.main").unwrap(),
                    name: "Main Instrument".into(),
                    plugin_id: "org.rackforge.rf-dls".into(),
                    state: None,
                    legacy_program_id: Some("custom.user.stage-piano".into()),
                    enabled: true,
                    midi_input_channel: None,
                    midi_note_low: 0,
                    midi_note_high: 127,
                    midi_transpose: 0,
                    midi_output: MidiOutputRoute::None,
                    audio_output_bus: "main".into(),
                    level_per_mille: 1_000,
                    pan_per_mille: 0,
                }],
            }],
            songs: vec![SongDefinition {
                schema_version: PERFORMANCE_SCHEMA_VERSION,
                id: song_id.clone(),
                name: "Opener".into(),
                enabled: true,
                parts: vec![SongPart {
                    id: SongPartId::new("part.intro").unwrap(),
                    name: "Intro".into(),
                    rack_id,
                    content: None,
                    patterns: Vec::new(),
                }],
            }],
            setlists: vec![SetlistDefinition {
                schema_version: PERFORMANCE_SCHEMA_VERSION,
                id: SetlistId::new("setlist.friday").unwrap(),
                name: "Friday".into(),
                enabled: true,
                entries: vec![SetlistEntry {
                    id: SetlistEntryId::new("entry.opener").unwrap(),
                    song_id,
                }],
            }],
            patterns: vec![PatternDefinition {
                id: PatternId::new("pattern.four-on-the-floor").unwrap(),
                name: "Four on the floor".into(),
                length_ticks: 4 * PATTERN_TICKS_PER_BEAT,
                notes: vec![PatternNoteSpec {
                    tick: 0,
                    duration_ticks: PATTERN_TICKS_PER_BEAT / 2,
                    key: 36,
                    velocity: 100,
                    channel: 0,
                    probability: 100,
                    condition: TrigCondition::Always,
                    locks: Vec::new(),
                }],
                view: PatternView::default(),
                swing_percent: PATTERN_SWING_STRAIGHT,
                root_key: 48,
                follow_after: 0,
                follow_action: FollowAction::None,
            }],
            sequencer_tabs: Vec::new(),
        }
    }

    #[test]
    fn part_pattern_bindings_are_validated_against_the_library() {
        let mut library = library();
        let pattern_id = library.patterns[0].id.clone();
        library.songs[0].parts[0].patterns = vec![SongPartPatternBinding {
            lane: 2,
            pattern_id: pattern_id.clone(),
        }];
        library.validate().expect("a resolvable binding validates");
        assert_eq!(
            library.resolve_part(&LiveLocation::Song {
                song_id: library.songs[0].id.clone(),
                part_id: library.songs[0].parts[0].id.clone(),
            })
            .expect("the part resolves")
            .patterns
            .len(),
            1
        );

        library.songs[0].parts[0].patterns[0].pattern_id =
            PatternId::new("pattern.vanished").unwrap();
        assert!(matches!(
            library.validate(),
            Err(PerformanceError::MissingPattern(_))
        ));

        library.songs[0].parts[0].patterns[0].pattern_id = pattern_id.clone();
        library.songs[0].parts[0].patterns[0].lane = 8;
        assert!(matches!(
            library.validate(),
            Err(PerformanceError::InvalidPatternLane(8))
        ));

        library.songs[0].parts[0].patterns = vec![
            SongPartPatternBinding { lane: 1, pattern_id: pattern_id.clone() },
            SongPartPatternBinding { lane: 1, pattern_id },
        ];
        assert!(library.validate().is_err(), "one pattern per lane per Part");
    }

    #[test]
    fn resolves_all_three_live_entry_shapes_to_one_rack() {
        let library = library();
        library.validate().unwrap();
        let locations = [
            LiveLocation::Rack {
                rack_id: RackId::new("rack.stage-piano").unwrap(),
            },
            LiveLocation::Song {
                song_id: SongId::new("song.opener").unwrap(),
                part_id: SongPartId::new("part.intro").unwrap(),
            },
            LiveLocation::Setlist {
                setlist_id: SetlistId::new("setlist.friday").unwrap(),
                entry_id: SetlistEntryId::new("entry.opener").unwrap(),
                part_id: SongPartId::new("part.intro").unwrap(),
            },
        ];
        for location in locations {
            assert_eq!(
                library.resolve(&location).unwrap().id.as_str(),
                "rack.stage-piano"
            );
        }
    }

    #[test]
    fn graph_backed_song_part_is_a_stable_playable_root() {
        let mut library = library();
        let child_id = library.racks[0].id.clone();
        let part = &mut library.songs[0].parts[0];
        let mut graph = RackGraph::from_slots(&[]);
        graph.nodes.extend([
            RackGraphNode {
                id: RackGraphNodeId::new("rack.layer").unwrap(),
                kind: RackGraphNodeKind::Rack {
                    rack_id: child_id.clone(),
                },
                position: RackGraphPosition { x: 360, y: 0 },
            },
            RackGraphNode {
                id: RackGraphNodeId::new("output.audio.00").unwrap(),
                kind: RackGraphNodeKind::AudioOutput {
                    bus_id: "main".into(),
                },
                position: RackGraphPosition { x: 720, y: 0 },
            },
        ]);
        graph.edges.extend([
            RackGraphEdge {
                id: RackGraphEdgeId::new("layer.midi").unwrap(),
                signal: RackGraphSignal::Midi,
                source: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("input.midi").unwrap(),
                    port_id: "out".into(),
                },
                target: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("rack.layer").unwrap(),
                    port_id: "midi_in".into(),
                },
                midi_transform: Some(RackMidiTransform::default()),
            },
            RackGraphEdge {
                id: RackGraphEdgeId::new("layer.audio").unwrap(),
                signal: RackGraphSignal::Audio,
                source: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("rack.layer").unwrap(),
                    port_id: "audio_out".into(),
                },
                target: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("output.audio.00").unwrap(),
                    port_id: "in".into(),
                },
                midi_transform: None,
            },
        ]);
        part.content = Some(SongPartGraph {
            keyboard_parts: None,
            slots: Vec::new(),
            graph,
        });
        part.rack_id = RackId::new("rack.legacy-may-be-missing").unwrap();

        library.validate().unwrap();
        let location = LiveLocation::Song {
            song_id: SongId::new("song.opener").unwrap(),
            part_id: SongPartId::new("part.intro").unwrap(),
        };
        let playable = library.resolve_playable(&location).unwrap();
        assert_eq!(playable.id.as_str(), "part.intro");
        assert!(playable.slots.is_empty());
        assert!(playable.resolved_graph().nodes.iter().any(|node| {
            matches!(&node.kind, RackGraphNodeKind::Rack { rack_id } if rack_id == &child_id)
        }));

        let mut live = LivePerformanceState::default();
        live.activate(location, playable.id);
        live.validate(&library).unwrap();
    }

    #[test]
    fn rejects_dangling_references_before_runtime() {
        let mut library = library();
        library.songs[0].parts[0].rack_id = RackId::new("rack.missing").unwrap();
        assert_eq!(
            library.validate(),
            Err(PerformanceError::MissingRack(
                RackId::new("rack.missing").unwrap()
            ))
        );
    }

    #[test]
    fn validates_keyboard_zone_and_transposition_bounds() {
        let mut invalid_range = library();
        invalid_range.racks[0].slots[0].midi_note_low = 61;
        invalid_range.racks[0].slots[0].midi_note_high = 60;
        assert_eq!(
            invalid_range.validate(),
            Err(PerformanceError::InvalidMidiNoteRange)
        );

        let mut invalid_transpose = library();
        invalid_transpose.racks[0].slots[0].midi_transpose = 49;
        assert_eq!(
            invalid_transpose.validate(),
            Err(PerformanceError::InvalidMidiTranspose)
        );

        let mut invalid_split = library();
        invalid_split.racks[0].keyboard_parts = Some(RackKeyboardParts {
            split_key: Some(0),
            ..RackKeyboardParts::default()
        });
        assert_eq!(
            invalid_split.validate(),
            Err(PerformanceError::InvalidKeyboardSplit)
        );

        let mut invalid_part_channel = library();
        let mut parts = RackKeyboardParts::default();
        parts.part_2.midi_channel = 17;
        invalid_part_channel.racks[0].keyboard_parts = Some(parts);
        assert_eq!(
            invalid_part_channel.validate(),
            Err(PerformanceError::InvalidMidiChannel)
        );
    }

    #[test]
    fn live_state_keeps_independent_mode_positions() {
        let library = library();
        let rack = LiveLocation::Rack {
            rack_id: RackId::new("rack.stage-piano").unwrap(),
        };
        let song = LiveLocation::Song {
            song_id: SongId::new("song.opener").unwrap(),
            part_id: SongPartId::new("part.intro").unwrap(),
        };
        let mut state = LivePerformanceState::default();
        state.activate(rack.clone(), RackId::new("rack.stage-piano").unwrap());
        state.activate(song.clone(), RackId::new("rack.stage-piano").unwrap());
        assert_eq!(state.rack, Some(rack));
        assert_eq!(state.song, Some(song.clone()));
        assert_eq!(state.active, Some(song));
        state.validate(&library).unwrap();
    }

    #[test]
    fn live_state_reconciles_a_deleted_setlist_to_the_sounding_rack() {
        let mut library = library();
        let setlist = LiveLocation::Setlist {
            setlist_id: SetlistId::new("setlist.friday").unwrap(),
            entry_id: SetlistEntryId::new("entry.opener").unwrap(),
            part_id: SongPartId::new("part.intro").unwrap(),
        };
        let rack_id = RackId::new("rack.stage-piano").unwrap();
        let mut state = LivePerformanceState::default();
        state.activate(setlist, rack_id.clone());

        library.setlists.clear();
        state.reconcile(&library);

        let rack = LiveLocation::Rack {
            rack_id: rack_id.clone(),
        };
        assert_eq!(state.mode, LiveBrowseMode::Rack);
        assert_eq!(state.setlist, None);
        assert_eq!(state.rack, Some(rack.clone()));
        assert_eq!(state.active, Some(rack));
        assert_eq!(state.active_rack_id, Some(rack_id));
        state.validate(&library).unwrap();
    }

    #[test]
    fn live_state_clears_a_deleted_active_rack() {
        let mut library = library();
        let rack_id = RackId::new("rack.stage-piano").unwrap();
        let location = LiveLocation::Rack {
            rack_id: rack_id.clone(),
        };
        let mut state = LivePerformanceState::default();
        state.activate(location, rack_id);

        library.setlists.clear();
        library.songs.clear();
        library.racks.clear();
        state.reconcile(&library);

        assert_eq!(state.rack, None);
        assert_eq!(state.active, None);
        assert_eq!(state.active_rack_id, None);
        state.validate(&library).unwrap();
    }

    #[test]
    fn live_state_deactivation_preserves_the_browsed_location() {
        let library = library();
        let rack_id = RackId::new("rack.stage-piano").unwrap();
        let location = LiveLocation::Rack {
            rack_id: rack_id.clone(),
        };
        let mut state = LivePerformanceState::default();
        state.activate(location.clone(), rack_id);

        state.deactivate();

        assert_eq!(state.rack, Some(location));
        assert_eq!(state.active, None);
        assert_eq!(state.active_rack_id, None);
        state.validate(&library).unwrap();
    }

    #[test]
    fn graph_positions_accept_and_normalize_browser_fractions() {
        let position: RackGraphPosition =
            serde_json::from_str(r#"{"x":387.0652173913044,"y":-20.5}"#).unwrap();
        assert_eq!(position, RackGraphPosition { x: 387, y: -21 });
        assert_eq!(
            serde_json::to_string(&position).unwrap(),
            r#"{"x":387,"y":-21}"#
        );
    }

    #[test]
    fn snapshot_round_trips_without_implicit_defaults() {
        let library = library();
        let snapshot = PerformanceSnapshot {
            schema_version: PERFORMANCE_SNAPSHOT_SCHEMA_VERSION,
            revision: LibraryRevision::new("0".repeat(64)).unwrap(),
            library,
            live: LivePerformanceState::default(),
        };
        snapshot.validate().unwrap();
        let bytes = serde_json::to_vec(&snapshot).unwrap();
        assert_eq!(
            serde_json::from_slice::<PerformanceSnapshot>(&bytes).unwrap(),
            snapshot
        );
    }

    #[test]
    fn legacy_nodes_and_items_migrate_to_slots_with_an_opaque_plugin_state() {
        let json = r#"{
            "schema_version":1,
            "id":"rack.legacy",
            "name":"Legacy",
            "enabled":true,
            "nodes":[{
                "id":"instrument.main",
                "plugin_id":"org.rackforge.rf-dls",
                "program_id":"dls.piano",
                "enabled":true
            }]
        }"#;
        let rack: RackDefinition = serde_json::from_str(json).unwrap();
        rack.validate().unwrap();
        assert_eq!(rack.slots[0].name, "Instrument");
        assert_eq!(rack.slots[0].midi_input_channel, None);
        assert_eq!(rack.slots[0].midi_note_low, 0);
        assert_eq!(rack.slots[0].midi_note_high, 127);
        assert_eq!(rack.slots[0].midi_transpose, 0);
        assert_eq!(
            rack.slots[0].legacy_program_id.as_deref(),
            Some("dls.piano")
        );
        let migrated = serde_json::to_value(rack).unwrap();
        assert!(migrated.get("slots").is_some());
        assert!(migrated.get("items").is_none());
        assert!(migrated.get("nodes").is_none());
        assert!(migrated["slots"][0].get("legacy_program_id").is_some());
        assert!(migrated["slots"][0].get("program_id").is_none());
    }

    #[test]
    fn legacy_items_migrate_to_slots() {
        let json = r#"{
            "schema_version":1,
            "id":"rack.items",
            "name":"Items migration",
            "enabled":true,
            "items":[{
                "id":"instrument.main",
                "name":"Instrument",
                "plugin_id":"org.rackforge.rf-dls",
                "plugin_state_id":"dls.piano",
                "enabled":true
            }]
        }"#;
        let rack: RackDefinition = serde_json::from_str(json).unwrap();
        rack.validate().unwrap();
        assert_eq!(rack.slots.len(), 1);
        assert_eq!(
            rack.slots[0].legacy_program_id.as_deref(),
            Some("dls.piano")
        );
        assert!(serde_json::to_value(rack).unwrap().get("slots").is_some());
    }

    #[test]
    fn flat_rack_migrates_to_a_deterministic_v2_graph() {
        let mut library = library();
        assert_eq!(library.migrate_rack_graphs(), 1);
        assert_eq!(library.migrate_rack_graphs(), 0);
        library.validate().unwrap();

        let rack = &library.racks[0];
        let graph = rack.graph.as_ref().unwrap();
        assert_eq!(graph.schema_version, RACK_GRAPH_SCHEMA_VERSION);
        assert!(graph.nodes.iter().any(|node| matches!(
            &node.kind,
            RackGraphNodeKind::Plugin { slot_id } if slot_id == &rack.slots[0].id
        )));
        assert!(graph.edges.iter().any(|edge| {
            edge.signal == RackGraphSignal::Midi
                && edge.source.port_id == "out"
                && edge.target.port_id == "midi_in"
        }));
        assert!(graph.edges.iter().any(|edge| {
            edge.signal == RackGraphSignal::Audio
                && edge.source.port_id == "audio_out"
                && edge.target.port_id == "in"
        }));

        let serialized_once = serde_json::to_vec(rack).unwrap();
        let serialized_twice = serde_json::to_vec(rack).unwrap();
        assert_eq!(serialized_once, serialized_twice);
    }

    #[test]
    fn graph_labels_round_trip_without_affecting_slots() {
        let mut library = library();
        library.migrate_rack_graphs();
        let original_slots = library.racks[0].slots.clone();
        library.racks[0]
            .graph
            .as_mut()
            .unwrap()
            .labels
            .push(RackGraphLabel {
                id: RackGraphLabelId::new("label.keys").unwrap(),
                text: "Main keyboards".into(),
                kind: RackGraphLabelKind::Section,
                tone: RackGraphLabelTone::Cyan,
                position: RackGraphPosition { x: 280, y: -40 },
                width: 360,
                height: 220,
            });
        library.validate().unwrap();

        let bytes = serde_json::to_vec(&library.racks[0]).unwrap();
        let decoded: RackDefinition = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.slots, original_slots);
        assert_eq!(decoded, library.racks[0]);
    }

    #[test]
    fn graph_rejects_invalid_ports_and_feedback_cycles() {
        let mut invalid_port = library();
        invalid_port.migrate_rack_graphs();
        invalid_port.racks[0].graph.as_mut().unwrap().edges[0]
            .source
            .port_id = "midi_out".into();
        assert!(matches!(
            invalid_port.validate(),
            Err(PerformanceError::InvalidRackGraphPort { .. })
        ));

        let mut library = library();
        let mut second = library.racks[0].slots[0].clone();
        second.id = RackSlotId::new("instrument.layer").unwrap();
        second.name = "Layer".into();
        library.racks[0].slots.push(second);
        library.migrate_rack_graphs();
        let graph = library.racks[0].graph.as_mut().unwrap();
        graph.edges.extend([
            RackGraphEdge {
                id: RackGraphEdgeId::new("cycle.forward").unwrap(),
                signal: RackGraphSignal::Audio,
                source: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("plugin.01").unwrap(),
                    port_id: "audio_out".into(),
                },
                target: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("plugin.02").unwrap(),
                    port_id: "audio_in".into(),
                },
                midi_transform: None,
            },
            RackGraphEdge {
                id: RackGraphEdgeId::new("cycle.back").unwrap(),
                signal: RackGraphSignal::Audio,
                source: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("plugin.02").unwrap(),
                    port_id: "audio_out".into(),
                },
                target: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("plugin.01").unwrap(),
                    port_id: "audio_in".into(),
                },
                midi_transform: None,
            },
        ]);
        assert_eq!(library.validate(), Err(PerformanceError::RackGraphCycle));
    }

    #[test]
    fn nested_rack_dependencies_must_exist_and_remain_acyclic() {
        fn add_nested_rack(rack: &mut RackDefinition, rack_id: RackId) {
            let graph = rack.graph.as_mut().unwrap();
            graph.nodes.push(RackGraphNode {
                id: RackGraphNodeId::new("rack.child").unwrap(),
                kind: RackGraphNodeKind::Rack { rack_id },
                position: RackGraphPosition { x: 540, y: 240 },
            });
            graph.edges.extend([
                RackGraphEdge {
                    id: RackGraphEdgeId::new("child.midi").unwrap(),
                    signal: RackGraphSignal::Midi,
                    source: RackGraphEndpoint {
                        node_id: RackGraphNodeId::new("input.midi").unwrap(),
                        port_id: "out".into(),
                    },
                    target: RackGraphEndpoint {
                        node_id: RackGraphNodeId::new("rack.child").unwrap(),
                        port_id: "midi_in".into(),
                    },
                    midi_transform: None,
                },
                RackGraphEdge {
                    id: RackGraphEdgeId::new("child.audio").unwrap(),
                    signal: RackGraphSignal::Audio,
                    source: RackGraphEndpoint {
                        node_id: RackGraphNodeId::new("rack.child").unwrap(),
                        port_id: "audio_out".into(),
                    },
                    target: RackGraphEndpoint {
                        node_id: RackGraphNodeId::new("output.audio.00").unwrap(),
                        port_id: "in".into(),
                    },
                    midi_transform: None,
                },
            ]);
        }

        let mut missing_dependency = library();
        missing_dependency.migrate_rack_graphs();
        add_nested_rack(
            &mut missing_dependency.racks[0],
            RackId::new("rack.missing").unwrap(),
        );
        assert_eq!(
            missing_dependency.validate(),
            Err(PerformanceError::MissingRack(
                RackId::new("rack.missing").unwrap()
            ))
        );

        let mut library = library();
        library.migrate_rack_graphs();
        let mut second = library.racks[0].clone();
        second.id = RackId::new("rack.layer").unwrap();
        second.name = "Layer".into();
        add_nested_rack(&mut library.racks[0], second.id.clone());
        add_nested_rack(&mut second, library.racks[0].id.clone());
        library.racks.push(second);
        assert_eq!(
            library.validate(),
            Err(PerformanceError::RackDependencyCycle)
        );
    }
}
