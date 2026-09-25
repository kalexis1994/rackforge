//! A player's controller map: what each input of one controller does in
//! each plugin.
//!
//! A map belongs to the player, not to the controller package: the package
//! is immutable and RackForge updates it, while the map is what the player
//! assigned. One map covers one controller across every plugin, so the same
//! button can switch the Leslie in RF-Organ and do something else in RF-106;
//! the plugin playing decides which applies. A map is exported whole as an
//! `.rfmap` file.
//!
//! A mapping names its plugin parameter by the schema's stable `id`, never by
//! index, which may change between plugin versions, and copies the input's
//! message, so the map still works where the package that named the input is
//! missing.

use crate::{
    MapLayer, MidiRoutingError, ModifierMode, ParameterLinkChannel, ParameterLinkId,
    ParameterLinkMessage, ParameterLinkMode, ParameterLinkPassThrough, RelativeEncoding,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTROLLER_MAP_SCHEMA_VERSION: u32 = 1;
pub const RFMAP_FORMAT: &str = "org.rackforge.map";
pub const RFMAP_SCHEMA_VERSION: u32 = 1;
/// Far above any real controller: a guard against a corrupt or hostile file.
pub const MAX_CONTROLLER_MAP_MAPPINGS: usize = 4096;
const MAX_TEXT_CHARS: usize = 128;

/// Every mapping a player made for one controller.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerMap {
    pub schema_version: u32,
    /// The controller package id, the map's key.
    pub controller_id: String,
    /// The controller's name when the map was last saved, for display.
    pub controller_name: String,
    /// The button that opens the Fn layer, if the controller has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modifier: Option<ControllerModifier>,
    #[serde(default)]
    pub plugins: Vec<PluginControlMap>,
}

/// A controller's Fn button: while it is held, or once it is latched, each
/// control does what its Fn-layer mapping says, where it has one. The
/// button itself does nothing else.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerModifier {
    pub input: MappedInput,
    #[serde(default)]
    pub mode: ModifierMode,
}

/// One controller's mappings in one plugin.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginControlMap {
    pub plugin_id: String,
    /// The plugin's name when the map was last saved, for display.
    pub plugin_name: String,
    #[serde(default)]
    pub mappings: Vec<ControlMapping>,
}

/// One input driving one plugin parameter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlMapping {
    pub id: ParameterLinkId,
    pub input: MappedInput,
    /// The parameter's stable schema `id`.
    pub parameter_id: String,
    #[serde(default)]
    pub mode: ParameterLinkMode,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
    /// Left out, a button consumes its message -- a pad that switches the
    /// Leslie must not also play a note -- and a knob passes it through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass_through: Option<ParameterLinkPassThrough>,
    /// The layer it acts in. Left out, the base one.
    #[serde(default, skip_serializing_if = "MapLayer::is_base")]
    pub layer: MapLayer,
}

impl ControlMapping {
    pub fn effective_pass_through(&self) -> ParameterLinkPassThrough {
        self.pass_through.unwrap_or(if self.mode.is_button() {
            ParameterLinkPassThrough::Consume
        } else {
            ParameterLinkPassThrough::PassThrough
        })
    }
}

/// The controller input a mapping listens to: its package id and name, and
/// a copy of the message it sends.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappedInput {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub channel: ParameterLinkChannel,
    pub message: ParameterLinkMessage,
    /// Set for an endless encoder that sends how far it turned: a copy of
    /// the package's, as the message is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative: Option<RelativeEncoding>,
}

/// The portable `.rfmap` file: one controller map and where it came from.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RfMapFile {
    pub format: String,
    pub schema_version: u32,
    pub exported_by: String,
    pub exported_unix_ms: u64,
    pub map: ControllerMap,
}

impl RfMapFile {
    pub fn new(map: ControllerMap, exported_by: impl Into<String>, exported_unix_ms: u64) -> Self {
        Self {
            format: RFMAP_FORMAT.into(),
            schema_version: RFMAP_SCHEMA_VERSION,
            exported_by: exported_by.into(),
            exported_unix_ms,
            map,
        }
    }

    pub fn validate(&self) -> Result<(), MidiRoutingError> {
        if self.format != RFMAP_FORMAT {
            return Err(invalid(format!("not an .rfmap file ({:?})", self.format)));
        }
        if self.schema_version != RFMAP_SCHEMA_VERSION {
            return Err(invalid(format!(
                "unsupported .rfmap schema {}",
                self.schema_version
            )));
        }
        self.map.validate()
    }
}

impl ControllerMap {
    pub fn new(controller_id: impl Into<String>, controller_name: impl Into<String>) -> Self {
        Self {
            schema_version: CONTROLLER_MAP_SCHEMA_VERSION,
            controller_id: controller_id.into(),
            controller_name: controller_name.into(),
            modifier: None,
            plugins: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), MidiRoutingError> {
        if self.schema_version != CONTROLLER_MAP_SCHEMA_VERSION {
            return Err(invalid(format!(
                "unsupported controller map schema {}",
                self.schema_version
            )));
        }
        validate_key(&self.controller_id, "controller id")?;
        validate_text(&self.controller_name, "controller name")?;
        if let Some(modifier) = &self.modifier {
            validate_key(&modifier.input.id, "Fn button input id")?;
            validate_text(&modifier.input.name, "Fn button input name")?;
            if matches!(
                modifier.input.message,
                ParameterLinkMessage::PitchBend | ParameterLinkMessage::ChannelPressure
            ) {
                return Err(invalid(
                    "the Fn button must send a note or a control change".into(),
                ));
            }
        }
        let mut plugins = BTreeSet::new();
        let mut mapping_ids = BTreeSet::new();
        let mut count = 0usize;
        for plugin in &self.plugins {
            validate_key(&plugin.plugin_id, "plugin id")?;
            validate_text(&plugin.plugin_name, "plugin name")?;
            if !plugins.insert(plugin.plugin_id.as_str()) {
                return Err(invalid(format!(
                    "plugin {:?} appears twice",
                    plugin.plugin_id
                )));
            }
            let mut inputs = BTreeSet::new();
            let mut messages: Vec<(MapLayer, ParameterLinkMessage, ParameterLinkChannel)> =
                Vec::new();
            for mapping in &plugin.mappings {
                count += 1;
                if count > MAX_CONTROLLER_MAP_MAPPINGS {
                    return Err(invalid("too many mappings".into()));
                }
                if !mapping_ids.insert(mapping.id.as_str()) {
                    return Err(invalid(format!("mapping id {} appears twice", mapping.id)));
                }
                validate_key(&mapping.input.id, "input id")?;
                validate_text(&mapping.input.name, "input name")?;
                validate_key(&mapping.parameter_id, "parameter id")?;
                mapping.mode.validate()?;
                if mapping.input.relative.is_some() {
                    crate::validate_relative(mapping.input.message, &mapping.mode)?;
                }
                if let ParameterLinkMessage::ControlChange { controller }
                | ParameterLinkMessage::Note { note: controller }
                | ParameterLinkMessage::PolyPressure { note: controller } = mapping.input.message
                    && controller > 127
                {
                    return Err(MidiRoutingError::InvalidParameterLinkNumber);
                }
                // The Fn button opens a layer; it does nothing else.
                if self.modifier.as_ref().is_some_and(|modifier| {
                    modifier.input.id == mapping.input.id
                        || (modifier.input.message == mapping.input.message
                            && modifier.input.channel == mapping.input.channel)
                }) {
                    return Err(invalid(format!(
                        "input {:?} is the Fn button and cannot be mapped",
                        mapping.input.id
                    )));
                }
                // One mapping per input, plugin and layer: the input does one
                // thing in each plugin, and one more with Fn. Two inputs may
                // drive the same parameter -- two buttons that each set the
                // Leslie to a speed.
                if !inputs.insert((mapping.layer, mapping.input.id.as_str())) {
                    return Err(invalid(format!(
                        "input {:?} is mapped twice in {:?}",
                        mapping.input.id, plugin.plugin_id
                    )));
                }
                // The same control under two names -- learnt from its message
                // once, picked from the package's list another time -- is
                // still one control.
                let heard = (mapping.layer, mapping.input.message, mapping.input.channel);
                if messages.contains(&heard) {
                    return Err(invalid(format!(
                        "two mappings in {:?} listen to the same message",
                        plugin.plugin_id
                    )));
                }
                messages.push(heard);
            }
        }
        Ok(())
    }

    pub fn plugin(&self, plugin_id: &str) -> Option<&PluginControlMap> {
        self.plugins
            .iter()
            .find(|plugin| plugin.plugin_id == plugin_id)
    }

    /// Whether the map holds nothing: no mapping and no Fn button.
    pub fn is_empty(&self) -> bool {
        // A chosen Fn button is kept even before anything is mapped with it.
        self.modifier.is_none() && self.plugins.iter().all(|plugin| plugin.mappings.is_empty())
    }
}

fn invalid(message: String) -> MidiRoutingError {
    MidiRoutingError::InvalidControllerMap(message)
}

/// Package, plugin, input and parameter ids come from other schemas with
/// their own rules; here they only have to be usable keys.
fn validate_key(value: &str, what: &str) -> Result<(), MidiRoutingError> {
    if value.trim().is_empty()
        || value.contains('\0')
        || value.chars().any(char::is_control)
        || value.chars().count() > MAX_TEXT_CHARS
    {
        return Err(invalid(format!("{what} {value:?} is not a usable key")));
    }
    Ok(())
}

fn validate_text(value: &str, what: &str) -> Result<(), MidiRoutingError> {
    if value.trim().is_empty() || value.contains('\0') || value.chars().count() > MAX_TEXT_CHARS {
        return Err(invalid(format!("{what} {value:?} is empty or too long")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LinkValue, StepDirection};

    fn mapping(id: &str, input: &str, mode: ParameterLinkMode) -> ControlMapping {
        ControlMapping {
            id: ParameterLinkId::new(id).unwrap(),
            input: MappedInput {
                id: input.into(),
                name: input.into(),
                channel: ParameterLinkChannel::Omni,
                // Each input sends a message of its own, as real ones do.
                message: ParameterLinkMessage::ControlChange {
                    controller: 20 + input.bytes().last().map_or(0, |byte| byte % 10),
                },
                relative: None,
            },
            parameter_id: "leslie.speed".into(),
            mode,
            invert: false,
            pass_through: None,
            layer: MapLayer::Base,
        }
    }

    /// An input does one thing per layer: once in the base layer and once
    /// with Fn. The Fn button itself is mapped in neither.
    #[test]
    fn an_input_does_one_thing_in_each_layer() {
        let base = mapping("map.base", "knob-1", ParameterLinkMode::Direct);
        let mut with_fn = mapping("map.fn", "knob-1", ParameterLinkMode::Direct);
        with_fn.layer = MapLayer::Fn;
        let mut layered = map(vec![base.clone(), with_fn.clone()]);
        layered.validate().unwrap();

        let mut twice = with_fn.clone();
        twice.id = ParameterLinkId::new("map.fn-2").unwrap();
        assert!(map(vec![base.clone(), with_fn, twice]).validate().is_err());

        layered.modifier = Some(ControllerModifier {
            input: mapping("map.shift", "button-9", ParameterLinkMode::Direct).input,
            mode: ModifierMode::default(),
        });
        layered.validate().unwrap();
        layered.modifier = Some(ControllerModifier {
            input: base.input.clone(),
            mode: ModifierMode::Hold,
        });
        assert!(
            layered.validate().is_err(),
            "the Fn button is a mapped knob"
        );

        let text = serde_json::to_string(&layered).unwrap();
        assert!(text.contains("\"layer\":\"fn\""));
        assert!(text.contains("\"mode\":\"hold\""));
        let back: ControllerMap = serde_json::from_str(&text).unwrap();
        assert_eq!(back, layered);
    }

    fn map(mappings: Vec<ControlMapping>) -> ControllerMap {
        let mut map = ControllerMap::new("user.oxygen-49", "Oxygen 49");
        map.plugins.push(PluginControlMap {
            plugin_id: "org.rackforge.organ".into(),
            plugin_name: "RF-Organ".into(),
            mappings,
        });
        map
    }

    fn value(value: f64) -> LinkValue {
        LinkValue::new(value).unwrap()
    }

    #[test]
    fn a_map_round_trips_through_an_rfmap_file() {
        let original = map(vec![mapping(
            "map.leslie",
            "button-1",
            ParameterLinkMode::Toggle {
                first: value(1.0),
                second: value(2.0),
            },
        )]);
        original.validate().unwrap();
        let file = RfMapFile::new(original.clone(), "RackForge test", 1);
        let text = serde_json::to_string_pretty(&file).unwrap();
        let back: RfMapFile = serde_json::from_str(&text).unwrap();
        back.validate().unwrap();
        assert_eq!(back.map, original);
        assert!(text.contains("\"format\": \"org.rackforge.map\""));
    }

    #[test]
    fn a_button_consumes_and_a_knob_passes_through() {
        let button = mapping("a", "button-1", ParameterLinkMode::Trigger);
        assert_eq!(
            button.effective_pass_through(),
            ParameterLinkPassThrough::Consume
        );
        let knob = mapping("b", "knob-1", ParameterLinkMode::Direct);
        assert_eq!(
            knob.effective_pass_through(),
            ParameterLinkPassThrough::PassThrough
        );
    }

    #[test]
    fn an_input_does_one_thing_in_each_plugin() {
        let twice = map(vec![
            mapping("a", "button-1", ParameterLinkMode::Trigger),
            mapping(
                "b",
                "button-1",
                ParameterLinkMode::Step {
                    direction: StepDirection::Up,
                    wrap: false,
                },
            ),
        ]);
        assert!(twice.validate().is_err());

        let two_buttons_one_parameter = map(vec![
            mapping(
                "a",
                "button-1",
                ParameterLinkMode::Set { value: value(1.0) },
            ),
            mapping(
                "b",
                "button-2",
                ParameterLinkMode::Set { value: value(2.0) },
            ),
        ]);
        two_buttons_one_parameter.validate().unwrap();
    }

    #[test]
    fn one_control_under_two_names_is_still_one_control() {
        let mut learnt = mapping("a", "cc.0.21", ParameterLinkMode::Trigger);
        let picked = mapping("b", "button-1", ParameterLinkMode::Trigger);
        learnt.input.message = picked.input.message;
        assert!(map(vec![learnt, picked]).validate().is_err());
    }

    #[test]
    fn a_cycle_needs_two_values_and_values_are_finite() {
        let short = map(vec![mapping(
            "a",
            "button-1",
            ParameterLinkMode::Cycle {
                values: vec![value(1.0)],
            },
        )]);
        assert!(short.validate().is_err());
        assert!(LinkValue::new(f64::NAN).is_err());
        assert!(serde_json::from_str::<LinkValue>("1e999").is_err());
    }

    #[test]
    fn a_file_of_another_kind_is_refused() {
        let mut file = RfMapFile::new(map(Vec::new()), "test", 1);
        file.format = "org.rackforge.preset".into();
        assert!(file.validate().is_err());
    }
}
