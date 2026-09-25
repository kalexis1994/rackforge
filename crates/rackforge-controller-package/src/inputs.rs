//! Schema 2's physical layer: the controls a controller has, and the
//! meanings a package gives them by name.
//!
//! Schema 1 described mappings, each repeating the MIDI message it listened
//! to; a control without a mapping did not exist. Schema 2 lists the controls
//! first -- every knob, fader, button, pad and wheel, with the message it
//! sends -- and roles and host actions refer to them by id. RackForge lowers
//! those into the schema 1 runtime structures, so every host that runs a
//! schema 1 package runs a schema 2 one unchanged.

use rackforge_control_profile::{CONTROL_PROFILE_SCHEMA_VERSION, SemanticControlId};
use rackforge_controller_api::{
    HeldControl, HostActionBinding, HostActionTarget, MidiButtonBinding, MidiControlChangeBinding,
    MidiNoteButtonBinding, MidiRealtime, SemanticControlBinding, SemanticControlMode,
    SemanticControlProfile,
};
use rackforge_midi_api::control_layout::{ControlSlot, SlottedInput};
use rackforge_midi_api::controller_map::MappedInput;
use rackforge_midi_api::{
    MidiChannel, ParameterLinkChannel, ParameterLinkMessage, RelativeEncoding,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const MAX_LABEL_CHARS: usize = 48;

/// What a physical control is. It decides which messages it may send, and
/// which modes a player can give it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Knob,
    Fader,
    Encoder,
    Button,
    Pad,
    Wheel,
    Pedal,
}

/// The message a control sends: exactly one of a control change, a note or
/// pitch bend on a zero-based channel, or a System Real Time message, which
/// has no channel.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputMidi {
    /// Required for a channel message; left out for a real time one.
    #[serde(default = "no_channel", skip_serializing_if = "is_no_channel")]
    pub channel: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cc: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<u8>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pitch_bend: bool,
    /// MIDI Start, Continue or Stop: what some keyboards' Play and Stop send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realtime: Option<MidiRealtime>,
}

/// What `channel` holds when a manifest leaves it out. Not a MIDI channel:
/// a channel message without one is refused, rather than put on channel 1.
pub const NO_CHANNEL: u8 = u8::MAX;

const fn no_channel() -> u8 {
    NO_CHANNEL
}

fn is_no_channel(channel: &u8) -> bool {
    *channel == NO_CHANNEL
}

/// An input's message reduced to what makes it unique: two controls never
/// send the same one.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum InputMessage {
    ControlChange { channel: u8, controller: u8 },
    Note { channel: u8, note: u8 },
    PitchBend { channel: u8 },
    Realtime(MidiRealtime),
}

impl InputMidi {
    pub fn message(self) -> Result<InputMessage, String> {
        if let Some(message) = self.realtime {
            if self.cc.is_some() || self.note.is_some() || self.pitch_bend {
                return Err(
                    "an input sends exactly one of cc, note, pitch_bend or realtime".into(),
                );
            }
            if self.channel != NO_CHANNEL {
                return Err("a real time message has no MIDI channel".into());
            }
            return Ok(InputMessage::Realtime(message));
        }
        if self.channel == NO_CHANNEL {
            return Err("a channel message needs its MIDI channel".into());
        }
        if self.channel > 15 {
            return Err(format!("MIDI channel {} is outside 0..15", self.channel));
        }
        match (self.cc, self.note, self.pitch_bend) {
            (Some(controller), None, false) => {
                if controller > 119 {
                    return Err(format!(
                        "MIDI CC {controller} is a channel-mode message, not a control"
                    ));
                }
                Ok(InputMessage::ControlChange {
                    channel: self.channel,
                    controller,
                })
            }
            (None, Some(note), false) => {
                if note > 127 {
                    return Err(format!("MIDI note {note} is outside 0..127"));
                }
                Ok(InputMessage::Note {
                    channel: self.channel,
                    note,
                })
            }
            (None, None, true) => Ok(InputMessage::PitchBend {
                channel: self.channel,
            }),
            _ => Err("an input sends exactly one of cc, note, pitch_bend or realtime".into()),
        }
    }
}

/// How a button reports itself. The values apply to a button that sends a
/// control change; a button that sends a note presses with a velocity and
/// releases with a note-off.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonReport {
    #[serde(default = "default_press")]
    pub press: u8,
    #[serde(default)]
    pub release: u8,
    /// The button reports only presses: there is no release to wait for.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub press_only: bool,
    /// The hardware toggles by itself: one press sends `press`, the next
    /// sends `release`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub latching: bool,
}

const fn default_press() -> u8 {
    127
}

impl Default for ButtonReport {
    fn default() -> Self {
        Self {
            press: default_press(),
            release: 0,
            press_only: false,
            latching: false,
        }
    }
}

/// How an endless encoder reports turning.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncoderEncoding {
    /// Reports a position, like a knob, without end stops.
    #[default]
    Absolute,
    RelativeTwosComplement,
    RelativeBinaryOffset,
    RelativeSignMagnitude,
}

/// One physical control.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerInput {
    /// Stable across package versions: a player's mappings refer to it.
    pub id: String,
    pub name: String,
    pub kind: InputKind,
    /// Only orders the editor's list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub midi: InputMidi,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<ButtonReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder: Option<EncoderEncoding>,
    /// The place this control fills in every plugin's control layout: what
    /// it does in each instrument RackForge offers a map for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<ControlSlot>,
    /// The button that opens the Fn layer, where the hardware sends it and
    /// nothing else changes while it is held. At most one per package.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub modifier: bool,
    /// False for a control whose messages must never reach an instrument,
    /// mapped or not: a DAW protocol's fader that sends pitch bend, or its
    /// buttons' notes. Maps and host actions still read it.
    #[serde(default = "plays_by_default", skip_serializing_if = "plays_as_usual")]
    pub plays: bool,
}

const fn plays_by_default() -> bool {
    true
}

fn plays_as_usual(plays: &bool) -> bool {
    *plays
}

impl ControllerInput {
    /// The control's message as one the hosts hold back from instruments,
    /// when the package says it never plays.
    pub fn held_control(&self) -> Option<HeldControl> {
        if self.plays {
            return None;
        }
        match self.midi.message().ok()? {
            InputMessage::ControlChange {
                channel,
                controller,
            } => Some(HeldControl::ControlChange {
                channel,
                controller,
            }),
            InputMessage::Note { channel, note } => Some(HeldControl::Note { channel, note }),
            InputMessage::PitchBend { channel } => Some(HeldControl::PitchBend { channel }),
            InputMessage::Realtime(_) => None,
        }
    }

    /// This control in its slot, with the message a map listens to; `None`
    /// without a slot.
    pub fn slotted_input(&self) -> Option<SlottedInput> {
        Some(SlottedInput {
            slot: self.slot?,
            input: self.mapped_input()?,
        })
    }

    /// The control as a map names it: its id, name and message. `None` for
    /// one a map cannot listen to: the pitch wheel, a real time message. A
    /// fader that sends pitch bend, as in a DAW protocol, is a fader.
    pub fn mapped_input(&self) -> Option<MappedInput> {
        let channel = MidiChannel::from_zero_based(self.midi.channel).ok()?;
        let message = match self.midi.message().ok()? {
            InputMessage::ControlChange { controller, .. } => {
                ParameterLinkMessage::ControlChange { controller }
            }
            InputMessage::Note { note, .. } => ParameterLinkMessage::Note { note },
            InputMessage::PitchBend { .. } if self.kind == InputKind::Fader => {
                ParameterLinkMessage::PitchBend
            }
            InputMessage::PitchBend { .. } | InputMessage::Realtime(_) => return None,
        };
        let relative = match (self.kind, self.encoder_encoding()) {
            (InputKind::Encoder, EncoderEncoding::RelativeTwosComplement) => {
                Some(RelativeEncoding::TwosComplement)
            }
            (InputKind::Encoder, EncoderEncoding::RelativeBinaryOffset) => {
                Some(RelativeEncoding::BinaryOffset)
            }
            (InputKind::Encoder, EncoderEncoding::RelativeSignMagnitude) => {
                Some(RelativeEncoding::SignMagnitude)
            }
            _ => None,
        };
        Some(MappedInput {
            id: self.id.clone(),
            name: self.name.clone(),
            channel: ParameterLinkChannel::Channel { channel },
            message,
            relative,
        })
    }

    /// Whether the control can fill its slot: a knob, fader or encoder a
    /// row of continuous controls, the modulation wheel the wheel's slot,
    /// a button or pad a switch or a step; each sending a control change
    /// or a note a map can listen to.
    fn validate_slot(&self, message: InputMessage) -> Result<(), String> {
        let Some(slot) = self.slot else {
            return Ok(());
        };
        let fits = match slot {
            ControlSlot::Control { .. } => matches!(
                self.kind,
                InputKind::Knob | InputKind::Fader | InputKind::Encoder
            ),
            ControlSlot::ModWheel => self.kind == InputKind::Wheel,
            ControlSlot::Switch { .. } | ControlSlot::Step { .. } => {
                matches!(self.kind, InputKind::Button | InputKind::Pad)
            }
        };
        if !fits {
            return Err(format!(
                "input {:?}: a {:?} cannot fill slot {slot}",
                self.id, self.kind
            ));
        }
        if !matches!(
            message,
            InputMessage::ControlChange { .. } | InputMessage::Note { .. }
        ) && !(self.kind == InputKind::Fader
            && matches!(message, InputMessage::PitchBend { .. }))
        {
            return Err(format!(
                "input {:?}: slot {slot} needs a control change or a note",
                self.id
            ));
        }
        Ok(())
    }

    /// The button report a button input uses, its own or the default.
    pub fn button_report(&self) -> ButtonReport {
        self.button.unwrap_or_default()
    }

    pub fn encoder_encoding(&self) -> EncoderEncoding {
        self.encoder.unwrap_or_default()
    }

    fn validate(&self) -> Result<InputMessage, String> {
        validate_input_id(&self.id)?;
        validate_label(&self.name, "input name")?;
        if let Some(group) = &self.group {
            validate_label(group, "input group")?;
        }
        let message = self
            .midi
            .message()
            .map_err(|error| format!("input {:?}: {error}", self.id))?;
        let allowed = match self.kind {
            InputKind::Knob | InputKind::Encoder | InputKind::Pedal => {
                matches!(message, InputMessage::ControlChange { .. })
            }
            // A DAW protocol's fader sends pitch bend, one channel each.
            InputKind::Fader => matches!(
                message,
                InputMessage::ControlChange { .. } | InputMessage::PitchBend { .. }
            ),
            InputKind::Pad => matches!(message, InputMessage::Note { .. }),
            InputKind::Button => matches!(
                message,
                InputMessage::ControlChange { .. }
                    | InputMessage::Note { .. }
                    | InputMessage::Realtime(_)
            ),
            InputKind::Wheel => matches!(
                message,
                InputMessage::PitchBend { .. } | InputMessage::ControlChange { .. }
            ),
        };
        if !allowed {
            return Err(format!(
                "input {:?}: a {:?} cannot send {message:?}",
                self.id, self.kind
            ));
        }
        if self.button.is_some() && self.kind != InputKind::Button {
            return Err(format!(
                "input {:?}: only a button has a button report",
                self.id
            ));
        }
        if self.button.is_some() && matches!(message, InputMessage::Realtime(_)) {
            return Err(format!(
                "input {:?}: a real time message has no values to report",
                self.id
            ));
        }
        if let Some(report) = self.button {
            if report.press > 127 || report.release > 127 {
                return Err(format!("input {:?}: button values are 0..127", self.id));
            }
            if matches!(message, InputMessage::ControlChange { .. })
                && !report.press_only
                && report.press == report.release
            {
                return Err(format!(
                    "input {:?}: a button's press and release values must differ",
                    self.id
                ));
            }
            if report.press_only && report.latching {
                return Err(format!(
                    "input {:?}: a latching button reports its release",
                    self.id
                ));
            }
        }
        if self.encoder.is_some() && self.kind != InputKind::Encoder {
            return Err(format!(
                "input {:?}: only an encoder has an encoding",
                self.id
            ));
        }
        self.validate_slot(message)?;
        if !self.plays && matches!(message, InputMessage::Realtime(_)) {
            return Err(format!(
                "input {:?}: a real time message never reaches an instrument anyway",
                self.id
            ));
        }
        if self.modifier {
            if !matches!(self.kind, InputKind::Button | InputKind::Pad)
                || !matches!(
                    message,
                    InputMessage::ControlChange { .. } | InputMessage::Note { .. }
                )
            {
                return Err(format!(
                    "input {:?}: the Fn button is a button or pad that sends a note or a control change",
                    self.id
                ));
            }
            if self.slot.is_some() {
                return Err(format!("input {:?}: the Fn button fills no slot", self.id));
            }
        }
        Ok(message)
    }
}

/// A semantic role given to one input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputRole {
    pub input: String,
    pub role: SemanticControlId,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
    /// How the reading moves the role. Absolute unless an encoder that
    /// reports a position is read by the distance it turns: the same
    /// hardware is read either way, so the role says which.
    #[serde(default, skip_serializing_if = "is_absolute")]
    pub mode: SemanticControlMode,
}

fn is_absolute(mode: &SemanticControlMode) -> bool {
    *mode == SemanticControlMode::Absolute
}

/// A host action given to one button.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputAction {
    pub input: String,
    pub target: HostActionTarget,
}

/// The Universal SysEx Identity Reply a device answers with: the most
/// reliable way to tell models apart. Declared by schema 2; matched once
/// hosts send the Identity Request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SysexIdentity {
    /// One byte, or three beginning with 0x00.
    pub manufacturer: Vec<u8>,
    pub family: u16,
    pub model: u16,
}

impl SysexIdentity {
    pub fn validate(&self) -> Result<(), String> {
        match self.manufacturer.as_slice() {
            [id] if (0x01..=0x7d).contains(id) => {}
            [0x00, high, low] if *high <= 0x7f && *low <= 0x7f && (*high, *low) != (0, 0) => {}
            _ => {
                return Err(
                    "a SysEx manufacturer ID is one byte 0x01..0x7D, or 0x00 and two 7-bit bytes"
                        .into(),
                );
            }
        }
        if self.family > 0x3fff || self.model > 0x3fff {
            return Err("SysEx family and model codes are 14-bit".into());
        }
        Ok(())
    }
}

/// Checks a schema 2 package's inputs and the meanings given to them.
pub fn validate_inputs(
    inputs: &[ControllerInput],
    roles: &[InputRole],
    actions: &[InputAction],
) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    let mut messages = BTreeMap::new();
    let mut slots = BTreeMap::new();
    if inputs.iter().filter(|input| input.modifier).count() > 1 {
        return Err("a package names one Fn button at most".into());
    }
    for input in inputs {
        let message = input.validate()?;
        if !ids.insert(input.id.as_str()) {
            return Err(format!("duplicate input id {:?}", input.id));
        }
        if let Some(slot) = input.slot
            && let Some(other) = slots.insert(slot, input.id.as_str())
        {
            return Err(format!(
                "inputs {other:?} and {:?} fill the same slot {slot}",
                input.id
            ));
        }
        if let Some(other) = messages.insert(message, input.id.as_str()) {
            return Err(format!(
                "inputs {other:?} and {:?} send the same message",
                input.id
            ));
        }
    }
    let by_id = inputs
        .iter()
        .map(|input| (input.id.as_str(), input))
        .collect::<BTreeMap<_, _>>();
    let mut meanings = BTreeSet::new();
    let mut role_names = BTreeSet::new();
    for binding in roles {
        let input = by_id.get(binding.input.as_str()).ok_or_else(|| {
            format!(
                "role {} names unknown input {:?}",
                binding.role, binding.input
            )
        })?;
        binding.role.validate().map_err(|error| error.to_string())?;
        role_control(input, binding.mode)?;
        if !role_names.insert(binding.role.as_str()) {
            return Err(format!("role {} is given to two inputs", binding.role));
        }
        if !meanings.insert(binding.input.as_str()) {
            return Err(format!(
                "input {:?} has more than one meaning",
                binding.input
            ));
        }
    }
    for action in actions {
        let input = by_id
            .get(action.input.as_str())
            .ok_or_else(|| format!("action names unknown input {:?}", action.input))?;
        action.target.validate()?;
        action_trigger(action.target, input)?;
        if input.modifier {
            return Err(format!(
                "input {:?} is the Fn button and cannot also have a host action",
                action.input
            ));
        }
        // The host takes an action's button before any map hears it.
        if let Some(slot) = input.slot {
            return Err(format!(
                "input {:?} has a host action and cannot also fill slot {slot}",
                action.input
            ));
        }
        if !meanings.insert(action.input.as_str()) {
            return Err(format!(
                "input {:?} has more than one meaning",
                action.input
            ));
        }
    }
    Ok(())
}

/// The schema 1 semantic profile these roles mean, or `None` without roles.
/// `source_id` is the stable MIDI source identity the profile speaks for.
pub fn lower_roles(
    source_id: &str,
    inputs: &[ControllerInput],
    roles: &[InputRole],
) -> Option<SemanticControlProfile> {
    if roles.is_empty() {
        return None;
    }
    let controls = roles
        .iter()
        .filter_map(|binding| {
            let input = inputs.iter().find(|input| input.id == binding.input)?;
            let midi_cc = role_control(input, binding.mode).ok()?;
            Some(SemanticControlBinding {
                role: binding.role.clone(),
                midi_cc,
                invert: binding.invert,
                mode: binding.mode,
            })
        })
        .collect();
    Some(SemanticControlProfile {
        schema_version: CONTROL_PROFILE_SCHEMA_VERSION,
        source_id: source_id.into(),
        controls,
    })
}

/// The schema 1 host actions these actions mean.
pub fn lower_actions(
    inputs: &[ControllerInput],
    actions: &[InputAction],
) -> Vec<HostActionBinding> {
    actions
        .iter()
        .filter_map(|action| {
            let input = inputs.iter().find(|input| input.id == action.input)?;
            action_trigger(action.target, input).ok()
        })
        .collect()
}

/// A role needs a control change the runtime can read: a knob, fader, pedal,
/// a wheel that sends a CC, or an encoder reporting a position -- read
/// absolutely, or, for the encoder only, by the distance it turns. Notes,
/// pitch bend and relative encodings are declared inputs the runtime does not
/// match yet.
fn role_control(
    input: &ControllerInput,
    mode: SemanticControlMode,
) -> Result<MidiControlChangeBinding, String> {
    let InputMessage::ControlChange {
        channel,
        controller,
    } = input.midi.message()?
    else {
        return Err(format!(
            "input {:?}: a role needs an input that sends a control change",
            input.id
        ));
    };
    match input.kind {
        InputKind::Knob | InputKind::Fader | InputKind::Pedal | InputKind::Wheel => {
            if mode == SemanticControlMode::Relative {
                return Err(format!(
                    "input {:?}: only an encoder is read relatively",
                    input.id
                ));
            }
        }
        InputKind::Encoder if input.encoder_encoding() == EncoderEncoding::Absolute => {}
        InputKind::Encoder => {
            return Err(format!(
                "input {:?}: roles do not read relative encoder encodings yet",
                input.id
            ));
        }
        InputKind::Button | InputKind::Pad => {
            return Err(format!(
                "input {:?}: a {:?} cannot carry a continuous role",
                input.id, input.kind
            ));
        }
    }
    Ok(MidiControlChangeBinding {
        channel,
        controller,
    })
}

/// A host action needs a button the runtime reads: one that sends a control
/// change and reports its release, one that sends a note -- a note-off is
/// its release -- or one that sends a real time message. A real time
/// message only presses, so it cannot hold a momentary action.
fn action_trigger(
    target: HostActionTarget,
    input: &ControllerInput,
) -> Result<HostActionBinding, String> {
    if input.kind != InputKind::Button {
        return Err(format!(
            "input {:?}: a host action needs a button",
            input.id
        ));
    }
    let (channel, controller) = match input.midi.message()? {
        InputMessage::ControlChange {
            channel,
            controller,
        } => (channel, controller),
        InputMessage::Realtime(message) => {
            if matches!(
                target,
                HostActionTarget::KeyboardParts | HostActionTarget::SequencerFill
            ) {
                return Err(format!(
                    "input {:?}: {target:?} is held, and a real time message has no release",
                    input.id
                ));
            }
            return Ok(HostActionBinding::realtime(target, message));
        }
        InputMessage::Note { channel, note } => {
            let binding = MidiNoteButtonBinding { channel, note };
            binding.validate()?;
            return Ok(HostActionBinding::note(target, binding));
        }
        InputMessage::PitchBend { .. } => {
            return Err(format!(
                "input {:?}: host actions read buttons that send a control change, a note \
                 or a real time message",
                input.id
            ));
        }
    };
    let report = input.button_report();
    if report.press_only {
        return Err(format!(
            "input {:?}: host actions need a button that reports its release",
            input.id
        ));
    }
    let binding = MidiButtonBinding {
        channel,
        controller,
        press_value: report.press,
        release_value: report.release,
    };
    binding.validate()?;
    Ok(HostActionBinding::control_change(target, binding))
}

fn validate_input_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
    {
        return Err(format!(
            "invalid input id {value:?}: lowercase letters, digits, '-' and '_'"
        ));
    }
    Ok(())
}

fn validate_label(value: &str, what: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.contains('\0') || value.chars().count() > MAX_LABEL_CHARS {
        return Err(format!(
            "{what} {value:?} is empty, too long or contains NUL"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn knob(id: &str, cc: u8) -> ControllerInput {
        ControllerInput {
            id: id.into(),
            name: id.into(),
            kind: InputKind::Knob,
            group: None,
            midi: InputMidi {
                channel: 0,
                cc: Some(cc),
                ..InputMidi::default()
            },
            button: None,
            encoder: None,
            slot: None,
            modifier: false,
            plays: true,
        }
    }

    fn button(id: &str, cc: u8) -> ControllerInput {
        ControllerInput {
            kind: InputKind::Button,
            ..knob(id, cc)
        }
    }

    fn role(input: &str, role: &str) -> InputRole {
        InputRole {
            input: input.into(),
            role: SemanticControlId::new(role).unwrap(),
            invert: false,
            mode: SemanticControlMode::Absolute,
        }
    }

    #[test]
    fn roles_and_actions_lower_to_the_schema_1_runtime() {
        let inputs = vec![knob("knob-1", 74), button("button-1", 119)];
        let roles = vec![role("knob-1", "synth.filter.cutoff")];
        let actions = vec![InputAction {
            input: "button-1".into(),
            target: HostActionTarget::KeyboardParts,
        }];
        validate_inputs(&inputs, &roles, &actions).unwrap();

        let profile = lower_roles("controller.org.example.pad", &inputs, &roles).unwrap();
        assert_eq!(profile.source_id, "controller.org.example.pad");
        assert_eq!(profile.controls.len(), 1);
        assert_eq!(profile.controls[0].midi_cc.controller, 74);
        assert_eq!(profile.controls[0].mode, SemanticControlMode::Absolute);
        profile.validate().unwrap();

        let lowered = lower_actions(&inputs, &actions);
        assert_eq!(
            lowered,
            vec![HostActionBinding::control_change(
                HostActionTarget::KeyboardParts,
                MidiButtonBinding {
                    channel: 0,
                    controller: 119,
                    press_value: 127,
                    release_value: 0,
                },
            )]
        );
    }

    fn realtime_button(id: &str, message: MidiRealtime) -> ControllerInput {
        ControllerInput {
            id: id.into(),
            name: id.into(),
            kind: InputKind::Button,
            group: None,
            midi: InputMidi {
                channel: NO_CHANNEL,
                realtime: Some(message),
                ..InputMidi::default()
            },
            button: None,
            encoder: None,
            slot: None,
            modifier: false,
            plays: true,
        }
    }

    fn slotted(mut input: ControllerInput, slot: &str) -> ControllerInput {
        input.slot = Some(slot.parse().unwrap());
        input
    }

    /// A knob fills a row, a button a switch or a step, and no two controls
    /// fill one slot; a button the host takes for an action fills none.
    #[test]
    fn a_control_fills_a_slot_of_its_own_kind() {
        let knob_row = slotted(knob("knob-1", 21), "control-1.1");
        let switch = slotted(button("button-1", 40), "switch-1.1");
        validate_inputs(&[knob_row.clone(), switch.clone()], &[], &[]).unwrap();

        let knob_switch = slotted(knob("knob-2", 22), "switch-1.2");
        assert!(validate_inputs(&[knob_switch], &[], &[]).is_err());
        let button_row = slotted(button("button-2", 41), "control-1.2");
        assert!(validate_inputs(&[button_row], &[], &[]).is_err());
        let twice = slotted(knob("knob-3", 23), "control-1.1");
        assert!(validate_inputs(&[knob_row.clone(), twice], &[], &[]).is_err());
        let action = InputAction {
            input: "button-1".into(),
            target: HostActionTarget::TransportPlay,
        };
        assert!(validate_inputs(&[switch], &[], &[action]).is_err());

        let slotted_input = knob_row.slotted_input().unwrap();
        assert_eq!(slotted_input.slot.to_string(), "control-1.1");
        assert_eq!(
            slotted_input.input.message,
            ParameterLinkMessage::ControlChange { controller: 21 }
        );
        assert!(knob("knob-4", 24).slotted_input().is_none());
    }

    /// A DAW protocol's fader sends pitch bend on a channel of its own: it
    /// fills a row as any fader does, and with `plays = false` no instrument
    /// hears it. The pitch wheel's bend stays unmappable.
    #[test]
    fn a_fader_that_sends_pitch_bend_fills_a_row_and_never_plays() {
        let mut fader = slotted(
            ControllerInput {
                id: "fader-3".into(),
                name: "Fader 3".into(),
                kind: InputKind::Fader,
                group: None,
                midi: InputMidi {
                    channel: 2,
                    pitch_bend: true,
                    ..InputMidi::default()
                },
                button: None,
                encoder: None,
                slot: None,
                modifier: false,
                plays: true,
            },
            "control-1.3",
        );
        validate_inputs(std::slice::from_ref(&fader), &[], &[]).unwrap();
        assert_eq!(
            fader.slotted_input().unwrap().input.message,
            ParameterLinkMessage::PitchBend
        );
        assert_eq!(fader.held_control(), None);
        fader.plays = false;
        validate_inputs(std::slice::from_ref(&fader), &[], &[]).unwrap();
        assert_eq!(
            fader.held_control(),
            Some(HeldControl::PitchBend { channel: 2 })
        );

        let mut wheel = fader.clone();
        wheel.kind = InputKind::Wheel;
        wheel.slot = None;
        assert!(wheel.mapped_input().is_none());

        let mut knob = knob("knob-9", 30);
        knob.midi = fader.midi;
        assert!(validate_inputs(&[knob], &[], &[]).is_err());

        let mut start = realtime_button("play", MidiRealtime::Start);
        start.plays = false;
        assert!(validate_inputs(&[start], &[], &[]).is_err());
    }

    /// A Launchkey MK4's Play and Stop send MIDI Start and Stop: they are
    /// buttons the transport actions can take, with no channel and no
    /// values of their own.
    #[test]
    fn a_play_button_that_sends_midi_start_takes_the_transport() {
        let inputs = vec![
            realtime_button("play", MidiRealtime::Start),
            realtime_button("stop", MidiRealtime::Stop),
        ];
        let actions = vec![
            InputAction {
                input: "play".into(),
                target: HostActionTarget::TransportPlay,
            },
            InputAction {
                input: "stop".into(),
                target: HostActionTarget::TransportStop,
            },
        ];
        validate_inputs(&inputs, &[], &actions).unwrap();
        assert_eq!(
            lower_actions(&inputs, &actions),
            vec![
                HostActionBinding::realtime(HostActionTarget::TransportPlay, MidiRealtime::Start),
                HostActionBinding::realtime(HostActionTarget::TransportStop, MidiRealtime::Stop),
            ]
        );

        // A held action needs a release, which MIDI Start never sends.
        let held = vec![InputAction {
            input: "play".into(),
            target: HostActionTarget::KeyboardParts,
        }];
        assert!(validate_inputs(&inputs, &[], &held).is_err());

        // Only a button sends one, without a channel or reported values.
        let mut knob = realtime_button("knob", MidiRealtime::Start);
        knob.kind = InputKind::Knob;
        assert!(knob.validate().is_err());
        let mut with_channel = realtime_button("play", MidiRealtime::Start);
        with_channel.midi.channel = 0;
        assert!(with_channel.validate().is_err());
        let mut with_values = realtime_button("play", MidiRealtime::Start);
        with_values.button = Some(ButtonReport::default());
        assert!(with_values.validate().is_err());
    }

    #[test]
    fn a_channel_message_without_its_channel_is_refused() {
        let parsed: ControllerInput = toml::from_str(
            r#"
            id = "knob-1"
            name = "Knob 1"
            kind = "knob"
            midi = { cc = 21 }
            "#,
        )
        .unwrap();
        assert!(parsed.validate().is_err());
        let play: ControllerInput = toml::from_str(
            r#"
            id = "play"
            name = "Play"
            kind = "button"
            midi = { realtime = "start" }
            "#,
        )
        .unwrap();
        assert_eq!(
            play.validate().unwrap(),
            InputMessage::Realtime(MidiRealtime::Start)
        );
    }

    #[test]
    fn an_encoder_is_read_absolutely_or_by_the_distance_it_turns() {
        let mut encoder = knob("encoder-1", 16);
        encoder.kind = InputKind::Encoder;
        let inputs = vec![encoder, knob("knob-1", 74)];

        let absolute = vec![role("encoder-1", "synth.lfo.rate")];
        validate_inputs(&inputs, &absolute, &[]).unwrap();
        let profile = lower_roles("controller.example", &inputs, &absolute).unwrap();
        assert_eq!(profile.controls[0].mode, SemanticControlMode::Absolute);

        let mut relative = absolute.clone();
        relative[0].mode = SemanticControlMode::Relative;
        validate_inputs(&inputs, &relative, &[]).unwrap();
        let profile = lower_roles("controller.example", &inputs, &relative).unwrap();
        assert_eq!(profile.controls[0].mode, SemanticControlMode::Relative);

        let mut knob_relative = vec![role("knob-1", "synth.filter.cutoff")];
        knob_relative[0].mode = SemanticControlMode::Relative;
        assert!(validate_inputs(&inputs, &knob_relative, &[]).is_err());
    }

    #[test]
    fn two_inputs_cannot_send_the_same_message() {
        let error = validate_inputs(&[knob("a", 74), knob("b", 74)], &[], &[]).unwrap_err();
        assert!(error.contains("same message"), "{error}");
    }

    #[test]
    fn an_input_carries_one_meaning() {
        let inputs = vec![knob("knob-1", 74)];
        let roles = vec![
            role("knob-1", "synth.filter.cutoff"),
            role("knob-1", "synth.filter.resonance"),
        ];
        assert!(validate_inputs(&inputs, &roles, &[]).is_err());
    }

    #[test]
    fn a_kind_sends_only_what_it_can() {
        let mut pad = knob("pad-1", 36);
        pad.kind = InputKind::Pad;
        assert!(validate_inputs(&[pad.clone()], &[], &[]).is_err());
        pad.midi = InputMidi {
            channel: 9,
            note: Some(36),
            ..InputMidi::default()
        };
        validate_inputs(&[pad], &[], &[]).unwrap();
    }

    #[test]
    fn a_pad_or_a_note_is_declared_but_not_yet_a_role() {
        let mut pad = knob("pad-1", 0);
        pad.kind = InputKind::Pad;
        pad.midi = InputMidi {
            channel: 9,
            note: Some(36),
            ..InputMidi::default()
        };
        let roles = vec![role("pad-1", "synth.filter.cutoff")];
        assert!(validate_inputs(&[pad], &roles, &[]).is_err());
    }

    #[test]
    fn a_host_action_needs_a_button_that_reports_its_release() {
        let mut press_only = button("button-1", 20);
        press_only.button = Some(ButtonReport {
            press_only: true,
            ..ButtonReport::default()
        });
        let actions = vec![InputAction {
            input: "button-1".into(),
            target: HostActionTarget::TapTempo,
        }];
        assert!(validate_inputs(&[press_only], &[], &actions).is_err());
    }

    #[test]
    fn sysex_manufacturer_ids_are_one_byte_or_three() {
        let identity = |manufacturer: Vec<u8>| SysexIdentity {
            manufacturer,
            family: 0x27,
            model: 1,
        };
        identity(vec![0x47]).validate().unwrap();
        identity(vec![0x00, 0x20, 0x6b]).validate().unwrap();
        assert!(identity(vec![0x7e]).validate().is_err());
        assert!(identity(vec![0x00, 0x00, 0x00]).validate().is_err());
        assert!(identity(vec![0x20, 0x6b]).validate().is_err());
    }
}
