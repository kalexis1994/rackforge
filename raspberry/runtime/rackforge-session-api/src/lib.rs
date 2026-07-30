use serde::{Deserialize, Serialize};
use std::fmt;

pub const SESSION_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_LIVE_SESSION_ID: &str = "live.main";
pub const DEFAULT_LIVE_INSTANCE_ID: &str = "live.main.instrument.1";

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, String> {
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

string_id!(SessionId);
string_id!(InstanceId);
string_id!(ClientId);

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);

impl Revision {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Result<Self, String> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| "session revision overflow".into())
    }
}

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
pub struct AddonInstanceState {
    pub instance_id: InstanceId,
    pub addon_id: String,
    pub addon_name: String,
    #[serde(default)]
    pub ui_layouts: Vec<String>,
    #[serde(default)]
    pub sounds: Vec<SoundSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_sound_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditionState {
    pub lease_id: u64,
    pub instance_id: InstanceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_sound_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionState {
    pub schema_version: u32,
    pub session_id: SessionId,
    pub revision: Revision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_instance_id: Option<InstanceId>,
    #[serde(default)]
    pub instances: Vec<AddonInstanceState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audition: Option<AuditionState>,
}

impl SessionState {
    pub fn new(session_id: SessionId) -> Self {
        Self {
            schema_version: SESSION_SCHEMA_VERSION,
            session_id,
            revision: Revision::ZERO,
            active_instance_id: None,
            instances: Vec::new(),
            audition: None,
        }
    }

    pub fn instance(&self, id: &InstanceId) -> Option<&AddonInstanceState> {
        self.instances
            .iter()
            .find(|instance| &instance.instance_id == id)
    }

    pub fn active_instance(&self) -> Option<&AddonInstanceState> {
        self.active_instance_id
            .as_ref()
            .and_then(|id| self.instance(id))
    }

    pub fn apply(&mut self, envelope: &EventEnvelope) -> Result<(), String> {
        if envelope.schema_version != SESSION_SCHEMA_VERSION {
            return Err(format!(
                "unsupported event schema version {}",
                envelope.schema_version
            ));
        }
        let expected = self.revision.next()?;
        if envelope.revision != expected {
            return Err(format!(
                "event revision {} does not follow {}",
                envelope.revision.get(),
                self.revision.get()
            ));
        }

        match &envelope.event {
            SessionEvent::SoundSelected {
                instance_id,
                sound_id,
            } => {
                let instance = self
                    .instances
                    .iter_mut()
                    .find(|instance| &instance.instance_id == instance_id)
                    .ok_or_else(|| format!("unknown instance {instance_id}"))?;
                if !instance.sounds.iter().any(|sound| sound.id == *sound_id) {
                    return Err(format!(
                        "unknown sound {sound_id:?} for instance {instance_id}"
                    ));
                }
                instance.selected_sound_id = Some(sound_id.clone());
            }
            SessionEvent::AuditionStarted {
                lease_id,
                instance_id,
                previous_sound_id,
            } => {
                if self.audition.is_some() {
                    return Err("audition focus is already leased".into());
                }
                if self.instance(instance_id).is_none() {
                    return Err(format!("unknown instance {instance_id}"));
                }
                self.audition = Some(AuditionState {
                    lease_id: *lease_id,
                    instance_id: instance_id.clone(),
                    previous_sound_id: previous_sound_id.clone(),
                });
            }
            SessionEvent::AuditionEnded {
                lease_id,
                instance_id,
                restored_sound_id,
                ..
            } => {
                let audition = self
                    .audition
                    .as_ref()
                    .ok_or_else(|| "audition focus is not leased".to_owned())?;
                if audition.lease_id != *lease_id || audition.instance_id != *instance_id {
                    return Err("audition lease does not match the active lease".into());
                }
                let instance = self
                    .instances
                    .iter_mut()
                    .find(|instance| &instance.instance_id == instance_id)
                    .ok_or_else(|| format!("unknown instance {instance_id}"))?;
                instance.selected_sound_id = restored_sound_id.clone();
                self.audition = None;
            }
        }
        self.revision = envelope.revision;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionCommand {
    SelectSound {
        instance_id: InstanceId,
        sound_id: String,
    },
    BeginAudition {
        instance_id: InstanceId,
    },
    KeepAuditionAlive {
        lease_id: u64,
    },
    EndAudition {
        lease_id: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEnvelope {
    pub schema_version: u32,
    pub client_id: ClientId,
    pub command_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<Revision>,
    pub command: SessionCommand,
}

impl CommandEnvelope {
    pub fn new(client_id: ClientId, command_id: u64, command: SessionCommand) -> Self {
        Self {
            schema_version: SESSION_SCHEMA_VERSION,
            client_id,
            command_id,
            expected_revision: None,
            command,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandRef {
    pub client_id: ClientId,
    pub command_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditionEndReason {
    Released,
    Expired,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionEvent {
    SoundSelected {
        instance_id: InstanceId,
        sound_id: String,
    },
    AuditionStarted {
        lease_id: u64,
        instance_id: InstanceId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous_sound_id: Option<String>,
    },
    AuditionEnded {
        lease_id: u64,
        instance_id: InstanceId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restored_sound_id: Option<String>,
        reason: AuditionEndReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventEnvelope {
    pub schema_version: u32,
    pub revision: Revision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandRef>,
    pub event: SessionEvent,
}

fn validate_identifier(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        })
    {
        return Err(format!("invalid identifier {value:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> SessionState {
        let instance_id = InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).unwrap();
        SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            session_id: SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            revision: Revision::ZERO,
            active_instance_id: Some(instance_id.clone()),
            instances: vec![AddonInstanceState {
                instance_id,
                addon_id: "org.rackforge.rf-dls".into(),
                addon_name: "RF-DLS".into(),
                ui_layouts: vec!["little@1".into()],
                sounds: vec![SoundSummary {
                    id: "dls.piano-1".into(),
                    name: "Piano 1".into(),
                    bank: Some("dls".into()),
                    detail: None,
                }],
                selected_sound_id: None,
            }],
            audition: None,
        }
    }

    fn event(revision: u64, event: SessionEvent) -> EventEnvelope {
        EventEnvelope {
            schema_version: SESSION_SCHEMA_VERSION,
            revision: Revision(revision),
            command: Some(CommandRef {
                client_id: ClientId::new("test.session").unwrap(),
                command_id: revision,
            }),
            event,
        }
    }

    #[test]
    fn applies_events_only_in_monotonic_order() {
        let mut session = session();
        let instance_id = session.active_instance_id.clone().unwrap();
        session
            .apply(&event(
                1,
                SessionEvent::SoundSelected {
                    instance_id: instance_id.clone(),
                    sound_id: "dls.piano-1".into(),
                },
            ))
            .unwrap();
        assert_eq!(session.revision, Revision::new(1));
        assert!(
            session
                .apply(&event(
                    3,
                    SessionEvent::SoundSelected {
                        instance_id,
                        sound_id: "dls.piano-1".into(),
                    }
                ))
                .is_err()
        );
    }

    #[test]
    fn audition_restores_the_previous_sound() {
        let mut session = session();
        let instance_id = session.active_instance_id.clone().unwrap();
        session
            .apply(&event(
                1,
                SessionEvent::SoundSelected {
                    instance_id: instance_id.clone(),
                    sound_id: "dls.piano-1".into(),
                },
            ))
            .unwrap();
        session
            .apply(&event(
                2,
                SessionEvent::AuditionStarted {
                    lease_id: 7,
                    instance_id: instance_id.clone(),
                    previous_sound_id: Some("dls.piano-1".into()),
                },
            ))
            .unwrap();
        session
            .apply(&event(
                3,
                SessionEvent::AuditionEnded {
                    lease_id: 7,
                    instance_id,
                    restored_sound_id: Some("dls.piano-1".into()),
                    reason: AuditionEndReason::Released,
                },
            ))
            .unwrap();
        assert!(session.audition.is_none());
        assert_eq!(
            session
                .active_instance()
                .unwrap()
                .selected_sound_id
                .as_deref(),
            Some("dls.piano-1")
        );
    }
}
