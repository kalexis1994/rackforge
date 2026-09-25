//! A plugin's control layout: which of its parameters a keyboard's controls
//! move, named by slot rather than by keyboard.
//!
//! A plugin says what goes in each slot -- its eight most played continuous
//! controls in `control-1`, the next eight in `control-2`, its switches in
//! `switch-1` -- and a controller package says which of its controls fills
//! each slot. RackForge puts the two together into the map each keyboard
//! is offered, so a plugin is laid out once for every keyboard, a keyboard
//! once for every plugin, and a control of the same kind does the same
//! thing everywhere.
//!
//! A plugin package carries its layout in [`CONTROL_LAYOUT_FILE`];
//! RackForge ships the layouts of its own instruments for packages that do
//! not carry one yet.

use crate::controller_map::{ControlMapping, ControllerMap, MappedInput, PluginControlMap};
use crate::{
    MapLayer, MidiRoutingError, ParameterLinkId, ParameterLinkMode, ParameterLinkPassThrough,
    StepDirection,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

pub const CONTROL_LAYOUT_FORMAT: &str = "org.rackforge.control-layout";
pub const CONTROL_LAYOUT_SCHEMA_VERSION: u32 = 1;
/// Where a plugin package carries its layout, if it has one.
pub const CONTROL_LAYOUT_FILE: &str = "metadata/control-layout.json";
/// Continuous rows and switch banks there are, and slots in each.
pub const SLOT_ROWS: u8 = 3;
pub const SLOTS_PER_ROW: u8 = 8;
/// Pairs of step buttons there are.
pub const STEP_PAIRS: u8 = 2;

/// A place on a keyboard, as a plugin and a controller package both name it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ControlSlot {
    /// A knob, fader or encoder in one of three rows, by importance:
    /// `control-1.3`. Row 1 holds the eight controls played most.
    Control { row: u8, index: u8 },
    /// A button or pad in one of three banks: `switch-2.5`. Bank 1 holds
    /// the switches played most.
    Switch { bank: u8, index: u8 },
    /// A button that steps a selector down or up: `step.down`, `step-2.up`.
    Step { pair: u8, direction: StepDirection },
    /// The modulation wheel: `mod-wheel`.
    ModWheel,
}

impl ControlSlot {
    /// Whether a knob, fader or wheel fills it, rather than a button.
    pub fn is_continuous(self) -> bool {
        matches!(self, Self::Control { .. } | Self::ModWheel)
    }
}

impl fmt::Display for ControlSlot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Control { row, index } => write!(formatter, "control-{row}.{index}"),
            Self::Switch { bank, index } => write!(formatter, "switch-{bank}.{index}"),
            Self::Step { pair, direction } => {
                let direction = match direction {
                    StepDirection::Down => "down",
                    StepDirection::Up => "up",
                };
                if *pair == 1 {
                    write!(formatter, "step.{direction}")
                } else {
                    write!(formatter, "step-{pair}.{direction}")
                }
            }
            Self::ModWheel => formatter.write_str("mod-wheel"),
        }
    }
}

impl FromStr for ControlSlot {
    type Err = MidiRoutingError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let unknown = || invalid(format!("{text:?} is not a control slot"));
        if text == "mod-wheel" {
            return Ok(Self::ModWheel);
        }
        let (head, tail) = text.split_once('.').ok_or_else(unknown)?;
        // A number in `first..=last`, in one spelling only: "01" is not "1".
        let number = |digits: &str, first: u8, last: u8| -> Option<u8> {
            let number = digits.parse::<u8>().ok()?;
            (digits == number.to_string() && (first..=last).contains(&number)).then_some(number)
        };
        if let Some(row) = head.strip_prefix("control-") {
            let row = number(row, 1, SLOT_ROWS).ok_or_else(unknown)?;
            let index = number(tail, 1, SLOTS_PER_ROW).ok_or_else(unknown)?;
            return Ok(Self::Control { row, index });
        }
        if let Some(bank) = head.strip_prefix("switch-") {
            let bank = number(bank, 1, SLOT_ROWS).ok_or_else(unknown)?;
            let index = number(tail, 1, SLOTS_PER_ROW).ok_or_else(unknown)?;
            return Ok(Self::Switch { bank, index });
        }
        // The first pair is `step`; the others carry their number.
        let pair = match head.strip_prefix("step-") {
            Some(pair) => number(pair, 2, STEP_PAIRS).ok_or_else(unknown)?,
            None if head == "step" => 1,
            None => return Err(unknown()),
        };
        let direction = match tail {
            "down" => StepDirection::Down,
            "up" => StepDirection::Up,
            _ => return Err(unknown()),
        };
        Ok(Self::Step { pair, direction })
    }
}

impl Serialize for ControlSlot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ControlSlot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// What a plugin puts in one slot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotMapping {
    pub slot: ControlSlot,
    /// The parameter's stable schema `id`.
    pub parameter_id: String,
    #[serde(default)]
    pub mode: ParameterLinkMode,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass_through: Option<ParameterLinkPassThrough>,
}

/// A plugin's control layout, as its `.json` document.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlLayout {
    pub format: String,
    pub schema_version: u32,
    pub plugin_id: String,
    /// For display, in maps made from the layout.
    pub plugin_name: String,
    pub slots: Vec<SlotMapping>,
}

impl ControlLayout {
    pub fn validate(&self) -> Result<(), MidiRoutingError> {
        if self.format != CONTROL_LAYOUT_FORMAT {
            return Err(invalid(format!("not a control layout ({:?})", self.format)));
        }
        if self.schema_version != CONTROL_LAYOUT_SCHEMA_VERSION {
            return Err(invalid(format!(
                "unsupported control layout schema {}",
                self.schema_version
            )));
        }
        crate::validate_identifier(&self.plugin_id).map_err(|_| {
            invalid(format!(
                "plugin id {:?} is not an identifier",
                self.plugin_id
            ))
        })?;
        if self.plugin_name.trim().is_empty() || self.plugin_name.chars().count() > 128 {
            return Err(invalid("the plugin name is empty or too long".into()));
        }
        let mut slots = BTreeSet::new();
        for mapping in &self.slots {
            if !slots.insert(mapping.slot) {
                return Err(invalid(format!("slot {} appears twice", mapping.slot)));
            }
            if mapping.parameter_id.trim().is_empty()
                || mapping.parameter_id.chars().any(char::is_control)
                || mapping.parameter_id.chars().count() > 128
            {
                return Err(invalid(format!(
                    "slot {} names no usable parameter",
                    mapping.slot
                )));
            }
            mapping.mode.validate()?;
            // A knob follows a position; a button acts on a press.
            if mapping.slot.is_continuous() == mapping.mode.is_button() {
                return Err(invalid(format!(
                    "slot {} does not take a {} mode",
                    mapping.slot,
                    if mapping.mode.is_button() {
                        "button"
                    } else {
                        "knob"
                    }
                )));
            }
        }
        Ok(())
    }
}

/// One control of a keyboard, and the slot its package says it fills.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlottedInput {
    pub slot: ControlSlot,
    pub input: MappedInput,
}

/// The map a keyboard is offered: each layout's slots on the controls that
/// fill them, and `modifier`, the button its package names as Fn, if any.
///
/// A keyboard without a row of continuous controls does not lose that
/// row's parameters: they move, in order, onto the controls of its other
/// rows that the plugin leaves empty. A keyboard with eight knobs and no
/// faders plays the organ's drawbars on them, and its gaps take the next
/// row's first controls. Switches and steps stay where they are named.
///
/// The Fn layer holds what the keyboard lacks: each row, bank or step pair
/// it has no controls for, in order, on the rows, banks or pairs it has.
/// With one row of knobs, Fn + knob 3 is `control-2.3`; with faders and
/// knobs, Fn + fader 3 is `control-3.3`; with two pad banks, Fn + pad is
/// the third bank.
pub fn derive_controller_map(
    controller_id: &str,
    controller_name: &str,
    inputs: &[SlottedInput],
    modifier: Option<&MappedInput>,
    layouts: &[ControlLayout],
) -> ControllerMap {
    let mut map = ControllerMap::new(controller_id, controller_name);
    map.modifier = modifier.map(|input| crate::controller_map::ControllerModifier {
        input: input.clone(),
        mode: crate::ModifierMode::default(),
    });
    let rows: BTreeSet<u8> = inputs
        .iter()
        .filter_map(|input| match input.slot {
            ControlSlot::Control { row, .. } => Some(row),
            _ => None,
        })
        .collect();
    let fn_slot = fn_layer_shift(inputs);
    for layout in layouts {
        let mut placed: Vec<(&SlottedInput, &SlotMapping, MapLayer)> = Vec::new();
        for mapping in &layout.slots {
            if let Some(input) = inputs.iter().find(|input| input.slot == mapping.slot) {
                placed.push((input, mapping, MapLayer::Base));
            }
        }
        // The rows this keyboard lacks, in order, onto the continuous
        // controls the layout leaves empty, in order.
        let mut movers = layout
            .slots
            .iter()
            .filter(|mapping| {
                matches!(mapping.slot, ControlSlot::Control { row, .. } if !rows.contains(&row))
            })
            .collect::<Vec<_>>();
        movers.sort_by_key(|mapping| mapping.slot);
        let mut gaps = inputs
            .iter()
            .filter(|input| {
                matches!(input.slot, ControlSlot::Control { .. })
                    && !layout
                        .slots
                        .iter()
                        .any(|mapping| mapping.slot == input.slot)
            })
            .collect::<Vec<_>>();
        gaps.sort_by_key(|input| input.slot);
        let mut gaps = gaps.into_iter();
        for mover in movers {
            let taken = placed.iter().any(|(input, mapping, _)| {
                input.slot.is_continuous() && mapping.parameter_id == mover.parameter_id
            });
            if taken {
                continue;
            }
            let Some(gap) = gaps.next() else { break };
            placed.push((gap, mover, MapLayer::Base));
        }
        // With Fn, each control reaches the slot the keyboard lacks.
        for input in inputs {
            let Some(target) = fn_slot(input.slot) else {
                continue;
            };
            if let Some(mapping) = layout.slots.iter().find(|mapping| mapping.slot == target) {
                placed.push((input, mapping, MapLayer::Fn));
            }
        }
        // In the keyboard's own order, as its editor lists its controls,
        // the base layer first.
        placed.sort_by_key(|(input, _, layer)| {
            (
                *layer,
                inputs
                    .iter()
                    .position(|candidate| std::ptr::eq(candidate, *input)),
            )
        });
        let mappings = placed
            .into_iter()
            .map(|(input, mapping, layer)| ControlMapping {
                id: match layer {
                    MapLayer::Base => link_id(&[controller_id, &layout.plugin_id, &input.input.id]),
                    MapLayer::Fn => {
                        link_id(&[controller_id, &layout.plugin_id, "fn", &input.input.id])
                    }
                },
                input: input.input.clone(),
                parameter_id: mapping.parameter_id.clone(),
                mode: mapping.mode.clone(),
                invert: mapping.invert,
                pass_through: mapping.pass_through,
                layer,
            })
            .collect::<Vec<_>>();
        if !mappings.is_empty() {
            map.plugins.push(PluginControlMap {
                plugin_id: layout.plugin_id.clone(),
                plugin_name: layout.plugin_name.clone(),
                mappings,
            });
        }
    }
    map
}

/// Where Fn takes each of a keyboard's slots: the rows, banks and step
/// pairs it lacks, in order, onto those it has, in order.
fn fn_layer_shift(inputs: &[SlottedInput]) -> impl Fn(ControlSlot) -> Option<ControlSlot> {
    let has = |group: fn(ControlSlot) -> Option<u8>, count: u8| {
        let present: Vec<u8> = (1..=count)
            .filter(|number| {
                inputs
                    .iter()
                    .any(|input| group(input.slot) == Some(*number))
            })
            .collect();
        let missing: Vec<u8> = (1..=count)
            .filter(|number| !present.contains(number))
            .collect();
        present
            .into_iter()
            .zip(missing)
            .collect::<std::collections::BTreeMap<u8, u8>>()
    };
    let rows = has(
        |slot| match slot {
            ControlSlot::Control { row, .. } => Some(row),
            _ => None,
        },
        SLOT_ROWS,
    );
    let banks = has(
        |slot| match slot {
            ControlSlot::Switch { bank, .. } => Some(bank),
            _ => None,
        },
        SLOT_ROWS,
    );
    let pairs = has(
        |slot| match slot {
            ControlSlot::Step { pair, .. } => Some(pair),
            _ => None,
        },
        STEP_PAIRS,
    );
    move |slot| match slot {
        ControlSlot::Control { row, index } => rows
            .get(&row)
            .map(|row| ControlSlot::Control { row: *row, index }),
        ControlSlot::Switch { bank, index } => banks
            .get(&bank)
            .map(|bank| ControlSlot::Switch { bank: *bank, index }),
        ControlSlot::Step { pair, direction } => pairs.get(&pair).map(|pair| ControlSlot::Step {
            pair: *pair,
            direction,
        }),
        ControlSlot::ModWheel => None,
    }
}

/// A mapping id from ids with their own rules, made a link id: lowercase,
/// and anything a link id may not hold becomes a hyphen.
fn link_id(parts: &[&str]) -> ParameterLinkId {
    let joined = parts
        .iter()
        .map(|part| {
            part.chars()
                .map(|character| {
                    let lower = character.to_ascii_lowercase();
                    if lower.is_ascii_lowercase() || lower.is_ascii_digit() || "-_.".contains(lower)
                    {
                        lower
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
                .trim_matches('.')
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join(".");
    let mut id = String::with_capacity(joined.len());
    for character in joined.chars() {
        if character == '.' && id.ends_with('.') {
            continue;
        }
        id.push(character);
    }
    ParameterLinkId::new(id).unwrap_or_else(|_| {
        ParameterLinkId::new("controller-map.mapping").expect("a valid fallback id")
    })
}

fn invalid(message: String) -> MidiRoutingError {
    MidiRoutingError::InvalidControllerMap(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LinkValue, MidiChannel, ParameterLinkChannel, ParameterLinkMessage};

    fn slot(text: &str) -> ControlSlot {
        text.parse().unwrap()
    }

    fn cc_input(slot_name: &str, id: &str, controller: u8) -> SlottedInput {
        SlottedInput {
            slot: slot(slot_name),
            input: MappedInput {
                id: id.into(),
                name: id.into(),
                channel: ParameterLinkChannel::Channel {
                    channel: MidiChannel::from_zero_based(0).unwrap(),
                },
                message: ParameterLinkMessage::ControlChange { controller },
            },
        }
    }

    fn direct(slot_name: &str, parameter: &str) -> SlotMapping {
        SlotMapping {
            slot: slot(slot_name),
            parameter_id: parameter.into(),
            mode: ParameterLinkMode::Direct,
            invert: false,
            pass_through: None,
        }
    }

    fn layout(slots: Vec<SlotMapping>) -> ControlLayout {
        ControlLayout {
            format: CONTROL_LAYOUT_FORMAT.into(),
            schema_version: CONTROL_LAYOUT_SCHEMA_VERSION,
            plugin_id: "org.rackforge.organ".into(),
            plugin_name: "RF-Organ".into(),
            slots,
        }
    }

    #[test]
    fn a_slot_has_one_spelling() {
        for text in [
            "control-1.1",
            "control-3.8",
            "switch-2.5",
            "step.down",
            "step-2.up",
            "mod-wheel",
        ] {
            assert_eq!(slot(text).to_string(), text);
        }
        for text in [
            "control-0.1",
            "control-4.1",
            "control-1.9",
            "control-1.01",
            "switch-1.0",
            "step-1.up",
            "step-3.up",
            "step.left",
            "modwheel",
            "control-1",
            "",
        ] {
            assert!(text.parse::<ControlSlot>().is_err(), "{text}");
        }
    }

    #[test]
    fn a_layout_puts_one_thing_in_a_slot_and_a_button_mode_on_a_button() {
        layout(vec![direct("control-1.1", "drawbar-16")])
            .validate()
            .unwrap();
        assert!(
            layout(vec![direct("control-1.1", "a"), direct("control-1.1", "b")])
                .validate()
                .is_err()
        );
        assert!(
            layout(vec![direct("switch-1.1", "percussion")])
                .validate()
                .is_err()
        );
        let mut toggle = direct("control-1.1", "percussion");
        toggle.mode = ParameterLinkMode::Toggle {
            first: LinkValue::new(0.0).unwrap(),
            second: LinkValue::new(1.0).unwrap(),
        };
        assert!(layout(vec![toggle]).validate().is_err());
    }

    #[test]
    fn a_keyboard_gets_each_slot_on_the_control_that_fills_it() {
        let inputs = [
            cc_input("control-2.1", "knob-1", 21),
            cc_input("control-1.1", "fader-1", 71),
        ];
        let map = derive_controller_map(
            "org.rackforge.novation-launchkey-mk3-49",
            "Novation Launchkey 49 [MK3]",
            &inputs,
            None,
            &[layout(vec![
                direct("control-1.1", "drawbar-16"),
                direct("control-2.1", "expression"),
                direct("control-3.1", "leakage"),
            ])],
        );
        map.validate().unwrap();
        let mappings = &map.plugins[0].mappings;
        let pairs = mappings
            .iter()
            .map(|mapping| {
                (
                    mapping.layer,
                    mapping.input.id.as_str(),
                    mapping.parameter_id.as_str(),
                )
            })
            .collect::<Vec<_>>();
        // In the keyboard's order, the base layer first; no gap for the
        // third row, which Fn + the first row reaches.
        assert_eq!(
            pairs,
            [
                (MapLayer::Base, "knob-1", "expression"),
                (MapLayer::Base, "fader-1", "drawbar-16"),
                (MapLayer::Fn, "fader-1", "leakage"),
            ]
        );
        assert_eq!(
            mappings[1].id.as_str(),
            "org.rackforge.novation-launchkey-mk3-49.org.rackforge.organ.fader-1"
        );
        assert_eq!(
            mappings[2].id.as_str(),
            "org.rackforge.novation-launchkey-mk3-49.org.rackforge.organ.fn.fader-1"
        );
        assert_eq!(map.modifier, None);
    }

    /// Fn reaches what a keyboard lacks: a second pad bank on a keyboard
    /// with one, the second step pair, and the button its package names
    /// becomes the Fn button.
    #[test]
    fn fn_reaches_the_banks_and_pairs_a_keyboard_lacks() {
        let pad = |slot: &str, id: &str, note: u8| SlottedInput {
            slot: slot.parse().unwrap(),
            input: MappedInput {
                id: id.into(),
                name: id.into(),
                channel: ParameterLinkChannel::Channel {
                    channel: MidiChannel::from_zero_based(9).unwrap(),
                },
                message: ParameterLinkMessage::Note { note },
            },
        };
        let inputs = [pad("switch-1.1", "pad-1", 40), pad("step.up", "right", 41)];
        let toggle = |slot: &str, parameter: &str| SlotMapping {
            mode: ParameterLinkMode::Toggle {
                first: crate::LinkValue::new(1.0).unwrap(),
                second: crate::LinkValue::new(0.0).unwrap(),
            },
            ..direct(slot, parameter)
        };
        let step = |slot: &str, parameter: &str| SlotMapping {
            mode: ParameterLinkMode::Step {
                direction: crate::StepDirection::Up,
                wrap: false,
            },
            ..direct(slot, parameter)
        };
        let shift = cc_input("control-1.1", "shift", 99).input;
        let map = derive_controller_map(
            "org.rackforge.pads",
            "Pads",
            &inputs,
            Some(&shift),
            &[layout(vec![
                toggle("switch-1.1", "percussion"),
                toggle("switch-2.1", "vibrato"),
                step("step.up", "registration"),
                step("step-2.up", "scanner"),
            ])],
        );
        map.validate().unwrap();
        let fn_layer = map.plugins[0]
            .mappings
            .iter()
            .filter(|mapping| mapping.layer == MapLayer::Fn)
            .map(|mapping| (mapping.input.id.as_str(), mapping.parameter_id.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(fn_layer, [("pad-1", "vibrato"), ("right", "scanner")]);
        assert_eq!(
            map.modifier
                .as_ref()
                .map(|modifier| modifier.input.id.as_str()),
            Some("shift")
        );
    }

    #[test]
    fn a_row_a_keyboard_lacks_moves_into_the_gaps_of_the_rows_it_has() {
        let inputs = (1..=4)
            .map(|index| {
                cc_input(
                    &format!("control-1.{index}"),
                    &format!("knob-{index}"),
                    20 + index,
                )
            })
            .collect::<Vec<_>>();
        let map = derive_controller_map(
            "org.rackforge.small",
            "Small",
            &inputs,
            None,
            &[layout(vec![
                direct("control-1.1", "bass"),
                direct("control-1.3", "treble"),
                direct("control-2.1", "hardness"),
                direct("control-2.2", "treble"),
                direct("control-2.3", "bell"),
                direct("control-3.1", "sustain"),
            ])],
        );
        let pairs = map.plugins[0]
            .mappings
            .iter()
            .filter(|mapping| mapping.layer == MapLayer::Base)
            .map(|mapping| (mapping.input.id.as_str(), mapping.parameter_id.as_str()))
            .collect::<Vec<_>>();
        // Treble is on knob 3 already: the second row's copy does not take
        // a gap. Row 3 comes after row 2, and runs out of gaps.
        assert_eq!(
            pairs,
            [
                ("knob-1", "bass"),
                ("knob-2", "hardness"),
                ("knob-3", "treble"),
                ("knob-4", "bell")
            ]
        );
    }

    #[test]
    fn a_layout_reads_from_its_json_document() {
        let text = r#"{
            "format": "org.rackforge.control-layout",
            "schema_version": 1,
            "plugin_id": "org.rackforge.organ",
            "plugin_name": "RF-Organ",
            "slots": [
                { "slot": "step.up", "parameter_id": "registration",
                  "mode": { "kind": "step", "direction": "up" } }
            ]
        }"#;
        let layout: ControlLayout = serde_json::from_str(text).unwrap();
        layout.validate().unwrap();
        assert_eq!(
            layout.slots[0].slot,
            ControlSlot::Step {
                pair: 1,
                direction: StepDirection::Up
            }
        );
        assert!(
            serde_json::to_string(&layout)
                .unwrap()
                .contains("\"slot\":\"step.up\"")
        );
    }
}
