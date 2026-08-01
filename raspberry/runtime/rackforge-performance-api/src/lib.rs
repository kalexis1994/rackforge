use rackforge_plugin_api::PluginStateReference;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use thiserror::Error;

pub const PERFORMANCE_SCHEMA_VERSION: u32 = 1;
pub const PERFORMANCE_SNAPSHOT_SCHEMA_VERSION: u32 = 4;
pub const MAX_RACKS: usize = 256;
pub const MAX_RACK_SLOTS: usize = 32;
pub const MAX_SONGS: usize = 256;
pub const MAX_SONG_PARTS: usize = 64;
pub const MAX_SETLISTS: usize = 128;
pub const MAX_SETLIST_ENTRIES: usize = 256;

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
performance_id!(SongId);
performance_id!(SongPartId);
performance_id!(SetlistId);
performance_id!(SetlistEntryId);

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
}

impl RackDefinition {
    pub fn validate(&self) -> Result<(), PerformanceError> {
        validate_schema(self.schema_version)?;
        validate_name(&self.name)?;
        if let Some(parts) = &self.keyboard_parts {
            parts.validate()?;
        }
        validate_count("rack slots", self.slots.len(), 1, MAX_RACK_SLOTS)?;
        unique(self.slots.iter().map(|slot| slot.id.as_str()), "rack slot")?;
        for slot in &self.slots {
            slot.validate()?;
        }
        if !self.slots.iter().any(|slot| slot.enabled) {
            return Err(PerformanceError::NoEnabledRackSlot);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SongPart {
    pub id: SongPartId,
    pub name: String,
    pub rack_id: RackId,
}

impl SongPart {
    fn validate(&self) -> Result<(), PerformanceError> {
        validate_name(&self.name)
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

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceLibrary {
    pub schema_version: u32,
    #[serde(default)]
    pub racks: Vec<RackDefinition>,
    #[serde(default)]
    pub songs: Vec<SongDefinition>,
    #[serde(default)]
    pub setlists: Vec<SetlistDefinition>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PerformanceEdit {
    PutRack { rack: RackDefinition },
    DeleteRack { rack_id: RackId },
    PutSong { song: SongDefinition },
    DeleteSong { song_id: SongId },
    PutSetlist { setlist: SetlistDefinition },
    DeleteSetlist { setlist_id: SetlistId },
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
        }
    }

    pub fn validate(&self) -> Result<(), PerformanceError> {
        validate_schema(self.schema_version)?;
        validate_count("racks", self.racks.len(), 0, MAX_RACKS)?;
        validate_count("songs", self.songs.len(), 0, MAX_SONGS)?;
        validate_count("setlists", self.setlists.len(), 0, MAX_SETLISTS)?;
        unique(self.racks.iter().map(|rack| rack.id.as_str()), "rack")?;
        unique(self.songs.iter().map(|song| song.id.as_str()), "song")?;
        unique(
            self.setlists.iter().map(|setlist| setlist.id.as_str()),
            "setlist",
        )?;
        for rack in &self.racks {
            rack.validate()?;
        }
        for song in &self.songs {
            song.validate()?;
            for part in &song.parts {
                if !self.racks.iter().any(|rack| rack.id == part.rack_id) {
                    return Err(PerformanceError::MissingRack(part.rack_id.clone()));
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
                library.resolve(location)?;
            }
        }
        if let Some(active) = &self.active {
            let rack = library.resolve(active)?;
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
    #[error("{field} count {actual} is outside {minimum}..={maximum}")]
    InvalidCount {
        field: &'static str,
        actual: usize,
        minimum: usize,
        maximum: usize,
    },
    #[error("duplicate {kind} id {id:?}")]
    DuplicateId { kind: &'static str, id: String },
    #[error("rack must contain at least one enabled Slot")]
    NoEnabledRackSlot,
    #[error("MIDI channel must be between 1 and 16")]
    InvalidMidiChannel,
    #[error("Rack Slot MIDI note range must be ordered within 0..127")]
    InvalidMidiNoteRange,
    #[error("Rack Slot MIDI transpose must be within -48..48 semitones")]
    InvalidMidiTranspose,
    #[error("keyboard split must start Part 2 on a MIDI note within 1..127")]
    InvalidKeyboardSplit,
    #[error("Rack Slot level or pan is outside its supported range")]
    InvalidSlotMix,
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
        }
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
}
