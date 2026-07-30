use rackforge_plugin_api::{
    PROGRAM_EDIT_SCHEMA_VERSION, PROGRAM_SCHEMA_VERSION, PreparedProgram, ProgramDocument,
};
use rf_dls::{EnvelopeSpec, Instrument, LfoSpec, PitchEnvelopeSpec, VoiceConfig};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const PLUGIN_ID: &str = "org.rackforge.rf-dls";
pub const PAYLOAD_VERSION: u32 = 6;
pub const EXCITER_PAYLOAD_VERSION: u32 = 5;
pub const REVERB_PAYLOAD_VERSION: u32 = 4;
pub const CHORUS_PAYLOAD_VERSION: u32 = 3;
pub const LAYER_PAYLOAD_VERSION: u32 = 2;
pub const LEGACY_PAYLOAD_VERSION: u32 = 1;
pub const PROGRAM_SUFFIX: &str = ".rackforge-program.json";
pub const MAX_LAYERS: usize = 2;
const MAX_PROGRAM_BYTES: u64 = 256 * 1024;
const MAX_PROGRAMS: usize = 1024;

#[derive(Clone, Debug)]
pub struct CustomProgram {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    pub slot: u16,
    pub gain: f32,
    pub layers: Vec<ProgramLayer>,
    pub effects: SharedEffects,
}

impl CustomProgram {
    pub fn preset_id(&self) -> String {
        format!("custom.{}", self.id)
    }

    pub fn from_document(document: ProgramDocument) -> Result<Self, String> {
        document.validate().map_err(|error| error.to_string())?;
        if document.plugin_id != PLUGIN_ID {
            return Err("program belongs to a different plugin".into());
        }
        if !matches!(document.plugin_state_version, 1 | 2) {
            return Err(format!(
                "unsupported RF-DLS state compatibility {}",
                document.plugin_state_version
            ));
        }
        let (slot, gain, layers, effects) = match document.payload_version {
            LEGACY_PAYLOAD_VERSION => {
                let payload: LegacyCustomProgramPayload = serde_json::from_value(document.payload)
                    .map_err(|error| format!("parsing RF-DLS v1 payload: {error}"))?;
                payload.validate()?;
                (
                    payload.slot,
                    payload.parameters.gain,
                    vec![ProgramLayer::from_legacy(
                        payload.source,
                        payload.parameters,
                    )],
                    SharedEffects::default(),
                )
            }
            EXCITER_PAYLOAD_VERSION => {
                let payload: V5CustomProgramPayload = serde_json::from_value(document.payload)
                    .map_err(|error| format!("parsing RF-DLS v5 payload: {error}"))?;
                let payload = payload.migrate()?;
                (payload.slot, payload.gain, payload.layers, payload.effects)
            }
            LAYER_PAYLOAD_VERSION
            | CHORUS_PAYLOAD_VERSION
            | REVERB_PAYLOAD_VERSION
            | PAYLOAD_VERSION => {
                let payload: CustomProgramPayload = serde_json::from_value(document.payload)
                    .map_err(|error| {
                        format!(
                            "parsing RF-DLS v{} payload: {error}",
                            document.payload_version
                        )
                    })?;
                payload.validate()?;
                (payload.slot, payload.gain, payload.layers, payload.effects)
            }
            version => return Err(format!("unsupported RF-DLS payload version {version}")),
        };
        let program = Self {
            id: document.id,
            name: document.name,
            category: document.category,
            slot,
            gain,
            layers,
            effects,
        };
        program.validate()?;
        Ok(program)
    }

    pub fn to_document(&self) -> Result<ProgramDocument, String> {
        Ok(ProgramDocument {
            schema_version: PROGRAM_SCHEMA_VERSION,
            id: self.id.clone(),
            name: self.name.clone(),
            plugin_id: PLUGIN_ID.into(),
            plugin_version: env!("CARGO_PKG_VERSION").into(),
            plugin_state_version: 2,
            payload_version: PAYLOAD_VERSION,
            category: self.category.clone(),
            tags: vec!["custom".into()],
            payload: serde_json::to_value(CustomProgramPayload {
                slot: self.slot,
                gain: self.gain,
                layers: self.layers.clone(),
                effects: self.effects,
            })
            .map_err(|error| format!("serializing RF-DLS payload: {error}"))?,
        })
    }

    pub fn prepared(&self) -> Result<PreparedProgram, String> {
        let source = &self
            .layers
            .iter()
            .find(|layer| layer.enabled)
            .ok_or_else(|| "CUSTOM program has no enabled layer".to_owned())?
            .source;
        Ok(PreparedProgram {
            schema_version: PROGRAM_EDIT_SCHEMA_VERSION,
            storage_path: format!("custom/{}{}", self.id, PROGRAM_SUFFIX),
            preview_sound_id: format!("dls.b{:08x}.p{:08x}", source.bank, source.program),
            document: self.to_document()?,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.slot == 0 || self.slot > 999 {
            return Err("CUSTOM slot must be between 1 and 999".into());
        }
        finite_range(self.gain, 0.0, 2.0, "program gain")?;
        if self.layers.is_empty() || self.layers.len() > MAX_LAYERS {
            return Err(format!(
                "CUSTOM program must contain between one and {MAX_LAYERS} layers"
            ));
        }
        if self.layers[0].id != "a" || !self.layers[0].enabled {
            return Err("CUSTOM program layer A is mandatory and must be enabled".into());
        }
        if self.layers.get(1).is_some_and(|layer| layer.id != "b") {
            return Err("the second CUSTOM program layer must be layer B".into());
        }
        let mut ids = BTreeSet::new();
        for layer in &self.layers {
            layer.validate()?;
            if !ids.insert(layer.id.as_str()) {
                return Err(format!("duplicate CUSTOM layer id {:?}", layer.id));
            }
        }
        self.effects.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustomProgramPayload {
    pub slot: u16,
    #[serde(default = "default_gain")]
    pub gain: f32,
    pub layers: Vec<ProgramLayer>,
    #[serde(default)]
    pub effects: SharedEffects,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V5CustomProgramPayload {
    slot: u16,
    #[serde(default = "default_gain")]
    gain: f32,
    layers: Vec<ProgramLayer>,
    #[serde(default)]
    effects: V5SharedEffects,
}

impl V5CustomProgramPayload {
    fn migrate(self) -> Result<CustomProgramPayload, String> {
        self.effects.exciter.validate()?;
        Ok(CustomProgramPayload {
            slot: self.slot,
            gain: self.gain,
            layers: self.layers,
            effects: SharedEffects {
                exciter: self.effects.exciter.migrate(),
                chorus: self.effects.chorus,
                reverb: self.effects.reverb,
            },
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct V5SharedEffects {
    exciter: V5ExciterEffectSettings,
    chorus: ChorusEffectSettings,
    reverb: ReverbEffectSettings,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct V5ExciterEffectSettings {
    enabled: bool,
    frequency_hz: f32,
    drive: f32,
    amount: f32,
}

impl Default for V5ExciterEffectSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            frequency_hz: 3_500.0,
            drive: 4.0,
            amount: 0.20,
        }
    }
}

impl V5ExciterEffectSettings {
    fn validate(&self) -> Result<(), String> {
        finite_range(self.frequency_hz, 200.0, 12_000.0, "v5 exciter frequency")?;
        finite_range(self.drive, 1.0, 12.0, "v5 exciter drive")?;
        finite_range(self.amount, 0.0, 1.0, "v5 exciter amount")
    }

    fn migrate(self) -> ExciterEffectSettings {
        const POINTS: [f32; 10] = [
            600.0, 800.0, 1_100.0, 1_500.0, 2_200.0, 3_000.0, 4_200.0, 5_800.0, 8_000.0, 11_000.0,
        ];
        let emphatic_point = POINTS
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| {
                (*left - self.frequency_hz)
                    .abs()
                    .total_cmp(&(*right - self.frequency_hz).abs())
            })
            .map(|(index, _)| index as u8 + 1)
            .unwrap_or(5);
        ExciterEffectSettings {
            enabled: self.enabled,
            blend: 1.0,
            emphatic_point,
            eq_low_db: 0.0,
            eq_high_db: 0.0,
            mix: self.amount,
        }
    }
}

impl CustomProgramPayload {
    fn validate(&self) -> Result<(), String> {
        CustomProgram {
            id: "validation".into(),
            name: "Validation".into(),
            category: None,
            slot: self.slot,
            gain: self.gain,
            layers: self.layers.clone(),
            effects: self.effects,
        }
        .validate()
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SharedEffects {
    pub exciter: ExciterEffectSettings,
    pub chorus: ChorusEffectSettings,
    pub reverb: ReverbEffectSettings,
}

impl SharedEffects {
    fn validate(&self) -> Result<(), String> {
        self.exciter.validate()?;
        self.chorus.validate()?;
        self.reverb.validate()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExciterEffectSettings {
    pub enabled: bool,
    pub blend: f32,
    pub emphatic_point: u8,
    pub eq_low_db: f32,
    pub eq_high_db: f32,
    pub mix: f32,
}

impl Default for ExciterEffectSettings {
    fn default() -> Self {
        let defaults = rackforge_dsp::ExciterParameters::default();
        Self {
            enabled: defaults.enabled,
            blend: defaults.blend,
            emphatic_point: defaults.emphatic_point,
            eq_low_db: defaults.eq_low_db,
            eq_high_db: defaults.eq_high_db,
            mix: defaults.mix,
        }
    }
}

impl ExciterEffectSettings {
    pub fn parameters(self) -> rackforge_dsp::ExciterParameters {
        rackforge_dsp::ExciterParameters {
            enabled: self.enabled,
            blend: self.blend,
            emphatic_point: self.emphatic_point,
            eq_low_db: self.eq_low_db,
            eq_high_db: self.eq_high_db,
            mix: self.mix,
        }
    }

    fn validate(&self) -> Result<(), String> {
        self.parameters()
            .validate()
            .map(|_| ())
            .map_err(|field| format!("{field} is outside its supported range"))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReverbEffectSettings {
    pub enabled: bool,
    pub size: f32,
    pub decay_seconds: f32,
    pub pre_delay_ms: f32,
    pub damping: f32,
    pub width: f32,
    pub mix: f32,
}

impl Default for ReverbEffectSettings {
    fn default() -> Self {
        let defaults = rackforge_dsp::ReverbParameters::default();
        Self {
            enabled: defaults.enabled,
            size: defaults.size,
            decay_seconds: defaults.decay_seconds,
            pre_delay_ms: defaults.pre_delay_ms,
            damping: defaults.damping,
            width: defaults.width,
            mix: defaults.mix,
        }
    }
}

impl ReverbEffectSettings {
    pub fn parameters(self) -> rackforge_dsp::ReverbParameters {
        rackforge_dsp::ReverbParameters {
            enabled: self.enabled,
            size: self.size,
            decay_seconds: self.decay_seconds,
            pre_delay_ms: self.pre_delay_ms,
            damping: self.damping,
            width: self.width,
            mix: self.mix,
        }
    }

    fn validate(&self) -> Result<(), String> {
        self.parameters()
            .validate()
            .map(|_| ())
            .map_err(|field| format!("{field} is outside its supported range"))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChorusEffectSettings {
    pub enabled: bool,
    pub rate_hz: f32,
    pub depth: f32,
    pub delay_ms: f32,
    pub feedback: f32,
    pub width: f32,
    pub mix: f32,
}

impl Default for ChorusEffectSettings {
    fn default() -> Self {
        let defaults = rackforge_dsp::ChorusParameters::default();
        Self {
            enabled: defaults.enabled,
            rate_hz: defaults.rate_hz,
            depth: defaults.depth,
            delay_ms: defaults.delay_ms,
            feedback: defaults.feedback,
            width: defaults.width,
            mix: defaults.mix,
        }
    }
}

impl ChorusEffectSettings {
    pub fn parameters(self) -> rackforge_dsp::ChorusParameters {
        rackforge_dsp::ChorusParameters {
            enabled: self.enabled,
            rate_hz: self.rate_hz,
            depth: self.depth,
            delay_ms: self.delay_ms,
            feedback: self.feedback,
            width: self.width,
            mix: self.mix,
        }
    }

    fn validate(&self) -> Result<(), String> {
        self.parameters()
            .validate()
            .map(|_| ())
            .map_err(|field| format!("{field} is outside its supported range"))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DlsSource {
    pub resource_id: String,
    pub bank: u32,
    pub program: u32,
}

impl DlsSource {
    fn validate(&self) -> Result<(), String> {
        if self.resource_id != "dls-bank" {
            return Err("CUSTOM source resource_id must be \"dls-bank\"".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramLayer {
    pub id: String,
    pub enabled: bool,
    pub source: DlsSource,
    #[serde(default)]
    pub key_range: MidiRange,
    #[serde(default)]
    pub velocity_range: MidiRange,
    #[serde(default)]
    pub parameters: LayerParameters,
}

impl ProgramLayer {
    pub fn inherited(id: impl Into<String>, source: DlsSource) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            source,
            key_range: MidiRange::default(),
            velocity_range: MidiRange::default(),
            parameters: LayerParameters::default(),
        }
    }

    fn from_legacy(source: DlsSource, parameters: LegacyProgramParameters) -> Self {
        Self {
            id: "a".into(),
            enabled: true,
            source,
            key_range: MidiRange::default(),
            velocity_range: MidiRange::default(),
            parameters: LayerParameters {
                gain: 1.0,
                transpose_semitones: parameters.transpose_semitones,
                fine_tune_cents: parameters.fine_tune_cents,
                pitch_bend_range_semitones: parameters.pitch_bend_range_semitones,
                modulation_depth: parameters.modulation_depth,
                amplitude_envelope: EnvelopeOverrides {
                    attack_seconds: parameters.attack_seconds,
                    decay_seconds: parameters.decay_seconds,
                    sustain_level: parameters.sustain_level,
                    release_seconds: parameters.release_seconds,
                },
                ..LayerParameters::default()
            },
        }
    }

    fn validate(&self) -> Result<(), String> {
        if !matches!(self.id.as_str(), "a" | "b") {
            return Err("RF-DLS layer id must be \"a\" or \"b\"".into());
        }
        self.source.validate()?;
        self.key_range.validate("key", &self.id)?;
        self.velocity_range.validate("velocity", &self.id)?;
        self.parameters.validate(&self.id)
    }

    pub fn accepts(&self, note: u8, velocity: u8) -> bool {
        self.enabled && self.key_range.contains(note) && self.velocity_range.contains(velocity)
    }

    pub fn voice_config(&self, instrument: &Instrument) -> VoiceConfig {
        VoiceConfig {
            amplitude_envelope: self
                .parameters
                .amplitude_envelope
                .resolve(instrument.envelope),
            pitch_envelope: self
                .parameters
                .pitch_envelope
                .resolve(instrument.pitch_envelope),
            lfo: self.parameters.lfo.resolve(instrument.lfo),
            pitch_offset_cents: f32::from(self.parameters.transpose_semitones) * 100.0
                + self.parameters.fine_tune_cents,
            pitch_bend_range_cents: self.parameters.pitch_bend_range_semitones * 100.0,
            modulation_depth: self.parameters.modulation_depth,
            gain: self.parameters.gain,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MidiRange {
    pub low: u8,
    pub high: u8,
}

impl Default for MidiRange {
    fn default() -> Self {
        Self { low: 0, high: 127 }
    }
}

impl MidiRange {
    fn validate(&self, name: &str, layer: &str) -> Result<(), String> {
        if self.low > self.high || self.high > 127 {
            return Err(format!("layer {layer:?} has invalid {name} range"));
        }
        Ok(())
    }

    fn contains(&self, value: u8) -> bool {
        (self.low..=self.high).contains(&value)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayerParameters {
    pub gain: f32,
    pub transpose_semitones: i8,
    pub fine_tune_cents: f32,
    pub pitch_bend_range_semitones: f32,
    pub modulation_depth: f32,
    pub amplitude_envelope: EnvelopeOverrides,
    pub pitch_envelope: PitchEnvelopeOverrides,
    pub lfo: LfoOverrides,
}

impl Default for LayerParameters {
    fn default() -> Self {
        Self {
            gain: 1.0,
            transpose_semitones: 0,
            fine_tune_cents: 0.0,
            pitch_bend_range_semitones: 2.0,
            modulation_depth: 1.0,
            amplitude_envelope: EnvelopeOverrides::default(),
            pitch_envelope: PitchEnvelopeOverrides::default(),
            lfo: LfoOverrides::default(),
        }
    }
}

impl LayerParameters {
    fn validate(&self, layer: &str) -> Result<(), String> {
        finite_range(self.gain, 0.0, 2.0, &format!("layer {layer} gain"))?;
        if !(-48..=48).contains(&self.transpose_semitones) {
            return Err(format!(
                "layer {layer:?} transpose must be between -48 and 48"
            ));
        }
        finite_range(
            self.fine_tune_cents,
            -100.0,
            100.0,
            &format!("layer {layer} fine tune"),
        )?;
        finite_range(
            self.pitch_bend_range_semitones,
            0.0,
            24.0,
            &format!("layer {layer} pitch bend range"),
        )?;
        finite_range(
            self.modulation_depth,
            0.0,
            2.0,
            &format!("layer {layer} modulation depth"),
        )?;
        self.amplitude_envelope.validate("amplitude envelope")?;
        self.pitch_envelope.validate()?;
        self.lfo.validate()
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EnvelopeOverrides {
    pub attack_seconds: Option<f32>,
    pub decay_seconds: Option<f32>,
    pub sustain_level: Option<f32>,
    pub release_seconds: Option<f32>,
}

impl EnvelopeOverrides {
    fn validate(&self, name: &str) -> Result<(), String> {
        optional_range(self.attack_seconds, 0.0, 60.0, &format!("{name} attack"))?;
        optional_range(self.decay_seconds, 0.0, 60.0, &format!("{name} decay"))?;
        optional_range(self.sustain_level, 0.0, 1.0, &format!("{name} sustain"))?;
        optional_range(self.release_seconds, 0.0, 60.0, &format!("{name} release"))
    }

    fn resolve(&self, inherited: EnvelopeSpec) -> EnvelopeSpec {
        EnvelopeSpec {
            attack_seconds: self.attack_seconds.unwrap_or(inherited.attack_seconds),
            decay_seconds: self.decay_seconds.unwrap_or(inherited.decay_seconds),
            sustain_level: self.sustain_level.unwrap_or(inherited.sustain_level),
            release_seconds: self.release_seconds.unwrap_or(inherited.release_seconds),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PitchEnvelopeOverrides {
    pub attack_seconds: Option<f32>,
    pub decay_seconds: Option<f32>,
    pub sustain_level: Option<f32>,
    pub release_seconds: Option<f32>,
    pub depth_cents: Option<f32>,
}

impl PitchEnvelopeOverrides {
    fn validate(&self) -> Result<(), String> {
        EnvelopeOverrides {
            attack_seconds: self.attack_seconds,
            decay_seconds: self.decay_seconds,
            sustain_level: self.sustain_level,
            release_seconds: self.release_seconds,
        }
        .validate("pitch envelope")?;
        optional_range(self.depth_cents, -4_800.0, 4_800.0, "pitch envelope depth")
    }

    fn resolve(&self, inherited: PitchEnvelopeSpec) -> PitchEnvelopeSpec {
        PitchEnvelopeSpec {
            envelope: EnvelopeOverrides {
                attack_seconds: self.attack_seconds,
                decay_seconds: self.decay_seconds,
                sustain_level: self.sustain_level,
                release_seconds: self.release_seconds,
            }
            .resolve(inherited.envelope),
            depth_cents: self.depth_cents.unwrap_or(inherited.depth_cents),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LfoOverrides {
    pub enabled: Option<bool>,
    pub frequency_hz: Option<f32>,
    pub delay_seconds: Option<f32>,
    pub pitch_depth_cents: Option<f32>,
    pub mod_wheel_pitch_depth_cents: Option<f32>,
    pub attenuation_depth_centibels: Option<f32>,
    pub mod_wheel_attenuation_depth_centibels: Option<f32>,
}

impl LfoOverrides {
    fn validate(&self) -> Result<(), String> {
        optional_range(self.frequency_hz, 0.01, 50.0, "LFO frequency")?;
        optional_range(self.delay_seconds, 0.0, 60.0, "LFO delay")?;
        optional_range(self.pitch_depth_cents, -2_400.0, 2_400.0, "LFO pitch depth")?;
        optional_range(
            self.mod_wheel_pitch_depth_cents,
            -2_400.0,
            2_400.0,
            "LFO mod-wheel pitch depth",
        )?;
        optional_range(
            self.attenuation_depth_centibels,
            -10_000.0,
            10_000.0,
            "LFO amplitude depth",
        )?;
        optional_range(
            self.mod_wheel_attenuation_depth_centibels,
            -10_000.0,
            10_000.0,
            "LFO mod-wheel amplitude depth",
        )
    }

    fn resolve(&self, inherited: LfoSpec) -> LfoSpec {
        let mut resolved = LfoSpec {
            frequency_hz: self.frequency_hz.unwrap_or(inherited.frequency_hz),
            delay_seconds: self.delay_seconds.unwrap_or(inherited.delay_seconds),
            pitch_depth_cents: self
                .pitch_depth_cents
                .unwrap_or(inherited.pitch_depth_cents),
            mod_wheel_pitch_depth_cents: self
                .mod_wheel_pitch_depth_cents
                .unwrap_or(inherited.mod_wheel_pitch_depth_cents),
            attenuation_depth_centibels: self
                .attenuation_depth_centibels
                .unwrap_or(inherited.attenuation_depth_centibels),
            mod_wheel_attenuation_depth_centibels: self
                .mod_wheel_attenuation_depth_centibels
                .unwrap_or(inherited.mod_wheel_attenuation_depth_centibels),
        };
        if self.enabled == Some(false) {
            resolved.pitch_depth_cents = 0.0;
            resolved.mod_wheel_pitch_depth_cents = 0.0;
            resolved.attenuation_depth_centibels = 0.0;
            resolved.mod_wheel_attenuation_depth_centibels = 0.0;
        }
        resolved
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct LegacyCustomProgramPayload {
    slot: u16,
    source: DlsSource,
    #[serde(default)]
    parameters: LegacyProgramParameters,
}

impl LegacyCustomProgramPayload {
    fn validate(&self) -> Result<(), String> {
        if self.slot == 0 || self.slot > 999 {
            return Err("CUSTOM slot must be between 1 and 999".into());
        }
        self.source.validate()?;
        self.parameters.validate()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct LegacyProgramParameters {
    gain: f32,
    transpose_semitones: i8,
    fine_tune_cents: f32,
    attack_seconds: Option<f32>,
    decay_seconds: Option<f32>,
    sustain_level: Option<f32>,
    release_seconds: Option<f32>,
    pitch_bend_range_semitones: f32,
    modulation_depth: f32,
}

impl Default for LegacyProgramParameters {
    fn default() -> Self {
        Self {
            gain: 1.0,
            transpose_semitones: 0,
            fine_tune_cents: 0.0,
            attack_seconds: None,
            decay_seconds: None,
            sustain_level: None,
            release_seconds: None,
            pitch_bend_range_semitones: 2.0,
            modulation_depth: 1.0,
        }
    }
}

impl LegacyProgramParameters {
    fn validate(&self) -> Result<(), String> {
        finite_range(self.gain, 0.0, 2.0, "gain")?;
        if !(-48..=48).contains(&self.transpose_semitones) {
            return Err("transpose_semitones must be between -48 and 48".into());
        }
        finite_range(self.fine_tune_cents, -100.0, 100.0, "fine_tune_cents")?;
        optional_range(self.attack_seconds, 0.0, 60.0, "attack_seconds")?;
        optional_range(self.decay_seconds, 0.0, 60.0, "decay_seconds")?;
        optional_range(self.sustain_level, 0.0, 1.0, "sustain_level")?;
        optional_range(self.release_seconds, 0.0, 60.0, "release_seconds")?;
        finite_range(
            self.pitch_bend_range_semitones,
            0.0,
            24.0,
            "pitch_bend_range_semitones",
        )?;
        finite_range(self.modulation_depth, 0.0, 2.0, "modulation_depth")
    }
}

fn default_gain() -> f32 {
    1.0
}

fn finite_range(value: f32, minimum: f32, maximum: f32, name: &str) -> Result<(), String> {
    if !value.is_finite() || !(minimum..=maximum).contains(&value) {
        return Err(format!("{name} must be between {minimum} and {maximum}"));
    }
    Ok(())
}

fn optional_range(
    value: Option<f32>,
    minimum: f32,
    maximum: f32,
    name: &str,
) -> Result<(), String> {
    value.map_or(Ok(()), |value| finite_range(value, minimum, maximum, name))
}

pub fn load_programs(
    data_root: Option<&Path>,
) -> Result<(Vec<CustomProgram>, Vec<String>), String> {
    let Some(data_root) = data_root else {
        return Ok((Vec::new(), Vec::new()));
    };
    let directory = data_root.join("custom");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("creating {}: {error}", directory.display()))?;
    let metadata = fs::symlink_metadata(&directory)
        .map_err(|error| format!("inspecting {}: {error}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("RF-DLS custom storage must be a real directory".into());
    }
    let canonical_root = fs::canonicalize(&directory)
        .map_err(|error| format!("resolving {}: {error}", directory.display()))?;
    let mut paths = fs::read_dir(&directory)
        .map_err(|error| format!("reading {}: {error}", directory.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(PROGRAM_SUFFIX))
        })
        .collect::<Vec<PathBuf>>();
    paths.sort();
    if paths.len() > MAX_PROGRAMS {
        return Err(format!(
            "RF-DLS custom storage contains more than {MAX_PROGRAMS} programs"
        ));
    }

    let mut programs = Vec::new();
    let mut warnings = Vec::new();
    let mut ids = BTreeSet::new();
    let mut slots = BTreeSet::new();
    for path in paths {
        match load_one(&canonical_root, &path) {
            Ok(program) if !ids.insert(program.id.clone()) => warnings.push(format!(
                "ignoring {}: duplicate CUSTOM id {:?}",
                path.display(),
                program.id
            )),
            Ok(program) if !slots.insert(program.slot) => warnings.push(format!(
                "ignoring {}: duplicate CUSTOM slot {}",
                path.display(),
                program.slot
            )),
            Ok(program) => programs.push(program),
            Err(error) => warnings.push(format!("ignoring {}: {error}", path.display())),
        }
    }
    programs.sort_by_key(|program| (program.slot, program.id.clone()));
    Ok((programs, warnings))
}

fn load_one(root: &Path, path: &Path) -> Result<CustomProgram, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("inspecting file: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("CUSTOM program must be a regular non-symlink file".into());
    }
    if metadata.len() > MAX_PROGRAM_BYTES {
        return Err(format!(
            "CUSTOM program exceeds the {MAX_PROGRAM_BYTES}-byte limit"
        ));
    }
    let canonical = fs::canonicalize(path).map_err(|error| format!("resolving file: {error}"))?;
    if !canonical.starts_with(root) {
        return Err("CUSTOM program escapes its private plugin directory".into());
    }
    let bytes = fs::read(&canonical).map_err(|error| format!("reading file: {error}"))?;
    let document: ProgramDocument =
        serde_json::from_slice(&bytes).map_err(|error| format!("parsing JSON: {error}"))?;
    CustomProgram::from_document(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn document() -> ProgramDocument {
        ProgramDocument {
            schema_version: 1,
            id: "user.warm-piano".into(),
            name: "Warm Piano".into(),
            plugin_id: PLUGIN_ID.into(),
            plugin_version: "0.1.0".into(),
            plugin_state_version: 2,
            payload_version: LEGACY_PAYLOAD_VERSION,
            category: Some("Piano".into()),
            tags: vec!["custom".into()],
            payload: json!({
                "slot": 1,
                "source": {
                    "resource_id": "dls-bank",
                    "bank": 0,
                    "program": 0
                },
                "parameters": {
                    "gain": 0.9,
                    "release_seconds": 1.2
                }
            }),
        }
    }

    #[test]
    fn parses_versioned_custom_program() {
        let program = CustomProgram::from_document(document()).unwrap();
        assert_eq!(program.preset_id(), "custom.user.warm-piano");
        assert_eq!(program.slot, 1);
        assert_eq!(program.gain, 0.9);
        assert_eq!(program.layers.len(), 1);
        assert_eq!(program.layers[0].id, "a");
        assert_eq!(program.layers[0].parameters.transpose_semitones, 0);
        assert_eq!(
            program.layers[0]
                .parameters
                .amplitude_envelope
                .release_seconds,
            Some(1.2)
        );
        let migrated = program.to_document().unwrap();
        assert_eq!(migrated.payload_version, PAYLOAD_VERSION);
        assert_eq!(
            CustomProgram::from_document(migrated).unwrap().id,
            "user.warm-piano"
        );
    }

    #[test]
    fn v2_programs_migrate_to_disabled_shared_effects() {
        let legacy = CustomProgram::from_document(document()).unwrap();
        let mut v2 = legacy.to_document().unwrap();
        v2.payload_version = LAYER_PAYLOAD_VERSION;
        v2.payload
            .as_object_mut()
            .unwrap()
            .remove("effects")
            .unwrap();

        let migrated = CustomProgram::from_document(v2).unwrap();
        assert_eq!(migrated.effects, SharedEffects::default());
        assert!(!migrated.effects.chorus.enabled);
        let encoded = migrated.to_document().unwrap();
        assert_eq!(encoded.payload_version, PAYLOAD_VERSION);
        assert!(encoded.payload.get("effects").is_some());
    }

    #[test]
    fn v3_programs_migrate_to_disabled_reverb() {
        let mut v3 = CustomProgram::from_document(document())
            .unwrap()
            .to_document()
            .unwrap();
        v3.payload_version = CHORUS_PAYLOAD_VERSION;
        v3.payload["effects"]
            .as_object_mut()
            .unwrap()
            .remove("reverb")
            .unwrap();

        let migrated = CustomProgram::from_document(v3).unwrap();
        assert!(!migrated.effects.reverb.enabled);
        assert_eq!(migrated.effects.reverb, ReverbEffectSettings::default());
        assert_eq!(
            migrated.to_document().unwrap().payload_version,
            PAYLOAD_VERSION
        );
    }

    #[test]
    fn v4_programs_migrate_to_disabled_exciter() {
        let mut v4 = CustomProgram::from_document(document())
            .unwrap()
            .to_document()
            .unwrap();
        v4.payload_version = REVERB_PAYLOAD_VERSION;
        v4.payload["effects"]
            .as_object_mut()
            .unwrap()
            .remove("exciter")
            .unwrap();

        let migrated = CustomProgram::from_document(v4).unwrap();
        assert!(!migrated.effects.exciter.enabled);
        assert_eq!(migrated.effects.exciter, ExciterEffectSettings::default());
        assert_eq!(
            migrated.to_document().unwrap().payload_version,
            PAYLOAD_VERSION
        );
    }

    #[test]
    fn v5_exciter_migrates_to_the_classic_parameter_model() {
        let mut v5 = CustomProgram::from_document(document())
            .unwrap()
            .to_document()
            .unwrap();
        v5.payload_version = EXCITER_PAYLOAD_VERSION;
        v5.payload["effects"]["exciter"] = json!({
            "enabled": true,
            "frequency_hz": 3500.0,
            "drive": 4.0,
            "amount": 0.2
        });

        let migrated = CustomProgram::from_document(v5).unwrap();
        assert!(migrated.effects.exciter.enabled);
        assert_eq!(migrated.effects.exciter.blend, 1.0);
        assert_eq!(migrated.effects.exciter.emphatic_point, 6);
        assert_eq!(migrated.effects.exciter.eq_low_db, 0.0);
        assert_eq!(migrated.effects.exciter.eq_high_db, 0.0);
        assert_eq!(migrated.effects.exciter.mix, 0.2);
        assert_eq!(
            migrated.to_document().unwrap().payload_version,
            PAYLOAD_VERSION
        );
    }

    #[test]
    fn rejects_invalid_source_and_ranges() {
        let mut invalid_source = document();
        invalid_source.payload["source"]["resource_id"] = json!("something-else");
        assert!(CustomProgram::from_document(invalid_source).is_err());

        let mut invalid_gain = document();
        invalid_gain.payload["parameters"]["gain"] = json!(9.0);
        assert!(CustomProgram::from_document(invalid_gain).is_err());

        let mut invalid_chorus = CustomProgram::from_document(document())
            .unwrap()
            .to_document()
            .unwrap();
        invalid_chorus.payload["effects"]["chorus"]["mix"] = json!(2.0);
        assert!(CustomProgram::from_document(invalid_chorus).is_err());

        let mut invalid_reverb = CustomProgram::from_document(document())
            .unwrap()
            .to_document()
            .unwrap();
        invalid_reverb.payload["effects"]["reverb"]["decay_seconds"] = json!(99.0);
        assert!(CustomProgram::from_document(invalid_reverb).is_err());

        let mut invalid_exciter = CustomProgram::from_document(document())
            .unwrap()
            .to_document()
            .unwrap();
        invalid_exciter.payload["effects"]["exciter"]["emphatic_point"] = json!(11);
        assert!(CustomProgram::from_document(invalid_exciter).is_err());
    }

    #[test]
    fn v2_layers_can_override_every_synthesis_stage() {
        let legacy = CustomProgram::from_document(document()).unwrap();
        let mut program = legacy.clone();
        let mut layer_b = program.layers[0].clone();
        layer_b.id = "b".into();
        layer_b.source.program = 48;
        layer_b.key_range = MidiRange { low: 48, high: 96 };
        layer_b.parameters.pitch_envelope.depth_cents = Some(1_200.0);
        layer_b.parameters.lfo.enabled = Some(true);
        layer_b.parameters.lfo.frequency_hz = Some(6.5);
        layer_b.parameters.lfo.delay_seconds = Some(0.75);
        layer_b.parameters.lfo.mod_wheel_pitch_depth_cents = Some(80.0);
        program.layers.push(layer_b);
        program.effects.chorus.enabled = true;
        program.effects.chorus.mix = 0.4;
        program.effects.reverb.enabled = true;
        program.effects.reverb.decay_seconds = 3.5;
        program.effects.exciter.enabled = true;
        program.effects.exciter.blend = 0.5;
        program.effects.exciter.emphatic_point = 7;
        program.effects.exciter.eq_high_db = 3.0;
        program.effects.exciter.mix = 0.35;

        let document = program.to_document().unwrap();
        assert_eq!(document.payload_version, PAYLOAD_VERSION);
        let decoded = CustomProgram::from_document(document).unwrap();
        assert_eq!(decoded.layers.len(), 2);
        assert_eq!(decoded.layers[1].source.program, 48);
        assert_eq!(decoded.layers[1].parameters.lfo.delay_seconds, Some(0.75));
        assert!(decoded.effects.chorus.enabled);
        assert_eq!(decoded.effects.chorus.mix, 0.4);
        assert!(decoded.effects.reverb.enabled);
        assert_eq!(decoded.effects.reverb.decay_seconds, 3.5);
        assert!(decoded.effects.exciter.enabled);
        assert_eq!(decoded.effects.exciter.blend, 0.5);
        assert_eq!(decoded.effects.exciter.emphatic_point, 7);
        assert_eq!(decoded.effects.exciter.eq_high_db, 3.0);
        assert_eq!(decoded.effects.exciter.mix, 0.35);
    }

    #[test]
    fn layer_a_is_mandatory_while_layer_b_may_be_disabled() {
        let mut program = CustomProgram::from_document(document()).unwrap();
        let mut layer_b = program.layers[0].clone();
        layer_b.id = "b".into();
        layer_b.enabled = false;
        program.layers.push(layer_b);
        assert!(program.validate().is_ok());

        program.layers[0].enabled = false;
        assert!(program.validate().is_err());

        program.layers[0].enabled = true;
        program.layers[0].id = "b".into();
        assert!(program.validate().is_err());
    }
}
