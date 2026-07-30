use crate::{
    MANIFEST_SCHEMA_VERSION, RUNTIME_DESCRIPTOR_SCHEMA_VERSION,
    abi::{is_compatible, pack_version},
    parameter::SchemaError,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Instrument,
    Effect,
    MidiProcessor,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    AudioInput,
    AudioOutput,
    MidiInput,
    MidiOutput,
    Presets,
    State,
    SampleAccurateAutomation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRequirement {
    pub id: String,
    pub name: String,
    pub kind: ResourceKind,
    #[serde(default)]
    pub required: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApiRequirement {
    pub major: u16,
    pub minor: u16,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub api: ApiRequirement,
    pub kind: PluginKind,
    pub state_version: u32,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub resources: Vec<ResourceRequirement>,
    #[serde(default)]
    pub ui_layouts: Vec<String>,
    pub binaries: BTreeMap<String, String>,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedSchema(self.schema_version));
        }
        validate_identifier(&self.id, true)
            .map_err(|_| ManifestError::InvalidPluginId(self.id.clone()))?;
        if self.name.trim().is_empty() {
            return Err(ManifestError::EmptyField("name"));
        }
        if self.vendor.trim().is_empty() {
            return Err(ManifestError::EmptyField("vendor"));
        }
        Version::parse(&self.version)
            .map_err(|_| ManifestError::InvalidVersion(self.version.clone()))?;
        if !is_compatible(pack_version(self.api.major, self.api.minor)) {
            return Err(ManifestError::UnsupportedApi {
                major: self.api.major,
                minor: self.api.minor,
            });
        }
        let unique: BTreeSet<_> = self.capabilities.iter().copied().collect();
        if unique.len() != self.capabilities.len() {
            return Err(ManifestError::DuplicateCapability);
        }
        if self.binaries.is_empty() {
            return Err(ManifestError::NoBinaries);
        }
        let mut layouts = BTreeSet::new();
        for layout in &self.ui_layouts {
            let Some((id, version)) = layout.rsplit_once('@') else {
                return Err(ManifestError::InvalidUiLayout(layout.clone()));
            };
            validate_identifier(id, false)
                .map_err(|_| ManifestError::InvalidUiLayout(layout.clone()))?;
            if version.is_empty()
                || !version.bytes().all(|byte| byte.is_ascii_digit())
                || !layouts.insert(layout.as_str())
            {
                return Err(ManifestError::InvalidUiLayout(layout.clone()));
            }
        }
        let mut resource_ids = BTreeSet::new();
        for resource in &self.resources {
            validate_identifier(&resource.id, false)
                .map_err(|_| ManifestError::InvalidResourceId(resource.id.clone()))?;
            if resource.name.trim().is_empty() {
                return Err(ManifestError::EmptyResourceName(resource.id.clone()));
            }
            if !resource_ids.insert(resource.id.as_str()) {
                return Err(ManifestError::DuplicateResource(resource.id.clone()));
            }
        }
        for (platform, path) in &self.binaries {
            validate_identifier(platform, false)
                .map_err(|_| ManifestError::InvalidPlatform(platform.clone()))?;
            if !is_safe_relative_path(path) {
                return Err(ManifestError::UnsafeBinaryPath(path.clone()));
            }
        }
        Ok(())
    }

    pub fn binary_for(&self, platform: &str) -> Result<&str, ManifestError> {
        self.binaries
            .get(platform)
            .map(String::as_str)
            .ok_or_else(|| ManifestError::MissingPlatform(platform.to_owned()))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeDescriptor {
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    pub state_version: u32,
}

impl RuntimeDescriptor {
    pub fn validate_against(&self, manifest: &PluginManifest) -> Result<(), ManifestError> {
        if self.schema_version != RUNTIME_DESCRIPTOR_SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedRuntimeSchema(self.schema_version));
        }
        if self.id != manifest.id {
            return Err(ManifestError::RuntimeMismatch("id"));
        }
        if self.version != manifest.version {
            return Err(ManifestError::RuntimeMismatch("version"));
        }
        if self.state_version != manifest.state_version {
            return Err(ManifestError::RuntimeMismatch("state_version"));
        }
        Ok(())
    }
}

fn validate_identifier(value: &str, require_dot: bool) -> Result<(), SchemaError> {
    if value.is_empty()
        || (require_dot && !value.contains('.'))
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        })
    {
        return Err(SchemaError::InvalidIdentifier(value.to_owned()));
    }
    Ok(())
}

fn is_safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ManifestError {
    #[error("unsupported manifest schema {0}")]
    UnsupportedSchema(u32),
    #[error("unsupported runtime descriptor schema {0}")]
    UnsupportedRuntimeSchema(u32),
    #[error("invalid plugin id {0:?}")]
    InvalidPluginId(String),
    #[error("{0} must not be empty")]
    EmptyField(&'static str),
    #[error("invalid semantic version {0:?}")]
    InvalidVersion(String),
    #[error("unsupported RackForge API {major}.{minor}")]
    UnsupportedApi { major: u16, minor: u16 },
    #[error("capability appears more than once")]
    DuplicateCapability,
    #[error("manifest declares no binaries")]
    NoBinaries,
    #[error("invalid resource id {0:?}")]
    InvalidResourceId(String),
    #[error("resource {0} has an empty display name")]
    EmptyResourceName(String),
    #[error("duplicate resource {0}")]
    DuplicateResource(String),
    #[error("invalid or duplicate UI layout {0:?}")]
    InvalidUiLayout(String),
    #[error("invalid platform identifier {0:?}")]
    InvalidPlatform(String),
    #[error("binary path must be a safe relative path: {0:?}")]
    UnsafeBinaryPath(String),
    #[error("plugin has no binary for platform {0}")]
    MissingPlatform(String),
    #[error("runtime descriptor does not match manifest field {0}")]
    RuntimeMismatch(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> PluginManifest {
        PluginManifest {
            schema_version: 1,
            id: "org.rackforge.gain".into(),
            name: "Gain".into(),
            vendor: "RackForge".into(),
            version: "0.1.0".into(),
            api: ApiRequirement { major: 1, minor: 0 },
            kind: PluginKind::Effect,
            state_version: 1,
            capabilities: vec![Capability::AudioInput, Capability::AudioOutput],
            resources: Vec::new(),
            ui_layouts: Vec::new(),
            binaries: BTreeMap::from([("linux-aarch64".into(), "lib/librackforge_gain.so".into())]),
        }
    }

    #[test]
    fn validates_a_well_formed_manifest() {
        assert_eq!(manifest().validate(), Ok(()));
    }

    #[test]
    fn rejects_escaping_binary_paths() {
        let mut candidate = manifest();
        candidate
            .binaries
            .insert("linux-aarch64".into(), "../escape.so".into());
        assert!(matches!(
            candidate.validate(),
            Err(ManifestError::UnsafeBinaryPath(_))
        ));
    }
}
