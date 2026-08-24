pub use rackforge_midi_api::{ParameterLink, ParameterLinkId};
pub use rackforge_performance_api::{
    LiveBrowseMode, LiveLocation, LivePerformanceState, RackDefinition, RackId,
};
use rackforge_program_api::{ProgramArtifact, ProgramEditorValue, ProgramEditorView};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

pub use rackforge_controller_api::{
    ButtonPhase, HostActionBinding, HostActionTarget, HostControlBinding, HostControlTarget,
    MidiButtonBinding, MidiControlChangeBinding, RackForgeParameterId, RackForgeParameterInput,
    SemanticControlBinding, SemanticControlInput, SemanticControlMode, SemanticControlProfile,
    rackforge_parameter_input, semantic_control_input, semantic_control_little_header,
};
pub use rackforge_surface_api::{
    SurfaceActivationReason, SurfaceActivationRequest, SurfaceActivationResponse, SurfaceMode,
};

pub const SESSION_SCHEMA_VERSION: u32 = 14;

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

/// Canonical value of a RackForge-owned parameter after interpreting one
/// semantic controller input. This is deliberately separate from plugin
/// parameters: it changes host state through the normal session/audio path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RackForgeParameterValue {
    MasterLevel(MasterLevel),
    MasterPan(MasterPan),
}

impl RackForgeParameterValue {
    pub const fn parameter(self) -> RackForgeParameterId {
        match self {
            Self::MasterLevel(_) => RackForgeParameterId::MasterLevel,
            Self::MasterPan(_) => RackForgeParameterId::MasterPan,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::MasterLevel(_) => "MASTER VOL",
            Self::MasterPan(_) => "MASTER PAN",
        }
    }

    pub fn display_value(self) -> String {
        match self {
            Self::MasterLevel(level) => {
                format!("{}%", (u32::from(level.get()) + 5) / 10)
            }
            Self::MasterPan(pan) => {
                let value = pan.get();
                if value == 0 {
                    "CENTER".to_owned()
                } else {
                    let side = if value < 0 { 'L' } else { 'R' };
                    format!("{side} {}%", (u32::from(value.unsigned_abs()) + 5) / 10)
                }
            }
        }
    }

    /// Generic LITTLE feedback for any RackForge-owned parameter.
    pub fn little_header(self) -> String {
        format!("{} {:>7}", self.label(), self.display_value())
    }
}

/// Stateful interpreter for semantic RackForge parameters.
///
/// Relative controls remember their previous physical reading and move from
/// the canonical host value. The first reading therefore anchors an endless
/// encoder instead of making the mix jump after startup or reconnection.
#[derive(Clone, Debug, Default)]
pub struct RackForgeParameterMapper {
    pan_previous: Option<u8>,
    pan_position: Option<i16>,
    pan_remainder: i32,
}

const RELATIVE_PAN_DETENT: i16 = 60;

fn relative_pan_through_detent(position: i16) -> i16 {
    let limit = i32::from(MasterPan::MAX);
    let detent = i32::from(RELATIVE_PAN_DETENT);
    let magnitude = i32::from(position.abs());
    if magnitude <= detent {
        return 0;
    }
    let scaled = ((magnitude - detent) * limit / (limit - detent)).min(limit) as i16;
    if position < 0 { -scaled } else { scaled }
}

impl RackForgeParameterMapper {
    pub fn sync_master_pan(&mut self, pan: MasterPan) {
        // Session refreshes happen after every command. Do not overwrite the
        // encoder's unsnapped internal position while it is being followed;
        // doing so would make it stick inside the virtual centre detent.
        if self.pan_previous.is_none() {
            self.pan_position = Some(pan.get());
        }
    }

    pub fn reset_physical_anchors(&mut self) {
        self.pan_previous = None;
        self.pan_remainder = 0;
    }

    pub fn apply(
        &mut self,
        input: RackForgeParameterInput,
        current_pan: MasterPan,
    ) -> Option<RackForgeParameterValue> {
        match (input.parameter, input.mode) {
            (RackForgeParameterId::MasterLevel, _) => Some(RackForgeParameterValue::MasterLevel(
                MasterLevel::from_midi(input.value),
            )),
            (RackForgeParameterId::MasterPan, SemanticControlMode::Absolute) => {
                let pan = MasterPan::from_midi_with_center_snap(input.value);
                self.pan_position = Some(pan.get());
                Some(RackForgeParameterValue::MasterPan(pan))
            }
            (RackForgeParameterId::MasterPan, SemanticControlMode::Relative) => {
                let Some(previous) = self.pan_previous.replace(input.value) else {
                    self.pan_position = Some(current_pan.get());
                    return None;
                };
                let turned = i32::from(input.value) - i32::from(previous);
                if turned == 0 {
                    return None;
                }
                let span = 2 * i32::from(MasterPan::MAX);
                let scaled = turned * span + self.pan_remainder;
                let moved = scaled / 127;
                self.pan_remainder = scaled % 127;
                let limit = i32::from(MasterPan::MAX);
                let current = self.pan_position.unwrap_or(current_pan.get());
                let next = (i32::from(current) + moved).clamp(-limit, limit) as i16;
                self.pan_position = Some(next);
                MasterPan::new(relative_pan_through_detent(next))
                    .ok()
                    .map(RackForgeParameterValue::MasterPan)
            }
        }
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

/// One bank a plugin groups its sounds into.
///
/// Sounds name their bank by identifier, and an identifier is not a label: it
/// is lowercase and punctuation-free by rule, so `Acordeon Hohner Corona II`
/// reaches a surface as `acordeon-hohner-corona-ii` and cannot be turned back.
/// Carrying the bank list alongside the sounds is what lets a surface print
/// the name the plugin actually chose.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BankSummary {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub order: i32,
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
    /// What kind of thing this sound is, as the plugin classifies it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Free-form marks the plugin attaches to the sound.
    ///
    /// The plugin API has always modelled these and the host used to drop
    /// them, which left a surface with one line of display text and no way to
    /// learn anything else about a sound. Nothing here interprets them: they
    /// mean whatever the plugin that wrote them and the surface that reads
    /// them agree they mean.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub editable: bool,
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
    pub config_available: bool,
    /// The banks the sounds are grouped into, in the plugin's own order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub banks: Vec<BankSummary>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ProgramArtifact>,
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
    #[serde(default)]
    pub live: LivePerformanceState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_instance_id: Option<InstanceId>,
    #[serde(default)]
    pub instances: Vec<PluginInstanceState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audition: Option<AuditionState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_draft: Option<ProgramDraftState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameter_links: Vec<ParameterLink>,
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
            live: LivePerformanceState::default(),
            active_instance_id: None,
            instances: Vec::new(),
            audition: None,
            program_draft: None,
            parameter_links: Vec::new(),
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
            SessionEvent::ActiveInstanceChanged { instance_id } => {
                if self.instance(instance_id).is_none() {
                    return Err(format!("unknown instance {instance_id}"));
                }
                self.active_instance_id = Some(instance_id.clone());
            }
            SessionEvent::LiveBrowseModeChanged { mode } => {
                self.live.mode = *mode;
            }
            SessionEvent::LiveTargetActivated { location, rack_id } => {
                self.live.activate(location.clone(), rack_id.clone());
            }
            SessionEvent::LiveStateReconciled { live } => {
                self.live = live.clone();
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
            SessionEvent::PluginStateRestored {
                instance_id,
                selected_sound_id,
            } => {
                let instance = self
                    .instances
                    .iter_mut()
                    .find(|instance| &instance.instance_id == instance_id)
                    .ok_or_else(|| format!("unknown instance {instance_id}"))?;
                // A host preset embeds complete plugin state. Its native-program hint may
                // legitimately disappear later, so it must never invalidate restoration.
                instance.selected_sound_id = selected_sound_id
                    .as_ref()
                    .filter(|sound_id| {
                        instance
                            .sounds
                            .iter()
                            .any(|sound| sound.id == sound_id.as_str())
                    })
                    .cloned();
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
            SessionEvent::ParameterLinkUpserted { link } => {
                link.validate().map_err(|error| error.to_string())?;
                if let Some(existing) = self
                    .parameter_links
                    .iter_mut()
                    .find(|existing| existing.id == link.id)
                {
                    *existing = link.clone();
                } else {
                    self.parameter_links.push(link.clone());
                }
            }
            SessionEvent::ParameterLinkRemoved { link_id } => {
                let before = self.parameter_links.len();
                self.parameter_links.retain(|link| &link.id != link_id);
                if self.parameter_links.len() == before {
                    return Err(format!("unknown parameter link {link_id}"));
                }
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
    RegisterHostBindings {
        controller_id: String,
        controls: Vec<HostControlBinding>,
        actions: Vec<HostActionBinding>,
        /// Current backend endpoint selected by the driver. Hosts resolve this
        /// display hint to their own stable MIDI source identity.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        midi_source_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        semantic_profile: Option<SemanticControlProfile>,
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
    /// Immediately destroys every running plugin DSP instance and leaves the
    /// session idle. Runtime termination is independent from the UI focus mode.
    EmergencyStop,
    SelectPlugin {
        instance_id: InstanceId,
    },
    SetLiveBrowseMode {
        mode: LiveBrowseMode,
    },
    ActivateLiveTarget {
        location: LiveLocation,
    },
    /// Auditions an unsaved Rack draft without changing the persisted LIVE target.
    PreviewRack {
        rack: RackDefinition,
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
    UpsertParameterLink {
        link: ParameterLink,
    },
    RemoveParameterLink {
        link_id: ParameterLinkId,
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
    ActiveInstanceChanged {
        instance_id: InstanceId,
    },
    LiveBrowseModeChanged {
        mode: LiveBrowseMode,
    },
    LiveTargetActivated {
        location: LiveLocation,
        rack_id: RackId,
    },
    LiveStateReconciled {
        live: LivePerformanceState,
    },
    SoundSelected {
        instance_id: InstanceId,
        sound_id: String,
    },
    PluginStateRestored {
        instance_id: InstanceId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selected_sound_id: Option<String>,
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
    ParameterLinkUpserted {
        link: ParameterLink,
    },
    ParameterLinkRemoved {
        link_id: ParameterLinkId,
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
    use rackforge_midi_api::{
        MidiSourceId, PARAMETER_LINK_SCHEMA_VERSION, ParameterLinkChannel, ParameterLinkMessage,
        ParameterLinkPassThrough, ParameterLinkSource, ParameterLinkTransform,
    };

    #[test]
    fn semantic_master_level_maps_to_a_canonical_global_value() {
        let mut mapper = RackForgeParameterMapper::default();
        let value = mapper
            .apply(
                RackForgeParameterInput {
                    parameter: RackForgeParameterId::MasterLevel,
                    value: 127,
                    mode: SemanticControlMode::Absolute,
                },
                MasterPan::CENTER,
            )
            .unwrap();
        assert_eq!(
            value,
            RackForgeParameterValue::MasterLevel(MasterLevel::UNITY)
        );
        assert_eq!(value.little_header(), "MASTER VOL    100%");
    }

    #[test]
    fn relative_master_pan_anchors_then_moves_from_the_host_value() {
        let mut mapper = RackForgeParameterMapper::default();
        let input = |value| RackForgeParameterInput {
            parameter: RackForgeParameterId::MasterPan,
            value,
            mode: SemanticControlMode::Relative,
        };
        let starting = MasterPan::new(-300).unwrap();
        assert!(mapper.apply(input(90), starting).is_none());
        let RackForgeParameterValue::MasterPan(moved) = mapper.apply(input(91), starting).unwrap()
        else {
            panic!("pan input returned the wrong RackForge parameter")
        };
        assert!(moved.get() > starting.get());
    }

    fn session() -> SessionState {
        let instance_id = InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).unwrap();
        SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            session_id: SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            revision: Revision::ZERO,
            active_mode: SurfaceMode::Live,
            master_level: MasterLevel::UNITY,
            master_pan: MasterPan::CENTER,
            live: LivePerformanceState::default(),
            active_instance_id: Some(instance_id.clone()),
            instances: vec![PluginInstanceState {
                instance_id,
                plugin_id: "org.rackforge.rf-dls".into(),
                plugin_name: "RF-DLS".into(),
                ui_layouts: vec!["little@1".into()],
                config_available: true,
                banks: Vec::new(),
                sounds: vec![SoundSummary {
                    id: "dls.piano-1".into(),
                    name: "Piano 1".into(),
                    bank: Some("dls".into()),
                    detail: None,
                    category: None,
                    tags: Vec::new(),
                    editable: false,
                }],
                selected_sound_id: None,
            }],
            audition: None,
            program_draft: None,
            parameter_links: Vec::new(),
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
            artifacts: Vec::new(),
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

    fn parameter_link(id: &str) -> ParameterLink {
        ParameterLink {
            schema_version: PARAMETER_LINK_SCHEMA_VERSION,
            id: ParameterLinkId::new(id).unwrap(),
            instance_id: DEFAULT_LIVE_INSTANCE_ID.into(),
            parameter_index: 17,
            source: ParameterLinkSource {
                source_id: MidiSourceId::new("usb.controller.main").unwrap(),
                display_name: "Stage Controller".into(),
            },
            channel: ParameterLinkChannel::Omni,
            message: ParameterLinkMessage::ControlChange { controller: 74 },
            transform: ParameterLinkTransform::default(),
            pass_through: ParameterLinkPassThrough::PassThrough,
        }
    }

    #[test]
    fn parameter_links_are_immutable_event_driven_session_state() {
        let mut state = session();
        let original = parameter_link("link.cutoff");
        state
            .apply(&event(
                1,
                SessionEvent::ParameterLinkUpserted {
                    link: original.clone(),
                },
            ))
            .unwrap();
        assert_eq!(state.parameter_links, vec![original.clone()]);

        let mut replacement = original.clone();
        replacement.transform.invert = true;
        state
            .apply(&event(
                2,
                SessionEvent::ParameterLinkUpserted {
                    link: replacement.clone(),
                },
            ))
            .unwrap();
        assert_eq!(state.parameter_links, vec![replacement]);

        state
            .apply(&event(
                3,
                SessionEvent::ParameterLinkRemoved {
                    link_id: original.id,
                },
            ))
            .unwrap();
        assert!(state.parameter_links.is_empty());
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
    fn active_plugin_instance_is_selected_by_event() {
        let mut state = session();
        let synth_id = InstanceId::new("play.org.rackforge.rf-kr106").unwrap();
        state.instances.push(PluginInstanceState {
            instance_id: synth_id.clone(),
            plugin_id: "org.rackforge.rf-kr106".into(),
            plugin_name: "RF-KR106".into(),
            ui_layouts: vec!["little@1".into()],
            config_available: false,
            banks: Vec::new(),
            sounds: Vec::new(),
            selected_sound_id: None,
        });
        state
            .apply(&event(
                1,
                SessionEvent::ActiveInstanceChanged {
                    instance_id: synth_id.clone(),
                },
            ))
            .unwrap();
        assert_eq!(state.active_instance_id.as_ref(), Some(&synth_id));
        assert_eq!(
            state.active_instance().unwrap().plugin_id,
            "org.rackforge.rf-kr106"
        );
    }

    #[test]
    fn complete_state_restore_does_not_depend_on_a_native_program_still_existing() {
        let mut state = session();
        let instance_id = state.active_instance_id.clone().unwrap();
        let event = EventEnvelope {
            schema_version: SESSION_SCHEMA_VERSION,
            revision: state.revision.next().unwrap(),
            command: None,
            event: SessionEvent::PluginStateRestored {
                instance_id,
                selected_sound_id: Some("custom.deleted-later".into()),
            },
        };
        state.apply(&event).unwrap();
        assert_eq!(state.active_instance().unwrap().selected_sound_id, None);
    }

    #[test]
    fn a_bank_keeps_the_name_its_identifier_cannot_carry() {
        // An identifier is lowercase and punctuation-free by rule, so a label
        // does not survive being turned into one. This is the whole reason
        // banks travel as a list of their own rather than as bare ids on the
        // sounds that belong to them.
        let bank = BankSummary {
            id: "acordeon-hohner-corona-ii".into(),
            name: "Acordeon Hohner Corona II".into(),
            order: 3,
        };
        let text = serde_json::to_string(&bank).unwrap();
        let read: BankSummary = serde_json::from_str(&text).unwrap();
        assert_eq!(read.name, "Acordeon Hohner Corona II");
        assert_ne!(read.name, read.id);
    }

    #[test]
    fn a_sound_carries_the_marks_its_plugin_attached() {
        let sound = SoundSummary {
            id: "sfz.trompetas-trompeta-x".into(),
            name: "TROMPETA X".into(),
            bank: Some("trompetas".into()),
            detail: Some("3 MiB".into()),
            category: Some("Instrument".into()),
            tags: vec!["sfz".into(), "keys:0-108".into(), "zones:15".into()],
            editable: false,
        };
        let text = serde_json::to_string(&sound).unwrap();
        let read: SoundSummary = serde_json::from_str(&text).unwrap();
        assert_eq!(read, sound);
    }

    #[test]
    fn a_sound_without_marks_writes_none_of_the_new_fields() {
        // Absent rather than empty, so a plugin that says nothing extra costs
        // nothing extra on the wire.
        let sound = SoundSummary {
            id: "dls.b00000000.p00000000".into(),
            name: "Piano 1".into(),
            bank: None,
            detail: None,
            category: None,
            tags: Vec::new(),
            editable: false,
        };
        let text = serde_json::to_string(&sound).unwrap();
        assert!(!text.contains("tags"), "{text}");
        assert!(!text.contains("category"), "{text}");
        let read: SoundSummary = serde_json::from_str(&text).unwrap();
        assert_eq!(read, sound);
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
                        category: None,
                        tags: Vec::new(),
                        editable: true,
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
