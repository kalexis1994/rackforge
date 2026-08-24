use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Version of the RackForge semantic control vocabulary.
///
/// Controller packages and plugin schemas declare this version independently
/// of their package format so the vocabulary can evolve without coupling their
/// release cycles.
pub const CONTROL_PROFILE_SCHEMA_VERSION: u32 = 1;

/// Stable, transport-independent meaning of a physical or plugin control.
///
/// This is intentionally an extensible identifier rather than a closed Rust
/// enum. Official roles live in [`roles`], while third parties can safely use a
/// namespaced identifier such as `vendor.example.super_filter.color`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SemanticControlId(String);

impl SemanticControlId {
    pub fn new(value: impl Into<String>) -> Result<Self, ControlProfileError> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }

    pub fn validate(&self) -> Result<(), ControlProfileError> {
        validate_identifier(&self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SemanticControlId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// RackForge Control Profile v1 roles.
///
/// String constants keep manifests and generated SDKs language-neutral. New
/// roles may be appended, but an existing role never changes meaning.
pub mod roles {
    /// RackForge-owned output level. Unlike `plugin.output.level`, this affects
    /// the complete host mix and is available even when no plugin is active.
    pub const RACKFORGE_MASTER_LEVEL: &str = "rackforge.master.level";
    /// RackForge-owned output balance for the complete host mix.
    pub const RACKFORGE_MASTER_PAN: &str = "rackforge.master.pan";
    pub const SYNTH_FILTER_CUTOFF: &str = "synth.filter.cutoff";
    pub const SYNTH_FILTER_RESONANCE: &str = "synth.filter.resonance";
    pub const SYNTH_FILTER_ENVELOPE_AMOUNT: &str = "synth.filter.envelope.amount";
    pub const SYNTH_FILTER_LFO_AMOUNT: &str = "synth.filter.lfo.amount";
    pub const SYNTH_FILTER_KEY_TRACKING: &str = "synth.filter.key_tracking";
    pub const SYNTH_AMP_ENVELOPE_ATTACK: &str = "synth.envelope.amp.attack";
    pub const SYNTH_AMP_ENVELOPE_DECAY: &str = "synth.envelope.amp.decay";
    pub const SYNTH_AMP_ENVELOPE_SUSTAIN: &str = "synth.envelope.amp.sustain";
    pub const SYNTH_AMP_ENVELOPE_RELEASE: &str = "synth.envelope.amp.release";
    pub const SYNTH_LFO_RATE: &str = "synth.lfo.rate";
    pub const SYNTH_LFO_DEPTH: &str = "synth.lfo.depth";
    pub const SYNTH_LFO_DELAY: &str = "synth.lfo.delay";
    pub const SYNTH_OSCILLATOR_PULSE_WIDTH: &str = "synth.oscillator.pulse_width";
    pub const SYNTH_OSCILLATOR_SUB_LEVEL: &str = "synth.oscillator.sub.level";
    pub const SYNTH_OSCILLATOR_NOISE_LEVEL: &str = "synth.oscillator.noise.level";
    pub const SYNTH_AMPLIFIER_LEVEL: &str = "synth.amplifier.level";
    pub const PLUGIN_OUTPUT_LEVEL: &str = "plugin.output.level";
    pub const MIXER_CHANNEL_LEVEL: &str = "mixer.channel.level";
    pub const MIXER_CHANNEL_PAN: &str = "mixer.channel.pan";
    pub const PERFORMANCE_MODULATION: &str = "performance.modulation";
    pub const PERFORMANCE_EXPRESSION: &str = "performance.expression";
    pub const PERFORMANCE_SUSTAIN: &str = "performance.sustain";

    pub const V1: &[&str] = &[
        RACKFORGE_MASTER_LEVEL,
        RACKFORGE_MASTER_PAN,
        SYNTH_FILTER_CUTOFF,
        SYNTH_FILTER_RESONANCE,
        SYNTH_FILTER_ENVELOPE_AMOUNT,
        SYNTH_FILTER_LFO_AMOUNT,
        SYNTH_FILTER_KEY_TRACKING,
        SYNTH_AMP_ENVELOPE_ATTACK,
        SYNTH_AMP_ENVELOPE_DECAY,
        SYNTH_AMP_ENVELOPE_SUSTAIN,
        SYNTH_AMP_ENVELOPE_RELEASE,
        SYNTH_LFO_RATE,
        SYNTH_LFO_DEPTH,
        SYNTH_LFO_DELAY,
        SYNTH_OSCILLATOR_PULSE_WIDTH,
        SYNTH_OSCILLATOR_SUB_LEVEL,
        SYNTH_OSCILLATOR_NOISE_LEVEL,
        SYNTH_AMPLIFIER_LEVEL,
        PLUGIN_OUTPUT_LEVEL,
        MIXER_CHANNEL_LEVEL,
        MIXER_CHANNEL_PAN,
        PERFORMANCE_MODULATION,
        PERFORMANCE_EXPRESSION,
        PERFORMANCE_SUSTAIN,
    ];
}

pub fn is_official_v1(role: &SemanticControlId) -> bool {
    roles::V1.contains(&role.as_str())
}

fn validate_identifier(value: &str) -> Result<(), ControlProfileError> {
    if value.is_empty()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        })
    {
        return Err(ControlProfileError::InvalidRole(value.to_owned()));
    }
    Ok(())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ControlProfileError {
    #[error("invalid semantic control role {0:?}")]
    InvalidRole(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_roles_are_stable_valid_identifiers() {
        for role in roles::V1 {
            let role = SemanticControlId::new(*role).unwrap();
            assert!(is_official_v1(&role));
        }
    }

    #[test]
    fn vendor_extensions_are_namespaced_but_malformed_roles_are_rejected() {
        assert!(SemanticControlId::new("vendor.example.filter.color").is_ok());
        for invalid in ["", ".cutoff", "cutoff.", "filter..cutoff", "Filter Cutoff"] {
            assert!(SemanticControlId::new(invalid).is_err());
        }
    }
}
