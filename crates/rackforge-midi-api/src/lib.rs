use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;
use std::fmt;
use thiserror::Error;

pub const MIDI_ROUTING_SCHEMA_VERSION: u32 = 1;
pub const MIDI_CHANNEL_COUNT: u8 = 16;
pub const DEFAULT_INPUT_BUS_ID: &str = "main";
pub const MAX_TRANSPOSE_SEMITONES: i8 = 48;
pub const PARAMETER_LINK_SCHEMA_VERSION: u32 = 1;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, MidiRoutingError> {
                let value = value.into();
                validate_identifier(&value)?;
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

string_id!(MidiSourceId);
string_id!(MidiRouteId);
string_id!(MidiTargetId);
string_id!(MidiInputBusId);
string_id!(ParameterLinkId);

/// A host-owned mapping from one persistent MIDI control to one public plugin
/// parameter. Plugin state remains opaque; links live with the RackForge
/// session and are compiled against the parameter schema before audio starts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterLink {
    pub schema_version: u32,
    pub id: ParameterLinkId,
    pub instance_id: String,
    pub parameter_index: u32,
    pub source: ParameterLinkSource,
    #[serde(default)]
    pub channel: ParameterLinkChannel,
    pub message: ParameterLinkMessage,
    #[serde(default)]
    pub transform: ParameterLinkTransform,
    #[serde(default)]
    pub pass_through: ParameterLinkPassThrough,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterLinkSource {
    /// Stable backend identity. The display name is only a UI snapshot.
    pub source_id: MidiSourceId,
    pub display_name: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParameterLinkChannel {
    #[default]
    Omni,
    Channel {
        channel: MidiChannel,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParameterLinkMessage {
    ControlChange { controller: u8 },
    PitchBend,
    Note { note: u8 },
    ChannelPressure,
    PolyPressure { note: u8 },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterLinkTransform {
    #[serde(default)]
    pub invert: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterLinkPassThrough {
    /// Observe the message and still deliver it to the instrument.
    #[default]
    PassThrough,
    /// Explicit opt-in for controls that must not reach the instrument.
    Consume,
}

impl ParameterLink {
    pub fn validate(&self) -> Result<(), MidiRoutingError> {
        if self.schema_version != PARAMETER_LINK_SCHEMA_VERSION {
            return Err(MidiRoutingError::UnsupportedParameterLinkSchema(
                self.schema_version,
            ));
        }
        if self.instance_id.trim().is_empty() || self.instance_id.contains('\0') {
            return Err(MidiRoutingError::InvalidParameterLinkInstance);
        }
        if self.source.display_name.trim().is_empty() {
            return Err(MidiRoutingError::EmptySourceName);
        }
        let number = match self.message {
            ParameterLinkMessage::ControlChange { controller }
            | ParameterLinkMessage::Note { note: controller }
            | ParameterLinkMessage::PolyPressure { note: controller } => Some(controller),
            ParameterLinkMessage::PitchBend | ParameterLinkMessage::ChannelPressure => None,
        };
        if number.is_some_and(|number| number > 127) {
            return Err(MidiRoutingError::InvalidParameterLinkNumber);
        }
        Ok(())
    }

    pub fn matches_channel(&self, channel: MidiChannel) -> bool {
        match self.channel {
            ParameterLinkChannel::Omni => true,
            ParameterLinkChannel::Channel { channel: expected } => expected == channel,
        }
    }
}

/// A compact identifier assigned while RackForge is running.
///
/// It is safe to copy through a real-time MIDI queue, but must never be
/// persisted. Persisted routes use [`MidiSourceId`] and are resolved when a
/// source is discovered.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct MidiSourceKey(u32);

impl MidiSourceKey {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiSourceDescriptor {
    pub id: MidiSourceId,
    pub name: String,
    #[serde(default)]
    pub primary: bool,
}

impl MidiSourceDescriptor {
    pub fn validate(&self) -> Result<(), MidiRoutingError> {
        if self.name.trim().is_empty() {
            return Err(MidiRoutingError::EmptySourceName);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct MidiChannel(u8);

impl MidiChannel {
    pub fn from_zero_based(value: u8) -> Result<Self, MidiRoutingError> {
        if value >= MIDI_CHANNEL_COUNT {
            return Err(MidiRoutingError::InvalidInternalChannel(value));
        }
        Ok(Self(value))
    }

    pub fn from_user_number(value: u8) -> Result<Self, MidiRoutingError> {
        if !(1..=MIDI_CHANNEL_COUNT).contains(&value) {
            return Err(MidiRoutingError::InvalidUserChannel(value));
        }
        Ok(Self(value - 1))
    }

    pub const fn zero_based(self) -> u8 {
        self.0
    }

    pub const fn user_number(self) -> u8 {
        self.0 + 1
    }
}

impl Serialize for MidiChannel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.user_number())
    }
}

impl<'de> Deserialize<'de> for MidiChannel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Self::from_user_number(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for MidiChannel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.user_number().fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginChannelModel {
    SinglePart,
    MultiPart,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum MidiSourceSelector {
    #[default]
    Primary,
    AllPerformance,
    Source {
        source_id: MidiSourceId,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum MidiChannelFilter {
    #[default]
    Omni,
    Channel {
        channel: MidiChannel,
    },
    Set {
        channels: Vec<MidiChannel>,
    },
}

impl MidiChannelFilter {
    pub fn validate(&self) -> Result<(), MidiRoutingError> {
        if let Self::Set { channels } = self {
            if channels.is_empty() {
                return Err(MidiRoutingError::EmptyChannelSet);
            }
            let unique = channels.iter().copied().collect::<BTreeSet<_>>();
            if unique.len() != channels.len() {
                return Err(MidiRoutingError::DuplicateChannel);
            }
        }
        Ok(())
    }

    fn matches(&self, channel: MidiChannel) -> bool {
        match self {
            Self::Omni => true,
            Self::Channel { channel: expected } => *expected == channel,
            Self::Set { channels } => channels.contains(&channel),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MidiMessageKind {
    Note,
    PolyPressure,
    ControlChange,
    ProgramChange,
    ChannelPressure,
    PitchBend,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiMessageFilter {
    pub kinds: Vec<MidiMessageKind>,
}

impl MidiMessageFilter {
    pub fn all() -> Self {
        Self {
            kinds: vec![
                MidiMessageKind::Note,
                MidiMessageKind::PolyPressure,
                MidiMessageKind::ControlChange,
                MidiMessageKind::ProgramChange,
                MidiMessageKind::ChannelPressure,
                MidiMessageKind::PitchBend,
            ],
        }
    }

    pub fn validate(&self) -> Result<(), MidiRoutingError> {
        if self.kinds.is_empty() {
            return Err(MidiRoutingError::EmptyMessageFilter);
        }
        let unique = self.kinds.iter().copied().collect::<BTreeSet<_>>();
        if unique.len() != self.kinds.len() {
            return Err(MidiRoutingError::DuplicateMessageKind);
        }
        Ok(())
    }

    fn matches(&self, kind: MidiMessageKind) -> bool {
        self.kinds.contains(&kind)
    }
}

impl Default for MidiMessageFilter {
    fn default() -> Self {
        Self::all()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiNoteRange {
    pub low: u8,
    pub high: u8,
}

impl MidiNoteRange {
    pub const FULL: Self = Self { low: 0, high: 127 };

    pub fn validate(self) -> Result<(), MidiRoutingError> {
        validate_midi_range("note", self.low, self.high)
    }

    fn contains(self, value: u8) -> bool {
        (self.low..=self.high).contains(&value)
    }
}

impl Default for MidiNoteRange {
    fn default() -> Self {
        Self::FULL
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiVelocityRange {
    pub low: u8,
    pub high: u8,
}

impl MidiVelocityRange {
    pub const FULL: Self = Self { low: 0, high: 127 };

    pub fn validate(self) -> Result<(), MidiRoutingError> {
        validate_midi_range("velocity", self.low, self.high)
    }

    fn contains(self, value: u8) -> bool {
        (self.low..=self.high).contains(&value)
    }
}

impl Default for MidiVelocityRange {
    fn default() -> Self {
        Self::FULL
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MidiTranspose(i8);

impl MidiTranspose {
    pub const NONE: Self = Self(0);

    pub fn new(semitones: i8) -> Result<Self, MidiRoutingError> {
        if !(-MAX_TRANSPOSE_SEMITONES..=MAX_TRANSPOSE_SEMITONES).contains(&semitones) {
            return Err(MidiRoutingError::InvalidTranspose(semitones));
        }
        Ok(Self(semitones))
    }

    pub const fn semitones(self) -> i8 {
        self.0
    }
}

impl Default for MidiTranspose {
    fn default() -> Self {
        Self::NONE
    }
}

impl Serialize for MidiTranspose {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i8(self.0)
    }
}

impl<'de> Deserialize<'de> for MidiTranspose {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = i8::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum MidiChannelTransform {
    #[default]
    Auto,
    Preserve,
    Channel {
        channel: MidiChannel,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiRouteMatch {
    #[serde(default)]
    pub source: MidiSourceSelector,
    #[serde(default)]
    pub channels: MidiChannelFilter,
    #[serde(default)]
    pub notes: MidiNoteRange,
    #[serde(default)]
    pub velocities: MidiVelocityRange,
    #[serde(default)]
    pub messages: MidiMessageFilter,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiRouteTransform {
    #[serde(default)]
    pub channel: MidiChannelTransform,
    #[serde(default)]
    pub transpose: MidiTranspose,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiRouteTarget {
    pub instance_id: MidiTargetId,
    #[serde(default = "default_input_bus_id")]
    pub input_bus_id: MidiInputBusId,
}

fn default_input_bus_id() -> MidiInputBusId {
    MidiInputBusId(DEFAULT_INPUT_BUS_ID.into())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiRoute {
    pub schema_version: u32,
    pub id: MidiRouteId,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub matches: MidiRouteMatch,
    #[serde(default)]
    pub transform: MidiRouteTransform,
    pub target: MidiRouteTarget,
}

fn default_true() -> bool {
    true
}

impl MidiRoute {
    pub fn validate(&self) -> Result<(), MidiRoutingError> {
        if self.schema_version != MIDI_ROUTING_SCHEMA_VERSION {
            return Err(MidiRoutingError::UnsupportedSchema(self.schema_version));
        }
        self.matches.channels.validate()?;
        self.matches.notes.validate()?;
        self.matches.velocities.validate()?;
        self.matches.messages.validate()?;
        Ok(())
    }

    pub fn compile(
        &self,
        sources: &MidiSourceRegistry,
        channel_model: PluginChannelModel,
    ) -> Result<CompiledMidiRoute, MidiRoutingError> {
        self.validate()?;
        let source = match &self.matches.source {
            MidiSourceSelector::Primary => ResolvedMidiSourceSelector::Key(sources.primary_key()?),
            MidiSourceSelector::AllPerformance => ResolvedMidiSourceSelector::Any,
            MidiSourceSelector::Source { source_id } => {
                ResolvedMidiSourceSelector::Key(sources.resolve(source_id)?)
            }
        };
        let output_channel = match self.transform.channel {
            MidiChannelTransform::Auto => match channel_model {
                PluginChannelModel::SinglePart => ResolvedChannelTransform::Channel(
                    MidiChannel::from_zero_based(0).expect("channel zero is valid"),
                ),
                PluginChannelModel::MultiPart => ResolvedChannelTransform::Preserve,
            },
            MidiChannelTransform::Preserve => ResolvedChannelTransform::Preserve,
            MidiChannelTransform::Channel { channel } => ResolvedChannelTransform::Channel(channel),
        };
        Ok(CompiledMidiRoute {
            id: self.id.clone(),
            enabled: self.enabled,
            source,
            channels: self.matches.channels.clone(),
            notes: self.matches.notes,
            velocities: self.matches.velocities,
            messages: self.matches.messages.clone(),
            output_channel,
            transpose: self.transform.transpose,
            target: self.target.clone(),
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct MidiSourceRegistry {
    sources: Vec<RegisteredMidiSource>,
}

#[derive(Clone, Debug)]
struct RegisteredMidiSource {
    key: MidiSourceKey,
    descriptor: MidiSourceDescriptor,
}

impl MidiSourceRegistry {
    pub fn register(
        &mut self,
        key: MidiSourceKey,
        descriptor: MidiSourceDescriptor,
    ) -> Result<(), MidiRoutingError> {
        descriptor.validate()?;
        if self.sources.iter().any(|source| source.key == key) {
            return Err(MidiRoutingError::DuplicateSourceKey(key.get()));
        }
        if self
            .sources
            .iter()
            .any(|source| source.descriptor.id == descriptor.id)
        {
            return Err(MidiRoutingError::DuplicateSourceId(
                descriptor.id.to_string(),
            ));
        }
        if descriptor.primary && self.sources.iter().any(|source| source.descriptor.primary) {
            return Err(MidiRoutingError::DuplicatePrimarySource);
        }
        self.sources.push(RegisteredMidiSource { key, descriptor });
        Ok(())
    }

    pub fn resolve(&self, id: &MidiSourceId) -> Result<MidiSourceKey, MidiRoutingError> {
        self.sources
            .iter()
            .find(|source| &source.descriptor.id == id)
            .map(|source| source.key)
            .ok_or_else(|| MidiRoutingError::UnknownSource(id.to_string()))
    }

    pub fn resolve_optional(&self, id: &MidiSourceId) -> Option<MidiSourceKey> {
        self.sources
            .iter()
            .find(|source| &source.descriptor.id == id)
            .map(|source| source.key)
    }

    /// Resolves a currently approved source by the backend display name.
    /// External controller drivers use this only as a discovery hint; the
    /// registry remains the authority for the stable identity and runtime key.
    pub fn resolve_name(&self, name: &str) -> Option<(MidiSourceKey, &MidiSourceDescriptor)> {
        self.sources
            .iter()
            .find(|source| source.descriptor.name == name)
            .map(|source| (source.key, &source.descriptor))
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &MidiSourceDescriptor> {
        self.sources.iter().map(|source| &source.descriptor)
    }

    pub fn descriptor(&self, key: MidiSourceKey) -> Option<&MidiSourceDescriptor> {
        self.sources
            .iter()
            .find(|source| source.key == key)
            .map(|source| &source.descriptor)
    }

    pub fn primary_key(&self) -> Result<MidiSourceKey, MidiRoutingError> {
        self.sources
            .iter()
            .find(|source| source.descriptor.primary)
            .map(|source| source.key)
            .ok_or(MidiRoutingError::MissingPrimarySource)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MidiPacket {
    pub frame: u32,
    pub length: u8,
    /// The message as MIDI 1.0 bytes. Always present: for a wide message
    /// these are its exact projection -- the top bits of `wide`, by the
    /// specification's translation rule -- and they are what every filter,
    /// range and curve in this crate reads. Filters do not widen.
    pub data: [u8; 3],
    /// The message's value at MIDI 2.0 width, when its source had one: the
    /// 16-bit velocity of a note, the 32 bits of a controller, a pressure or
    /// a bend. `None` means the bytes ARE the message. Nothing in this crate
    /// reads it; the host's vocabulary does, so a plug-in that asked for the
    /// width receives it and one that did not is none the wiser.
    pub wide: Option<u32>,
}

impl MidiPacket {
    pub fn new(frame: u32, message: &[u8]) -> Result<Self, MidiRoutingError> {
        let Some(status) = message.first().copied() else {
            return Err(MidiRoutingError::EmptyMessage);
        };
        if !(0x80..0xf0).contains(&status) {
            return Err(MidiRoutingError::UnsupportedStatus(status));
        }
        let expected = match status & 0xf0 {
            0xc0 | 0xd0 => 2,
            _ => 3,
        };
        if message.len() != expected {
            return Err(MidiRoutingError::InvalidMessageLength {
                status,
                expected,
                actual: message.len(),
            });
        }
        if message[1..].iter().any(|byte| *byte > 0x7f) {
            return Err(MidiRoutingError::InvalidDataByte);
        }
        let mut data = [0_u8; 3];
        data[..message.len()].copy_from_slice(message);
        Ok(Self {
            frame,
            length: message.len() as u8,
            data,
            wide: None,
        })
    }

    /// A channel-voice message at MIDI 2.0 width, with its byte projection
    /// derived by the specification's rule: the top bits. A note-on whose
    /// velocity would project to 0 projects to 1, since 0 would turn it into
    /// a note-off; a program change has no width and carries none.
    pub fn wide(frame: u32, status: u8, index: u8, value: u32) -> Result<Self, MidiRoutingError> {
        let index = index & 0x7f;
        let (length, data) = match status & 0xf0 {
            0x80 => (3, [status, index, ((value & 0xffff) >> 9) as u8]),
            0x90 => (3, [status, index, (((value & 0xffff) >> 9) as u8).max(1)]),
            0xa0 | 0xb0 => (3, [status, index, (value >> 25) as u8]),
            0xc0 => (2, [status, index, 0]),
            0xd0 => (2, [status, (value >> 25) as u8, 0]),
            0xe0 => {
                let bend = value >> 18;
                (3, [status, (bend & 0x7f) as u8, (bend >> 7) as u8])
            }
            _ => return Err(MidiRoutingError::UnsupportedStatus(status)),
        };
        Ok(Self {
            frame,
            length,
            data,
            wide: (status & 0xf0 != 0xc0).then_some(value),
        })
    }

    pub fn channel(self) -> MidiChannel {
        MidiChannel(self.data[0] & 0x0f)
    }

    pub fn kind(self) -> MidiMessageKind {
        match self.data[0] & 0xf0 {
            0x80 | 0x90 => MidiMessageKind::Note,
            0xa0 => MidiMessageKind::PolyPressure,
            0xb0 => MidiMessageKind::ControlChange,
            0xc0 => MidiMessageKind::ProgramChange,
            0xd0 => MidiMessageKind::ChannelPressure,
            0xe0 => MidiMessageKind::PitchBend,
            _ => unreachable!("MidiPacket construction validates channel voice status"),
        }
    }

    pub fn note(self) -> Option<u8> {
        matches!(self.data[0] & 0xf0, 0x80 | 0x90 | 0xa0).then_some(self.data[1])
    }

    pub fn note_on_velocity(self) -> Option<u8> {
        (self.data[0] & 0xf0 == 0x90 && self.data[2] != 0).then_some(self.data[2])
    }

    fn with_channel(mut self, channel: MidiChannel) -> Self {
        self.data[0] = (self.data[0] & 0xf0) | channel.zero_based();
        self
    }

    fn transpose(mut self, transpose: MidiTranspose) -> Option<Self> {
        if !matches!(self.data[0] & 0xf0, 0x80 | 0x90 | 0xa0) {
            return Some(self);
        }
        let note = i16::from(self.data[1]) + i16::from(transpose.semitones());
        if !(0..=127).contains(&note) {
            return None;
        }
        self.data[1] = note as u8;
        Some(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IngressMidiEvent {
    pub source: MidiSourceKey,
    pub packet: MidiPacket,
}

#[derive(Clone, Debug)]
enum ResolvedMidiSourceSelector {
    Any,
    Key(MidiSourceKey),
}

#[derive(Clone, Copy, Debug)]
enum ResolvedChannelTransform {
    Preserve,
    Channel(MidiChannel),
}

#[derive(Clone, Debug)]
pub struct CompiledMidiRoute {
    id: MidiRouteId,
    enabled: bool,
    source: ResolvedMidiSourceSelector,
    channels: MidiChannelFilter,
    notes: MidiNoteRange,
    velocities: MidiVelocityRange,
    messages: MidiMessageFilter,
    output_channel: ResolvedChannelTransform,
    transpose: MidiTranspose,
    target: MidiRouteTarget,
}

#[derive(Clone, Copy, Debug)]
pub struct RoutedMidiEvent<'route> {
    pub route_id: &'route MidiRouteId,
    pub target: &'route MidiRouteTarget,
    pub packet: MidiPacket,
}

impl CompiledMidiRoute {
    /// Routes one event without allocating or mutating route state.
    ///
    /// Multiple compiled routes may match the same ingress event; the host
    /// intentionally evaluates every route to implement layers.
    pub fn route<'route>(&'route self, event: IngressMidiEvent) -> Option<RoutedMidiEvent<'route>> {
        if !self.enabled
            || !match self.source {
                ResolvedMidiSourceSelector::Any => true,
                ResolvedMidiSourceSelector::Key(key) => key == event.source,
            }
            || !self.channels.matches(event.packet.channel())
            || !self.messages.matches(event.packet.kind())
        {
            return None;
        }
        if let Some(note) = event.packet.note()
            && !self.notes.contains(note)
        {
            return None;
        }
        // Velocity filtering starts voices only. Note-off must not be rejected
        // by its release velocity; the note ownership ledger will later route
        // it to exactly the destinations that received the note-on.
        if let Some(velocity) = event.packet.note_on_velocity()
            && !self.velocities.contains(velocity)
        {
            return None;
        }
        let channel = match self.output_channel {
            ResolvedChannelTransform::Preserve => event.packet.channel(),
            ResolvedChannelTransform::Channel(channel) => channel,
        };
        let packet = event
            .packet
            .with_channel(channel)
            .transpose(self.transpose)?;
        Some(RoutedMidiEvent {
            route_id: &self.id,
            target: &self.target,
            packet,
        })
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum MidiRoutingError {
    #[error("unsupported MIDI routing schema version {0}")]
    UnsupportedSchema(u32),
    #[error("invalid identifier {0:?}")]
    InvalidIdentifier(String),
    #[error("MIDI source name cannot be empty")]
    EmptySourceName,
    #[error("MIDI channel {0} is outside internal range 0..15")]
    InvalidInternalChannel(u8),
    #[error("MIDI channel {0} is outside user range 1..16")]
    InvalidUserChannel(u8),
    #[error("MIDI channel set cannot be empty")]
    EmptyChannelSet,
    #[error("MIDI channel set contains a duplicate")]
    DuplicateChannel,
    #[error("MIDI message filter cannot be empty")]
    EmptyMessageFilter,
    #[error("MIDI message filter contains a duplicate")]
    DuplicateMessageKind,
    #[error("{kind} range {low}..={high} is invalid")]
    InvalidRange {
        kind: &'static str,
        low: u8,
        high: u8,
    },
    #[error("transpose {0} is outside -48..=48 semitones")]
    InvalidTranspose(i8),
    #[error("duplicate runtime MIDI source key {0}")]
    DuplicateSourceKey(u32),
    #[error("duplicate persistent MIDI source id {0:?}")]
    DuplicateSourceId(String),
    #[error("only one MIDI source may be primary")]
    DuplicatePrimarySource,
    #[error("no primary MIDI source is available")]
    MissingPrimarySource,
    #[error("unknown MIDI source {0:?}")]
    UnknownSource(String),
    #[error("MIDI message cannot be empty")]
    EmptyMessage,
    #[error("unsupported MIDI status 0x{0:02x}")]
    UnsupportedStatus(u8),
    #[error("MIDI status 0x{status:02x} requires {expected} bytes, received {actual}")]
    InvalidMessageLength {
        status: u8,
        expected: usize,
        actual: usize,
    },
    #[error("MIDI data bytes must be in 0..=127")]
    InvalidDataByte,
    #[error("unsupported parameter-link schema version {0}")]
    UnsupportedParameterLinkSchema(u32),
    #[error("parameter link instance_id cannot be empty")]
    InvalidParameterLinkInstance,
    #[error("parameter link message number must be in 0..=127")]
    InvalidParameterLinkNumber,
}

fn validate_identifier(value: &str) -> Result<(), MidiRoutingError> {
    if value.is_empty()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        })
    {
        return Err(MidiRoutingError::InvalidIdentifier(value.into()));
    }
    Ok(())
}

fn validate_midi_range(kind: &'static str, low: u8, high: u8) -> Result<(), MidiRoutingError> {
    if low > 127 || high > 127 || low > high {
        return Err(MidiRoutingError::InvalidRange { kind, low, high });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// The bytes of a wide packet are its top bits, and a note-on never
    /// projects to the byte that would make it a note-off.
    #[test]
    fn a_wide_packet_projects_its_top_bits() {
        let full = super::MidiPacket::wide(3, 0x92, 60, 0xffff).unwrap();
        assert_eq!(
            (full.length, full.data, full.wide),
            (3, [0x92, 60, 127], Some(0xffff))
        );
        assert_eq!(full.note_on_velocity(), Some(127));
        let whisper = super::MidiPacket::wide(0, 0x90, 60, 1).unwrap();
        assert_eq!(whisper.data, [0x90, 60, 1]);
        assert_eq!(
            super::MidiPacket::wide(0, 0x90, 60, 0x1ff).unwrap().data[2],
            1
        );
        let pedal = super::MidiPacket::wide(0, 0xb0, 64, 0x8000_0000).unwrap();
        assert_eq!(pedal.data, [0xb0, 64, 64]);
        let centre = super::MidiPacket::wide(0, 0xe1, 0, 0x8000_0000).unwrap();
        assert_eq!(centre.data, [0xe1, 0x00, 0x40]);
        let program = super::MidiPacket::wide(0, 0xc0, 5, 7).unwrap();
        assert_eq!(
            (program.length, program.data, program.wide),
            (2, [0xc0, 5, 0], None)
        );
        assert_eq!(
            super::MidiPacket::new(0, &[0x90, 60, 100]).unwrap().wide,
            None
        );
        assert!(super::MidiPacket::wide(0, 0xf8, 0, 0).is_err());
    }

    use super::*;

    fn channel(number: u8) -> MidiChannel {
        MidiChannel::from_user_number(number).unwrap()
    }

    fn source(id: &str, name: &str, primary: bool) -> MidiSourceDescriptor {
        MidiSourceDescriptor {
            id: MidiSourceId::new(id).unwrap(),
            name: name.into(),
            primary,
        }
    }

    fn registry() -> MidiSourceRegistry {
        let mut registry = MidiSourceRegistry::default();
        registry
            .register(
                MidiSourceKey::new(10),
                source("controller.keylab", "KeyLab Essential 61 mk3", true),
            )
            .unwrap();
        registry
            .register(
                MidiSourceKey::new(20),
                source("controller.pads", "External pads", false),
            )
            .unwrap();
        registry
    }

    fn route() -> MidiRoute {
        MidiRoute {
            schema_version: MIDI_ROUTING_SCHEMA_VERSION,
            id: MidiRouteId::new("route.main").unwrap(),
            enabled: true,
            matches: MidiRouteMatch::default(),
            transform: MidiRouteTransform::default(),
            target: MidiRouteTarget {
                instance_id: MidiTargetId::new("live.instrument.1").unwrap(),
                input_bus_id: MidiInputBusId::new(DEFAULT_INPUT_BUS_ID).unwrap(),
            },
        }
    }

    fn ingress(source: u32, message: &[u8]) -> IngressMidiEvent {
        IngressMidiEvent {
            source: MidiSourceKey::new(source),
            packet: MidiPacket::new(0, message).unwrap(),
        }
    }

    #[test]
    fn channels_are_human_numbered_in_documents_and_zero_based_in_midi() {
        let channel = MidiChannel::from_zero_based(15).unwrap();
        assert_eq!(channel.user_number(), 16);
        assert_eq!(serde_json::to_string(&channel).unwrap(), "16");
        assert_eq!(
            serde_json::from_str::<MidiChannel>("1")
                .unwrap()
                .zero_based(),
            0
        );
        assert!(serde_json::from_str::<MidiChannel>("0").is_err());
        assert!(MidiChannel::from_zero_based(16).is_err());
    }

    #[test]
    fn source_registry_rejects_ambiguous_identity() {
        let mut sources = MidiSourceRegistry::default();
        sources
            .register(MidiSourceKey::new(1), source("controller.a", "A", true))
            .unwrap();
        assert_eq!(
            sources
                .register(MidiSourceKey::new(2), source("controller.b", "B", true))
                .unwrap_err(),
            MidiRoutingError::DuplicatePrimarySource
        );
        assert_eq!(
            sources
                .register(MidiSourceKey::new(3), source("controller.a", "A2", false))
                .unwrap_err(),
            MidiRoutingError::DuplicateSourceId("controller.a".into())
        );
    }

    #[test]
    fn external_drivers_resolve_names_through_the_host_registry() {
        let sources = registry();
        let (key, descriptor) = sources.resolve_name("KeyLab Essential 61 mk3").unwrap();
        assert_eq!(key, MidiSourceKey::new(10));
        assert_eq!(descriptor.id.as_str(), "controller.keylab");
        assert!(sources.resolve_name("Disabled controller").is_none());
    }

    #[test]
    fn single_part_auto_normalizes_the_output_to_channel_one() {
        let compiled = route()
            .compile(&registry(), PluginChannelModel::SinglePart)
            .unwrap();
        let routed = compiled.route(ingress(10, &[0x96, 60, 100])).unwrap();
        assert_eq!(routed.packet.data, [0x90, 60, 100]);
    }

    #[test]
    fn multi_part_auto_preserves_the_input_channel() {
        let compiled = route()
            .compile(&registry(), PluginChannelModel::MultiPart)
            .unwrap();
        let routed = compiled.route(ingress(10, &[0x96, 60, 100])).unwrap();
        assert_eq!(routed.packet.data, [0x96, 60, 100]);
    }

    #[test]
    fn explicit_channel_filter_and_remap_are_independent() {
        let mut definition = route();
        definition.matches.channels = MidiChannelFilter::Channel {
            channel: channel(3),
        };
        definition.transform.channel = MidiChannelTransform::Channel {
            channel: channel(12),
        };
        let compiled = definition
            .compile(&registry(), PluginChannelModel::MultiPart)
            .unwrap();

        assert!(compiled.route(ingress(10, &[0x91, 60, 100])).is_none());
        assert_eq!(
            compiled
                .route(ingress(10, &[0x92, 60, 100]))
                .unwrap()
                .packet
                .data,
            [0x9b, 60, 100]
        );
    }

    #[test]
    fn split_and_transpose_route_only_the_selected_notes() {
        let mut definition = route();
        definition.matches.notes = MidiNoteRange { low: 36, high: 59 };
        definition.transform.transpose = MidiTranspose::new(12).unwrap();
        let compiled = definition
            .compile(&registry(), PluginChannelModel::SinglePart)
            .unwrap();

        assert!(compiled.route(ingress(10, &[0x90, 60, 100])).is_none());
        assert_eq!(
            compiled
                .route(ingress(10, &[0x90, 48, 100]))
                .unwrap()
                .packet
                .data,
            [0x90, 60, 100]
        );
    }

    #[test]
    fn transposed_notes_outside_midi_range_are_dropped_not_clamped() {
        let mut definition = route();
        definition.transform.transpose = MidiTranspose::new(12).unwrap();
        let compiled = definition
            .compile(&registry(), PluginChannelModel::SinglePart)
            .unwrap();
        assert!(compiled.route(ingress(10, &[0x90, 120, 100])).is_none());
    }

    #[test]
    fn velocity_filter_does_not_swallow_note_off() {
        let mut definition = route();
        definition.matches.velocities = MidiVelocityRange {
            low: 100,
            high: 127,
        };
        let compiled = definition
            .compile(&registry(), PluginChannelModel::SinglePart)
            .unwrap();

        assert!(compiled.route(ingress(10, &[0x90, 60, 90])).is_none());
        assert!(compiled.route(ingress(10, &[0x90, 60, 110])).is_some());
        assert!(compiled.route(ingress(10, &[0x80, 60, 1])).is_some());
        assert!(compiled.route(ingress(10, &[0x90, 60, 0])).is_some());
    }

    #[test]
    fn source_selectors_isolate_controllers() {
        let mut definition = route();
        definition.matches.source = MidiSourceSelector::Source {
            source_id: MidiSourceId::new("controller.pads").unwrap(),
        };
        let compiled = definition
            .compile(&registry(), PluginChannelModel::SinglePart)
            .unwrap();
        assert!(compiled.route(ingress(10, &[0x90, 60, 100])).is_none());
        assert!(compiled.route(ingress(20, &[0x90, 60, 100])).is_some());
    }

    #[test]
    fn the_same_note_can_match_multiple_routes_for_layers() {
        let first = route()
            .compile(&registry(), PluginChannelModel::SinglePart)
            .unwrap();
        let mut second_definition = route();
        second_definition.id = MidiRouteId::new("route.layer.2").unwrap();
        second_definition.target.instance_id = MidiTargetId::new("live.instrument.2").unwrap();
        let second = second_definition
            .compile(&registry(), PluginChannelModel::SinglePart)
            .unwrap();
        let event = ingress(10, &[0x90, 60, 100]);

        let destinations = [&first, &second]
            .into_iter()
            .filter_map(|route| route.route(event))
            .map(|routed| routed.target.instance_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(destinations, vec!["live.instrument.1", "live.instrument.2"]);
    }

    #[test]
    fn non_note_messages_are_filtered_and_remapped_without_transposition() {
        let mut definition = route();
        definition.matches.messages = MidiMessageFilter {
            kinds: vec![MidiMessageKind::ControlChange],
        };
        definition.transform.channel = MidiChannelTransform::Channel {
            channel: channel(4),
        };
        definition.transform.transpose = MidiTranspose::new(12).unwrap();
        let compiled = definition
            .compile(&registry(), PluginChannelModel::SinglePart)
            .unwrap();

        assert!(compiled.route(ingress(10, &[0x90, 60, 100])).is_none());
        assert_eq!(
            compiled
                .route(ingress(10, &[0xb8, 1, 64]))
                .unwrap()
                .packet
                .data,
            [0xb3, 1, 64]
        );
    }

    #[test]
    fn invalid_routes_and_packets_fail_before_realtime_processing() {
        let mut definition = route();
        definition.matches.channels = MidiChannelFilter::Set { channels: vec![] };
        assert_eq!(
            definition
                .compile(&registry(), PluginChannelModel::SinglePart)
                .unwrap_err(),
            MidiRoutingError::EmptyChannelSet
        );
        assert!(MidiPacket::new(0, &[0x90, 60]).is_err());
        assert!(MidiPacket::new(0, &[0xf8]).is_err());
        assert!(MidiPacket::new(0, &[0x90, 60, 255]).is_err());
    }
}
