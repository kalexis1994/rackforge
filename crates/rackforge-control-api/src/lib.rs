pub use rackforge_audio_api::{AudioOutputProfile, AudioOutputState};
pub use rackforge_performance_api::{LibraryRevision, PerformanceEdit, PerformanceSnapshot};
pub use rackforge_plugin_api::{HostPreset, HostPresetSummary, ParameterSchema};
use serde::{Deserialize, Serialize};

pub use rackforge_session_api::{
    AuditionEndReason, AuditionState, ClientId, CommandEnvelope, CommandRef, EventEnvelope,
    InstanceId, PluginInstanceState, ProgramDraftState, Revision, SESSION_SCHEMA_VERSION,
    SessionCommand, SessionEvent, SessionId, SessionState, SoundSummary, SurfaceActivationReason,
    SurfaceActivationRequest, SurfaceActivationResponse, SurfaceMode,
};

pub const CONTROL_SCHEMA_VERSION: u32 = 9;
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
    PluginParameters {
        instance_id: InstanceId,
    },
    SetPluginParameter {
        instance_id: InstanceId,
        parameter_index: u32,
        value: f64,
    },
    AudioSnapshot,
    ApplyAudioOutput {
        profile: AudioOutputProfile,
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
    AudioSnapshot {
        snapshot: Box<AudioOutputState>,
    },
    AudioApplied {
        snapshot: Box<AudioOutputState>,
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
