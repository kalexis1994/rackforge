use crate::{
    MANIFEST_SCHEMA_VERSION, RUNTIME_DESCRIPTOR_SCHEMA_VERSION,
    abi::{is_compatible, pack_version},
    parameter::SchemaError,
};
use rackforge_midi_api::{DEFAULT_INPUT_BUS_ID, MidiInputBusId, PluginChannelModel};
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
    /// Optional path inside this plugin's private data directory. Hosts may
    /// resolve it automatically when no explicit resource override was
    /// supplied. The guest receives bytes, never this filesystem path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_path: Option<String>,
    /// Optional immutable resource distributed inside the plugin package.
    /// This is intended for redistributable assets such as open sample banks;
    /// user overrides still take precedence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_path: Option<String>,
    /// Optional list of manifest resources that may be discovered inside this
    /// file. Hosts treat such a resource as an import container: every entry is
    /// authenticated by the plugin against these target ids and persisted in
    /// the target's own private `data_path`. The container itself is never a
    /// substitute for the resources it happens to contain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub import_targets: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApiRequirement {
    pub major: u16,
    pub minor: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PortableAbi {
    #[serde(rename = "wasm-v1")]
    WasmV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortableComponent {
    pub abi: PortableAbi,
    pub path: String,
    pub runtime_descriptor: String,
    pub parameter_schema: String,
    pub preset_catalog: String,
    /// Maximum linear memory requested by this component. The host applies a
    /// conservative default when omitted and rejects requests above its cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_limit_mib: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WebSurfaceKind {
    Play,
    Config,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebSurface {
    pub kind: WebSurfaceKind,
    pub entry: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebUi {
    pub api_version: u16,
    pub surfaces: Vec<WebSurface>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MidiProgramChangePolicy {
    #[default]
    Ignore,
    PluginDefined,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MidiInputBus {
    pub id: MidiInputBusId,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginMidiContract {
    pub channel_model: PluginChannelModel,
    pub input_buses: Vec<MidiInputBus>,
    #[serde(default)]
    pub program_change: MidiProgramChangePolicy,
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
    #[serde(default)]
    pub config_mode: bool,
    #[serde(default)]
    pub web_ui: Option<WebUi>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi: Option<PluginMidiContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<PortableComponent>,
    #[serde(default)]
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
        match (&self.component, self.binaries.is_empty()) {
            (None, true) => return Err(ManifestError::NoRuntime),
            (Some(_), false) => return Err(ManifestError::AmbiguousRuntime),
            _ => {}
        }
        if let Some(component) = &self.component {
            for path in [
                &component.path,
                &component.runtime_descriptor,
                &component.parameter_schema,
                &component.preset_catalog,
            ] {
                if !is_safe_relative_path(path) {
                    return Err(ManifestError::UnsafeComponentPath(path.clone()));
                }
            }
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
            if let Some(path) = &resource.data_path
                && !is_safe_relative_path(path)
            {
                return Err(ManifestError::UnsafeResourcePath {
                    id: resource.id.clone(),
                    path: path.clone(),
                });
            }
            if let Some(path) = &resource.package_path
                && !is_safe_relative_path(path)
            {
                return Err(ManifestError::UnsafePackagedResourcePath {
                    id: resource.id.clone(),
                    path: path.clone(),
                });
            }
        }
        for resource in &self.resources {
            if resource.import_targets.is_empty() {
                continue;
            }
            if resource.kind != ResourceKind::File
                || resource.required
                || resource.data_path.is_some()
                || resource.package_path.is_some()
            {
                return Err(ManifestError::InvalidResourceImporter(resource.id.clone()));
            }
            let mut targets = BTreeSet::new();
            for target_id in &resource.import_targets {
                if target_id == &resource.id || !targets.insert(target_id.as_str()) {
                    return Err(ManifestError::InvalidResourceImportTarget {
                        importer: resource.id.clone(),
                        target: target_id.clone(),
                    });
                }
                let Some(target) = self
                    .resources
                    .iter()
                    .find(|candidate| candidate.id == *target_id)
                else {
                    return Err(ManifestError::InvalidResourceImportTarget {
                        importer: resource.id.clone(),
                        target: target_id.clone(),
                    });
                };
                if target.kind != ResourceKind::File || target.data_path.is_none() {
                    return Err(ManifestError::InvalidResourceImportTarget {
                        importer: resource.id.clone(),
                        target: target_id.clone(),
                    });
                }
            }
        }
        if let Some(component) = &self.component
            && let Some(memory_limit_mib) = component.memory_limit_mib
            && !(64..=512).contains(&memory_limit_mib)
        {
            return Err(ManifestError::InvalidPortableMemoryLimit(memory_limit_mib));
        }
        for (platform, path) in &self.binaries {
            validate_identifier(platform, false)
                .map_err(|_| ManifestError::InvalidPlatform(platform.clone()))?;
            if !is_safe_relative_path(path) {
                return Err(ManifestError::UnsafeBinaryPath(path.clone()));
            }
        }
        if let Some(web_ui) = &self.web_ui {
            if web_ui.api_version != 1 {
                return Err(ManifestError::UnsupportedWebApi(web_ui.api_version));
            }
            if web_ui.surfaces.is_empty() {
                return Err(ManifestError::NoWebSurfaces);
            }
            let mut kinds = BTreeSet::new();
            for surface in &web_ui.surfaces {
                if !kinds.insert(surface.kind) {
                    return Err(ManifestError::DuplicateWebSurface(surface.kind));
                }
                if !is_safe_relative_path(&surface.entry)
                    || Path::new(&surface.entry)
                        .extension()
                        .and_then(|value| value.to_str())
                        != Some("html")
                {
                    return Err(ManifestError::UnsafeWebEntry(surface.entry.clone()));
                }
            }
        }
        if let Some(midi) = &self.midi {
            if !self.capabilities.contains(&Capability::MidiInput) {
                return Err(ManifestError::MidiContractWithoutInputCapability);
            }
            if midi.input_buses.is_empty() {
                return Err(ManifestError::NoMidiInputBuses);
            }
            let mut bus_ids = BTreeSet::new();
            for bus in &midi.input_buses {
                if bus.name.trim().is_empty() {
                    return Err(ManifestError::EmptyMidiInputBusName(bus.id.to_string()));
                }
                if !bus_ids.insert(bus.id.as_str()) {
                    return Err(ManifestError::DuplicateMidiInputBus(bus.id.to_string()));
                }
            }
            if self.kind == PluginKind::Instrument && !bus_ids.contains(DEFAULT_INPUT_BUS_ID) {
                return Err(ManifestError::InstrumentMissingMainMidiBus);
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

    pub fn portable_component(&self) -> Option<&PortableComponent> {
        self.component.as_ref()
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
    #[error("manifest declares neither a portable component nor native binaries")]
    NoRuntime,
    #[error("manifest cannot mix a portable component with native binaries")]
    AmbiguousRuntime,
    #[error("portable component path must be a safe relative path: {0:?}")]
    UnsafeComponentPath(String),
    #[error("invalid resource id {0:?}")]
    InvalidResourceId(String),
    #[error("resource {0} has an empty display name")]
    EmptyResourceName(String),
    #[error("duplicate resource {0}")]
    DuplicateResource(String),
    #[error("resource {id} data path must be a safe relative path: {path:?}")]
    UnsafeResourcePath { id: String, path: String },
    #[error("resource {id} package path must be a safe relative path: {path:?}")]
    UnsafePackagedResourcePath { id: String, path: String },
    #[error("resource {0} is not a valid import container")]
    InvalidResourceImporter(String),
    #[error("resource {importer} has invalid import target {target:?}")]
    InvalidResourceImportTarget { importer: String, target: String },
    #[error("portable memory limit must be between 64 and 512 MiB, found {0}")]
    InvalidPortableMemoryLimit(u32),
    #[error("invalid or duplicate UI layout {0:?}")]
    InvalidUiLayout(String),
    #[error("invalid platform identifier {0:?}")]
    InvalidPlatform(String),
    #[error("binary path must be a safe relative path: {0:?}")]
    UnsafeBinaryPath(String),
    #[error("unsupported RackForge Web Plugin API {0}")]
    UnsupportedWebApi(u16),
    #[error("web UI declares no surfaces")]
    NoWebSurfaces,
    #[error("web surface {0:?} appears more than once")]
    DuplicateWebSurface(WebSurfaceKind),
    #[error("web entry must be a safe relative HTML path: {0:?}")]
    UnsafeWebEntry(String),
    #[error("MIDI contract requires the midi_input capability")]
    MidiContractWithoutInputCapability,
    #[error("MIDI contract declares no input buses")]
    NoMidiInputBuses,
    #[error("MIDI input bus {0} has an empty display name")]
    EmptyMidiInputBusName(String),
    #[error("MIDI input bus {0} appears more than once")]
    DuplicateMidiInputBus(String),
    #[error("instrument MIDI contract must declare the main input bus")]
    InstrumentMissingMainMidiBus,
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
            config_mode: false,
            web_ui: None,
            midi: None,
            component: None,
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

    #[test]
    fn validates_private_resource_data_paths() {
        let mut candidate = manifest();
        candidate.resources.push(ResourceRequirement {
            id: "sample-bank".into(),
            name: "Sample bank".into(),
            kind: ResourceKind::File,
            required: false,
            data_path: Some("roms/bank.bin".into()),
            package_path: None,
            import_targets: Vec::new(),
        });
        assert_eq!(candidate.validate(), Ok(()));
        candidate.resources[0].data_path = Some("../outside.bin".into());
        assert!(matches!(
            candidate.validate(),
            Err(ManifestError::UnsafeResourcePath { .. })
        ));
    }

    #[test]
    fn validates_resource_importers_and_their_persistent_targets() {
        let mut candidate = manifest();
        candidate.resources.push(ResourceRequirement {
            id: "sample-bank".into(),
            name: "Sample bank".into(),
            kind: ResourceKind::File,
            required: false,
            data_path: Some("roms/bank.bin".into()),
            package_path: None,
            import_targets: Vec::new(),
        });
        candidate.resources.push(ResourceRequirement {
            id: "sample-archive".into(),
            name: "Sample archive".into(),
            kind: ResourceKind::File,
            required: false,
            data_path: None,
            package_path: None,
            import_targets: vec!["sample-bank".into()],
        });
        assert_eq!(candidate.validate(), Ok(()));

        candidate.resources[1].import_targets = vec!["missing-bank".into()];
        assert!(matches!(
            candidate.validate(),
            Err(ManifestError::InvalidResourceImportTarget { .. })
        ));
    }

    #[test]
    fn accepts_one_platform_independent_component() {
        let mut candidate = manifest();
        candidate.binaries.clear();
        candidate.component = Some(PortableComponent {
            abi: PortableAbi::WasmV1,
            path: "component.wasm".into(),
            runtime_descriptor: "metadata/runtime.json".into(),
            parameter_schema: "metadata/parameters.json".into(),
            preset_catalog: "metadata/presets.json".into(),
            memory_limit_mib: None,
        });
        assert_eq!(candidate.validate(), Ok(()));
    }

    #[test]
    fn rejects_ambiguous_native_and_portable_payloads() {
        let mut candidate = manifest();
        candidate.component = Some(PortableComponent {
            abi: PortableAbi::WasmV1,
            path: "component.wasm".into(),
            runtime_descriptor: "metadata/runtime.json".into(),
            parameter_schema: "metadata/parameters.json".into(),
            preset_catalog: "metadata/presets.json".into(),
            memory_limit_mib: None,
        });
        assert_eq!(candidate.validate(), Err(ManifestError::AmbiguousRuntime));
    }

    #[test]
    fn validates_static_web_surfaces() {
        let mut candidate = manifest();
        candidate.web_ui = Some(WebUi {
            api_version: 1,
            surfaces: vec![
                WebSurface {
                    kind: WebSurfaceKind::Play,
                    entry: "web/play.html".into(),
                },
                WebSurface {
                    kind: WebSurfaceKind::Config,
                    entry: "web/config.html".into(),
                },
            ],
        });
        assert_eq!(candidate.validate(), Ok(()));
    }

    #[test]
    fn rejects_duplicate_or_escaping_web_surfaces() {
        let mut duplicate = manifest();
        duplicate.web_ui = Some(WebUi {
            api_version: 1,
            surfaces: vec![
                WebSurface {
                    kind: WebSurfaceKind::Play,
                    entry: "web/play.html".into(),
                },
                WebSurface {
                    kind: WebSurfaceKind::Play,
                    entry: "web/other.html".into(),
                },
            ],
        });
        assert!(matches!(
            duplicate.validate(),
            Err(ManifestError::DuplicateWebSurface(WebSurfaceKind::Play))
        ));

        let mut escaping = manifest();
        escaping.web_ui = Some(WebUi {
            api_version: 1,
            surfaces: vec![WebSurface {
                kind: WebSurfaceKind::Config,
                entry: "../config.html".into(),
            }],
        });
        assert!(matches!(
            escaping.validate(),
            Err(ManifestError::UnsafeWebEntry(_))
        ));
    }

    #[test]
    fn validates_an_instrument_midi_contract() {
        let mut candidate = manifest();
        candidate.kind = PluginKind::Instrument;
        candidate.capabilities.push(Capability::MidiInput);
        candidate.midi = Some(PluginMidiContract {
            channel_model: PluginChannelModel::SinglePart,
            input_buses: vec![MidiInputBus {
                id: MidiInputBusId::new(DEFAULT_INPUT_BUS_ID).unwrap(),
                name: "Main".into(),
            }],
            program_change: MidiProgramChangePolicy::Ignore,
        });
        assert_eq!(candidate.validate(), Ok(()));
    }

    #[test]
    fn rejects_ambiguous_or_incomplete_midi_contracts() {
        let mut no_capability = manifest();
        no_capability.midi = Some(PluginMidiContract {
            channel_model: PluginChannelModel::SinglePart,
            input_buses: vec![MidiInputBus {
                id: MidiInputBusId::new(DEFAULT_INPUT_BUS_ID).unwrap(),
                name: "Main".into(),
            }],
            program_change: MidiProgramChangePolicy::Ignore,
        });
        assert_eq!(
            no_capability.validate(),
            Err(ManifestError::MidiContractWithoutInputCapability)
        );

        let mut missing_main = manifest();
        missing_main.kind = PluginKind::Instrument;
        missing_main.capabilities.push(Capability::MidiInput);
        missing_main.midi = Some(PluginMidiContract {
            channel_model: PluginChannelModel::MultiPart,
            input_buses: vec![MidiInputBus {
                id: MidiInputBusId::new("parts").unwrap(),
                name: "Parts".into(),
            }],
            program_change: MidiProgramChangePolicy::PluginDefined,
        });
        assert_eq!(
            missing_main.validate(),
            Err(ManifestError::InstrumentMissingMainMidiBus)
        );
    }
}
