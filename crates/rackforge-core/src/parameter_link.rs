use anyhow::{Result, bail};
use rackforge_midi_api::controller_map::ControllerMap;
use rackforge_midi_api::{
    IngressMidiEvent, MidiChannel, MidiMessageKind, MidiSourceId, MidiSourceKey,
    PARAMETER_LINK_SCHEMA_VERSION, ParameterLink, ParameterLinkChannel, ParameterLinkId,
    ParameterLinkMessage, ParameterLinkMode, ParameterLinkPassThrough, ParameterLinkSource,
    ParameterLinkTransform, StepDirection,
};
use rackforge_plugin_api::abi::ParameterEventV1;
use rackforge_plugin_api::{ParameterDescriptor, ParameterKind, ParameterSchema, ParameterTaper};
use rackforge_session_api::{RackForgeParameterId, SemanticControlProfile};

use crate::parameter_touch::{PARAMETER_TOUCHES, ParameterTouch, TouchPickup};
use crate::validate_parameter_write;

/// How close, as a fraction of the range, a control has to come to the
/// parameter before an absolute controller takes it over. About four steps
/// of a seven-bit controller.
const PICKUP_WINDOW: f64 = 0.03;

/// Where an absolute controller stands against its parameter.
///
/// A knob or a fader keeps its own position; the parameter under it keeps
/// another — restored with the session, set from the screen, left by an
/// earlier session's knob. Sending the knob's position the moment it is
/// touched makes the parameter jump, and a knob brushed while playing puts a
/// value nobody chose where it stays. So a link starts detached, and takes
/// the parameter over only once the control has come to where the parameter
/// is, or has crossed it.
#[derive(Clone, Copy, Debug, Default)]
struct Pickup {
    engaged: bool,
    /// The parameter's position, 0..=1 over its range, as last known: what
    /// the link sent, or what the host said was set from elsewhere.
    parameter: Option<f64>,
    /// The control's last position, so a crossing can be seen.
    input: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct CompiledParameterLink {
    pub link: ParameterLink,
    pub source_key: MidiSourceKey,
    parameter: ParameterDescriptor,
    pickup: Pickup,
    /// The parameter's value as this link last wrote or saw it: what a
    /// toggle, cycle or step starts from when the host cannot say.
    last_value: Option<f64>,
    /// The key the screen knows the link's instance by.
    instance_key: u64,
}

/// How many presses a Step takes to cross a float parameter's range: its own
/// step is often a hundredth or finer, far too fine for a button.
const FLOAT_STEPS_PER_RANGE: f64 = 20.0;

#[derive(Clone, Copy, Debug)]
pub struct ParameterLinkOutput {
    pub event: ParameterEventV1,
    pub pass_through: ParameterLinkPassThrough,
}

impl CompiledParameterLink {
    pub fn new(
        link: ParameterLink,
        source_key: MidiSourceKey,
        schema: &ParameterSchema,
    ) -> Result<Self> {
        link.validate()?;
        let parameter = schema
            .parameters
            .iter()
            .find(|parameter| parameter.index == link.parameter_index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown plugin parameter {}", link.parameter_index))?;
        if parameter.flags.read_only || matches!(parameter.kind, ParameterKind::Meter { .. }) {
            bail!("plugin parameter {} is read-only", parameter.id);
        }
        validate_parameter_write(schema, parameter.index, default_value(&parameter.kind))?;
        validate_mode(&link.mode, &parameter, schema)?;
        let instance_key = crate::parameter_touch::instance_key(&link.instance_id);
        Ok(Self {
            link,
            source_key,
            parameter,
            pickup: Pickup::default(),
            last_value: None,
            instance_key,
        })
    }

    /// Tells the screen what this link just did to its parameter.
    fn touch(&self, value: f64, pickup: TouchPickup) {
        PARAMETER_TOUCHES.record(ParameterTouch {
            instance_key: self.instance_key,
            parameter_index: self.parameter.index,
            value,
            pickup,
        });
    }

    /// The parameter value at a position along the control's travel: the
    /// inverse of [`Self::position_of`].
    fn value_at(&self, position: f64) -> f64 {
        match &self.link.mode {
            ParameterLinkMode::Range { min, max } => {
                range_value(&self.parameter.kind, min.get(), max.get(), position)
            }
            _ => map_parameter_value(&self.parameter.kind, position),
        }
    }

    /// The parameter's index on its plugin.
    pub fn parameter_index(&self) -> u32 {
        self.parameter.index
    }

    /// The parameter was set by something other than this link — the
    /// screen, a restored session — so the control is no longer where the
    /// parameter is, and has to come back to it before it takes over again.
    pub fn observe_parameter(&mut self, instance_id: &str, parameter_index: u32, value: f64) {
        if self.link.instance_id != instance_id || self.parameter.index != parameter_index {
            return;
        }
        self.pickup.parameter = Some(self.position_of(value));
        self.pickup.engaged = false;
        self.pickup.input = None;
        self.last_value = Some(value);
    }

    /// Where a parameter value sits along the control's travel, 0..=1: over
    /// the parameter's range, or over the link's own range.
    fn position_of(&self, value: f64) -> f64 {
        match &self.link.mode {
            ParameterLinkMode::Range { min, max } => {
                let (min, max) = (min.get(), max.get());
                ((value - min) / (max - min)).clamp(0.0, 1.0)
            }
            _ => normalize_parameter_value(&self.parameter.kind, value),
        }
    }

    fn output(&mut self, frame: u32, value: f64) -> ParameterLinkOutput {
        self.last_value = Some(value);
        self.touch(value, TouchPickup::Engaged);
        ParameterLinkOutput {
            event: ParameterEventV1 {
                frame,
                parameter_index: self.parameter.index,
                value,
            },
            pass_through: self.link.pass_through,
        }
    }

    /// A button acts on its press, and for Hold and Trigger on its release
    /// too. A press is a control change at 64 or above, or a note with a
    /// velocity; a button that never reports its release simply presses
    /// again. Toggle, Cycle and Step start from where the parameter is --
    /// set from the screen or not -- so a press always does what it says.
    fn apply_button(
        &mut self,
        frame: u32,
        normalized: f64,
        current: impl FnOnce(u32) -> Option<f64>,
    ) -> Option<ParameterLinkOutput> {
        let pressed = normalized >= 0.5;
        let value = match &self.link.mode {
            ParameterLinkMode::Set { value } => pressed.then(|| value.get())?,
            ParameterLinkMode::Hold {
                pressed: down,
                released,
            } => {
                if pressed {
                    down.get()
                } else {
                    released.get()
                }
            }
            ParameterLinkMode::Trigger => {
                if pressed {
                    1.0
                } else {
                    0.0
                }
            }
            ParameterLinkMode::Toggle { first, second } => {
                if !pressed {
                    return None;
                }
                let at = self.current_value(current);
                if same_value(at, first.get()) {
                    second.get()
                } else {
                    first.get()
                }
            }
            ParameterLinkMode::Cycle { values } => {
                if !pressed {
                    return None;
                }
                let at = self.current_value(current);
                let next = values
                    .iter()
                    .position(|value| same_value(at, value.get()))
                    .map_or(0, |index| (index + 1) % values.len());
                values[next].get()
            }
            ParameterLinkMode::Step { direction, wrap } => {
                if !pressed {
                    return None;
                }
                let at = self.current_value(current);
                step_value(&self.parameter.kind, at, *direction, *wrap)
            }
            ParameterLinkMode::Direct
            | ParameterLinkMode::Range { .. }
            | ParameterLinkMode::Zones { .. } => return None,
        };
        Some(self.output(frame, value))
    }

    fn current_value(&self, current: impl FnOnce(u32) -> Option<f64>) -> f64 {
        current(self.parameter.index)
            .or(self.last_value)
            .unwrap_or_else(|| default_value(&self.parameter.kind))
    }

    /// Map one incoming message onto the parameter. `current` is asked for
    /// the parameter's value the first time an absolute control is touched
    /// and the link does not yet know where the parameter is; a host that
    /// cannot say answers `None`, and the control takes over at once.
    pub fn apply(
        &mut self,
        ingress: IngressMidiEvent,
        current: impl FnOnce(u32) -> Option<f64>,
    ) -> Option<ParameterLinkOutput> {
        if ingress.source != self.source_key || !self.link.matches_channel(ingress.packet.channel())
        {
            return None;
        }
        let normalized = normalized_input(self.link.message, ingress.packet)?;
        let normalized = if self.link.transform.invert {
            1.0 - normalized
        } else {
            normalized
        };
        let frame = ingress.packet.frame;
        if self.link.mode.is_button() {
            return self.apply_button(frame, normalized, current);
        }
        // A zone is a choice made on purpose, not a position to pick up.
        if let ParameterLinkMode::Zones { values } = &self.link.mode {
            let zone = ((normalized * values.len() as f64) as usize).min(values.len() - 1);
            let value = values[zone].get();
            return Some(self.output(frame, value));
        }
        // Only a control that holds a position needs picking up. A bend
        // wheel springs back, pressure and notes are momentary: they are
        // gestures, and a gesture is taken as it comes.
        if matches!(
            self.link.message,
            ParameterLinkMessage::ControlChange { .. }
        ) && !self.pickup.engaged
        {
            let known = match self.pickup.parameter {
                Some(position) => Some(position),
                None => current(self.parameter.index).map(|value| self.position_of(value)),
            };
            match known {
                None => self.pickup.engaged = true,
                Some(position) => {
                    let crossed = self.pickup.input.is_some_and(|previous| {
                        (previous - position) * (normalized - position) <= 0.0
                    });
                    if (normalized - position).abs() <= PICKUP_WINDOW || crossed {
                        self.pickup.engaged = true;
                    } else {
                        self.pickup.parameter = Some(position);
                        self.pickup.input = Some(normalized);
                        // Said, not swallowed: the screen shows where the
                        // parameter is and which way to move to reach it.
                        self.touch(
                            self.value_at(position),
                            if normalized < position {
                                TouchPickup::MoveUp
                            } else {
                                TouchPickup::MoveDown
                            },
                        );
                        return None;
                    }
                }
            }
        }
        self.pickup.input = Some(normalized);
        self.pickup.parameter = Some(normalized);
        let value = match &self.link.mode {
            ParameterLinkMode::Range { min, max } => {
                range_value(&self.parameter.kind, min.get(), max.get(), normalized)
            }
            _ => map_parameter_value(&self.parameter.kind, normalized),
        };
        Some(self.output(frame, value))
    }
}

/// A mode names values the parameter must accept, and asks of the parameter
/// what it can do: a range needs a continuous one, a step something to step
/// through, a trigger a trigger.
fn validate_mode(
    mode: &ParameterLinkMode,
    parameter: &ParameterDescriptor,
    schema: &ParameterSchema,
) -> Result<()> {
    match (mode, &parameter.kind) {
        (ParameterLinkMode::Direct, _) => return Ok(()),
        (
            ParameterLinkMode::Range { .. },
            ParameterKind::Float { .. } | ParameterKind::Integer { .. },
        )
        | (ParameterLinkMode::Trigger, ParameterKind::Trigger | ParameterKind::Boolean { .. })
        | (
            ParameterLinkMode::Step { .. },
            ParameterKind::Float { .. }
            | ParameterKind::Integer { .. }
            | ParameterKind::Enum { .. }
            | ParameterKind::Boolean { .. },
        )
        | (
            ParameterLinkMode::Zones { .. }
            | ParameterLinkMode::Set { .. }
            | ParameterLinkMode::Toggle { .. }
            | ParameterLinkMode::Cycle { .. }
            | ParameterLinkMode::Hold { .. },
            ParameterKind::Float { .. }
            | ParameterKind::Integer { .. }
            | ParameterKind::Enum { .. }
            | ParameterKind::Boolean { .. },
        ) => {}
        (mode, kind) => bail!(
            "plugin parameter {} ({}) cannot be driven by a {} mode",
            parameter.id,
            kind_name(kind),
            mode_name(mode)
        ),
    }
    for value in mode.values() {
        validate_parameter_write(schema, parameter.index, value).map_err(|error| {
            anyhow::anyhow!(
                "plugin parameter {} does not accept {value}: {error}",
                parameter.id
            )
        })?;
    }
    Ok(())
}

fn kind_name(kind: &ParameterKind) -> &'static str {
    match kind {
        ParameterKind::Float { .. } => "float",
        ParameterKind::Integer { .. } => "integer",
        ParameterKind::Boolean { .. } => "boolean",
        ParameterKind::Enum { .. } => "choice",
        ParameterKind::Trigger => "trigger",
        ParameterKind::Meter { .. } => "meter",
    }
}

fn mode_name(mode: &ParameterLinkMode) -> &'static str {
    match mode {
        ParameterLinkMode::Direct => "direct",
        ParameterLinkMode::Range { .. } => "range",
        ParameterLinkMode::Zones { .. } => "zones",
        ParameterLinkMode::Set { .. } => "set",
        ParameterLinkMode::Toggle { .. } => "toggle",
        ParameterLinkMode::Cycle { .. } => "cycle",
        ParameterLinkMode::Hold { .. } => "hold",
        ParameterLinkMode::Step { .. } => "step",
        ParameterLinkMode::Trigger => "trigger",
    }
}

/// Parameter values are compared as the plugin reports them: a choice's
/// value exactly, a float to within a hair of rounding.
fn same_value(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0)
}

/// A position along a Range, in the parameter's own units and steps. The
/// range may run downwards.
fn range_value(kind: &ParameterKind, min: f64, max: f64, normalized: f64) -> f64 {
    let raw = min + (max - min) * normalized.clamp(0.0, 1.0);
    let (low, high) = if min <= max { (min, max) } else { (max, min) };
    match kind {
        ParameterKind::Float {
            minimum,
            step,
            taper,
            ..
        } => {
            if *taper == ParameterTaper::Logarithmic || *step <= 0.0 {
                raw.clamp(low, high)
            } else {
                quantize(raw, *minimum, *step).clamp(low, high)
            }
        }
        ParameterKind::Integer { minimum, step, .. } => {
            let step = (*step).max(1) as f64;
            quantize(raw, *minimum as f64, step).clamp(low, high)
        }
        _ => raw.clamp(low, high),
    }
}

/// One step from `at`, up or down, stopping at the ends or wrapping past
/// them.
fn step_value(kind: &ParameterKind, at: f64, direction: StepDirection, wrap: bool) -> f64 {
    let up = direction == StepDirection::Up;
    match kind {
        ParameterKind::Float {
            minimum,
            maximum,
            step,
            ..
        } => {
            let increment = ((maximum - minimum) / FLOAT_STEPS_PER_RANGE).max(*step);
            let next = if up { at + increment } else { at - increment };
            wrap_or_clamp(quantize(next, *minimum, *step), *minimum, *maximum, wrap)
        }
        ParameterKind::Integer {
            minimum,
            maximum,
            step,
            ..
        } => {
            let increment = (*step).max(1) as f64;
            let next = if up { at + increment } else { at - increment };
            wrap_or_clamp(next, *minimum as f64, *maximum as f64, wrap)
        }
        ParameterKind::Enum { choices, .. } => {
            let count = choices.len();
            let index = choices
                .iter()
                .position(|choice| same_value(choice.value as f64, at))
                .unwrap_or(0);
            let next = match (up, wrap) {
                (true, true) => (index + 1) % count,
                (true, false) => (index + 1).min(count - 1),
                (false, true) => (index + count - 1) % count,
                (false, false) => index.saturating_sub(1),
            };
            choices[next].value as f64
        }
        ParameterKind::Boolean { .. } => {
            if wrap {
                if at >= 0.5 { 0.0 } else { 1.0 }
            } else if up {
                1.0
            } else {
                0.0
            }
        }
        ParameterKind::Trigger | ParameterKind::Meter { .. } => at,
    }
}

fn wrap_or_clamp(value: f64, minimum: f64, maximum: f64, wrap: bool) -> f64 {
    if !wrap {
        return value.clamp(minimum, maximum);
    }
    // Past the top is the bottom and past the bottom is the top: a wrap
    // lands on the other end, not somewhere inside it.
    if value > maximum + 1e-9 {
        minimum
    } else if value < minimum - 1e-9 {
        maximum
    } else {
        value
    }
}

/// Compiles controller-declared semantic controls for one plugin instance.
///
/// These links are runtime defaults, never persisted session objects. An
/// explicit user link wins when it targets either the same physical control or
/// the same semantic plugin parameter, preventing double writes.
pub struct SemanticParameterLinkContext<'a> {
    pub controller_id: &'a str,
    pub controller_name: &'a str,
    pub profile: &'a SemanticControlProfile,
    pub runtime_source_id: &'a MidiSourceId,
    pub source_key: MidiSourceKey,
    pub instance_id: &'a str,
    pub schema: &'a ParameterSchema,
    pub explicit_links: &'a [ParameterLink],
}

pub fn compile_semantic_parameter_links(
    context: SemanticParameterLinkContext<'_>,
) -> Result<Vec<CompiledParameterLink>> {
    let SemanticParameterLinkContext {
        controller_id,
        controller_name,
        profile,
        runtime_source_id,
        source_key,
        instance_id,
        schema,
        explicit_links,
    } = context;
    profile.validate().map_err(anyhow::Error::msg)?;
    schema.validate().map_err(anyhow::Error::msg)?;
    let mut compiled = Vec::new();
    for control in &profile.controls {
        // RackForge-owned parameters share the semantic controller profile,
        // but they are never candidates for a plugin ParameterEvent.
        if RackForgeParameterId::from_role(&control.role).is_some() {
            continue;
        }
        let Some(parameter) = schema.parameter_for_semantic_role(&control.role) else {
            continue;
        };
        let channel = MidiChannel::from_zero_based(control.midi_cc.channel)?;
        let message = ParameterLinkMessage::ControlChange {
            controller: control.midi_cc.controller,
        };
        if explicit_links.iter().any(|link| {
            explicit_link_overrides_semantic(
                link,
                instance_id,
                parameter.index,
                runtime_source_id,
                channel,
                message,
            )
        }) {
            continue;
        }
        let link = ParameterLink {
            schema_version: PARAMETER_LINK_SCHEMA_VERSION,
            id: ParameterLinkId::new(format!(
                "auto.{controller_id}.{instance_id}.{}",
                control.role.as_str()
            ))?,
            instance_id: instance_id.to_owned(),
            parameter_index: parameter.index,
            source: ParameterLinkSource {
                source_id: runtime_source_id.clone(),
                display_name: controller_name.to_owned(),
            },
            channel: ParameterLinkChannel::Channel { channel },
            message,
            transform: ParameterLinkTransform {
                invert: control.invert,
            },
            pass_through: ParameterLinkPassThrough::PassThrough,
            mode: ParameterLinkMode::Direct,
        };
        compiled.push(CompiledParameterLink::new(link, source_key, schema)?);
    }
    Ok(compiled)
}

/// Compiles a player's controller map for one plugin instance: the mappings
/// the map holds for the instance's plugin, each resolved from its parameter
/// id to the instance's own parameter.
///
/// These links sit between the session's explicit links and the semantic
/// defaults: an explicit link on the same parameter or the same control wins
/// over a mapping, and a mapping wins over a default (pass the compiled links'
/// `link`s to [`compile_semantic_parameter_links`] as explicit).
pub struct ControllerMapLinkContext<'a> {
    pub map: &'a ControllerMap,
    pub plugin_id: &'a str,
    pub runtime_source_id: &'a MidiSourceId,
    pub source_name: &'a str,
    pub source_key: MidiSourceKey,
    pub instance_id: &'a str,
    pub schema: &'a ParameterSchema,
    pub explicit_links: &'a [ParameterLink],
}

/// A controller map compiled for one instance.
#[derive(Debug, Default)]
pub struct CompiledControllerMap {
    pub links: Vec<CompiledParameterLink>,
    /// Mappings that could not be compiled, with why: a parameter a newer
    /// plugin version no longer has stays in the map, pending, and costs the
    /// other mappings nothing.
    pub pending: Vec<(String, String)>,
}

pub fn compile_controller_map_links(
    context: ControllerMapLinkContext<'_>,
) -> CompiledControllerMap {
    let ControllerMapLinkContext {
        map,
        plugin_id,
        runtime_source_id,
        source_name,
        source_key,
        instance_id,
        schema,
        explicit_links,
    } = context;
    let mut compiled = CompiledControllerMap::default();
    let Some(plugin) = map.plugin(plugin_id) else {
        return compiled;
    };
    for mapping in &plugin.mappings {
        let pending = |reason: String| (mapping.id.as_str().to_owned(), reason);
        let Some(parameter) = schema
            .parameters
            .iter()
            .find(|parameter| parameter.id == mapping.parameter_id)
        else {
            compiled.pending.push(pending(format!(
                "the plugin has no parameter {:?}",
                mapping.parameter_id
            )));
            continue;
        };
        let channel = match mapping.input.channel {
            ParameterLinkChannel::Omni => None,
            ParameterLinkChannel::Channel { channel } => Some(channel),
        };
        if explicit_links.iter().any(|link| {
            link.instance_id == instance_id
                && (link.parameter_index == parameter.index
                    || (link.source.source_id == *runtime_source_id
                        && link.message == mapping.input.message
                        && channel.is_none_or(|channel| link.matches_channel(channel))))
        }) {
            continue;
        }
        let id = match ParameterLinkId::new(format!("map.{instance_id}.{}", mapping.id)) {
            Ok(id) => id,
            Err(error) => {
                compiled.pending.push(pending(error.to_string()));
                continue;
            }
        };
        let link = ParameterLink {
            schema_version: PARAMETER_LINK_SCHEMA_VERSION,
            id,
            instance_id: instance_id.to_owned(),
            parameter_index: parameter.index,
            source: ParameterLinkSource {
                source_id: runtime_source_id.clone(),
                display_name: source_name.to_owned(),
            },
            channel: mapping.input.channel,
            message: mapping.input.message,
            transform: ParameterLinkTransform {
                invert: mapping.invert,
            },
            pass_through: mapping.effective_pass_through(),
            mode: mapping.mode.clone(),
        };
        match CompiledParameterLink::new(link, source_key, schema) {
            Ok(link) => compiled.links.push(link),
            Err(error) => compiled.pending.push(pending(format!("{error:#}"))),
        }
    }
    compiled
}

fn explicit_link_overrides_semantic(
    link: &ParameterLink,
    instance_id: &str,
    parameter_index: u32,
    source_id: &MidiSourceId,
    channel: MidiChannel,
    message: ParameterLinkMessage,
) -> bool {
    if link.instance_id != instance_id {
        return false;
    }
    if link.parameter_index == parameter_index {
        return true;
    }
    link.source.source_id == *source_id
        && link.message == message
        && match link.channel {
            ParameterLinkChannel::Omni => true,
            ParameterLinkChannel::Channel { channel: explicit } => explicit == channel,
        }
}

fn default_value(kind: &ParameterKind) -> f64 {
    match kind {
        ParameterKind::Float { default, .. } => *default,
        ParameterKind::Integer { default, .. } => *default as f64,
        ParameterKind::Boolean { default } => f64::from(*default),
        ParameterKind::Enum { default, .. } => *default as f64,
        ParameterKind::Trigger => 0.0,
        ParameterKind::Meter { minimum, .. } => *minimum,
    }
}

fn normalized_input(
    message: ParameterLinkMessage,
    packet: rackforge_midi_api::MidiPacket,
) -> Option<f64> {
    let status = packet.data[0] & 0xf0;
    match message {
        ParameterLinkMessage::ControlChange { controller }
            if packet.kind() == MidiMessageKind::ControlChange && packet.data[1] == controller =>
        {
            Some(f64::from(packet.data[2]) / 127.0)
        }
        ParameterLinkMessage::PitchBend if packet.kind() == MidiMessageKind::PitchBend => {
            let raw = u16::from(packet.data[1]) | (u16::from(packet.data[2]) << 7);
            Some(if raw <= 8192 {
                0.5 * f64::from(raw) / 8192.0
            } else {
                0.5 + 0.5 * f64::from(raw - 8192) / 8191.0
            })
        }
        ParameterLinkMessage::Note { note }
            if matches!(status, 0x80 | 0x90) && packet.data[1] == note =>
        {
            Some(if status == 0x80 || packet.data[2] == 0 {
                0.0
            } else {
                f64::from(packet.data[2]) / 127.0
            })
        }
        ParameterLinkMessage::ChannelPressure
            if packet.kind() == MidiMessageKind::ChannelPressure =>
        {
            Some(f64::from(packet.data[1]) / 127.0)
        }
        ParameterLinkMessage::PolyPressure { note }
            if packet.kind() == MidiMessageKind::PolyPressure && packet.data[1] == note =>
        {
            Some(f64::from(packet.data[2]) / 127.0)
        }
        _ => None,
    }
}

fn map_parameter_value(kind: &ParameterKind, normalized: f64) -> f64 {
    let normalized = normalized.clamp(0.0, 1.0);
    match kind {
        ParameterKind::Float {
            minimum,
            maximum,
            step,
            taper,
            ..
        } => {
            // A controller sends a position, and the parameter says what a
            // position means. Spreading a logarithmic range linearly puts
            // almost all of it at the top: a knob offering a sixteenth of its
            // value to sixteen times it would reach that value at controller
            // 8 of 127, and spend the rest of the wheel above it. The taper
            // is the same one the panel and the little screen read, so a
            // wheel and a fader land in the same place.
            if *taper == ParameterTaper::Logarithmic && *minimum > 0.0 {
                (*minimum * (*maximum / *minimum).powf(normalized)).clamp(*minimum, *maximum)
            } else {
                quantize(
                    *minimum + (*maximum - *minimum) * normalized,
                    *minimum,
                    *step,
                )
                .clamp(*minimum, *maximum)
            }
        }
        ParameterKind::Integer {
            minimum,
            maximum,
            step,
            ..
        } => {
            let raw = *minimum as f64 + (*maximum - *minimum) as f64 * normalized;
            let steps = ((raw - *minimum as f64) / *step as f64).round();
            (*minimum as f64 + steps * *step as f64).clamp(*minimum as f64, *maximum as f64)
        }
        ParameterKind::Boolean { .. } | ParameterKind::Trigger => {
            if normalized >= 0.5 {
                1.0
            } else {
                0.0
            }
        }
        ParameterKind::Enum { choices, .. } => {
            let index = (normalized * choices.len().saturating_sub(1) as f64).round() as usize;
            choices[index.min(choices.len() - 1)].value as f64
        }
        ParameterKind::Meter { minimum, .. } => *minimum,
    }
}

fn quantize(value: f64, origin: f64, step: f64) -> f64 {
    origin + ((value - origin) / step).round() * step
}

/// Where a parameter's value sits over its range, 0..=1: the inverse of
/// [`map_parameter_value`], on the same taper.
fn normalize_parameter_value(kind: &ParameterKind, value: f64) -> f64 {
    let position = match kind {
        ParameterKind::Float {
            minimum,
            maximum,
            taper,
            ..
        } => {
            if *taper == ParameterTaper::Logarithmic && *minimum > 0.0 && *maximum > *minimum {
                (value.max(*minimum) / *minimum).ln() / (*maximum / *minimum).ln()
            } else if *maximum > *minimum {
                (value - *minimum) / (*maximum - *minimum)
            } else {
                0.0
            }
        }
        ParameterKind::Integer {
            minimum, maximum, ..
        } => {
            if *maximum > *minimum {
                (value - *minimum as f64) / (*maximum - *minimum) as f64
            } else {
                0.0
            }
        }
        ParameterKind::Boolean { .. } | ParameterKind::Trigger => {
            if value >= 0.5 {
                1.0
            } else {
                0.0
            }
        }
        ParameterKind::Enum { choices, .. } => {
            let position = choices
                .iter()
                .position(|choice| choice.value as f64 == value)
                .unwrap_or(0);
            if choices.len() > 1 {
                position as f64 / (choices.len() - 1) as f64
            } else {
                0.0
            }
        }
        ParameterKind::Meter {
            minimum, maximum, ..
        } => {
            if *maximum > *minimum {
                (value - *minimum) / (*maximum - *minimum)
            } else {
                0.0
            }
        }
    };
    if position.is_finite() {
        position.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_midi_api::{
        MidiChannel, MidiPacket, MidiSourceId, PARAMETER_LINK_SCHEMA_VERSION, ParameterLinkChannel,
        ParameterLinkId, ParameterLinkSource, ParameterLinkTransform,
    };
    use rackforge_plugin_api::{
        EnumChoice, PARAMETER_SCHEMA_VERSION, PageDescriptor, ParameterFlags,
        PluginSemanticControl, SemanticControlId, SuggestedControl,
    };
    use rackforge_session_api::{
        MidiControlChangeBinding, SemanticControlBinding, SemanticControlProfile,
    };

    fn schema(kind: ParameterKind) -> ParameterSchema {
        ParameterSchema {
            schema_version: PARAMETER_SCHEMA_VERSION,
            display_decimals: None,
            pages: vec![PageDescriptor {
                id: "main".into(),
                name: "Main".into(),
                order: 0,
                header: None,
            }],
            parameters: vec![ParameterDescriptor {
                index: 17,
                id: "cutoff".into(),
                name: "Cutoff".into(),
                page: "main".into(),
                group: None,
                order: 0,
                kind,
                flags: ParameterFlags {
                    automatable: true,
                    ..Default::default()
                },
                suggested_control: SuggestedControl::Knob,
            }],
            semantic_controls: Vec::new(),
        }
    }

    fn link(message: ParameterLinkMessage) -> ParameterLink {
        ParameterLink {
            schema_version: PARAMETER_LINK_SCHEMA_VERSION,
            id: ParameterLinkId::new("link.cutoff").unwrap(),
            instance_id: "desktop.main".into(),
            parameter_index: 17,
            source: ParameterLinkSource {
                source_id: MidiSourceId::new("windows.endpoint.42").unwrap(),
                display_name: "Controller".into(),
            },
            channel: ParameterLinkChannel::Channel {
                channel: MidiChannel::from_user_number(2).unwrap(),
            },
            message,
            transform: ParameterLinkTransform::default(),
            pass_through: ParameterLinkPassThrough::PassThrough,
            mode: ParameterLinkMode::Direct,
        }
    }

    fn ingress(message: &[u8]) -> IngressMidiEvent {
        IngressMidiEvent {
            source: MidiSourceKey::new(7),
            packet: MidiPacket::new(0, message).unwrap(),
        }
    }

    /// A wheel on a logarithmic parameter turns through the value the way a
    /// fader does, not through its arithmetic middle.
    #[test]
    fn cc_follows_a_logarithmic_taper() {
        let mut compiled = CompiledParameterLink::new(
            link(ParameterLinkMessage::ControlChange { controller: 74 }),
            MidiSourceKey::new(7),
            &schema(ParameterKind::Float {
                // A model knob: a sixteenth of its compiled value to sixteen
                // times it, which is the shape most of the piano's are.
                minimum: 0.5,
                maximum: 128.0,
                default: 8.0,
                step: 0.08,
                unit: None,
                taper: ParameterTaper::Logarithmic,
            }),
        )
        .unwrap();
        let mut at = |controller: u8| {
            compiled
                .apply(ingress(&[0xb1, 74, controller]), |_| None)
                .unwrap()
                .event
                .value
        };
        assert!((at(0) - 0.5).abs() < 1e-9, "the bottom is the minimum");
        assert!((at(127) - 128.0).abs() < 1e-9, "the top is the maximum");
        // Half a wheel is the geometric middle -- 8, the compiled value --
        // where a linear map would have put 64 and left the useful half of
        // the range in the first eight controller steps. Controller 64 is
        // 0.5039 of the way rather than half, and on a range this wide one
        // step of the wheel is 2%, so the window is a step wide.
        let middle = at(64);
        assert!(
            (middle - 8.0).abs() < 0.25,
            "half the wheel reached {middle}, not the value it is centred on"
        );
        assert!(at(63) < 8.0 && at(64) > 8.0, "the value is not straddled");
    }

    #[test]
    fn an_absolute_control_picks_the_parameter_up_where_it_is() {
        let float = schema(ParameterKind::Float {
            minimum: 0.0,
            maximum: 4.0,
            default: 0.0,
            step: 0.01,
            unit: None,
            taper: ParameterTaper::Linear,
        });
        let mut compiled = CompiledParameterLink::new(
            link(ParameterLinkMessage::ControlChange { controller: 102 }),
            MidiSourceKey::new(7),
            &float,
        )
        .unwrap();
        // The parameter sits at 0.0 and the knob arrives at two thirds: the
        // knob is not where the parameter is, so nothing moves.
        let parameter_at = |value: f64| move |_index: u32| Some(value);
        assert!(
            compiled
                .apply(ingress(&[0xb1, 102, 84]), parameter_at(0.0))
                .is_none()
        );
        assert!(
            compiled
                .apply(ingress(&[0xb1, 102, 60]), parameter_at(0.0))
                .is_none()
        );
        // Coming down to it, the knob takes over within the window...
        let output = compiled
            .apply(ingress(&[0xb1, 102, 3]), parameter_at(0.0))
            .unwrap();
        assert!(
            (output.event.value - 4.0 * 3.0 / 127.0).abs() < 0.011,
            "a step of 0.01"
        );
        // ...and follows from then on, wherever it goes.
        let output = compiled
            .apply(ingress(&[0xb1, 102, 100]), parameter_at(0.0))
            .unwrap();
        assert!((output.event.value - 4.0 * 100.0 / 127.0).abs() < 0.011);
        // The screen moves the parameter elsewhere: the knob is detached
        // again, and crossing the new position is enough to take over.
        compiled.observe_parameter("desktop.main", 17, 2.0);
        assert!(
            compiled
                .apply(ingress(&[0xb1, 102, 20]), parameter_at(2.0))
                .is_none()
        );
        let output = compiled
            .apply(ingress(&[0xb1, 102, 90]), parameter_at(2.0))
            .unwrap();
        assert!((output.event.value - 4.0 * 90.0 / 127.0).abs() < 0.011);
        // Another instance's parameter is not this link's business.
        compiled.observe_parameter("elsewhere", 17, 0.0);
        assert!(
            compiled
                .apply(ingress(&[0xb1, 102, 91]), parameter_at(0.0))
                .is_some()
        );
    }

    #[test]
    fn a_host_that_cannot_say_where_the_parameter_is_lets_the_control_take_over() {
        let mut compiled = CompiledParameterLink::new(
            link(ParameterLinkMessage::ControlChange { controller: 74 }),
            MidiSourceKey::new(7),
            &schema(ParameterKind::Float {
                minimum: -1.0,
                maximum: 1.0,
                default: 0.0,
                step: 0.01,
                unit: None,
                taper: ParameterTaper::Linear,
            }),
        )
        .unwrap();
        assert!(
            compiled
                .apply(ingress(&[0xb1, 74, 127]), |_| None)
                .is_some()
        );
    }

    #[test]
    fn a_gesture_is_never_held_back_by_the_pickup() {
        let mut compiled = CompiledParameterLink::new(
            link(ParameterLinkMessage::PitchBend),
            MidiSourceKey::new(7),
            &schema(ParameterKind::Float {
                minimum: -1.0,
                maximum: 1.0,
                default: 0.0,
                step: 0.01,
                unit: None,
                taper: ParameterTaper::Linear,
            }),
        )
        .unwrap();
        // The parameter is at the bottom and the wheel starts at the top.
        assert!(
            compiled
                .apply(ingress(&[0xe1, 0x7f, 0x7f]), |_| Some(-1.0))
                .is_some()
        );
    }

    #[test]
    fn normalizing_a_value_inverts_the_mapping() {
        let log = ParameterKind::Float {
            minimum: 0.25,
            maximum: 4.0,
            default: 1.0,
            step: 0.01,
            unit: None,
            taper: ParameterTaper::Logarithmic,
        };
        for position in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let value = map_parameter_value(&log, position);
            assert!((normalize_parameter_value(&log, value) - position).abs() < 0.02);
        }
        let choices = ParameterKind::Enum {
            default: 0,
            choices: vec![
                EnumChoice {
                    value: 0,
                    name: "Pitch".into(),
                },
                EnumChoice {
                    value: 1,
                    name: "Amplitude".into(),
                },
                EnumChoice {
                    value: 2,
                    name: "Both".into(),
                },
            ],
        };
        assert_eq!(normalize_parameter_value(&choices, 2.0), 1.0);
        assert_eq!(normalize_parameter_value(&choices, 1.0), 0.5);
        assert_eq!(normalize_parameter_value(&ParameterKind::Trigger, 1.0), 1.0);
    }

    #[test]
    fn cc_scales_float_and_preserves_pass_through() {
        let mut compiled = CompiledParameterLink::new(
            link(ParameterLinkMessage::ControlChange { controller: 74 }),
            MidiSourceKey::new(7),
            &schema(ParameterKind::Float {
                minimum: -1.0,
                maximum: 1.0,
                default: 0.0,
                step: 0.01,
                unit: None,
                taper: ParameterTaper::Linear,
            }),
        )
        .unwrap();
        let output = compiled.apply(ingress(&[0xb1, 74, 127]), |_| None).unwrap();
        assert_eq!(output.event.value, 1.0);
        assert_eq!(output.pass_through, ParameterLinkPassThrough::PassThrough);
        assert!(
            compiled
                .apply(ingress(&[0xb0, 74, 127]), |_| None)
                .is_none()
        );
    }

    #[test]
    fn pitch_bend_maps_endpoints_and_exact_center() {
        let mut compiled = CompiledParameterLink::new(
            link(ParameterLinkMessage::PitchBend),
            MidiSourceKey::new(7),
            &schema(ParameterKind::Float {
                minimum: -1.0,
                maximum: 1.0,
                default: 0.0,
                step: 0.000001,
                unit: None,
                taper: ParameterTaper::Linear,
            }),
        )
        .unwrap();
        assert_eq!(
            compiled
                .apply(ingress(&[0xe1, 0, 0]), |_| None)
                .unwrap()
                .event
                .value,
            -1.0
        );
        assert_eq!(
            compiled
                .apply(ingress(&[0xe1, 0, 64]), |_| None)
                .unwrap()
                .event
                .value,
            0.0
        );
        assert_eq!(
            compiled
                .apply(ingress(&[0xe1, 127, 127]), |_| None)
                .unwrap()
                .event
                .value,
            1.0
        );
    }

    #[test]
    fn bool_enum_trigger_and_pressure_quantize_to_valid_values() {
        let mut boolean = CompiledParameterLink::new(
            link(ParameterLinkMessage::ChannelPressure),
            MidiSourceKey::new(7),
            &schema(ParameterKind::Boolean { default: false }),
        )
        .unwrap();
        assert_eq!(
            boolean
                .apply(ingress(&[0xd1, 63]), |_| None)
                .unwrap()
                .event
                .value,
            0.0
        );
        assert_eq!(
            boolean
                .apply(ingress(&[0xd1, 64]), |_| None)
                .unwrap()
                .event
                .value,
            1.0
        );
        let mut enumeration = CompiledParameterLink::new(
            link(ParameterLinkMessage::ControlChange { controller: 1 }),
            MidiSourceKey::new(7),
            &schema(ParameterKind::Enum {
                default: 10,
                choices: vec![
                    EnumChoice {
                        value: 10,
                        name: "A".into(),
                    },
                    EnumChoice {
                        value: 20,
                        name: "B".into(),
                    },
                    EnumChoice {
                        value: 90,
                        name: "C".into(),
                    },
                ],
            }),
        )
        .unwrap();
        assert_eq!(
            enumeration
                .apply(ingress(&[0xb1, 1, 127]), |_| None)
                .unwrap()
                .event
                .value,
            90.0
        );
        let mut trigger = CompiledParameterLink::new(
            link(ParameterLinkMessage::Note { note: 60 }),
            MidiSourceKey::new(7),
            &schema(ParameterKind::Trigger),
        )
        .unwrap();
        assert_eq!(
            trigger
                .apply(ingress(&[0x91, 60, 100]), |_| None)
                .unwrap()
                .event
                .value,
            1.0
        );
        assert_eq!(
            trigger
                .apply(ingress(&[0x81, 60, 0]), |_| None)
                .unwrap()
                .event
                .value,
            0.0
        );
    }

    #[test]
    fn read_only_and_unknown_parameters_are_rejected_before_runtime() {
        let mut read_only = schema(ParameterKind::Boolean { default: false });
        read_only.parameters[0].flags.read_only = true;
        assert!(
            CompiledParameterLink::new(
                link(ParameterLinkMessage::ChannelPressure),
                MidiSourceKey::new(7),
                &read_only
            )
            .is_err()
        );
        let mut missing = link(ParameterLinkMessage::ChannelPressure);
        missing.parameter_index = 99;
        assert!(
            CompiledParameterLink::new(
                missing,
                MidiSourceKey::new(7),
                &schema(ParameterKind::Boolean { default: false })
            )
            .is_err()
        );
    }

    /// A Leslie speed, as an organ publishes it: three named choices.
    fn leslie() -> ParameterSchema {
        schema(ParameterKind::Enum {
            default: 0,
            choices: ["Stop", "Chorale", "Tremolo"]
                .into_iter()
                .enumerate()
                .map(|(value, name)| EnumChoice {
                    value: value as u32,
                    name: name.into(),
                })
                .collect(),
        })
    }

    fn value(value: f64) -> rackforge_midi_api::LinkValue {
        rackforge_midi_api::LinkValue::new(value).unwrap()
    }

    fn button(mode: ParameterLinkMode, schema: &ParameterSchema) -> CompiledParameterLink {
        let mut link = link(ParameterLinkMessage::ControlChange { controller: 20 });
        link.mode = mode;
        CompiledParameterLink::new(link, MidiSourceKey::new(7), schema).unwrap()
    }

    fn press(compiled: &mut CompiledParameterLink, at: f64) -> Option<f64> {
        compiled
            .apply(ingress(&[0xb1, 20, 127]), |_| Some(at))
            .map(|output| output.event.value)
    }

    fn release(compiled: &mut CompiledParameterLink, at: f64) -> Option<f64> {
        compiled
            .apply(ingress(&[0xb1, 20, 0]), |_| Some(at))
            .map(|output| output.event.value)
    }

    #[test]
    fn a_button_sets_toggles_and_cycles_the_leslie() {
        let schema = leslie();
        let mut set = button(ParameterLinkMode::Set { value: value(1.0) }, &schema);
        assert_eq!(press(&mut set, 0.0), Some(1.0));
        assert_eq!(release(&mut set, 1.0), None, "a set acts on the press only");

        // Chorale <-> Tremolo, from wherever the screen left it.
        let mut toggle = button(
            ParameterLinkMode::Toggle {
                first: value(1.0),
                second: value(2.0),
            },
            &schema,
        );
        assert_eq!(press(&mut toggle, 0.0), Some(1.0), "from Stop, the first");
        assert_eq!(press(&mut toggle, 1.0), Some(2.0));
        assert_eq!(press(&mut toggle, 2.0), Some(1.0));

        let mut cycle = button(
            ParameterLinkMode::Cycle {
                values: vec![value(0.0), value(1.0), value(2.0)],
            },
            &schema,
        );
        assert_eq!(press(&mut cycle, 0.0), Some(1.0));
        assert_eq!(press(&mut cycle, 1.0), Some(2.0));
        assert_eq!(
            press(&mut cycle, 2.0),
            Some(0.0),
            "after the last, the first"
        );
    }

    #[test]
    fn a_held_button_is_tremolo_until_it_is_let_go() {
        let schema = leslie();
        let mut hold = button(
            ParameterLinkMode::Hold {
                pressed: value(2.0),
                released: value(1.0),
            },
            &schema,
        );
        assert_eq!(press(&mut hold, 1.0), Some(2.0));
        assert_eq!(release(&mut hold, 2.0), Some(1.0));
    }

    #[test]
    fn a_button_that_only_reports_presses_presses_again() {
        let schema = leslie();
        let mut cycle = button(
            ParameterLinkMode::Cycle {
                values: vec![value(0.0), value(1.0), value(2.0)],
            },
            &schema,
        );
        // No release in between, and no host value to read: the link
        // remembers what it wrote.
        let mut presses = (0..4).map(|_| {
            cycle
                .apply(ingress(&[0xb1, 20, 127]), |_| None)
                .map(|output| output.event.value)
        });
        assert_eq!(presses.next(), Some(Some(1.0)));
        assert_eq!(presses.next(), Some(Some(2.0)));
        assert_eq!(presses.next(), Some(Some(0.0)));
    }

    #[test]
    fn a_step_stops_at_the_end_or_wraps_past_it() {
        let schema = leslie();
        let mut up = button(
            ParameterLinkMode::Step {
                direction: StepDirection::Up,
                wrap: false,
            },
            &schema,
        );
        assert_eq!(press(&mut up, 1.0), Some(2.0));
        assert_eq!(press(&mut up, 2.0), Some(2.0), "the top is the top");
        let mut around = button(
            ParameterLinkMode::Step {
                direction: StepDirection::Up,
                wrap: true,
            },
            &schema,
        );
        assert_eq!(press(&mut around, 2.0), Some(0.0));

        let volume = schema_float(0.0, 1.0, 0.001);
        let mut down = button(
            ParameterLinkMode::Step {
                direction: StepDirection::Down,
                wrap: false,
            },
            &volume,
        );
        // A button crosses a float in twenty presses, not a thousand.
        let next = press(&mut down, 0.5).unwrap();
        assert!((next - 0.45).abs() < 1e-9, "{next}");
        assert_eq!(press(&mut down, 0.0), Some(0.0));
    }

    fn schema_float(minimum: f64, maximum: f64, step: f64) -> ParameterSchema {
        schema(ParameterKind::Float {
            minimum,
            maximum,
            default: minimum,
            step,
            unit: None,
            taper: ParameterTaper::Linear,
        })
    }

    #[test]
    fn a_fader_spans_a_range_or_chooses_a_zone() {
        let drive = schema_float(0.0, 10.0, 0.1);
        let mut range = link(ParameterLinkMessage::ControlChange { controller: 74 });
        range.mode = ParameterLinkMode::Range {
            min: value(2.0),
            max: value(6.0),
        };
        let mut range = CompiledParameterLink::new(range, MidiSourceKey::new(7), &drive).unwrap();
        let at = |range: &mut CompiledParameterLink, controller: u8| {
            range
                .apply(ingress(&[0xb1, 74, controller]), |_| None)
                .unwrap()
                .event
                .value
        };
        assert!((at(&mut range, 0) - 2.0).abs() < 1e-9);
        assert!((at(&mut range, 127) - 6.0).abs() < 1e-9);

        let schema = leslie();
        let mut zones = link(ParameterLinkMessage::ControlChange { controller: 74 });
        zones.mode = ParameterLinkMode::Zones {
            values: vec![value(0.0), value(1.0), value(2.0)],
        };
        let mut zones = CompiledParameterLink::new(zones, MidiSourceKey::new(7), &schema).unwrap();
        let zone = |zones: &mut CompiledParameterLink, controller: u8| {
            zones
                .apply(ingress(&[0xb1, 74, controller]), |_| Some(2.0))
                .unwrap()
                .event
                .value
        };
        assert_eq!(zone(&mut zones, 0), 0.0);
        assert_eq!(zone(&mut zones, 64), 1.0);
        assert_eq!(zone(&mut zones, 127), 2.0);
    }

    #[test]
    fn a_range_picks_the_parameter_up_within_its_own_span() {
        let drive = schema_float(0.0, 10.0, 0.1);
        let mut range = link(ParameterLinkMessage::ControlChange { controller: 74 });
        range.mode = ParameterLinkMode::Range {
            min: value(2.0),
            max: value(6.0),
        };
        let mut range = CompiledParameterLink::new(range, MidiSourceKey::new(7), &drive).unwrap();
        // The parameter sits at 4, the middle of the range: a fader at the
        // top does not move it; brought to the middle, it takes over.
        assert!(
            range
                .apply(ingress(&[0xb1, 74, 127]), |_| Some(4.0))
                .is_none()
        );
        assert!(
            range
                .apply(ingress(&[0xb1, 74, 64]), |_| Some(4.0))
                .is_some()
        );
    }

    #[test]
    fn a_mode_is_checked_against_the_parameter_before_it_runs() {
        let speeds = leslie();
        let mut not_a_choice = link(ParameterLinkMessage::ControlChange { controller: 20 });
        not_a_choice.mode = ParameterLinkMode::Set { value: value(7.0) };
        assert!(CompiledParameterLink::new(not_a_choice, MidiSourceKey::new(7), &speeds).is_err());

        let mut range_on_choices = link(ParameterLinkMessage::ControlChange { controller: 20 });
        range_on_choices.mode = ParameterLinkMode::Range {
            min: value(0.0),
            max: value(2.0),
        };
        assert!(
            CompiledParameterLink::new(range_on_choices, MidiSourceKey::new(7), &speeds).is_err()
        );

        let mut trigger = link(ParameterLinkMessage::ControlChange { controller: 20 });
        trigger.mode = ParameterLinkMode::Trigger;
        let mut compiled = CompiledParameterLink::new(
            trigger,
            MidiSourceKey::new(7),
            &schema(ParameterKind::Trigger),
        )
        .unwrap();
        assert_eq!(press(&mut compiled, 0.0), Some(1.0));
        assert_eq!(release(&mut compiled, 1.0), Some(0.0));
    }

    fn organ_map(parameter_id: &str) -> ControllerMap {
        use rackforge_midi_api::controller_map::{ControlMapping, MappedInput, PluginControlMap};
        let mut map = ControllerMap::new("user.oxygen-49", "Oxygen 49");
        map.plugins.push(PluginControlMap {
            plugin_id: "org.rackforge.organ".into(),
            plugin_name: "RF-Organ".into(),
            mappings: vec![ControlMapping {
                id: ParameterLinkId::new("leslie").unwrap(),
                input: MappedInput {
                    id: "button-1".into(),
                    name: "Button 1".into(),
                    channel: ParameterLinkChannel::Omni,
                    message: ParameterLinkMessage::ControlChange { controller: 20 },
                },
                parameter_id: parameter_id.into(),
                mode: ParameterLinkMode::Toggle {
                    first: value(1.0),
                    second: value(2.0),
                },
                invert: false,
                pass_through: None,
            }],
        });
        map
    }

    #[test]
    fn a_controller_map_becomes_links_between_the_session_and_the_defaults() {
        let speeds = leslie();
        let source = MidiSourceId::new("alsa.oxygen-49").unwrap();
        let compile = |map: &ControllerMap, plugin_id: &str, explicit: &[ParameterLink]| {
            compile_controller_map_links(ControllerMapLinkContext {
                map,
                plugin_id,
                runtime_source_id: &source,
                source_name: "Oxygen 49",
                source_key: MidiSourceKey::new(7),
                instance_id: "desktop.main",
                schema: &speeds,
                explicit_links: explicit,
            })
        };

        // The organ's mapping, resolved by parameter id, a button that
        // consumes what it presses.
        let map = organ_map("cutoff");
        let mut compiled = compile(&map, "org.rackforge.organ", &[]);
        assert!(compiled.pending.is_empty(), "{:?}", compiled.pending);
        assert_eq!(compiled.links.len(), 1);
        assert_eq!(compiled.links[0].link.parameter_index, 17);
        let output = compiled.links[0]
            .apply(ingress(&[0xb0, 20, 127]), |_| Some(1.0))
            .unwrap();
        assert_eq!(output.event.value, 2.0);
        assert_eq!(output.pass_through, ParameterLinkPassThrough::Consume);

        // Another plugin has no mapping here.
        assert!(compile(&map, "org.rackforge.rf-106", &[]).links.is_empty());

        // A parameter the plugin no longer has stays pending.
        let stale = compile(&organ_map("leslie.gone"), "org.rackforge.organ", &[]);
        assert!(stale.links.is_empty());
        assert_eq!(stale.pending.len(), 1);

        // The session's own link on the same parameter wins.
        let explicit = link(ParameterLinkMessage::ControlChange { controller: 74 });
        assert!(
            compile(&map, "org.rackforge.organ", std::slice::from_ref(&explicit))
                .links
                .is_empty()
        );
    }

    #[test]
    fn semantic_defaults_bind_by_role_and_explicit_links_override_them() {
        let mut plugin = schema(ParameterKind::Float {
            minimum: 0.0,
            maximum: 1.0,
            default: 0.5,
            step: 0.01,
            unit: None,
            taper: ParameterTaper::Linear,
        });
        plugin.semantic_controls = vec![PluginSemanticControl {
            role: SemanticControlId::new("synth.filter.cutoff").unwrap(),
            parameter_index: 17,
        }];
        let profile = SemanticControlProfile {
            schema_version: rackforge_plugin_api::CONTROL_PROFILE_SCHEMA_VERSION,
            source_id: "controller.arturia.keylab.midi".into(),
            controls: vec![SemanticControlBinding {
                role: SemanticControlId::new("synth.filter.cutoff").unwrap(),
                midi_cc: MidiControlChangeBinding {
                    channel: 0,
                    controller: 109,
                },
                invert: false,
                mode: rackforge_session_api::SemanticControlMode::Absolute,
            }],
        };
        let runtime_source_id = MidiSourceId::new("windows.endpoint.42").unwrap();

        let mut automatic = compile_semantic_parameter_links(SemanticParameterLinkContext {
            controller_id: "org.rackforge.arturia",
            controller_name: "Arturia KeyLab",
            profile: &profile,
            runtime_source_id: &runtime_source_id,
            source_key: MidiSourceKey::new(5),
            instance_id: "desktop.main",
            schema: &plugin,
            explicit_links: &[],
        })
        .unwrap();
        assert_eq!(automatic.len(), 1);
        assert_eq!(automatic[0].link.parameter_index, 17);
        assert_eq!(automatic[0].link.source.source_id, runtime_source_id);
        assert_eq!(
            automatic[0]
                .apply(
                    IngressMidiEvent {
                        source: MidiSourceKey::new(5),
                        packet: MidiPacket::new(0, &[0xb0, 109, 127]).unwrap(),
                    },
                    |_| None
                )
                .unwrap()
                .event
                .value,
            1.0
        );

        let explicit_same_parameter = link(ParameterLinkMessage::ControlChange { controller: 74 });
        assert!(
            compile_semantic_parameter_links(SemanticParameterLinkContext {
                controller_id: "org.rackforge.arturia",
                controller_name: "Arturia KeyLab",
                profile: &profile,
                runtime_source_id: &MidiSourceId::new("windows.endpoint.42").unwrap(),
                source_key: MidiSourceKey::new(5),
                instance_id: "desktop.main",
                schema: &plugin,
                explicit_links: &[explicit_same_parameter],
            })
            .unwrap()
            .is_empty()
        );
    }
}
