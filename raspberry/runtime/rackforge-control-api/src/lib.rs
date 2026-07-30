use serde::{Deserialize, Serialize};

pub const CONTROL_SCHEMA_VERSION: u32 = 1;
pub const CONTROL_SOCKET_NAME: &str = "live-control.sock";
pub const MAX_CONTROL_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoundSummary {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bank: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveSnapshot {
    pub schema_version: u32,
    pub plugin_id: String,
    pub plugin_name: String,
    #[serde(default)]
    pub ui_layouts: Vec<String>,
    pub sounds: Vec<SoundSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_sound_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlRequest {
    Snapshot,
    SelectSound { id: String },
    BeginAudition { plugin_id: String },
    KeepAuditionAlive { lease_id: u64 },
    EndAudition { lease_id: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlResponse {
    Snapshot { snapshot: LiveSnapshot },
    Ok { selected_sound_id: String },
    AuditionGranted { lease_id: u64 },
    AuditionKeptAlive { lease_id: u64 },
    AuditionEnded,
    Error { message: String },
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

    #[test]
    fn request_and_snapshot_round_trip() {
        let request = ControlRequest::SelectSound {
            id: "dls.b00000000.p00000048".into(),
        };
        let encoded = encode_line(&request).unwrap();
        assert_eq!(
            decode_request(&encoded).unwrap(),
            ControlRequest::SelectSound {
                id: "dls.b00000000.p00000048".into()
            }
        );

        let response = ControlResponse::Snapshot {
            snapshot: LiveSnapshot {
                schema_version: CONTROL_SCHEMA_VERSION,
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
            },
        };
        assert_eq!(
            decode_response(&encode_line(&response).unwrap()).unwrap(),
            response
        );
    }

    #[test]
    fn audition_lease_requests_round_trip() {
        for request in [
            ControlRequest::BeginAudition {
                plugin_id: "org.rackforge.rf-dls".into(),
            },
            ControlRequest::KeepAuditionAlive { lease_id: 42 },
            ControlRequest::EndAudition { lease_id: 42 },
        ] {
            assert_eq!(
                decode_request(&encode_line(&request).unwrap()).unwrap(),
                request
            );
        }
    }
}
