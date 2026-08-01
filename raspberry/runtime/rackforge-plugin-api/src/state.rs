use crate::program::validate_plugin_identifier;
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PLUGIN_STATE_REFERENCE_SCHEMA_VERSION: u32 = 1;
pub const HOST_PRESET_SCHEMA_VERSION: u32 = 1;
pub const MAX_HOST_PRESET_NAME_BYTES: usize = 96;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginStateReference {
    pub schema_version: u32,
    pub plugin_id: String,
    pub plugin_version: String,
    pub state_version: u32,
    pub blob_sha256: String,
    pub byte_length: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_sound_id: Option<String>,
}

impl PluginStateReference {
    pub fn validate(&self) -> Result<(), PluginStateError> {
        if self.schema_version != PLUGIN_STATE_REFERENCE_SCHEMA_VERSION {
            return Err(PluginStateError::UnsupportedReferenceSchema(
                self.schema_version,
            ));
        }
        validate_plugin_identifier(&self.plugin_id)
            .map_err(|_| PluginStateError::InvalidPluginId(self.plugin_id.clone()))?;
        Version::parse(&self.plugin_version)
            .map_err(|_| PluginStateError::InvalidPluginVersion(self.plugin_version.clone()))?;
        if self.state_version == 0 {
            return Err(PluginStateError::InvalidStateVersion);
        }
        if self.blob_sha256.len() != 64
            || !self
                .blob_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(PluginStateError::InvalidBlobHash);
        }
        if self.byte_length == 0 {
            return Err(PluginStateError::EmptyState);
        }
        if self
            .selected_sound_id
            .as_deref()
            .is_some_and(|value| value.is_empty() || value.len() > 256 || value.contains('\0'))
        {
            return Err(PluginStateError::InvalidSoundHint);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPreset {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub plugin_id: String,
    pub created_unix_ms: u64,
    pub updated_unix_ms: u64,
    pub state: PluginStateReference,
}

impl HostPreset {
    pub fn validate(&self) -> Result<(), PluginStateError> {
        if self.schema_version != HOST_PRESET_SCHEMA_VERSION {
            return Err(PluginStateError::UnsupportedPresetSchema(
                self.schema_version,
            ));
        }
        validate_preset_id(&self.id)?;
        validate_preset_name(&self.name)?;
        validate_plugin_identifier(&self.plugin_id)
            .map_err(|_| PluginStateError::InvalidPluginId(self.plugin_id.clone()))?;
        self.state.validate()?;
        if self.state.plugin_id != self.plugin_id {
            return Err(PluginStateError::PluginMismatch);
        }
        if self.updated_unix_ms < self.created_unix_ms {
            return Err(PluginStateError::InvalidTimestamp);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPresetSummary {
    pub id: String,
    pub name: String,
    pub plugin_id: String,
    pub plugin_version: String,
    pub state_version: u32,
    pub updated_unix_ms: u64,
}

impl From<&HostPreset> for HostPresetSummary {
    fn from(value: &HostPreset) -> Self {
        Self {
            id: value.id.clone(),
            name: value.name.clone(),
            plugin_id: value.plugin_id.clone(),
            plugin_version: value.state.plugin_version.clone(),
            state_version: value.state.state_version,
            updated_unix_ms: value.updated_unix_ms,
        }
    }
}

pub fn validate_preset_name(value: &str) -> Result<(), PluginStateError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_HOST_PRESET_NAME_BYTES || trimmed.contains('\0') {
        return Err(PluginStateError::InvalidPresetName);
    }
    Ok(())
}

pub fn validate_preset_id(value: &str) -> Result<(), PluginStateError> {
    if value.is_empty()
        || value.len() > 128
        || value.starts_with('.')
        || value.ends_with('.')
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
    {
        return Err(PluginStateError::InvalidPresetId);
    }
    Ok(())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PluginStateError {
    #[error("unsupported plugin-state reference schema {0}")]
    UnsupportedReferenceSchema(u32),
    #[error("unsupported RackForge preset schema {0}")]
    UnsupportedPresetSchema(u32),
    #[error("invalid plugin id {0:?}")]
    InvalidPluginId(String),
    #[error("invalid plugin version {0:?}")]
    InvalidPluginVersion(String),
    #[error("plugin state version must be greater than zero")]
    InvalidStateVersion,
    #[error("plugin state SHA-256 is invalid")]
    InvalidBlobHash,
    #[error("plugin state is empty")]
    EmptyState,
    #[error("plugin state sound hint is invalid")]
    InvalidSoundHint,
    #[error("RackForge preset id is invalid")]
    InvalidPresetId,
    #[error("RackForge preset name is empty, too long or contains NUL")]
    InvalidPresetName,
    #[error("RackForge preset and state belong to different plugins")]
    PluginMismatch,
    #[error("RackForge preset timestamps are invalid")]
    InvalidTimestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> PluginStateReference {
        PluginStateReference {
            schema_version: PLUGIN_STATE_REFERENCE_SCHEMA_VERSION,
            plugin_id: "org.rackforge.rf-dls".into(),
            plugin_version: "0.1.0".into(),
            state_version: 3,
            blob_sha256: "a".repeat(64),
            byte_length: 42,
            selected_sound_id: Some("dls.b00000000.p00000000".into()),
        }
    }

    #[test]
    fn validates_versioned_opaque_state_reference() {
        assert_eq!(state().validate(), Ok(()));
    }

    #[test]
    fn preset_must_match_its_plugin() {
        let preset = HostPreset {
            schema_version: HOST_PRESET_SCHEMA_VERSION,
            id: "warm-piano".into(),
            name: "Warm Piano".into(),
            plugin_id: "org.rackforge.other".into(),
            created_unix_ms: 1,
            updated_unix_ms: 1,
            state: state(),
        };
        assert_eq!(preset.validate(), Err(PluginStateError::PluginMismatch));
    }
}
