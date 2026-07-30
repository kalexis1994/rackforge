use crate::custom_program::{CustomProgram, ProgramLayer};
use rackforge_plugin_api::{
    PROGRAM_EDITOR_SCHEMA_VERSION, ProgramEditorChoice, ProgramEditorField, ProgramEditorFieldKind,
    ProgramEditorPage, ProgramEditorValue, ProgramEditorView,
};

pub fn view(program: &CustomProgram) -> ProgramEditorView {
    let mut pages = vec![layer_page(&program.layers[0], false)];
    if let Some(layer) = program.layers.get(1) {
        pages.push(layer_page(layer, true));
    } else {
        pages.push(ProgramEditorPage {
            id: "layer-b".into(),
            label: "LAYER B".into(),
            detail: "Optional layer".into(),
            enabled: true,
            pages: Vec::new(),
            fields: vec![toggle(
                "layer.b.enabled",
                "ENABLED",
                "Enable layer B",
                false,
                false,
            )],
        });
    }
    pages.push(ProgramEditorPage {
        id: "fx".into(),
        label: "FX".into(),
        detail: "Shared FX chain".into(),
        enabled: false,
        pages: Vec::new(),
        fields: vec![choice(
            "fx.status",
            "STATUS",
            "Coming next",
            "none",
            vec![("none", "NO FX")],
            false,
        )],
    });
    pages.push(ProgramEditorPage {
        id: "output".into(),
        label: "OUTPUT".into(),
        detail: "Final program gain".into(),
        enabled: true,
        pages: Vec::new(),
        fields: vec![number(
            "program.gain",
            "VOLUME",
            "Program output gain",
            Some(scaled(program.gain, 100.0)),
            0,
            200,
            1,
            2,
            None,
            false,
            true,
        )],
    });
    ProgramEditorView {
        schema_version: PROGRAM_EDITOR_SCHEMA_VERSION,
        title: "RF-DLS".into(),
        pages,
    }
}

fn layer_page(layer: &ProgramLayer, optional: bool) -> ProgramEditorPage {
    let prefix = format!("layer.{}", layer.id);
    let mut fields = Vec::new();
    if optional {
        fields.push(toggle(
            &format!("{prefix}.enabled"),
            "ENABLED",
            "Enable layer B",
            layer.enabled,
            false,
        ));
    }
    let source_id = format!(
        "dls.b{:08x}.p{:08x}",
        layer.source.bank, layer.source.program
    );
    let p = &layer.parameters;
    ProgramEditorPage {
        id: format!("layer-{}", layer.id),
        label: format!("LAYER {}", layer.id.to_ascii_uppercase()),
        detail: if optional {
            "Optional layer".into()
        } else {
            "Required layer".into()
        },
        enabled: !optional || layer.enabled,
        fields,
        pages: vec![
            page(
                &format!("{prefix}.timbre"),
                "TIMBRE",
                "DLS source",
                vec![sound(
                    &format!("{prefix}.sound"),
                    "TIMBRE",
                    "DLS source",
                    &source_id,
                )],
            ),
            page(
                &format!("{prefix}.amp-env"),
                "AMP ENV",
                "Amplitude ADSR",
                vec![
                    optional_number(
                        &format!("{prefix}.amp.attack"),
                        "ATTACK",
                        "Attack time",
                        p.amplitude_envelope.attack_seconds,
                        0,
                        6000,
                        1,
                        2,
                        Some("s"),
                    ),
                    optional_number(
                        &format!("{prefix}.amp.decay"),
                        "DECAY",
                        "Decay time",
                        p.amplitude_envelope.decay_seconds,
                        0,
                        6000,
                        1,
                        2,
                        Some("s"),
                    ),
                    optional_number(
                        &format!("{prefix}.amp.sustain"),
                        "SUSTAIN",
                        "Sustain level",
                        p.amplitude_envelope.sustain_level,
                        0,
                        100,
                        1,
                        2,
                        None,
                    ),
                    optional_number(
                        &format!("{prefix}.amp.release"),
                        "RELEASE",
                        "Release time",
                        p.amplitude_envelope.release_seconds,
                        0,
                        6000,
                        1,
                        2,
                        Some("s"),
                    ),
                ],
            ),
            page(
                &format!("{prefix}.pitch-env"),
                "PITCH ENV",
                "Pitch EG override",
                vec![
                    optional_number(
                        &format!("{prefix}.pitch.attack"),
                        "ATTACK",
                        "Attack time",
                        p.pitch_envelope.attack_seconds,
                        0,
                        6000,
                        1,
                        2,
                        Some("s"),
                    ),
                    optional_number(
                        &format!("{prefix}.pitch.decay"),
                        "DECAY",
                        "Decay time",
                        p.pitch_envelope.decay_seconds,
                        0,
                        6000,
                        1,
                        2,
                        Some("s"),
                    ),
                    optional_number(
                        &format!("{prefix}.pitch.sustain"),
                        "SUSTAIN",
                        "Sustain level",
                        p.pitch_envelope.sustain_level,
                        0,
                        100,
                        1,
                        2,
                        None,
                    ),
                    optional_number(
                        &format!("{prefix}.pitch.release"),
                        "RELEASE",
                        "Release time",
                        p.pitch_envelope.release_seconds,
                        0,
                        6000,
                        1,
                        2,
                        Some("s"),
                    ),
                    optional_number(
                        &format!("{prefix}.pitch.depth"),
                        "DEPTH",
                        "Envelope depth",
                        p.pitch_envelope.depth_cents,
                        -4800,
                        4800,
                        1,
                        0,
                        Some("ct"),
                    ),
                ],
            ),
            page(
                &format!("{prefix}.lfo"),
                "LFO",
                "Rate delay depth",
                vec![
                    choice(
                        &format!("{prefix}.lfo.enabled"),
                        "ENABLED",
                        "LFO override",
                        match p.lfo.enabled {
                            None => "inherit",
                            Some(true) => "on",
                            Some(false) => "off",
                        },
                        vec![("inherit", "INHERIT"), ("on", "ON"), ("off", "OFF")],
                        true,
                    ),
                    optional_number(
                        &format!("{prefix}.lfo.frequency"),
                        "RATE",
                        "LFO frequency",
                        p.lfo.frequency_hz,
                        1,
                        5000,
                        1,
                        2,
                        Some("Hz"),
                    ),
                    optional_number(
                        &format!("{prefix}.lfo.delay"),
                        "DELAY",
                        "LFO delay",
                        p.lfo.delay_seconds,
                        0,
                        6000,
                        1,
                        2,
                        Some("s"),
                    ),
                    optional_number(
                        &format!("{prefix}.lfo.pitch"),
                        "PITCH",
                        "Pitch depth",
                        p.lfo.pitch_depth_cents,
                        -2400,
                        2400,
                        1,
                        0,
                        Some("ct"),
                    ),
                    optional_number(
                        &format!("{prefix}.lfo.mod-pitch"),
                        "MOD PITCH",
                        "Wheel pitch depth",
                        p.lfo.mod_wheel_pitch_depth_cents,
                        -2400,
                        2400,
                        1,
                        0,
                        Some("ct"),
                    ),
                    optional_number(
                        &format!("{prefix}.lfo.amp"),
                        "AMP",
                        "Amplitude depth",
                        p.lfo.attenuation_depth_centibels,
                        -10000,
                        10000,
                        1,
                        0,
                        Some("cB"),
                    ),
                    optional_number(
                        &format!("{prefix}.lfo.mod-amp"),
                        "MOD AMP",
                        "Wheel amp depth",
                        p.lfo.mod_wheel_attenuation_depth_centibels,
                        -10000,
                        10000,
                        1,
                        0,
                        Some("cB"),
                    ),
                ],
            ),
            page(
                &format!("{prefix}.tuning"),
                "TUNING",
                "Pitch and expression",
                vec![
                    number(
                        &format!("{prefix}.transpose"),
                        "OCT/PITCH",
                        "Transpose semitones",
                        Some(i64::from(p.transpose_semitones)),
                        -48,
                        48,
                        1,
                        0,
                        Some("st"),
                        false,
                        true,
                    ),
                    number(
                        &format!("{prefix}.fine"),
                        "FINE",
                        "Fine tune",
                        Some(scaled(p.fine_tune_cents, 1.0)),
                        -100,
                        100,
                        1,
                        0,
                        Some("ct"),
                        false,
                        true,
                    ),
                    number(
                        &format!("{prefix}.bend"),
                        "PITCH BEND",
                        "Wheel range",
                        Some(scaled(p.pitch_bend_range_semitones, 10.0)),
                        0,
                        240,
                        1,
                        1,
                        Some("st"),
                        false,
                        true,
                    ),
                    number(
                        &format!("{prefix}.mod-depth"),
                        "MOD DEPTH",
                        "Modulation amount",
                        Some(scaled(p.modulation_depth, 100.0)),
                        0,
                        200,
                        1,
                        2,
                        None,
                        false,
                        true,
                    ),
                ],
            ),
            page(
                &format!("{prefix}.range"),
                "RANGE",
                "Key and velocity",
                vec![
                    number(
                        &format!("{prefix}.key-low"),
                        "KEY LOW",
                        "Lowest MIDI note",
                        Some(i64::from(layer.key_range.low)),
                        0,
                        127,
                        1,
                        0,
                        None,
                        false,
                        true,
                    ),
                    number(
                        &format!("{prefix}.key-high"),
                        "KEY HIGH",
                        "Highest MIDI note",
                        Some(i64::from(layer.key_range.high)),
                        0,
                        127,
                        1,
                        0,
                        None,
                        false,
                        true,
                    ),
                    number(
                        &format!("{prefix}.vel-low"),
                        "VEL LOW",
                        "Lowest velocity",
                        Some(i64::from(layer.velocity_range.low)),
                        0,
                        127,
                        1,
                        0,
                        None,
                        false,
                        true,
                    ),
                    number(
                        &format!("{prefix}.vel-high"),
                        "VEL HIGH",
                        "Highest velocity",
                        Some(i64::from(layer.velocity_range.high)),
                        0,
                        127,
                        1,
                        0,
                        None,
                        false,
                        true,
                    ),
                ],
            ),
            page(
                &format!("{prefix}.volume"),
                "VOLUME",
                "Layer mix gain",
                vec![number(
                    &format!("{prefix}.gain"),
                    "VOLUME",
                    "Layer mix gain",
                    Some(scaled(p.gain, 100.0)),
                    0,
                    200,
                    1,
                    2,
                    None,
                    false,
                    true,
                )],
            ),
        ],
    }
}

pub fn apply(
    program: &mut CustomProgram,
    field_id: &str,
    value: ProgramEditorValue,
) -> Result<(), String> {
    if field_id == "program.gain" {
        program.gain = required_number(value, field_id)? as f32 / 100.0;
        return Ok(());
    }
    let rest = field_id
        .strip_prefix("layer.")
        .ok_or_else(|| format!("unknown RF-DLS editor field {field_id:?}"))?;
    let (layer_id, parameter) = rest
        .split_once('.')
        .ok_or_else(|| format!("invalid RF-DLS editor field {field_id:?}"))?;
    let index = match layer_id {
        "a" => 0,
        "b" => 1,
        _ => return Err(format!("unknown RF-DLS layer {layer_id:?}")),
    };
    if index == 1 && program.layers.len() == 1 {
        if parameter != "enabled" {
            return Err("layer B must be enabled before it can be edited".into());
        }
        let enabled = required_boolean(value, field_id)?;
        if enabled {
            let mut layer = program.layers[0].clone();
            layer.id = "b".into();
            layer.enabled = true;
            program.layers.push(layer);
        }
        return Ok(());
    }
    let layer = program
        .layers
        .get_mut(index)
        .ok_or_else(|| format!("RF-DLS layer {layer_id:?} is unavailable"))?;
    match parameter {
        "enabled" if index == 1 => layer.enabled = required_boolean(value, field_id)?,
        "sound" => {
            let ProgramEditorValue::SoundId(id) = value else {
                return Err(format!("{field_id:?} requires a sound id"));
            };
            let (bank, preset) = crate::parse_dynamic_preset_id(&id)
                .ok_or_else(|| format!("invalid RF-DLS sound id {id:?}"))?;
            layer.source.bank = bank;
            layer.source.program = preset;
        }
        "gain" => layer.parameters.gain = required_number(value, field_id)? as f32 / 100.0,
        "transpose" => layer.parameters.transpose_semitones = checked_i8(value, field_id)?,
        "fine" => layer.parameters.fine_tune_cents = required_number(value, field_id)? as f32,
        "bend" => {
            layer.parameters.pitch_bend_range_semitones =
                required_number(value, field_id)? as f32 / 10.0
        }
        "mod-depth" => {
            layer.parameters.modulation_depth = required_number(value, field_id)? as f32 / 100.0
        }
        "key-low" => layer.key_range.low = checked_u8(value, field_id)?,
        "key-high" => layer.key_range.high = checked_u8(value, field_id)?,
        "vel-low" => layer.velocity_range.low = checked_u8(value, field_id)?,
        "vel-high" => layer.velocity_range.high = checked_u8(value, field_id)?,
        "amp.attack" => {
            layer.parameters.amplitude_envelope.attack_seconds =
                optional_scaled(value, 100.0, field_id)?
        }
        "amp.decay" => {
            layer.parameters.amplitude_envelope.decay_seconds =
                optional_scaled(value, 100.0, field_id)?
        }
        "amp.sustain" => {
            layer.parameters.amplitude_envelope.sustain_level =
                optional_scaled(value, 100.0, field_id)?
        }
        "amp.release" => {
            layer.parameters.amplitude_envelope.release_seconds =
                optional_scaled(value, 100.0, field_id)?
        }
        "pitch.attack" => {
            layer.parameters.pitch_envelope.attack_seconds =
                optional_scaled(value, 100.0, field_id)?
        }
        "pitch.decay" => {
            layer.parameters.pitch_envelope.decay_seconds = optional_scaled(value, 100.0, field_id)?
        }
        "pitch.sustain" => {
            layer.parameters.pitch_envelope.sustain_level = optional_scaled(value, 100.0, field_id)?
        }
        "pitch.release" => {
            layer.parameters.pitch_envelope.release_seconds =
                optional_scaled(value, 100.0, field_id)?
        }
        "pitch.depth" => {
            layer.parameters.pitch_envelope.depth_cents = optional_scaled(value, 1.0, field_id)?
        }
        "lfo.enabled" => {
            let ProgramEditorValue::Choice(choice) = value else {
                return Err(format!("{field_id:?} requires a choice"));
            };
            layer.parameters.lfo.enabled = match choice.as_str() {
                "inherit" => None,
                "on" => Some(true),
                "off" => Some(false),
                _ => return Err(format!("invalid LFO enabled choice {choice:?}")),
            };
        }
        "lfo.frequency" => {
            layer.parameters.lfo.frequency_hz = optional_scaled(value, 100.0, field_id)?
        }
        "lfo.delay" => {
            layer.parameters.lfo.delay_seconds = optional_scaled(value, 100.0, field_id)?
        }
        "lfo.pitch" => {
            layer.parameters.lfo.pitch_depth_cents = optional_scaled(value, 1.0, field_id)?
        }
        "lfo.mod-pitch" => {
            layer.parameters.lfo.mod_wheel_pitch_depth_cents =
                optional_scaled(value, 1.0, field_id)?
        }
        "lfo.amp" => {
            layer.parameters.lfo.attenuation_depth_centibels =
                optional_scaled(value, 1.0, field_id)?
        }
        "lfo.mod-amp" => {
            layer.parameters.lfo.mod_wheel_attenuation_depth_centibels =
                optional_scaled(value, 1.0, field_id)?
        }
        _ => return Err(format!("unknown RF-DLS editor field {field_id:?}")),
    }
    Ok(())
}

fn page(id: &str, label: &str, detail: &str, fields: Vec<ProgramEditorField>) -> ProgramEditorPage {
    ProgramEditorPage {
        id: id.into(),
        label: label.into(),
        detail: detail.into(),
        enabled: true,
        pages: Vec::new(),
        fields,
    }
}

fn toggle(
    id: &str,
    label: &str,
    detail: &str,
    value: bool,
    live_preview: bool,
) -> ProgramEditorField {
    ProgramEditorField {
        id: id.into(),
        label: label.into(),
        detail: detail.into(),
        value: ProgramEditorValue::Boolean(value),
        kind: ProgramEditorFieldKind::Toggle,
        live_preview,
    }
}

fn sound(id: &str, label: &str, detail: &str, value: &str) -> ProgramEditorField {
    ProgramEditorField {
        id: id.into(),
        label: label.into(),
        detail: detail.into(),
        value: ProgramEditorValue::SoundId(value.into()),
        kind: ProgramEditorFieldKind::Sound {
            bank: Some("dls".into()),
        },
        live_preview: true,
    }
}

#[allow(clippy::too_many_arguments)]
fn number(
    id: &str,
    label: &str,
    detail: &str,
    value: Option<i64>,
    minimum: i64,
    maximum: i64,
    step: i64,
    decimals: u8,
    unit: Option<&str>,
    allow_inherited: bool,
    live_preview: bool,
) -> ProgramEditorField {
    ProgramEditorField {
        id: id.into(),
        label: label.into(),
        detail: detail.into(),
        value: value.map_or(ProgramEditorValue::Inherited, ProgramEditorValue::Integer),
        kind: ProgramEditorFieldKind::Number {
            minimum,
            maximum,
            step,
            decimals,
            unit: unit.map(str::to_owned),
            allow_inherited,
        },
        live_preview,
    }
}

#[allow(clippy::too_many_arguments)]
fn optional_number(
    id: &str,
    label: &str,
    detail: &str,
    value: Option<f32>,
    minimum: i64,
    maximum: i64,
    step: i64,
    decimals: u8,
    unit: Option<&str>,
) -> ProgramEditorField {
    number(
        id,
        label,
        detail,
        value.map(|value| scaled(value, 10_i64.pow(u32::from(decimals)) as f32)),
        minimum,
        maximum,
        step,
        decimals,
        unit,
        true,
        true,
    )
}

fn choice(
    id: &str,
    label: &str,
    detail: &str,
    value: &str,
    options: Vec<(&str, &str)>,
    live_preview: bool,
) -> ProgramEditorField {
    ProgramEditorField {
        id: id.into(),
        label: label.into(),
        detail: detail.into(),
        value: ProgramEditorValue::Choice(value.into()),
        kind: ProgramEditorFieldKind::Choice {
            options: options
                .into_iter()
                .map(|(value, label)| ProgramEditorChoice {
                    value: value.into(),
                    label: label.into(),
                    detail: None,
                })
                .collect(),
        },
        live_preview,
    }
}

fn scaled(value: f32, scale: f32) -> i64 {
    (value * scale).round() as i64
}

fn required_number(value: ProgramEditorValue, field: &str) -> Result<i64, String> {
    match value {
        ProgramEditorValue::Integer(value) => Ok(value),
        _ => Err(format!("{field:?} requires an integer editor value")),
    }
}

fn required_boolean(value: ProgramEditorValue, field: &str) -> Result<bool, String> {
    match value {
        ProgramEditorValue::Boolean(value) => Ok(value),
        _ => Err(format!("{field:?} requires a boolean editor value")),
    }
}

fn optional_scaled(
    value: ProgramEditorValue,
    scale: f32,
    field: &str,
) -> Result<Option<f32>, String> {
    match value {
        ProgramEditorValue::Inherited => Ok(None),
        ProgramEditorValue::Integer(value) => Ok(Some(value as f32 / scale)),
        _ => Err(format!(
            "{field:?} requires a numeric or inherited editor value"
        )),
    }
}

fn checked_u8(value: ProgramEditorValue, field: &str) -> Result<u8, String> {
    u8::try_from(required_number(value, field)?)
        .map_err(|_| format!("{field:?} is outside the u8 range"))
}

fn checked_i8(value: ProgramEditorValue, field: &str) -> Result<i8, String> {
    i8::try_from(required_number(value, field)?)
        .map_err(|_| format!("{field:?} is outside the i8 range"))
}
