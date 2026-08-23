use anyhow::{Result, bail};
use rackforge_midi_api::{
    IngressMidiEvent, MidiChannel, MidiMessageKind, MidiSourceId, MidiSourceKey,
    PARAMETER_LINK_SCHEMA_VERSION, ParameterLink, ParameterLinkChannel, ParameterLinkId,
    ParameterLinkMessage, ParameterLinkPassThrough, ParameterLinkSource, ParameterLinkTransform,
};
use rackforge_plugin_api::abi::ParameterEventV1;
use rackforge_plugin_api::{ParameterDescriptor, ParameterKind, ParameterSchema};
use rackforge_session_api::SemanticControlProfile;

use crate::validate_parameter_write;

#[derive(Clone, Debug)]
pub struct CompiledParameterLink {
    pub link: ParameterLink,
    pub source_key: MidiSourceKey,
    parameter: ParameterDescriptor,
}

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
        Ok(Self {
            link,
            source_key,
            parameter,
        })
    }

    pub fn apply(&self, ingress: IngressMidiEvent) -> Option<ParameterLinkOutput> {
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
        Some(ParameterLinkOutput {
            event: ParameterEventV1 {
                frame: ingress.packet.frame,
                parameter_index: self.parameter.index,
                value: map_parameter_value(&self.parameter.kind, normalized),
            },
            pass_through: self.link.pass_through,
        })
    }
}

/// Compiles controller-declared semantic controls for one plugin instance.
///
/// These links are runtime defaults, never persisted session objects. An
/// explicit user link wins when it targets either the same physical control or
/// the same semantic plugin parameter, preventing double writes.
pub fn compile_semantic_parameter_links(
    controller_id: &str,
    controller_name: &str,
    profile: &SemanticControlProfile,
    runtime_source_id: &MidiSourceId,
    source_key: MidiSourceKey,
    instance_id: &str,
    schema: &ParameterSchema,
    explicit_links: &[ParameterLink],
) -> Result<Vec<CompiledParameterLink>> {
    profile.validate().map_err(anyhow::Error::msg)?;
    schema.validate().map_err(anyhow::Error::msg)?;
    let mut compiled = Vec::new();
    for control in &profile.controls {
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
        };
        compiled.push(CompiledParameterLink::new(link, source_key, schema)?);
    }
    Ok(compiled)
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
            ..
        } => quantize(
            *minimum + (*maximum - *minimum) * normalized,
            *minimum,
            *step,
        )
        .clamp(*minimum, *maximum),
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
        }
    }

    fn ingress(message: &[u8]) -> IngressMidiEvent {
        IngressMidiEvent {
            source: MidiSourceKey::new(7),
            packet: MidiPacket::new(0, message).unwrap(),
        }
    }

    #[test]
    fn cc_scales_float_and_preserves_pass_through() {
        let compiled = CompiledParameterLink::new(
            link(ParameterLinkMessage::ControlChange { controller: 74 }),
            MidiSourceKey::new(7),
            &schema(ParameterKind::Float {
                minimum: -1.0,
                maximum: 1.0,
                default: 0.0,
                step: 0.01,
                unit: None,
            }),
        )
        .unwrap();
        let output = compiled.apply(ingress(&[0xb1, 74, 127])).unwrap();
        assert_eq!(output.event.value, 1.0);
        assert_eq!(output.pass_through, ParameterLinkPassThrough::PassThrough);
        assert!(compiled.apply(ingress(&[0xb0, 74, 127])).is_none());
    }

    #[test]
    fn pitch_bend_maps_endpoints_and_exact_center() {
        let compiled = CompiledParameterLink::new(
            link(ParameterLinkMessage::PitchBend),
            MidiSourceKey::new(7),
            &schema(ParameterKind::Float {
                minimum: -1.0,
                maximum: 1.0,
                default: 0.0,
                step: 0.000001,
                unit: None,
            }),
        )
        .unwrap();
        assert_eq!(
            compiled.apply(ingress(&[0xe1, 0, 0])).unwrap().event.value,
            -1.0
        );
        assert_eq!(
            compiled.apply(ingress(&[0xe1, 0, 64])).unwrap().event.value,
            0.0
        );
        assert_eq!(
            compiled
                .apply(ingress(&[0xe1, 127, 127]))
                .unwrap()
                .event
                .value,
            1.0
        );
    }

    #[test]
    fn bool_enum_trigger_and_pressure_quantize_to_valid_values() {
        let boolean = CompiledParameterLink::new(
            link(ParameterLinkMessage::ChannelPressure),
            MidiSourceKey::new(7),
            &schema(ParameterKind::Boolean { default: false }),
        )
        .unwrap();
        assert_eq!(
            boolean.apply(ingress(&[0xd1, 63])).unwrap().event.value,
            0.0
        );
        assert_eq!(
            boolean.apply(ingress(&[0xd1, 64])).unwrap().event.value,
            1.0
        );
        let enumeration = CompiledParameterLink::new(
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
                .apply(ingress(&[0xb1, 1, 127]))
                .unwrap()
                .event
                .value,
            90.0
        );
        let trigger = CompiledParameterLink::new(
            link(ParameterLinkMessage::Note { note: 60 }),
            MidiSourceKey::new(7),
            &schema(ParameterKind::Trigger),
        )
        .unwrap();
        assert_eq!(
            trigger
                .apply(ingress(&[0x91, 60, 100]))
                .unwrap()
                .event
                .value,
            1.0
        );
        assert_eq!(
            trigger.apply(ingress(&[0x81, 60, 0])).unwrap().event.value,
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

    #[test]
    fn semantic_defaults_bind_by_role_and_explicit_links_override_them() {
        let mut plugin = schema(ParameterKind::Float {
            minimum: 0.0,
            maximum: 1.0,
            default: 0.5,
            step: 0.01,
            unit: None,
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
            }],
        };
        let runtime_source_id = MidiSourceId::new("windows.endpoint.42").unwrap();

        let automatic = compile_semantic_parameter_links(
            "org.rackforge.arturia",
            "Arturia KeyLab",
            &profile,
            &runtime_source_id,
            MidiSourceKey::new(5),
            "desktop.main",
            &plugin,
            &[],
        )
        .unwrap();
        assert_eq!(automatic.len(), 1);
        assert_eq!(automatic[0].link.parameter_index, 17);
        assert_eq!(automatic[0].link.source.source_id, runtime_source_id);
        assert_eq!(
            automatic[0]
                .apply(IngressMidiEvent {
                    source: MidiSourceKey::new(5),
                    packet: MidiPacket::new(0, &[0xb0, 109, 127]).unwrap(),
                })
                .unwrap()
                .event
                .value,
            1.0
        );

        let explicit_same_parameter = link(ParameterLinkMessage::ControlChange { controller: 74 });
        assert!(
            compile_semantic_parameter_links(
                "org.rackforge.arturia",
                "Arturia KeyLab",
                &profile,
                &MidiSourceId::new("windows.endpoint.42").unwrap(),
                MidiSourceKey::new(5),
                "desktop.main",
                &plugin,
                &[explicit_same_parameter],
            )
            .unwrap()
            .is_empty()
        );
    }
}
