use crate::PROGRAM_SCHEMA_VERSION;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramDocument {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub plugin_id: String,
    pub plugin_version: String,
    pub plugin_state_version: u32,
    pub payload_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub payload: Value,
}

impl ProgramDocument {
    pub fn validate(&self) -> Result<(), ProgramError> {
        if self.schema_version != PROGRAM_SCHEMA_VERSION {
            return Err(ProgramError::UnsupportedSchema(self.schema_version));
        }
        validate_program_identifier(&self.id)?;
        validate_plugin_identifier(&self.plugin_id)?;
        if self.name.trim().is_empty() || self.name.contains('\0') {
            return Err(ProgramError::InvalidName);
        }
        if Version::parse(&self.plugin_version).is_err() {
            return Err(ProgramError::InvalidPluginVersion(
                self.plugin_version.clone(),
            ));
        }
        if self.plugin_state_version == 0 {
            return Err(ProgramError::InvalidPluginStateVersion);
        }
        if self.payload_version == 0 {
            return Err(ProgramError::InvalidPayloadVersion);
        }
        if self
            .category
            .as_ref()
            .is_some_and(|category| category.trim().is_empty() || category.contains('\0'))
        {
            return Err(ProgramError::InvalidCategory);
        }
        let mut tags = BTreeSet::new();
        for tag in &self.tags {
            let normalized = tag.trim();
            if normalized.is_empty()
                || normalized.contains('\0')
                || !tags.insert(normalized.to_lowercase())
            {
                return Err(ProgramError::InvalidTag(tag.clone()));
            }
        }
        if !self.payload.is_object() {
            return Err(ProgramError::PayloadMustBeObject);
        }
        Ok(())
    }
}

pub fn validate_program_identifier(value: &str) -> Result<(), ProgramError> {
    validate_identifier(value, false)
        .map_err(|_| ProgramError::InvalidProgramIdentifier(value.to_owned()))
}

pub fn validate_plugin_identifier(value: &str) -> Result<(), ProgramError> {
    validate_identifier(value, true)
        .map_err(|_| ProgramError::InvalidPluginIdentifier(value.to_owned()))
}

fn validate_identifier(value: &str, require_dot: bool) -> Result<(), ()> {
    if value.is_empty()
        || (require_dot && !value.contains('.'))
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        })
    {
        return Err(());
    }
    Ok(())
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ProgramError {
    #[error("unsupported program schema {0}")]
    UnsupportedSchema(u32),
    #[error("invalid program identifier {0:?}")]
    InvalidProgramIdentifier(String),
    #[error("invalid plugin identifier {0:?}")]
    InvalidPluginIdentifier(String),
    #[error("program name is empty or contains NUL")]
    InvalidName,
    #[error("invalid plugin version {0:?}")]
    InvalidPluginVersion(String),
    #[error("plugin state version must be positive")]
    InvalidPluginStateVersion,
    #[error("payload version must be positive")]
    InvalidPayloadVersion,
    #[error("program category is empty or contains NUL")]
    InvalidCategory,
    #[error("invalid or duplicate program tag {0:?}")]
    InvalidTag(String),
    #[error("program payload must be a JSON object")]
    PayloadMustBeObject,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_program() -> ProgramDocument {
        ProgramDocument {
            schema_version: 1,
            id: "user.piano-strings".into(),
            name: "Piano + Strings".into(),
            plugin_id: "org.rackforge.roland-scva".into(),
            plugin_version: "0.1.0".into(),
            plugin_state_version: 2,
            payload_version: 1,
            category: Some("Layered".into()),
            tags: vec!["Piano".into(), "Strings".into()],
            payload: json!({"layers": []}),
        }
    }

    #[test]
    fn validates_generic_program_envelope() {
        assert_eq!(valid_program().validate(), Ok(()));
    }

    #[test]
    fn rejects_paths_and_duplicate_case_insensitive_tags() {
        let mut program = valid_program();
        program.id = "../escape".into();
        assert!(matches!(
            program.validate(),
            Err(ProgramError::InvalidProgramIdentifier(_))
        ));

        let mut program = valid_program();
        program.tags = vec!["Piano".into(), "piano".into()];
        assert!(matches!(
            program.validate(),
            Err(ProgramError::InvalidTag(_))
        ));
    }
}
