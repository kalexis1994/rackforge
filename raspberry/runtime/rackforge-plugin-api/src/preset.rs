use crate::PRESET_CATALOG_SCHEMA_VERSION;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PresetCatalog {
    pub schema_version: u32,
    #[serde(default)]
    pub banks: Vec<BankDescriptor>,
    #[serde(default)]
    pub presets: Vec<PresetDescriptor>,
}

impl PresetCatalog {
    pub fn validate(&self) -> Result<(), PresetError> {
        if self.schema_version != PRESET_CATALOG_SCHEMA_VERSION {
            return Err(PresetError::UnsupportedSchema(self.schema_version));
        }
        let mut bank_ids = BTreeSet::new();
        for bank in &self.banks {
            validate_identifier(&bank.id)?;
            if bank.name.trim().is_empty() {
                return Err(PresetError::EmptyName(bank.id.clone()));
            }
            if !bank_ids.insert(bank.id.as_str()) {
                return Err(PresetError::DuplicateBank(bank.id.clone()));
            }
        }

        let mut preset_ids = BTreeSet::new();
        for preset in &self.presets {
            validate_identifier(&preset.id)?;
            if preset.name.trim().is_empty() {
                return Err(PresetError::EmptyName(preset.id.clone()));
            }
            if !preset_ids.insert(preset.id.as_str()) {
                return Err(PresetError::DuplicatePreset(preset.id.clone()));
            }
            if let Some(bank) = &preset.bank
                && !bank_ids.contains(bank.as_str())
            {
                return Err(PresetError::UnknownBank {
                    preset: preset.id.clone(),
                    bank: bank.clone(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BankDescriptor {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub order: i32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PresetDescriptor {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub bank: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub order: i32,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn validate_identifier(value: &str) -> Result<(), PresetError> {
    if value.is_empty()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        })
    {
        return Err(PresetError::InvalidIdentifier(value.to_owned()));
    }
    Ok(())
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum PresetError {
    #[error("unsupported preset catalog schema {0}")]
    UnsupportedSchema(u32),
    #[error("invalid identifier {0:?}")]
    InvalidIdentifier(String),
    #[error("empty display name for {0}")]
    EmptyName(String),
    #[error("duplicate bank {0}")]
    DuplicateBank(String),
    #[error("duplicate preset {0}")]
    DuplicatePreset(String),
    #[error("preset {preset} references unknown bank {bank}")]
    UnknownBank { preset: String, bank: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_banked_presets() {
        let catalog = PresetCatalog {
            schema_version: 1,
            banks: vec![BankDescriptor {
                id: "factory".into(),
                name: "Factory".into(),
                order: 0,
            }],
            presets: vec![PresetDescriptor {
                id: "factory.piano-1".into(),
                name: "Piano 1".into(),
                description: None,
                bank: Some("factory".into()),
                category: Some("Piano".into()),
                order: 0,
                tags: vec!["acoustic".into()],
            }],
        };
        assert_eq!(catalog.validate(), Ok(()));
    }
}
