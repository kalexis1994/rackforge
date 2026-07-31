pub use rackforge_audio_api::{AudioOutputProfile, AudioOutputState};
use serde::{Deserialize, Serialize};

pub use rackforge_session_api::{
    AuditionEndReason, AuditionState, ClientId, CommandEnvelope, CommandRef, EventEnvelope,
    InstanceId, PluginInstanceState, ProgramDraftState, Revision, SESSION_SCHEMA_VERSION,
    SessionCommand, SessionEvent, SessionId, SessionState, SoundSummary, SurfaceActivationReason,
    SurfaceActivationRequest, SurfaceActivationResponse, SurfaceMode,
};

pub const CONTROL_SCHEMA_VERSION: u32 = 2;
pub const CONTROL_SOCKET_NAME: &str = "live-control.sock";
pub const MAX_CONTROL_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlRequest {
    Snapshot,
    AudioSnapshot,
    ApplyAudioOutput { profile: AudioOutputProfile },
    Events { after_revision: Revision },
    Dispatch { envelope: CommandEnvelope },
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlResponse {
    Snapshot {
        snapshot: Box<SessionState>,
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
                active_instance_id: Some(instance_id()),
                instances: vec![PluginInstanceState {
                    instance_id: instance_id(),
                    plugin_id: "org.rackforge.rf-dls".into(),
                    plugin_name: "RF-DLS".into(),
                    ui_layouts: vec!["little@1".into()],
                    sounds: vec![SoundSummary {
                        id: "dls.b00000000.p00000000".into(),
                        name: "Piano 1".into(),
                        bank: Some("dls".into()),
                        detail: Some("B000 P000".into()),
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
