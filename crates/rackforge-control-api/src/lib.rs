pub use rackforge_audio_api::OutputMeterSnapshot;
pub use rackforge_audio_api::{AudioOutputProfile, AudioOutputState};
pub use rackforge_midi_api::{
    MidiChannel, MidiSourceDescriptor, ParameterLink, ParameterLinkChannel, ParameterLinkId,
    ParameterLinkMessage, ParameterLinkPassThrough, ParameterLinkSource, ParameterLinkTransform,
};
pub use rackforge_performance_api::{
    LibraryRevision, LivePerformanceState, PatternDefinition, PerformanceEdit, PerformanceLibrary,
    PerformanceSnapshot,
};
pub use rackforge_plugin_api::{
    HostPreset, HostPresetSummary, ParameterSchema, PluginStateReference,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub use rackforge_session_api::{
    AuditionEndReason, AuditionState, ClientId, CommandEnvelope, CommandRef, EventEnvelope,
    InstanceId, PluginInstanceState, ProgramDraftState, Revision, SESSION_SCHEMA_VERSION,
    SessionCommand, SessionEvent, SessionId, SessionState, SoundSummary, SurfaceActivationReason,
    SurfaceActivationRequest, SurfaceActivationResponse, SurfaceMode,
};

pub const CONTROL_SCHEMA_VERSION: u32 = 16;
pub const CONTROL_SOCKET_NAME: &str = "live-control.sock";
/// Sized for the largest documents the wire carries: a `.rfpreset` embeds
/// one base64-encoded 1 MiB plugin state; a `.rflive` show embeds every
/// state its racks reference.
pub const MAX_CONTROL_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

pub const RFPRESET_FORMAT: &str = "org.rackforge.preset";
pub const RFPRESET_SCHEMA_VERSION: u32 = 1;

/// Portable, self-contained preset file. The opaque state remains owned by
/// the plugin; RackForge only authenticates and transports it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RfPresetFile {
    pub format: String,
    pub schema_version: u32,
    pub exported_by: String,
    pub exported_unix_ms: u64,
    pub preset: HostPreset,
    pub state_encoding: RfPresetStateEncoding,
    pub state_base64: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RfPresetStateEncoding {
    Base64,
}

pub const RFLIVE_FORMAT: &str = "org.rackforge.live";
pub const RFLIVE_SCHEMA_VERSION: u32 = 1;

/// Portable, self-contained show file: the whole performance library plus
/// every plugin state its Racks reference, embedded the way `.rfpreset`
/// embeds one. Carries a manifest of the plugins the show needs — never
/// the plugins themselves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RfLiveFile {
    pub format: String,
    pub schema_version: u32,
    pub exported_by: String,
    pub exported_unix_ms: u64,
    /// The show's display name; also seeds the file name.
    pub name: String,
    /// Racks, songs, setlists and sequencer patterns, exactly as edited.
    pub library: PerformanceLibrary,
    /// Every plugin state a Rack Slot references, one entry per unique
    /// blob. The state stays opaque: RackForge authenticates and
    /// transports it, the plugin owns it.
    #[serde(default)]
    pub states: Vec<RfLiveEmbeddedState>,
    /// What the show needs installed to sound.
    #[serde(default)]
    pub requirements: Vec<RfLiveRequirement>,
    /// The transport as it stood at export: the show's tempo and meter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tempo_bpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beats_per_bar: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat_unit: Option<u8>,
    /// Where the artist stood: browse mode and active Rack/Song/Setlist.
    /// The importing surface reactivates it so the show opens sounding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<LivePerformanceState>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RfLiveEmbeddedState {
    pub reference: PluginStateReference,
    pub state_base64: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RfLiveRequirement {
    pub plugin_id: String,
    /// The version the embedded states were made with.
    pub version: String,
}

/// What an importing musician is told before committing: sizes, what the
/// upsert would replace, and which plugins are missing on this machine.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RfLiveImportPreview {
    pub name: String,
    pub racks: u32,
    pub songs: u32,
    pub setlists: u32,
    pub patterns: u32,
    pub states: u32,
    #[serde(default)]
    pub tabs: u32,
    pub missing_plugins: Vec<RfLiveRequirement>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetImportConflictPolicy {
    #[default]
    Reject,
    Replace,
    KeepBoth,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetImportConflictKind {
    Id,
    Name,
    IdAndName,
    Ambiguous,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RfPresetImportPreview {
    pub preset: HostPresetSummary,
    pub byte_length: u32,
    pub conflict: Option<PresetImportConflictKind>,
    pub compatible: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PluginParameterControlCommand {
    Read {
        instance_id: InstanceId,
    },
    Set {
        instance_id: InstanceId,
        parameter_index: u32,
        value: f64,
    },
}

pub fn parse_plugin_parameter_control_command(
    method: &str,
    expected_instance_id: &str,
    params: &serde_json::Value,
) -> Result<PluginParameterControlCommand, String> {
    let instance_id = params
        .get("instance_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "plugin parameter command is missing instance_id".to_owned())?;
    if instance_id != expected_instance_id {
        return Err(format!(
            "plugin instance {instance_id:?} is not the active host instance"
        ));
    }
    let instance_id = InstanceId::new(instance_id)?;
    match method {
        "plugin_parameters" => Ok(PluginParameterControlCommand::Read { instance_id }),
        "set_plugin_parameter" => {
            let parameter_index = params
                .get("parameter_index")
                .and_then(serde_json::Value::as_u64)
                .and_then(|index| u32::try_from(index).ok())
                .ok_or_else(|| {
                    "plugin parameter command has an invalid parameter_index".to_owned()
                })?;
            let value = params
                .get("value")
                .and_then(serde_json::Value::as_f64)
                .filter(|value| value.is_finite())
                .ok_or_else(|| "plugin parameter command has an invalid value".to_owned())?;
            Ok(PluginParameterControlCommand::Set {
                instance_id,
                parameter_index,
                value,
            })
        }
        _ => Err(format!("unknown plugin parameter command {method:?}")),
    }
}

/// The scales key-follow can snap into. `Chromatic` is a pure transpose;
/// the rest quantise every transposed note into the scale rooted at the
/// held key — "suena en la escala de esa nota".
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SequencerScale {
    Chromatic,
    Major,
    Minor,
    Dorian,
    Mixolydian,
    PentatonicMajor,
    PentatonicMinor,
}

/// Which grid line a launch or stop waits for. `Now` is this block.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SequencerQuantize {
    Now,
    NextBeat,
    NextBar,
}

/// One instruction to the host sequencer. The quantise boundary is resolved
/// by the host against its own transport, never by the client's clock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SequencerCommand {
    TransportStart,
    /// Holds position and lets sounding notes finish; a paused show resumes
    /// where it was.
    TransportStop,
    /// Ends every sounding note immediately and clears all lanes.
    TransportPanic,
    SetTempo {
        bpm: f64,
    },
    SetSignature {
        beats_per_bar: u8,
        beat_unit: u8,
    },
    QueuePattern {
        lane: u8,
        pattern: PatternDefinition,
        quantize: SequencerQuantize,
    },
    /// Stores a pattern in one of the lane's variation slots without
    /// launching it — the quiet half of the Session grid.
    LoadSlot {
        lane: u8,
        slot: u8,
        pattern: PatternDefinition,
    },
    /// Switches the lane to a stored variation, quantised: the A/B/C/D
    /// jump. The slot becomes the lane's active one; pads relaunch it.
    LaunchSlot {
        lane: u8,
        slot: u8,
        quantize: SequencerQuantize,
    },
    /// Relaunches the pattern the lane already holds. A lane remembers its
    /// pattern across stops the way a groovebox track does, so a PERFORM
    /// pad needs no document to press play again.
    LaunchLane {
        lane: u8,
        quantize: SequencerQuantize,
    },
    StopLane {
        lane: u8,
        quantize: SequencerQuantize,
    },
    SetLaneMuted {
        lane: u8,
        muted: bool,
    },
    /// Names the lane the player is working on — the deck's open tab.
    /// Lane-less hardware gestures (a controller's single REC button)
    /// resolve against it.
    SetFocusLane {
        lane: u8,
    },
    /// Arms or disarms live capture on a lane: while armed and the
    /// transport runs, played notes are recorded against the transport's
    /// beat, to be drained by `SequencerCaptureTake`. Arming while the
    /// transport is stopped starts it — REC means ready to record.
    SetCapture {
        lane: u8,
        on: bool,
    },
    /// Enables or disables MIDI clock out: the machine as the backline's
    /// conductor, 24 pulses per quarter plus start/continue/stop.
    SetClockOut {
        on: bool,
    },
    /// Holds or releases FILL: the performance switch the fill/not-fill
    /// trig conditions listen to. Momentary by design — the caller sends
    /// press and release.
    SetFill {
        on: bool,
    },
    /// Puts a lane in key-follow: its pattern sounds only while a key is
    /// held, transposed so the phrase's root follows the played note —
    /// the SH-101's party trick. `None` returns the lane to looping.
    SetLaneFollow {
        lane: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<SequencerScale>,
    },
    /// Which MIDI channel this lane listens to for key-follow and capture.
    /// `None` is OMNI — the whole keyboard speaks to the lane. A defined
    /// channel lets a keyboard split conduct one lane from its lower zone
    /// while the upper zone plays normally.
    SetLaneListenChannel {
        lane: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<u8>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequencerLaneStatus {
    pub playing: bool,
    pub queued: bool,
    /// A stop boundary is set: still sounding, going quiet at the bar.
    #[serde(default)]
    pub stopping: bool,
    /// The lane is in key-follow, waiting on (or following) a held key.
    #[serde(default)]
    pub following: bool,
    /// The lane is armed for live capture.
    #[serde(default)]
    pub capturing: bool,
    /// The MIDI channel the lane listens to; `None` is OMNI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen_channel: Option<u8>,
    /// Which variation slot is the lane's active one.
    #[serde(default)]
    pub active_slot: u8,
    /// The names of the patterns stored per slot; `None` is an empty slot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<Option<String>>,
    pub muted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_name: Option<String>,
}

/// One note the player performed while a lane was armed for capture:
/// absolute transport beats, exactly as the engine heard them. Quantising
/// belongs to the editing surface, which knows the pattern's grid.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedNoteV1 {
    pub beat: f64,
    pub key: u8,
    pub velocity: u8,
    pub duration_beats: f64,
}

/// What a transport display shows. Bars and beats are 1-based because that
/// is how musicians count.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequencerStatusV1 {
    pub running: bool,
    /// The FILL performance switch is held.
    #[serde(default)]
    pub fill: bool,
    /// MIDI clock out is running.
    #[serde(default)]
    pub clock_out: bool,
    /// The lane the player is working on (the deck's open tab).
    #[serde(default)]
    pub focus_lane: u8,
    pub tempo_bpm: f64,
    pub beats_per_bar: u8,
    pub beat_unit: u8,
    pub bar: u64,
    pub beat_in_bar: u8,
    /// Progress through the current beat, `0.0..1.0`.
    pub beat_phase: f64,
    pub lanes: Vec<SequencerLaneStatus>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlRequest {
    Snapshot,
    PerformanceSnapshot,
    EditPerformance {
        expected_revision: LibraryRevision,
        edit: PerformanceEdit,
    },
    PluginPresets {
        plugin_id: String,
    },
    SavePluginPreset {
        instance_id: InstanceId,
        name: String,
    },
    LoadPluginPreset {
        instance_id: InstanceId,
        preset_id: String,
    },
    RenamePluginPreset {
        plugin_id: String,
        preset_id: String,
        name: String,
    },
    DeletePluginPreset {
        plugin_id: String,
        preset_id: String,
    },
    PluginPreset {
        plugin_id: String,
        preset_id: String,
    },
    ExportPluginPreset {
        plugin_id: String,
        preset_id: String,
    },
    InspectPluginPreset {
        target_plugin_id: String,
        file: Box<RfPresetFile>,
    },
    ImportPluginPreset {
        target_plugin_id: String,
        file: Box<RfPresetFile>,
        #[serde(default)]
        conflict_policy: PresetImportConflictPolicy,
    },
    /// Assembles the whole performance library and every referenced plugin
    /// state into one portable `.rflive` document.
    ExportLiveShow {
        name: String,
    },
    /// Validates a `.rflive` document against this machine without
    /// changing anything: what it holds, what it would replace, what is
    /// missing.
    InspectLiveShow {
        file: Box<RfLiveFile>,
    },
    /// Imports a `.rflive` document: stores the embedded states, then
    /// upserts every document through the library's own edit machinery.
    /// Existing entries with the same id are replaced; everything else is
    /// kept.
    ImportLiveShow {
        file: Box<RfLiveFile>,
    },
    /// Creates an immutable state snapshot in an isolated plugin instance.
    ///
    /// This is the safe path used by Rack Slot editors: selecting a sound for
    /// a draft must never mutate the standalone PLAY instance.
    MaterializePluginState {
        plugin_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sound_id: Option<String>,
    },
    PluginParameters {
        instance_id: InstanceId,
    },
    SetPluginParameter {
        instance_id: InstanceId,
        parameter_index: u32,
        value: f64,
    },
    /// Reads parameters from one immutable plugin state without touching PLAY.
    PluginStateParameters {
        state: PluginStateReference,
    },
    /// Produces a new immutable state after changing one isolated parameter.
    SetPluginStateParameter {
        state: PluginStateReference,
        parameter_index: u32,
        value: f64,
    },
    /// Lists stable MIDI identities, including disconnected saved endpoints.
    MidiSources,
    /// Arms a transient observer. No persisted link is changed by Learn.
    BeginMidiLearn {
        instance_id: String,
        parameter_index: u32,
    },
    MidiLearnStatus {
        learn_id: u64,
    },
    CancelMidiLearn {
        learn_id: u64,
    },
    LoadPluginResource {
        plugin_id: String,
        instance_id: InstanceId,
        resource_id: String,
        path: PathBuf,
        #[serde(default)]
        persist: bool,
        /// Replaces the live audio instance for audition only. The supplied
        /// path is not retained in the dynamic resource registry and the new
        /// resource chooses its own first playable sound.
        #[serde(default)]
        preview: bool,
    },
    /// Removes one installed private resource and reactivates the instance
    /// with the remaining resource set, restoring package defaults.
    ClearPluginResource {
        plugin_id: String,
        instance_id: InstanceId,
        resource_id: String,
    },
    AudioSnapshot,
    /// Drains the post-master peaks accumulated since the previous request.
    /// This is transient telemetry and never advances the session revision.
    OutputMeter,
    ApplyAudioOutput {
        profile: AudioOutputProfile,
    },
    /// Sends a transient channel-voice message from an authenticated UI.
    ///
    /// These messages never mutate the persisted session or advance its
    /// revision. The client identity exists so the host can release only the
    /// notes owned by a connection that disappears.
    VirtualMidi {
        client_id: ClientId,
        /// Physical input selected by an external controller driver.
        ///
        /// The host resolves this display name against its own approved MIDI
        /// source registry. UI-owned virtual keyboards leave it empty.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_name: Option<String>,
        message: VirtualMidiMessage,
    },
    ReleaseVirtualMidi {
        client_id: ClientId,
    },
    /// Drives the host sequencer. Transient like `VirtualMidi`: the pattern
    /// travels inline so the audio side never consults the library, and the
    /// session revision never advances.
    Sequencer {
        command: SequencerCommand,
    },
    /// Reads the transport and lane state for display. Transient telemetry,
    /// polled like `OutputMeter`.
    SequencerStatus,
    /// Drains the notes a lane captured since the last take. Transient,
    /// polled by the recording surface while its REC key is down.
    SequencerCaptureTake {
        lane: u8,
    },
    Events {
        after_revision: Revision,
    },
    Dispatch {
        envelope: CommandEnvelope,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlErrorCode {
    InvalidRequest,
    Conflict,
    NotFound,
    Rejected,
    Unavailable,
    Timeout,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlResponse {
    Snapshot {
        snapshot: Box<SessionState>,
    },
    PerformanceSnapshot {
        snapshot: Box<PerformanceSnapshot>,
    },
    PerformanceEdited {
        snapshot: Box<PerformanceSnapshot>,
    },
    PluginPresets {
        plugin_id: String,
        presets: Vec<HostPresetSummary>,
    },
    PluginPresetSaved {
        preset: Box<HostPreset>,
        presets: Vec<HostPresetSummary>,
    },
    PluginPresetLoaded {
        preset: Box<HostPreset>,
        revision: Revision,
    },
    PluginPresetRenamed {
        preset: Box<HostPreset>,
        presets: Vec<HostPresetSummary>,
    },
    PluginPresetDeleted {
        plugin_id: String,
        preset_id: String,
        presets: Vec<HostPresetSummary>,
    },
    PluginPreset {
        preset: Box<HostPreset>,
    },
    PluginPresetExported {
        file_name: String,
        file: Box<RfPresetFile>,
    },
    PluginPresetInspected {
        preview: Box<RfPresetImportPreview>,
    },
    LiveShowExported {
        file_name: String,
        file: Box<RfLiveFile>,
    },
    LiveShowInspected {
        preview: Box<RfLiveImportPreview>,
    },
    LiveShowImported {
        preview: Box<RfLiveImportPreview>,
        snapshot: Box<PerformanceSnapshot>,
    },
    PluginPresetImported {
        preset: Box<HostPreset>,
        presets: Vec<HostPresetSummary>,
    },
    PluginStateMaterialized {
        state: Box<PluginStateReference>,
    },
    PluginParameters {
        instance_id: InstanceId,
        schema: Box<ParameterSchema>,
        values: Vec<PluginParameterValue>,
    },
    PluginParameterSet {
        instance_id: InstanceId,
        parameter_index: u32,
        value: f64,
    },
    PluginStateParameters {
        state: Box<PluginStateReference>,
        schema: Box<ParameterSchema>,
        values: Vec<PluginParameterValue>,
    },
    PluginStateParameterSet {
        state: Box<PluginStateReference>,
        parameter_index: u32,
        value: f64,
    },
    MidiSources {
        sources: Vec<MidiSourceStatus>,
    },
    MidiLearnStarted {
        learn_id: u64,
    },
    MidiLearnStatus {
        learn_id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        candidate: Option<MidiLearnCandidate>,
    },
    MidiLearnCancelled {
        learn_id: u64,
    },
    PluginResourceLoaded {
        instance_id: InstanceId,
        resource_id: String,
    },
    PluginResourceCleared {
        instance_id: InstanceId,
        resource_id: String,
    },
    AudioSnapshot {
        snapshot: Box<AudioOutputState>,
    },
    OutputMeter {
        meter: OutputMeterSnapshot,
    },
    SequencerAccepted,
    SequencerStatus {
        sequencer: SequencerStatusV1,
    },
    SequencerCapture {
        notes: Vec<CapturedNoteV1>,
    },
    AudioApplied {
        snapshot: Box<AudioOutputState>,
    },
    VirtualMidiAccepted {
        client_id: ClientId,
        active_notes: u16,
    },
    VirtualMidiReleased {
        client_id: ClientId,
    },
    Events {
        current_revision: Revision,
        events: Vec<EventEnvelope>,
    },
    CommandApplied {
        client_id: ClientId,
        command_id: u64,
        revision: Revision,
        events: Vec<EventEnvelope>,
    },
    Error {
        code: ControlErrorCode,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        current_revision: Option<Revision>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginParameterValue {
    pub index: u32,
    pub value: f64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiSourceStatus {
    pub source: MidiSourceDescriptor,
    pub connected: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiLearnCandidate {
    pub source: MidiSourceDescriptor,
    pub channel: MidiChannel,
    pub message: ParameterLinkMessage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VirtualMidiMessage {
    pub status: u8,
    pub data1: u8,
    pub data2: u8,
}

impl VirtualMidiMessage {
    pub fn validate(self) -> Result<(), &'static str> {
        if self.data1 > 0x7f || self.data2 > 0x7f {
            return Err("MIDI data bytes must be in 0..=127");
        }
        match self.status & 0xf0 {
            // Three-byte channel voice messages. Virtual MIDI is also the
            // transport used by external controller packages that own their
            // WinMM endpoint, so CC, pressure and pitch bend must survive the
            // bridge for MIDI Learn and parameter links.
            0x80 | 0x90 | 0xa0 | 0xb0 | 0xe0 => Ok(()),
            _ => Err("virtual MIDI only accepts three-byte channel voice messages"),
        }
    }

    pub const fn channel(self) -> u8 {
        self.status & 0x0f
    }

    pub const fn note_on(self) -> Option<u8> {
        if self.status & 0xf0 == 0x90 && self.data2 != 0 {
            Some(self.data1)
        } else {
            None
        }
    }

    pub const fn note_off(self) -> Option<u8> {
        if self.status & 0xf0 == 0x80 || (self.status & 0xf0 == 0x90 && self.data2 == 0) {
            Some(self.data1)
        } else {
            None
        }
    }

    pub const fn is_sustain(self) -> bool {
        self.status & 0xf0 == 0xb0 && self.data1 == 64
    }

    pub const fn bytes(self) -> [u8; 3] {
        [self.status, self.data1, self.data2]
    }
}

pub fn encode_line<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn decode_request(bytes: &[u8]) -> Result<ControlRequest, serde_json::Error> {
    serde_json::from_slice(bytes)
}

pub fn decode_response(bytes: &[u8]) -> Result<ControlResponse, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// Client-side control transport, shared by every driver and tool.
///
/// The framed protocol (one JSON line in, one JSON line out per
/// connection) is transport-agnostic; what differs per platform is how a
/// process reaches the core. Linux hosts serve a Unix socket. Hosts on
/// every platform can serve TCP on loopback, and a spawned driver finds
/// it through `RACKFORGE_CONTROL_ADDR` (e.g. `127.0.0.1:52104`), which
/// takes precedence so a supervisor can always point its children at the
/// right core.
pub mod transport {
    use super::{ControlRequest, ControlResponse, encode_line};
    use std::io::{self, BufRead, BufReader, Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::path::PathBuf;
    use std::time::Duration;

    pub const CONTROL_ADDR_ENV: &str = "RACKFORGE_CONTROL_ADDR";
    const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
    const IO_TIMEOUT: Duration = Duration::from_secs(5);

    #[derive(Clone, Debug)]
    pub enum ControlEndpoint {
        Tcp(SocketAddr),
        #[cfg(unix)]
        Unix(PathBuf),
    }

    /// Reusable framed connection for latency-sensitive clients. Controller
    /// drivers use this to forward performance MIDI without opening one TCP
    /// connection for every note.
    pub struct ControlConnection {
        stream: ControlStream,
    }

    enum ControlStream {
        Tcp(TcpStream),
        #[cfg(unix)]
        Unix(std::os::unix::net::UnixStream),
    }

    impl ControlConnection {
        pub fn connect(endpoint: &ControlEndpoint) -> io::Result<Self> {
            let stream = match endpoint {
                ControlEndpoint::Tcp(address) => {
                    let stream = TcpStream::connect_timeout(address, IO_TIMEOUT)?;
                    stream.set_nodelay(true)?;
                    stream.set_read_timeout(Some(IO_TIMEOUT))?;
                    stream.set_write_timeout(Some(IO_TIMEOUT))?;
                    ControlStream::Tcp(stream)
                }
                #[cfg(unix)]
                ControlEndpoint::Unix(path) => {
                    let stream = std::os::unix::net::UnixStream::connect(path)?;
                    stream.set_read_timeout(Some(IO_TIMEOUT))?;
                    stream.set_write_timeout(Some(IO_TIMEOUT))?;
                    ControlStream::Unix(stream)
                }
            };
            Ok(Self { stream })
        }

        pub fn exchange(&mut self, request: &ControlRequest) -> io::Result<ControlResponse> {
            let line = encode_line(request)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            match &mut self.stream {
                ControlStream::Tcp(stream) => round_trip(stream, &line),
                #[cfg(unix)]
                ControlStream::Unix(stream) => round_trip(stream, &line),
            }
        }
    }

    /// Resolves the endpoint: `RACKFORGE_CONTROL_ADDR` wins everywhere;
    /// on Unix the caller's socket-path rule is the fallback. On other
    /// platforms the address is the only route, so its absence is an
    /// error the caller can report.
    pub fn endpoint_from_env(
        default_socket: impl FnOnce() -> PathBuf,
    ) -> io::Result<ControlEndpoint> {
        if let Some(address) = std::env::var_os(CONTROL_ADDR_ENV) {
            let text = address.to_string_lossy();
            let parsed: SocketAddr = text.parse().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{CONTROL_ADDR_ENV} {text:?} is not host:port: {error}"),
                )
            })?;
            return Ok(ControlEndpoint::Tcp(parsed));
        }
        #[cfg(unix)]
        {
            Ok(ControlEndpoint::Unix(default_socket()))
        }
        #[cfg(not(unix))]
        {
            let _ = default_socket;
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{CONTROL_ADDR_ENV} is not set and this platform has no control socket"),
            ))
        }
    }

    /// One control exchange: connect, send the request, read the response.
    pub fn exchange(
        endpoint: &ControlEndpoint,
        request: &ControlRequest,
    ) -> io::Result<ControlResponse> {
        let line = encode_line(request)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        match endpoint {
            ControlEndpoint::Tcp(address) => {
                let mut stream = TcpStream::connect_timeout(address, IO_TIMEOUT)?;
                stream.set_nodelay(true)?;
                stream.set_read_timeout(Some(IO_TIMEOUT))?;
                stream.set_write_timeout(Some(IO_TIMEOUT))?;
                round_trip(&mut stream, &line)
            }
            #[cfg(unix)]
            ControlEndpoint::Unix(path) => {
                let mut stream = std::os::unix::net::UnixStream::connect(path)?;
                stream.set_read_timeout(Some(IO_TIMEOUT))?;
                stream.set_write_timeout(Some(IO_TIMEOUT))?;
                round_trip(&mut stream, &line)
            }
        }
    }

    fn round_trip<S: Read + Write>(stream: &mut S, line: &[u8]) -> io::Result<ControlResponse> {
        stream.write_all(line)?;
        stream.flush()?;
        let mut bytes = Vec::new();
        BufReader::new(stream)
            .take(MAX_RESPONSE_BYTES)
            .read_until(b'\n', &mut bytes)?;
        serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_session_api::{
        DEFAULT_LIVE_INSTANCE_ID, DEFAULT_LIVE_SESSION_ID, SESSION_SCHEMA_VERSION,
    };

    fn instance_id() -> InstanceId {
        InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).unwrap()
    }

    #[test]
    fn command_and_snapshot_round_trip() {
        let request = ControlRequest::Dispatch {
            envelope: CommandEnvelope::new(
                ClientId::new("test.control").unwrap(),
                41,
                SessionCommand::SelectSound {
                    instance_id: instance_id(),
                    sound_id: "dls.b00000000.p00000048".into(),
                },
            ),
        };
        let encoded = encode_line(&request).unwrap();
        assert_eq!(decode_request(&encoded).unwrap(), request);

        let response = ControlResponse::Snapshot {
            snapshot: Box::new(SessionState {
                schema_version: SESSION_SCHEMA_VERSION,
                session_id: SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
                revision: Revision::ZERO,
                active_mode: rackforge_session_api::SurfaceMode::Live,
                master_level: rackforge_session_api::MasterLevel::UNITY,
                master_pan: rackforge_session_api::MasterPan::CENTER,
                live: rackforge_session_api::LivePerformanceState::default(),
                active_instance_id: Some(instance_id()),
                instances: vec![PluginInstanceState {
                    instance_id: instance_id(),
                    plugin_id: "org.rackforge.rf-dls".into(),
                    plugin_name: "RF-DLS".into(),
                    plugin_short_name: "RF-DLS".into(),
                    ui_layouts: vec!["little@1".into()],
                    config_available: true,
                    banks: Vec::new(),
                    sounds: vec![SoundSummary {
                        id: "dls.b00000000.p00000000".into(),
                        name: "Piano 1".into(),
                        bank: Some("dls".into()),
                        detail: Some("B000 P000".into()),
                        category: None,
                        tags: Vec::new(),
                        editable: false,
                    }],
                    selected_sound_id: Some("dls.b00000000.p00000000".into()),
                }],
                audition: None,
                program_draft: None,
                parameter_links: Vec::new(),
            }),
        };
        assert_eq!(
            decode_response(&encode_line(&response).unwrap()).unwrap(),
            response
        );
    }

    #[test]
    fn virtual_midi_requests_are_strict_and_round_trip() {
        let client_id = ClientId::new("web.touch.test-1").unwrap();
        let request = ControlRequest::VirtualMidi {
            client_id: client_id.clone(),
            source_name: None,
            message: VirtualMidiMessage {
                status: 0x90,
                data1: 60,
                data2: 100,
            },
        };
        assert_eq!(
            decode_request(&encode_line(&request).unwrap()).unwrap(),
            request
        );
        assert!(
            VirtualMidiMessage {
                status: 0x90,
                data1: 60,
                data2: 100
            }
            .validate()
            .is_ok()
        );
        assert!(
            VirtualMidiMessage {
                status: 0xb0,
                data1: 64,
                data2: 127
            }
            .validate()
            .is_ok()
        );
        assert!(
            VirtualMidiMessage {
                status: 0xb0,
                data1: 1,
                data2: 127
            }
            .validate()
            .is_ok()
        );
        assert!(
            VirtualMidiMessage {
                status: 0xe0,
                data1: 0,
                data2: 64
            }
            .validate()
            .is_ok()
        );
        assert!(
            VirtualMidiMessage {
                status: 0x90,
                data1: 128,
                data2: 100
            }
            .validate()
            .is_err()
        );

        let release = ControlRequest::ReleaseVirtualMidi { client_id };
        assert_eq!(
            decode_request(&encode_line(&release).unwrap()).unwrap(),
            release
        );
    }

    #[test]
    fn event_queries_round_trip() {
        let request = ControlRequest::Events {
            after_revision: Revision::ZERO,
        };
        assert_eq!(
            decode_request(&encode_line(&request).unwrap()).unwrap(),
            request
        );
    }

    #[test]
    fn audio_requests_round_trip() {
        let request = ControlRequest::AudioSnapshot;
        assert_eq!(
            decode_request(&encode_line(&request).unwrap()).unwrap(),
            request
        );
        let meter_request = ControlRequest::OutputMeter;
        assert_eq!(
            decode_request(&encode_line(&meter_request).unwrap()).unwrap(),
            meter_request
        );
        let meter_response = ControlResponse::OutputMeter {
            meter: OutputMeterSnapshot {
                left_peak: 0.75,
                right_peak: 1.02,
            },
        };
        assert_eq!(
            serde_json::from_slice::<ControlResponse>(
                &serde_json::to_vec(&meter_response).unwrap()
            )
            .unwrap(),
            meter_response
        );
    }

    #[test]
    fn preset_mutation_requests_round_trip() {
        let rename = ControlRequest::RenamePluginPreset {
            plugin_id: "org.rackforge.rf-dls".into(),
            preset_id: "warm-piano-1234".into(),
            name: "Stage Piano".into(),
        };
        assert_eq!(
            decode_request(&encode_line(&rename).unwrap()).unwrap(),
            rename
        );

        let delete = ControlRequest::DeletePluginPreset {
            plugin_id: "org.rackforge.rf-dls".into(),
            preset_id: "warm-piano-1234".into(),
        };
        assert_eq!(
            decode_request(&encode_line(&delete).unwrap()).unwrap(),
            delete
        );
    }

    #[test]
    fn portable_preset_requests_are_strict_and_round_trip() {
        let state = PluginStateReference {
            schema_version: 1,
            plugin_id: "org.rackforge.rf-dls".into(),
            plugin_version: "1.2.3".into(),
            state_version: 4,
            blob_sha256: "a".repeat(64),
            byte_length: 3,
            selected_sound_id: Some("grand.piano".into()),
        };
        let preset = HostPreset {
            schema_version: 1,
            id: "stage-grand".into(),
            name: "Stage Grand".into(),
            plugin_id: state.plugin_id.clone(),
            created_unix_ms: 1,
            updated_unix_ms: 2,
            state,
        };
        let file = RfPresetFile {
            format: RFPRESET_FORMAT.into(),
            schema_version: RFPRESET_SCHEMA_VERSION,
            exported_by: "RackForge test".into(),
            exported_unix_ms: 3,
            preset,
            state_encoding: RfPresetStateEncoding::Base64,
            state_base64: "YWJj".into(),
        };
        let inspect = ControlRequest::InspectPluginPreset {
            target_plugin_id: "org.rackforge.rf-dls".into(),
            file: Box::new(file.clone()),
        };
        assert_eq!(
            decode_request(&encode_line(&inspect).unwrap()).unwrap(),
            inspect
        );
        let import = ControlRequest::ImportPluginPreset {
            target_plugin_id: "org.rackforge.rf-dls".into(),
            file: Box::new(file),
            conflict_policy: PresetImportConflictPolicy::KeepBoth,
        };
        assert_eq!(
            decode_request(&encode_line(&import).unwrap()).unwrap(),
            import
        );
    }

    #[test]
    fn plugin_parameter_requests_round_trip() {
        let read = ControlRequest::PluginParameters {
            instance_id: instance_id(),
        };
        assert_eq!(decode_request(&encode_line(&read).unwrap()).unwrap(), read);

        let write = ControlRequest::SetPluginParameter {
            instance_id: instance_id(),
            parameter_index: 3,
            value: 0.625,
        };
        assert_eq!(
            decode_request(&encode_line(&write).unwrap()).unwrap(),
            write
        );

        let schema: ParameterSchema = serde_json::from_str(
            r#"{
              "schema_version":1,
              "pages":[{"id":"filter","name":"Filter","order":0}],
              "parameters":[{
                "index":3,"id":"cutoff","name":"Cutoff","page":"filter","order":0,
                "kind":{"type":"float","minimum":0.0,"maximum":1.0,"default":0.5,"step":0.01},
                "flags":{},"suggested_control":"knob"
              }]
            }"#,
        )
        .unwrap();
        let response = ControlResponse::PluginParameters {
            instance_id: instance_id(),
            schema: Box::new(schema),
            values: vec![PluginParameterValue {
                index: 3,
                value: 0.625,
            }],
        };
        assert_eq!(
            decode_response(&encode_line(&response).unwrap()).unwrap(),
            response
        );
    }

    #[test]
    fn isolated_plugin_state_requests_round_trip() {
        let materialize = ControlRequest::MaterializePluginState {
            plugin_id: "org.rackforge.rf-m1".into(),
            sound_id: Some("m1.piano.01".into()),
        };
        assert_eq!(
            decode_request(&encode_line(&materialize).unwrap()).unwrap(),
            materialize
        );

        let state = PluginStateReference {
            schema_version: 1,
            plugin_id: "org.rackforge.rf-m1".into(),
            plugin_version: "0.1.3".into(),
            state_version: 2,
            blob_sha256: "a".repeat(64),
            byte_length: 42,
            selected_sound_id: Some("m1.piano.01".into()),
        };
        let read = ControlRequest::PluginStateParameters {
            state: state.clone(),
        };
        assert_eq!(decode_request(&encode_line(&read).unwrap()).unwrap(), read);
        let write = ControlRequest::SetPluginStateParameter {
            state: state.clone(),
            parameter_index: 7,
            value: 0.75,
        };
        assert_eq!(
            decode_request(&encode_line(&write).unwrap()).unwrap(),
            write
        );

        let response = ControlResponse::PluginStateParameterSet {
            state: Box::new(state),
            parameter_index: 7,
            value: 0.75,
        };
        assert_eq!(
            decode_response(&encode_line(&response).unwrap()).unwrap(),
            response
        );
    }

    #[test]
    fn plugin_resource_requests_round_trip_with_owner() {
        let request = ControlRequest::LoadPluginResource {
            plugin_id: "org.rackforge.rf-soundfonts".into(),
            instance_id: instance_id(),
            resource_id: "factory-soundfont".into(),
            path: PathBuf::from("/authorized/library/piano.sf2"),
            persist: true,
            preview: false,
        };
        assert_eq!(
            decode_request(&encode_line(&request).unwrap()).unwrap(),
            request
        );

        let response = ControlResponse::PluginResourceLoaded {
            instance_id: instance_id(),
            resource_id: "factory-soundfont".into(),
        };
        assert_eq!(
            decode_response(&encode_line(&response).unwrap()).unwrap(),
            response
        );
    }

    #[test]
    fn performance_snapshot_request_round_trips() {
        let request = ControlRequest::PerformanceSnapshot;
        assert_eq!(
            decode_request(&encode_line(&request).unwrap()).unwrap(),
            request
        );
    }

    #[test]
    fn transient_program_preview_commands_round_trip() {
        let preview = ControlRequest::Dispatch {
            envelope: CommandEnvelope::new(
                ClientId::new("test.preview").unwrap(),
                51,
                SessionCommand::PreviewProgramDraft {
                    draft_id: 7,
                    document_json: r#"{"payload":{"gain":0.75}}"#.into(),
                },
            ),
        };
        assert_eq!(
            decode_request(&encode_line(&preview).unwrap()).unwrap(),
            preview
        );

        let restore = ControlRequest::Dispatch {
            envelope: CommandEnvelope::new(
                ClientId::new("test.preview").unwrap(),
                52,
                SessionCommand::RestoreProgramDraftPreview { draft_id: 7 },
            ),
        };
        assert_eq!(
            decode_request(&encode_line(&restore).unwrap()).unwrap(),
            restore
        );
    }

    #[test]
    fn play_ownership_commands_round_trip_as_one_contract() {
        let commands = [
            SessionCommand::CancelProgramEdit { draft_id: 19 },
            SessionCommand::SetActiveMode {
                mode: rackforge_session_api::SurfaceMode::Play,
            },
            SessionCommand::SelectPlugin {
                instance_id: InstanceId::new("desktop.org.rackforge.rf-m1").unwrap(),
            },
        ];

        for (index, command) in commands.into_iter().enumerate() {
            let request = ControlRequest::Dispatch {
                envelope: CommandEnvelope::new(
                    ClientId::new("test.play-ownership").unwrap(),
                    index as u64 + 1,
                    command,
                ),
            };
            assert_eq!(
                decode_request(&encode_line(&request).unwrap()).unwrap(),
                request
            );
        }
    }

    #[test]
    fn virtual_midi_note_and_release_requests_round_trip() {
        let client_id = ClientId::new("test.controller-midi").unwrap();
        for request in [
            ControlRequest::VirtualMidi {
                client_id: client_id.clone(),
                source_name: Some("Stage Controller".into()),
                message: VirtualMidiMessage {
                    status: 0x90,
                    data1: 60,
                    data2: 100,
                },
            },
            ControlRequest::ReleaseVirtualMidi { client_id },
        ] {
            assert_eq!(
                decode_request(&encode_line(&request).unwrap()).unwrap(),
                request
            );
        }
    }

    #[test]
    fn midi_source_and_learn_contract_round_trips() {
        let source = MidiSourceDescriptor {
            id: rackforge_midi_api::MidiSourceId::new("usb.controller.main").unwrap(),
            name: "Stage Controller".into(),
            primary: true,
        };
        let requests = [
            ControlRequest::MidiSources,
            ControlRequest::BeginMidiLearn {
                instance_id: "live.main.instrument.1".into(),
                parameter_index: 17,
            },
            ControlRequest::MidiLearnStatus { learn_id: 9 },
            ControlRequest::CancelMidiLearn { learn_id: 9 },
        ];
        for request in requests {
            assert_eq!(
                decode_request(&encode_line(&request).unwrap()).unwrap(),
                request
            );
        }
        let response = ControlResponse::MidiLearnStatus {
            learn_id: 9,
            candidate: Some(MidiLearnCandidate {
                source,
                channel: MidiChannel::from_user_number(2).unwrap(),
                message: ParameterLinkMessage::ControlChange { controller: 74 },
            }),
        };
        assert_eq!(
            decode_response(&encode_line(&response).unwrap()).unwrap(),
            response
        );
    }

    #[test]
    fn parses_host_plugin_parameter_read_and_write_commands() {
        assert_eq!(
            parse_plugin_parameter_control_command(
                "plugin_parameters",
                "android-main",
                &serde_json::json!({"instance_id": "android-main"}),
            )
            .unwrap(),
            PluginParameterControlCommand::Read {
                instance_id: InstanceId::new("android-main").unwrap(),
            }
        );
        assert_eq!(
            parse_plugin_parameter_control_command(
                "set_plugin_parameter",
                "android-main",
                &serde_json::json!({
                    "instance_id": "android-main",
                    "parameter_index": 123,
                    "value": 0.625,
                }),
            )
            .unwrap(),
            PluginParameterControlCommand::Set {
                instance_id: InstanceId::new("android-main").unwrap(),
                parameter_index: 123,
                value: 0.625,
            }
        );
    }

    #[test]
    fn rejects_plugin_parameter_commands_for_another_host_instance() {
        assert!(
            parse_plugin_parameter_control_command(
                "plugin_parameters",
                "android-main",
                &serde_json::json!({"instance_id": "desktop.org.rackforge.rf-106"}),
            )
            .unwrap_err()
            .contains("not the active host instance")
        );
    }

    #[test]
    fn rejects_malformed_plugin_parameter_commands() {
        for parameter_index in [
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!(u64::from(u32::MAX) + 1),
        ] {
            assert!(
                parse_plugin_parameter_control_command(
                    "set_plugin_parameter",
                    "android-main",
                    &serde_json::json!({
                        "instance_id": "android-main",
                        "parameter_index": parameter_index,
                        "value": 0.5,
                    }),
                )
                .unwrap_err()
                .contains("invalid parameter_index")
            );
        }
        for value in [
            serde_json::Value::Null,
            serde_json::json!("NaN"),
            serde_json::json!(true),
        ] {
            assert!(
                parse_plugin_parameter_control_command(
                    "set_plugin_parameter",
                    "android-main",
                    &serde_json::json!({
                        "instance_id": "android-main",
                        "parameter_index": 1,
                        "value": value,
                    }),
                )
                .unwrap_err()
                .contains("invalid value")
            );
        }
    }
}
