use rackforge_program_api::{ProgramEditorValue, ProgramEditorView};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

pub use rackforge_controller_api::{
    HostControlBinding, HostControlTarget, MidiControlChangeBinding,
};
pub use rackforge_surface_api::{
    SurfaceActivationReason, SurfaceActivationRequest, SurfaceActivationResponse, SurfaceMode,
};

pub const SESSION_SCHEMA_VERSION: u32 = 6;

fn default_surface_mode() -> SurfaceMode {
    SurfaceMode::Live
}
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MasterLevel(u16);

impl MasterLevel {
    pub const MAX: u16 = 1_000;
    pub const SILENT: Self = Self(0);
    pub const UNITY: Self = Self(Self::MAX);

    pub fn new(value: u16) -> Result<Self, String> {
        if value > Self::MAX {
            return Err(format!(
                "master level {value} exceeds maximum {}",
                Self::MAX
            ));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub fn from_midi(value: u8) -> Self {
        Self((u32::from(value) * u32::from(Self::MAX) / 127) as u16)
    }

    pub fn amplitude(self) -> f32 {
        let normalized = f32::from(self.0) / f32::from(Self::MAX);
        normalized * normalized
    }
}

impl Default for MasterLevel {
    fn default() -> Self {
        Self::UNITY
    }
}

impl<'de> Deserialize<'de> for MasterLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MasterPan(i16);

impl MasterPan {
    pub const MAX: i16 = 1_000;
    pub const LEFT: Self = Self(-Self::MAX);
    pub const CENTER: Self = Self(0);
    pub const RIGHT: Self = Self(Self::MAX);
    pub const MIDI_SNAP_LOW: u8 = 60;
    pub const MIDI_SNAP_HIGH: u8 = 68;

    pub fn new(value: i16) -> Result<Self, String> {
        if !(-Self::MAX..=Self::MAX).contains(&value) {
            return Err(format!(
                "master pan {value} is outside -{}..={}",
                Self::MAX,
                Self::MAX
            ));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> i16 {
        self.0
    }

    pub fn from_midi_with_center_snap(value: u8) -> Self {
        if value < Self::MIDI_SNAP_LOW {
            let distance = i32::from(Self::MIDI_SNAP_LOW - value);
            let range = i32::from(Self::MIDI_SNAP_LOW);
            return Self((-(distance * i32::from(Self::MAX) / range)) as i16);
        }
        if value > Self::MIDI_SNAP_HIGH {
            let distance = i32::from(value - Self::MIDI_SNAP_HIGH);
            let range = i32::from(127 - Self::MIDI_SNAP_HIGH);
            return Self((distance * i32::from(Self::MAX) / range) as i16);
        }
        Self::CENTER
    }

    pub fn balance(self) -> (f32, f32) {
        let normalized = f32::from(self.0) / f32::from(Self::MAX);
        if normalized < 0.0 {
            (1.0, 1.0 + normalized)
        } else {
            (1.0 - normalized, 1.0)
        }
    }
}

impl Default for MasterPan {
    fn default() -> Self {
        Self::CENTER
    }
}

impl<'de> Deserialize<'de> for MasterPan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = i16::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

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
pub struct PluginInstanceState {
    pub instance_id: InstanceId,
    #[serde(alias = "addon_id")]
    pub plugin_id: String,
    #[serde(alias = "addon_name")]
    pub plugin_name: String,
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
pub struct ProgramDraftState {
    pub draft_id: u64,
    pub instance_id: InstanceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_program_id: Option<String>,
    pub name: String,
    pub preview_sound_id: String,
    pub storage_path: String,
    pub document_json: String,
    pub editor: ProgramEditorView,
    pub dirty: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionState {
    pub schema_version: u32,
    pub session_id: SessionId,
    pub revision: Revision,
    #[serde(default = "default_surface_mode")]
    pub active_mode: SurfaceMode,
    #[serde(default)]
    pub master_level: MasterLevel,
    #[serde(default)]
    pub master_pan: MasterPan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_instance_id: Option<InstanceId>,
    #[serde(default)]
    pub instances: Vec<PluginInstanceState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audition: Option<AuditionState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_draft: Option<ProgramDraftState>,
}

impl SessionState {
    pub fn new(session_id: SessionId) -> Self {
        Self {
            schema_version: SESSION_SCHEMA_VERSION,
            session_id,
            revision: Revision::ZERO,
            active_mode: SurfaceMode::Live,
            master_level: MasterLevel::UNITY,
            master_pan: MasterPan::CENTER,
            active_instance_id: None,
            instances: Vec::new(),
            audition: None,
            program_draft: None,
        }
    }

    pub fn instance(&self, id: &InstanceId) -> Option<&PluginInstanceState> {
        self.instances
            .iter()
            .find(|instance| &instance.instance_id == id)
    }

    pub fn active_instance(&self) -> Option<&PluginInstanceState> {
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
            SessionEvent::MasterLevelChanged { level } => {
                self.master_level = *level;
            }
            SessionEvent::MasterPanChanged { pan } => {
                self.master_pan = *pan;
            }
            SessionEvent::ActiveModeChanged { mode } => {
                self.active_mode = *mode;
            }
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
                if self
                    .program_draft
                    .as_ref()
                    .is_some_and(|draft| draft.instance_id == *instance_id)
                {
                    return Err(
                        "program draft must be saved or cancelled before audition ends".into(),
                    );
                }
                let instance = self
                    .instances
                    .iter_mut()
                    .find(|instance| &instance.instance_id == instance_id)
                    .ok_or_else(|| format!("unknown instance {instance_id}"))?;
                instance.selected_sound_id = restored_sound_id.clone();
                self.audition = None;
            }
            SessionEvent::ProgramEditStarted { draft } => {
                if self.program_draft.is_some() {
                    return Err("a program draft is already active".into());
                }
                if self
                    .audition
                    .as_ref()
                    .is_none_or(|audition| audition.instance_id != draft.instance_id)
                {
                    return Err("program draft requires matching audition focus".into());
                }
                if self.instance(&draft.instance_id).is_none()
                    || draft.draft_id == 0
                    || draft.name.trim().is_empty()
                    || draft.preview_sound_id.trim().is_empty()
                    || draft.storage_path.trim().is_empty()
                    || draft.document_json.trim().is_empty()
                {
                    return Err("program draft metadata is invalid".into());
                }
                draft
                    .editor
                    .validate()
                    .map_err(|error| format!("program editor is invalid: {error}"))?;
                self.program_draft = Some(draft.clone());
            }
            SessionEvent::ProgramDraftUpdated { draft } => {
                let current = self
                    .program_draft
                    .as_ref()
                    .ok_or_else(|| "program draft is not active".to_owned())?;
                if current.draft_id != draft.draft_id
                    || current.instance_id != draft.instance_id
                    || draft.name.trim().is_empty()
                    || draft.preview_sound_id.trim().is_empty()
                    || draft.storage_path.trim().is_empty()
                    || draft.document_json.trim().is_empty()
                {
                    return Err("program draft update does not match the active draft".into());
                }
                draft
                    .editor
                    .validate()
                    .map_err(|error| format!("program editor is invalid: {error}"))?;
                if self
                    .audition
                    .as_ref()
                    .is_none_or(|audition| audition.instance_id != draft.instance_id)
                {
                    return Err("program draft lost matching audition focus".into());
                }
                self.program_draft = Some(draft.clone());
            }
            SessionEvent::ProgramSaved {
                draft_id,
                instance_id,
                sound,
            } => {
                let current = self
                    .program_draft
                    .as_ref()
                    .ok_or_else(|| "program draft is not active".to_owned())?;
                if current.draft_id != *draft_id || current.instance_id != *instance_id {
                    return Err("saved program does not match the active draft".into());
                }
                let instance = self
                    .instances
                    .iter_mut()
                    .find(|instance| &instance.instance_id == instance_id)
                    .ok_or_else(|| format!("unknown instance {instance_id}"))?;
                if let Some(existing) = instance
                    .sounds
                    .iter_mut()
                    .find(|existing| existing.id == sound.id)
                {
                    *existing = sound.clone();
                } else {
                    instance.sounds.push(sound.clone());
                }
                self.program_draft = None;
            }
            SessionEvent::ProgramEditCancelled {
                draft_id,
                instance_id,
            } => {
                let current = self
                    .program_draft
                    .as_ref()
                    .ok_or_else(|| "program draft is not active".to_owned())?;
                if current.draft_id != *draft_id || current.instance_id != *instance_id {
                    return Err("cancelled program does not match the active draft".into());
                }
                self.program_draft = None;
            }
            SessionEvent::SurfaceActivated {
                instance_id,
                request,
                response,
            } => {
                if self.instance(instance_id).is_none() {
                    return Err(format!("unknown instance {instance_id}"));
                }
                request.validate().map_err(|error| error.to_string())?;
                response.validate().map_err(|error| error.to_string())?;
            }
        }
        self.revision = envelope.revision;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionCommand {
    RegisterHostControls {
        controller_id: String,
        bindings: Vec<HostControlBinding>,
    },
    SetMasterLevel {
        level: MasterLevel,
    },
    SetMasterPan {
        pan: MasterPan,
    },
    SetActiveMode {
        mode: SurfaceMode,
    },
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
    BeginProgramEdit {
        instance_id: InstanceId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        program_id: Option<String>,
    },
    ReplaceProgramDraft {
        draft_id: u64,
        document_json: String,
    },
    PreviewProgramDraft {
        draft_id: u64,
        document_json: String,
    },
    EditProgramDraftField {
        draft_id: u64,
        field_id: String,
        value: ProgramEditorValue,
        #[serde(default)]
        preview: bool,
    },
    RestoreProgramDraftPreview {
        draft_id: u64,
    },
    SaveProgramDraft {
        draft_id: u64,
    },
    CancelProgramEdit {
        draft_id: u64,
    },
    ActivateSurface {
        instance_id: InstanceId,
        request: SurfaceActivationRequest,
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
    MasterLevelChanged {
        level: MasterLevel,
    },
    MasterPanChanged {
        pan: MasterPan,
    },
    ActiveModeChanged {
        mode: SurfaceMode,
    },
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
    ProgramEditStarted {
        draft: ProgramDraftState,
    },
    ProgramDraftUpdated {
        draft: ProgramDraftState,
    },
    ProgramSaved {
        draft_id: u64,
        instance_id: InstanceId,
        sound: SoundSummary,
    },
    ProgramEditCancelled {
        draft_id: u64,
        instance_id: InstanceId,
    },
    SurfaceActivated {
        instance_id: InstanceId,
        request: SurfaceActivationRequest,
        response: SurfaceActivationResponse,
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
            active_mode: SurfaceMode::Live,
            master_level: MasterLevel::UNITY,
            master_pan: MasterPan::CENTER,
            active_instance_id: Some(instance_id.clone()),
            instances: vec![PluginInstanceState {
                instance_id,
                plugin_id: "org.rackforge.rf-dls".into(),
                plugin_name: "RF-DLS".into(),
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
            program_draft: None,
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

    fn program_draft(instance_id: InstanceId, dirty: bool) -> ProgramDraftState {
        ProgramDraftState {
            draft_id: 17,
            instance_id,
            original_program_id: None,
            name: "CUSTOM 001".into(),
            preview_sound_id: "dls.piano-1".into(),
            storage_path: "custom/user.custom-001.rackforge-program.json".into(),
            document_json: r#"{"id":"user.custom-001","payload":{}}"#.into(),
            editor: rackforge_program_api::ProgramEditorView {
                schema_version: rackforge_program_api::PROGRAM_EDITOR_SCHEMA_VERSION,
                title: "TEST".into(),
                pages: vec![rackforge_program_api::ProgramEditorPage {
                    id: "output".into(),
                    label: "OUTPUT".into(),
                    detail: "Program output".into(),
                    enabled: true,
                    pages: Vec::new(),
                    fields: vec![rackforge_program_api::ProgramEditorField {
                        id: "program.gain".into(),
                        label: "GAIN".into(),
                        detail: "Output gain".into(),
                        value: rackforge_program_api::ProgramEditorValue::Integer(100),
                        kind: rackforge_program_api::ProgramEditorFieldKind::Number {
                            minimum: 0,
                            maximum: 200,
                            step: 1,
                            decimals: 2,
                            unit: None,
                            allow_inherited: false,
                        },
                        live_preview: true,
                    }],
                }],
            },
            dirty,
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
    fn active_mode_is_event_driven_and_legacy_snapshots_default_to_live() {
        let mut state = session();
        state
            .apply(&event(
                1,
                SessionEvent::ActiveModeChanged {
                    mode: SurfaceMode::Play,
                },
            ))
            .unwrap();
        assert_eq!(state.active_mode, SurfaceMode::Play);

        let mut legacy = serde_json::to_value(session()).unwrap();
        legacy.as_object_mut().unwrap().remove("active_mode");
        legacy.as_object_mut().unwrap().remove("master_level");
        legacy.as_object_mut().unwrap().remove("master_pan");
        let restored: SessionState = serde_json::from_value(legacy).unwrap();
        assert_eq!(restored.active_mode, SurfaceMode::Live);
        assert_eq!(restored.master_level, MasterLevel::UNITY);
        assert_eq!(restored.master_pan, MasterPan::CENTER);
    }

    #[test]
    fn master_level_is_bounded_and_event_driven() {
        assert!(MasterLevel::new(MasterLevel::MAX + 1).is_err());
        assert!(serde_json::from_str::<MasterLevel>("1001").is_err());
        assert_eq!(MasterLevel::from_midi(0), MasterLevel::SILENT);
        assert_eq!(MasterLevel::from_midi(127), MasterLevel::UNITY);

        let mut state = session();
        let level = MasterLevel::new(625).unwrap();
        state
            .apply(&event(1, SessionEvent::MasterLevelChanged { level }))
            .unwrap();
        assert_eq!(state.master_level, level);
    }

    #[test]
    fn master_pan_snaps_to_center_and_is_event_driven() {
        assert!(MasterPan::new(MasterPan::MAX + 1).is_err());
        assert!(MasterPan::new(-MasterPan::MAX - 1).is_err());
        assert_eq!(MasterPan::from_midi_with_center_snap(0), MasterPan::LEFT);
        assert_eq!(MasterPan::from_midi_with_center_snap(127), MasterPan::RIGHT);
        for value in MasterPan::MIDI_SNAP_LOW..=MasterPan::MIDI_SNAP_HIGH {
            assert_eq!(
                MasterPan::from_midi_with_center_snap(value),
                MasterPan::CENTER
            );
        }
        assert!(MasterPan::from_midi_with_center_snap(59).get() < 0);
        assert!(MasterPan::from_midi_with_center_snap(69).get() > 0);
        assert_eq!(MasterPan::LEFT.balance(), (1.0, 0.0));
        assert_eq!(MasterPan::CENTER.balance(), (1.0, 1.0));
        assert_eq!(MasterPan::RIGHT.balance(), (0.0, 1.0));

        let mut state = session();
        let pan = MasterPan::new(-375).unwrap();
        state
            .apply(&event(1, SessionEvent::MasterPanChanged { pan }))
            .unwrap();
        assert_eq!(state.master_pan, pan);
    }

    #[test]
    fn reads_legacy_instance_fields_but_writes_only_plugin_vocabulary() {
        let legacy = r#"{
            "schema_version": 1,
            "session_id": "live.main",
            "revision": 0,
            "active_instance_id": "live.main.instrument.1",
            "instances": [{
                "instance_id": "live.main.instrument.1",
                "addon_id": "org.rackforge.rf-dls",
                "addon_name": "RF-DLS"
            }]
        }"#;
        let session: SessionState = serde_json::from_str(legacy).unwrap();
        assert_eq!(session.instances[0].plugin_id, "org.rackforge.rf-dls");

        let serialized = serde_json::to_string(&session).unwrap();
        assert!(serialized.contains("\"plugin_id\""));
        assert!(serialized.contains("\"plugin_name\""));
        assert!(!serialized.contains("\"addon_id\""));
        assert!(!serialized.contains("\"addon_name\""));
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

    #[test]
    fn program_edit_lifecycle_is_bound_to_audition_and_publishes_saved_sound() {
        let mut session = session();
        let instance_id = session.active_instance_id.clone().unwrap();
        session
            .apply(&event(
                1,
                SessionEvent::AuditionStarted {
                    lease_id: 7,
                    instance_id: instance_id.clone(),
                    previous_sound_id: None,
                },
            ))
            .unwrap();
        session
            .apply(&event(
                2,
                SessionEvent::ProgramEditStarted {
                    draft: program_draft(instance_id.clone(), false),
                },
            ))
            .unwrap();

        assert!(
            session
                .apply(&event(
                    3,
                    SessionEvent::AuditionEnded {
                        lease_id: 7,
                        instance_id: instance_id.clone(),
                        restored_sound_id: None,
                        reason: AuditionEndReason::Released,
                    },
                ))
                .is_err()
        );
        session
            .apply(&event(
                3,
                SessionEvent::ProgramDraftUpdated {
                    draft: program_draft(instance_id.clone(), true),
                },
            ))
            .unwrap();
        session
            .apply(&event(
                4,
                SessionEvent::ProgramSaved {
                    draft_id: 17,
                    instance_id: instance_id.clone(),
                    sound: SoundSummary {
                        id: "custom.user.custom-001".into(),
                        name: "CUSTOM 001".into(),
                        bank: Some("custom".into()),
                        detail: Some("CUSTOM 001".into()),
                    },
                },
            ))
            .unwrap();
        session
            .apply(&event(
                5,
                SessionEvent::AuditionEnded {
                    lease_id: 7,
                    instance_id,
                    restored_sound_id: None,
                    reason: AuditionEndReason::Released,
                },
            ))
            .unwrap();

        assert!(session.program_draft.is_none());
        assert!(session.audition.is_none());
        assert!(
            session
                .active_instance()
                .unwrap()
                .sounds
                .iter()
                .any(|sound| sound.id == "custom.user.custom-001")
        );
    }
}
