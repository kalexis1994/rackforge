use rackforge_control_profile::{CONTROL_PROFILE_SCHEMA_VERSION, SemanticControlId, roles};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const LITTLE_V1: &str = "little@1";
pub const LITTLE_TEXT_COLUMNS: usize = 18;
pub const LITTLE_BODY_ROWS: usize = 2;
pub const LITTLE_SOFT_KEYS: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostControlTarget {
    MasterLevel,
    MasterPan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostActionTarget {
    KeyboardParts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiControlChangeBinding {
    pub channel: u8,
    pub controller: u8,
}

impl MidiControlChangeBinding {
    pub fn validate(self) -> Result<(), String> {
        if self.channel > 15 {
            return Err(format!("MIDI channel {} is outside 0..15", self.channel));
        }
        if self.controller > 119 {
            return Err(format!(
                "MIDI CC {} is a channel-mode message, not a continuous control",
                self.controller
            ));
        }
        Ok(())
    }

    pub fn value(self, message: &[u8]) -> Option<u8> {
        let [status, controller, value] = message else {
            return None;
        };
        (*status == (0xb0 | self.channel) && *controller == self.controller).then_some(*value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostControlBinding {
    pub target: HostControlTarget,
    pub midi_cc: MidiControlChangeBinding,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonPhase {
    Press,
    Release,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiButtonBinding {
    pub channel: u8,
    pub controller: u8,
    pub press_value: u8,
    pub release_value: u8,
}

impl MidiButtonBinding {
    pub fn validate(self) -> Result<(), String> {
        MidiControlChangeBinding {
            channel: self.channel,
            controller: self.controller,
        }
        .validate()?;
        if self.press_value > 127 || self.release_value > 127 {
            return Err("MIDI button values must be within 0..127".into());
        }
        if self.press_value == self.release_value {
            return Err("MIDI button press and release values must differ".into());
        }
        Ok(())
    }

    pub fn phase(self, message: &[u8]) -> Option<ButtonPhase> {
        let value = MidiControlChangeBinding {
            channel: self.channel,
            controller: self.controller,
        }
        .value(message)?;
        if value == self.press_value {
            Some(ButtonPhase::Press)
        } else if value == self.release_value {
            Some(ButtonPhase::Release)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostActionBinding {
    pub target: HostActionTarget,
    pub midi_cc: MidiButtonBinding,
}

/// The semantic controls exposed by one stable MIDI endpoint of a controller.
///
/// This declaration maps the device's physical MIDI dialect to RackForge's
/// transport-independent control vocabulary. It does not name a plugin or a
/// plugin parameter index.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticControlProfile {
    pub schema_version: u32,
    pub source_id: String,
    #[serde(default)]
    pub controls: Vec<SemanticControlBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticControlBinding {
    pub role: SemanticControlId,
    pub midi_cc: MidiControlChangeBinding,
    #[serde(default)]
    pub invert: bool,
    /// How a physical reading changes its semantic destination.
    ///
    /// Absolute is appropriate for faders and ordinary knobs. Relative keeps
    /// the current RackForge value and moves it by the distance travelled by
    /// an absolute-reporting endless encoder, avoiding jumps after reconnect.
    #[serde(default)]
    pub mode: SemanticControlMode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticControlMode {
    #[default]
    Absolute,
    Relative,
}

/// RackForge-owned parameters addressable by the public semantic control
/// vocabulary. Plugins never own these values and controllers do not receive
/// a private shortcut for them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RackForgeParameterId {
    MasterLevel,
    MasterPan,
}

impl RackForgeParameterId {
    pub fn from_role(role: &SemanticControlId) -> Option<Self> {
        match role.as_str() {
            roles::RACKFORGE_MASTER_LEVEL => Some(Self::MasterLevel),
            roles::RACKFORGE_MASTER_PAN => Some(Self::MasterPan),
            _ => None,
        }
    }

    pub const fn role(self) -> &'static str {
        match self {
            Self::MasterLevel => roles::RACKFORGE_MASTER_LEVEL,
            Self::MasterPan => roles::RACKFORGE_MASTER_PAN,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RackForgeParameterInput {
    pub parameter: RackForgeParameterId,
    pub value: u8,
    pub mode: SemanticControlMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticControlInput {
    pub role: SemanticControlId,
    pub value: u8,
    pub mode: SemanticControlMode,
}

/// Observes every semantic control without deciding whether its MIDI message
/// is consumed. Plugin roles must continue through normal MIDI routing.
pub fn semantic_control_input(
    profile: &SemanticControlProfile,
    message: &[u8],
) -> Option<SemanticControlInput> {
    profile.controls.iter().find_map(|binding| {
        let value = binding.midi_cc.value(message)?;
        Some(SemanticControlInput {
            role: binding.role.clone(),
            value: if binding.invert { 127 - value } else { value },
            mode: binding.mode,
        })
    })
}

/// Compact, controller-independent feedback for LITTLE's 18-column header.
pub fn semantic_control_little_header(input: &SemanticControlInput) -> String {
    let label = match input.role.as_str() {
        roles::SYNTH_FILTER_CUTOFF => "FILTER CUTOFF",
        roles::SYNTH_FILTER_RESONANCE => "RESONANCE",
        roles::SYNTH_FILTER_ENVELOPE_AMOUNT => "FILTER ENV",
        roles::SYNTH_FILTER_LFO_AMOUNT => "FILTER LFO",
        roles::SYNTH_FILTER_KEY_TRACKING => "KEY TRACK",
        roles::SYNTH_AMP_ENVELOPE_ATTACK => "AMP ATTACK",
        roles::SYNTH_AMP_ENVELOPE_DECAY => "AMP DECAY",
        roles::SYNTH_AMP_ENVELOPE_SUSTAIN => "AMP SUSTAIN",
        roles::SYNTH_AMP_ENVELOPE_RELEASE => "AMP RELEASE",
        roles::SYNTH_LFO_RATE => "LFO RATE",
        roles::SYNTH_LFO_DEPTH => "LFO DEPTH",
        roles::SYNTH_LFO_DELAY => "LFO DELAY",
        roles::SYNTH_OSCILLATOR_PULSE_WIDTH => "PULSE WIDTH",
        roles::SYNTH_OSCILLATOR_SUB_LEVEL => "SUB LEVEL",
        roles::SYNTH_OSCILLATOR_NOISE_LEVEL => "NOISE LEVEL",
        roles::SYNTH_AMPLIFIER_LEVEL => "AMP LEVEL",
        roles::PLUGIN_OUTPUT_LEVEL => "PLUGIN LEVEL",
        roles::MIXER_CHANNEL_LEVEL => "CHANNEL LEVEL",
        roles::MIXER_CHANNEL_PAN => "CHANNEL PAN",
        roles::PERFORMANCE_MODULATION => "MODULATION",
        roles::PERFORMANCE_EXPRESSION => "EXPRESSION",
        roles::PERFORMANCE_SUSTAIN => "SUSTAIN",
        _ => input.role.as_str().rsplit('.').next().unwrap_or("CONTROL"),
    };
    let mut label = label
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == ' ')
        .flat_map(char::to_uppercase)
        .take(13)
        .collect::<String>();
    if label.is_empty() {
        label.push_str("CONTROL");
    }
    let value = if input.role.as_str() == roles::PERFORMANCE_SUSTAIN {
        if input.value >= 64 {
            "ON".into()
        } else {
            "OFF".into()
        }
    } else {
        format!("{}%", (u32::from(input.value) * 100 + 63) / 127)
    };
    format!("{label:<13}{value:>5}")
}

/// Resolves a raw MIDI message through the same semantic profile used for
/// plugin parameters, selecting only roles owned by RackForge itself.
pub fn rackforge_parameter_input(
    profile: &SemanticControlProfile,
    message: &[u8],
) -> Option<RackForgeParameterInput> {
    let input = semantic_control_input(profile, message)?;
    Some(RackForgeParameterInput {
        parameter: RackForgeParameterId::from_role(&input.role)?,
        value: input.value,
        mode: input.mode,
    })
}

impl SemanticControlProfile {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != CONTROL_PROFILE_SCHEMA_VERSION {
            return Err(format!(
                "unsupported semantic control profile {}",
                self.schema_version
            ));
        }
        validate_identifier(&self.source_id)?;
        if self.controls.is_empty() {
            return Err("semantic control profile has no controls".into());
        }
        let mut roles = BTreeSet::new();
        let mut midi = BTreeSet::new();
        for binding in &self.controls {
            binding.role.validate().map_err(|error| error.to_string())?;
            binding.midi_cc.validate()?;
            if !roles.insert(binding.role.as_str()) {
                return Err(format!("duplicate semantic control role {}", binding.role));
            }
            if !midi.insert((binding.midi_cc.channel, binding.midi_cc.controller)) {
                return Err(format!(
                    "duplicate semantic MIDI binding ch={} cc={}",
                    binding.midi_cc.channel, binding.midi_cc.controller
                ));
            }
        }
        Ok(())
    }

    pub fn validate_against_reserved(
        &self,
        controls: &[HostControlBinding],
        actions: &[HostActionBinding],
    ) -> Result<(), String> {
        self.validate()?;
        let reserved = controls
            .iter()
            .map(|binding| (binding.midi_cc.channel, binding.midi_cc.controller))
            .chain(
                actions
                    .iter()
                    .map(|binding| (binding.midi_cc.channel, binding.midi_cc.controller)),
            )
            .collect::<BTreeSet<_>>();
        for binding in &self.controls {
            if reserved.contains(&(binding.midi_cc.channel, binding.midi_cc.controller)) {
                return Err(format!(
                    "semantic control {} reuses reserved MIDI binding ch={} cc={}",
                    binding.role, binding.midi_cc.channel, binding.midi_cc.controller
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceContract {
    pub id: String,
    pub text_columns: u16,
    pub header_rows: u8,
    pub body_rows: u8,
    pub soft_keys: u8,
    pub navigation: NavigationCapabilities,
}

impl SurfaceContract {
    pub fn little_v1() -> Self {
        Self {
            id: LITTLE_V1.into(),
            text_columns: LITTLE_TEXT_COLUMNS as u16,
            header_rows: 1,
            body_rows: LITTLE_BODY_ROWS as u8,
            soft_keys: LITTLE_SOFT_KEYS as u8,
            navigation: NavigationCapabilities {
                previous: true,
                next: true,
                confirm: true,
                back: true,
            },
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_versioned_id(&self.id)?;
        if self.text_columns == 0
            || self.body_rows == 0
            || self.soft_keys == 0
            || !self.navigation.is_complete()
        {
            return Err(format!("surface {:?} has incomplete capabilities", self.id));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationCapabilities {
    pub previous: bool,
    pub next: bool,
    pub confirm: bool,
    pub back: bool,
}

impl NavigationCapabilities {
    pub fn is_complete(self) -> bool {
        self.previous && self.next && self.confirm && self.back
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceQuality {
    Native,
    CertifiedCompatibility,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GestureCapabilities {
    pub soft_key_long_press: bool,
    pub emergency_home_chord: bool,
}

impl GestureCapabilities {
    pub fn validate(self) -> Result<(), String> {
        if self.emergency_home_chord && !self.soft_key_long_press {
            return Err("emergency home chord requires soft-key long press detection".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceImplementation {
    pub layout_id: String,
    pub quality: SurfaceQuality,
    pub priority: u16,
    #[serde(default)]
    pub gestures: GestureCapabilities,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerProfile {
    pub id: String,
    pub name: String,
    pub driver_id: String,
    pub surfaces: Vec<SurfaceImplementation>,
    #[serde(default)]
    pub host_controls: Vec<HostControlBinding>,
    #[serde(default)]
    pub host_actions: Vec<HostActionBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_profile: Option<SemanticControlProfile>,
}

impl ControllerProfile {
    pub fn validate(&self) -> Result<(), String> {
        validate_identifier(&self.id)?;
        validate_identifier(&self.driver_id)?;
        if self.name.trim().is_empty() || self.surfaces.is_empty() {
            return Err("controller profile needs a name and at least one surface".into());
        }
        let mut ids = BTreeSet::new();
        let mut priorities = BTreeSet::new();
        for surface in &self.surfaces {
            validate_versioned_id(&surface.layout_id)?;
            surface.gestures.validate()?;
            if !ids.insert(surface.layout_id.as_str()) {
                return Err(format!("duplicate surface {:?}", surface.layout_id));
            }
            if !priorities.insert(surface.priority) {
                return Err(format!("duplicate surface priority {}", surface.priority));
            }
        }
        let mut targets = BTreeSet::new();
        let mut midi_bindings = BTreeSet::new();
        for binding in &self.host_controls {
            binding.midi_cc.validate()?;
            if !targets.insert(binding.target as u8) {
                return Err(format!(
                    "duplicate reserved host control {:?}",
                    binding.target
                ));
            }
            if !midi_bindings.insert((binding.midi_cc.channel, binding.midi_cc.controller)) {
                return Err(format!(
                    "duplicate reserved MIDI binding ch={} cc={}",
                    binding.midi_cc.channel, binding.midi_cc.controller
                ));
            }
        }
        let mut action_targets = BTreeSet::new();
        for binding in &self.host_actions {
            binding.midi_cc.validate()?;
            if !action_targets.insert(binding.target as u8) {
                return Err(format!(
                    "duplicate reserved host action {:?}",
                    binding.target
                ));
            }
            if !midi_bindings.insert((binding.midi_cc.channel, binding.midi_cc.controller)) {
                return Err(format!(
                    "duplicate reserved MIDI binding ch={} cc={}",
                    binding.midi_cc.channel, binding.midi_cc.controller
                ));
            }
        }
        if let Some(profile) = &self.semantic_profile {
            profile.validate_against_reserved(&self.host_controls, &self.host_actions)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NegotiatedSurface {
    pub layout_id: String,
    pub quality: SurfaceQuality,
}

/// Chooses only an implementation explicitly declared by both sides.
///
/// Screen size, control count and layout names are never inferred.
pub fn negotiate_surface(
    controller: &ControllerProfile,
    plugin_layouts: &[String],
) -> Option<NegotiatedSurface> {
    controller
        .surfaces
        .iter()
        .filter(|surface| plugin_layouts.contains(&surface.layout_id))
        .min_by_key(|surface| surface.priority)
        .map(|surface| NegotiatedSurface {
            layout_id: surface.layout_id.clone(),
            quality: surface.quality,
        })
}

pub trait ControllerDriver: Sync {
    fn profile(&self) -> &ControllerProfile;
    fn matches_display_output(&self, port_name: &str) -> bool;
    fn matches_surface_input(&self, port_name: &str) -> bool;
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

fn validate_versioned_id(value: &str) -> Result<(), String> {
    let (id, version) = value
        .rsplit_once('@')
        .ok_or_else(|| format!("surface id {value:?} is not versioned"))?;
    validate_identifier(id)?;
    if version.is_empty() || !version.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("invalid surface version in {value:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn medium_controller(with_little: bool) -> ControllerProfile {
        let mut surfaces = vec![SurfaceImplementation {
            layout_id: "medium@1".into(),
            quality: SurfaceQuality::Native,
            priority: 0,
            gestures: GestureCapabilities::default(),
        }];
        if with_little {
            surfaces.push(SurfaceImplementation {
                layout_id: LITTLE_V1.into(),
                quality: SurfaceQuality::CertifiedCompatibility,
                priority: 1,
                gestures: GestureCapabilities::default(),
            });
        }
        ControllerProfile {
            id: "example.medium".into(),
            name: "Example Medium".into(),
            driver_id: "org.rackforge.example-medium".into(),
            surfaces,
            host_controls: Vec::new(),
            host_actions: Vec::new(),
            semantic_profile: None,
        }
    }

    #[test]
    fn semantic_controls_cannot_collide_with_reserved_host_controls() {
        let mut controller = medium_controller(true);
        controller.host_controls.push(HostControlBinding {
            target: HostControlTarget::MasterLevel,
            midi_cc: MidiControlChangeBinding {
                channel: 0,
                controller: 113,
            },
        });
        controller.semantic_profile = Some(SemanticControlProfile {
            schema_version: CONTROL_PROFILE_SCHEMA_VERSION,
            source_id: "controller.example.midi".into(),
            controls: vec![SemanticControlBinding {
                role: SemanticControlId::new("plugin.output.level").unwrap(),
                midi_cc: MidiControlChangeBinding {
                    channel: 0,
                    controller: 113,
                },
                invert: false,
                mode: SemanticControlMode::Absolute,
            }],
        });
        assert!(controller.validate().is_err());
    }

    #[test]
    fn rackforge_parameters_are_resolved_from_the_semantic_profile() {
        let profile = SemanticControlProfile {
            schema_version: CONTROL_PROFILE_SCHEMA_VERSION,
            source_id: "controller.example.midi".into(),
            controls: vec![SemanticControlBinding {
                role: SemanticControlId::new(roles::RACKFORGE_MASTER_PAN).unwrap(),
                midi_cc: MidiControlChangeBinding {
                    channel: 0,
                    controller: 104,
                },
                invert: true,
                mode: SemanticControlMode::Relative,
            }],
        };
        assert_eq!(
            rackforge_parameter_input(&profile, &[0xb0, 104, 27]),
            Some(RackForgeParameterInput {
                parameter: RackForgeParameterId::MasterPan,
                value: 100,
                mode: SemanticControlMode::Relative,
            })
        );
        assert!(rackforge_parameter_input(&profile, &[0xb0, 103, 27]).is_none());
    }

    #[test]
    fn semantic_feedback_is_bounded_for_little() {
        let input = SemanticControlInput {
            role: SemanticControlId::new(roles::SYNTH_FILTER_CUTOFF).unwrap(),
            value: 127,
            mode: SemanticControlMode::Absolute,
        };
        let header = semantic_control_little_header(&input);
        assert_eq!(header, "FILTER CUTOFF 100%");
        assert_eq!(header.len(), LITTLE_TEXT_COLUMNS);
    }

    #[test]
    fn never_infers_little_from_a_larger_display() {
        let controller = medium_controller(false);
        assert_eq!(negotiate_surface(&controller, &[LITTLE_V1.into()]), None);
    }

    #[test]
    fn uses_only_explicit_certified_compatibility() {
        let controller = medium_controller(true);
        assert_eq!(
            negotiate_surface(&controller, &[LITTLE_V1.into()]),
            Some(NegotiatedSurface {
                layout_id: LITTLE_V1.into(),
                quality: SurfaceQuality::CertifiedCompatibility,
            })
        );
    }

    #[test]
    fn prefers_the_explicit_native_layout() {
        let controller = medium_controller(true);
        assert_eq!(
            negotiate_surface(&controller, &[LITTLE_V1.into(), "medium@1".into()]),
            Some(NegotiatedSurface {
                layout_id: "medium@1".into(),
                quality: SurfaceQuality::Native,
            })
        );
    }

    #[test]
    fn reserved_host_control_matches_only_its_exact_midi_cc() {
        let binding = MidiControlChangeBinding {
            channel: 0,
            controller: 82,
        };
        assert_eq!(binding.value(&[0xb0, 82, 91]), Some(91));
        assert_eq!(binding.value(&[0xb1, 82, 91]), None);
        assert_eq!(binding.value(&[0xb0, 83, 91]), None);
        assert!(
            MidiControlChangeBinding {
                channel: 16,
                controller: 82
            }
            .validate()
            .is_err()
        );
        assert!(
            MidiControlChangeBinding {
                channel: 0,
                controller: 123
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn momentary_host_action_has_explicit_press_and_release_values() {
        let binding = MidiButtonBinding {
            channel: 0,
            controller: 119,
            press_value: 127,
            release_value: 0,
        };
        assert_eq!(binding.phase(&[0xb0, 119, 127]), Some(ButtonPhase::Press));
        assert_eq!(binding.phase(&[0xb0, 119, 0]), Some(ButtonPhase::Release));
        assert_eq!(binding.phase(&[0xb0, 119, 64]), None);
        assert!(binding.validate().is_ok());
        assert!(
            MidiButtonBinding {
                press_value: 1,
                release_value: 1,
                ..binding
            }
            .validate()
            .is_err()
        );
    }
}
