pub use rackforge_audio_api::{AudioOutputProfile, AudioOutputState};
pub use rackforge_performance_api::{LibraryRevision, PerformanceEdit, PerformanceSnapshot};
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

pub const CONTROL_SCHEMA_VERSION: u32 = 13;
pub const CONTROL_SOCKET_NAME: &str = "live-control.sock";
pub const MAX_CONTROL_MESSAGE_BYTES: usize = 64 * 1024;

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
    LoadPluginResource {
        plugin_id: String,
        instance_id: InstanceId,
        resource_id: String,
        path: PathBuf,
        #[serde(default)]
        persist: bool,
    },
    AudioSnapshot,
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
        message: VirtualMidiMessage,
    },
    ReleaseVirtualMidi {
        client_id: ClientId,
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
    PluginResourceLoaded {
        instance_id: InstanceId,
        resource_id: String,
    },
    AudioSnapshot {
        snapshot: Box<AudioOutputState>,
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
            0x80 | 0x90 => Ok(()),
            0xb0 if self.data1 == 64 => Ok(()),
            0xb0 => Err("the touch controller only accepts sustain control changes"),
            _ => Err("the touch controller only accepts note and sustain messages"),
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
            .is_err()
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
}
