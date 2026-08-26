use rackforge_controller_api::{
    ControllerProfile, HostActionBinding, HostControlBinding, SemanticControlProfile,
    SurfaceImplementation,
};
use semver::{BuildMetadata, Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

pub mod supervise;

pub const CONTROLLER_PACKAGE_SCHEMA_VERSION: u32 = 1;
pub const CONTROLLER_DRIVER_API_VERSION: &str = "1.1.0";
pub const CONTROLLER_MANIFEST_FILE: &str = "rackforge-controller.toml";
pub const INSTALL_RECORD_SCHEMA_VERSION: u32 = 1;
pub const PROCESS_DRIVER_PROTOCOL_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_PACKAGE_FILES: usize = 512;
const MAX_PACKAGE_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DriverRuntimeKind {
    ProcessV1,
    WasmV1,
    /// A manifest-only controller interpreted by RackForge itself. It cannot
    /// send SysEx or own a display, and therefore runs unchanged on every
    /// host without loading community code.
    DeclarativeV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverRuntime {
    pub kind: DriverRuntimeKind,
    #[serde(default)]
    pub entrypoints: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointRole {
    SurfaceInput,
    DisplayOutput,
    PerformanceInput,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointMatcher {
    pub role: EndpointRole,
    #[serde(default)]
    pub name_contains: Vec<String>,
    #[serde(default)]
    pub name_contains_any: Vec<String>,
    #[serde(default)]
    pub name_ends_with: Option<String>,
    #[serde(default)]
    pub exclude_contains: Vec<String>,
}

impl EndpointMatcher {
    pub fn matches(&self, endpoint_name: &str) -> bool {
        let folded = endpoint_name.trim().to_ascii_lowercase();
        self.name_contains
            .iter()
            .all(|part| folded.contains(&part.to_ascii_lowercase()))
            && (self.name_contains_any.is_empty()
                || self
                    .name_contains_any
                    .iter()
                    .any(|part| folded.contains(&part.to_ascii_lowercase())))
            && self
                .name_ends_with
                .as_ref()
                .is_none_or(|suffix| folded.ends_with(&suffix.to_ascii_lowercase()))
            && self
                .exclude_contains
                .iter()
                .all(|part| !folded.contains(&part.to_ascii_lowercase()))
    }

    fn validate(&self) -> Result<(), PackageError> {
        if self.name_contains.is_empty()
            && self.name_contains_any.is_empty()
            && self.name_ends_with.is_none()
        {
            return Err(PackageError::InvalidManifest(
                "endpoint matcher needs a positive name condition".into(),
            ));
        }
        for value in self
            .name_contains
            .iter()
            .chain(self.name_contains_any.iter())
            .chain(self.exclude_contains.iter())
            .chain(self.name_ends_with.iter())
        {
            if value.trim().is_empty() || value.contains('\0') {
                return Err(PackageError::InvalidManifest(
                    "endpoint matcher contains an empty or NUL pattern".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceMatcher {
    pub id: String,
    #[serde(default)]
    pub usb_vendor_id: u16,
    #[serde(default)]
    pub usb_product_ids: Vec<u16>,
    #[serde(default)]
    pub product_names: Vec<String>,
    pub endpoints: Vec<EndpointMatcher>,
}

impl DeviceMatcher {
    fn validate(&self, runtime: DriverRuntimeKind) -> Result<(), PackageError> {
        validate_identifier(&self.id)?;
        let has_usb_identity = self.usb_vendor_id != 0 || !self.usb_product_ids.is_empty();
        if has_usb_identity
            && (self.usb_vendor_id == 0
                || self.usb_product_ids.is_empty()
                || self.usb_product_ids.contains(&0))
        {
            return Err(PackageError::InvalidManifest(
                "device matcher USB identity requires a non-zero VID and at least one non-zero PID"
                    .into(),
            ));
        }
        if runtime != DriverRuntimeKind::DeclarativeV1 && !has_usb_identity {
            return Err(PackageError::InvalidManifest(
                "executable controller device matchers require USB VID and PID values".into(),
            ));
        }
        if self
            .product_names
            .iter()
            .any(|name| name.trim().is_empty() || name.contains('\0'))
        {
            return Err(PackageError::InvalidManifest(
                "device matcher contains an empty or NUL product name".into(),
            ));
        }
        if self.endpoints.is_empty() {
            return Err(PackageError::InvalidManifest(
                "device matcher requires at least one endpoint".into(),
            ));
        }
        for endpoint in &self.endpoints {
            endpoint.validate()?;
        }
        let roles = self
            .endpoints
            .iter()
            .map(|endpoint| endpoint.role)
            .collect::<BTreeSet<_>>();
        if runtime == DriverRuntimeKind::DeclarativeV1 {
            if roles.contains(&EndpointRole::DisplayOutput)
                || !(roles.contains(&EndpointRole::SurfaceInput)
                    || roles.contains(&EndpointRole::PerformanceInput))
            {
                return Err(PackageError::InvalidManifest(
                    "declarative controllers require an input endpoint and cannot declare display outputs"
                        .into(),
                ));
            }
        } else if !roles.contains(&EndpointRole::SurfaceInput)
            || !roles.contains(&EndpointRole::DisplayOutput)
        {
            return Err(PackageError::InvalidManifest(
                "display controllers require surface_input and display_output endpoints".into(),
            ));
        }
        Ok(())
    }

    fn matches_input_endpoint(&self, endpoint_name: &str) -> bool {
        self.endpoints.iter().any(|endpoint| {
            matches!(
                endpoint.role,
                EndpointRole::SurfaceInput | EndpointRole::PerformanceInput
            ) && endpoint.matches(endpoint_name)
        })
    }
}

impl Ord for EndpointRole {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

impl PartialOrd for EndpointRole {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerPermissions {
    pub midi_input: bool,
    pub midi_output: bool,
    pub sysex: bool,
    pub usb_metadata: bool,
    pub raw_usb: bool,
    pub firmware_write: bool,
    pub filesystem: bool,
    pub network: bool,
}

impl ControllerPermissions {
    fn validate(self, runtime: DriverRuntimeKind) -> Result<(), PackageError> {
        if !self.midi_input {
            return Err(PackageError::InvalidManifest(
                "controller packages require MIDI input".into(),
            ));
        }
        if self.raw_usb || self.firmware_write || self.filesystem || self.network {
            return Err(PackageError::InvalidManifest(
                "controller package v1 forbids raw USB, firmware, filesystem and network access"
                    .into(),
            ));
        }
        if runtime == DriverRuntimeKind::DeclarativeV1
            && (self.midi_output || self.sysex || self.usb_metadata)
        {
            return Err(PackageError::InvalidManifest(
                "declarative-v1 is input-only and cannot request MIDI output, SysEx or USB metadata"
                    .into(),
            ));
        }
        if runtime != DriverRuntimeKind::DeclarativeV1 && !self.midi_output {
            return Err(PackageError::InvalidManifest(
                "executable controller packages require MIDI output".into(),
            ));
        }
        Ok(())
    }
}

/// An enabled declarative controller resolved against one physical MIDI
/// input. Hosts use the physical source id for routing; the package's
/// `semantic_profile.source_id` remains the stable vocabulary identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarativeControllerBinding {
    pub controller_id: String,
    pub controller_name: String,
    pub endpoint_name: String,
    pub host_controls: Vec<HostControlBinding>,
    pub host_actions: Vec<HostActionBinding>,
    pub semantic_profile: Option<SemanticControlProfile>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclarativeMidiDevice<'a> {
    pub endpoint_name: &'a str,
    pub product_name: Option<&'a str>,
    pub usb_vendor_id: Option<u16>,
    pub usb_product_id: Option<u16>,
}

impl DeviceMatcher {
    fn matches_declarative_device(&self, device: DeclarativeMidiDevice<'_>) -> bool {
        if !self.matches_input_endpoint(device.endpoint_name) {
            return false;
        }
        if let (Some(expected), Some(actual)) = (device.usb_vendor_id, Some(self.usb_vendor_id))
            && actual != 0
            && expected != actual
        {
            return false;
        }
        if let Some(product_id) = device.usb_product_id
            && !self.usb_product_ids.is_empty()
            && !self.usb_product_ids.contains(&product_id)
        {
            return false;
        }
        if let Some(product_name) = device.product_name
            && !self.product_names.is_empty()
            && !self
                .product_names
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(product_name.trim()))
        {
            return false;
        }
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactIntegrity {
    pub sha256: BTreeMap<String, String>,
}

/// One user-facing setting a controller package exposes. The host renders
/// these generically (the panel is derived from the schema, never
/// hardcoded), persists the values in the store, and hands them to the
/// driver, which alone decides what a setting MEANS on its hardware.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerSetting {
    pub id: String,
    pub name: String,
    pub kind: ControllerSettingKind,
    pub default: String,
    #[serde(default)]
    pub page: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControllerSettingKind {
    /// An sRGB color, serialized as `#rrggbb`.
    Color,
}

impl ControllerSetting {
    pub fn validate(&self) -> Result<(), PackageError> {
        if self.id.is_empty()
            || !self
                .id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(PackageError::InvalidManifest(format!(
                "invalid setting id {:?}",
                self.id
            )));
        }
        self.validate_value(&self.default).map_err(|error| {
            PackageError::InvalidManifest(format!("setting {:?}: {error}", self.id))
        })
    }

    /// Whether `value` is admissible for this setting's kind.
    pub fn validate_value(&self, value: &str) -> Result<(), String> {
        match self.kind {
            ControllerSettingKind::Color => {
                let ok = value.len() == 7
                    && value.starts_with('#')
                    && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit);
                if ok {
                    Ok(())
                } else {
                    Err(format!("{value:?} is not an #rrggbb color"))
                }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerPackageManifest {
    pub schema_version: u32,
    pub kind: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub controller_api: String,
    pub runtime: DriverRuntime,
    pub permissions: ControllerPermissions,
    pub devices: Vec<DeviceMatcher>,
    #[serde(default)]
    pub surfaces: Vec<SurfaceImplementation>,
    #[serde(default)]
    pub host_controls: Vec<HostControlBinding>,
    #[serde(default)]
    pub host_actions: Vec<HostActionBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_profile: Option<SemanticControlProfile>,
    #[serde(default)]
    pub integrity: Option<ArtifactIntegrity>,
    #[serde(default)]
    pub settings: Vec<ControllerSetting>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessDriverInfo {
    pub protocol_version: u32,
    pub id: String,
    pub controller_api: String,
    pub layouts: Vec<String>,
    #[serde(default)]
    pub host_controls: Vec<HostControlBinding>,
    #[serde(default)]
    pub host_actions: Vec<HostActionBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_profile: Option<SemanticControlProfile>,
}

impl ProcessDriverInfo {
    pub fn validate_against(
        &self,
        manifest: &ControllerPackageManifest,
    ) -> Result<(), PackageError> {
        if self.protocol_version != PROCESS_DRIVER_PROTOCOL_VERSION {
            return Err(PackageError::DriverContract(format!(
                "unsupported process driver protocol {}",
                self.protocol_version
            )));
        }
        if self.id != manifest.id {
            return Err(PackageError::DriverContract(format!(
                "driver id {:?} does not match manifest {:?}",
                self.id, manifest.id
            )));
        }
        let driver_api = Version::parse(&self.controller_api).map_err(|error| {
            PackageError::DriverContract(format!("invalid driver API version: {error}"))
        })?;
        let requirement = VersionReq::parse(&manifest.controller_api).map_err(|error| {
            PackageError::DriverContract(format!("invalid manifest API requirement: {error}"))
        })?;
        if !requirement.matches(&driver_api) {
            return Err(PackageError::DriverContract(format!(
                "driver API {} does not satisfy {}",
                self.controller_api, manifest.controller_api
            )));
        }
        let expected = manifest
            .surfaces
            .iter()
            .map(|surface| surface.layout_id.as_str())
            .collect::<BTreeSet<_>>();
        let actual = self
            .layouts
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if self.layouts.is_empty() || expected != actual {
            return Err(PackageError::DriverContract(
                "driver layouts do not match the manifest".into(),
            ));
        }
        if self.host_controls != manifest.host_controls {
            return Err(PackageError::DriverContract(
                "driver reserved host controls do not match the manifest".into(),
            ));
        }
        if self.host_actions != manifest.host_actions {
            return Err(PackageError::DriverContract(
                "driver reserved host actions do not match the manifest".into(),
            ));
        }
        if self.semantic_profile != manifest.semantic_profile {
            return Err(PackageError::DriverContract(
                "driver semantic control profile does not match the manifest".into(),
            ));
        }
        Ok(())
    }
}

impl ControllerPackageManifest {
    pub fn validate(&self) -> Result<(), PackageError> {
        if self.schema_version != CONTROLLER_PACKAGE_SCHEMA_VERSION {
            return Err(PackageError::InvalidManifest(format!(
                "unsupported controller package schema {}",
                self.schema_version
            )));
        }
        if self.kind != "controller" {
            return Err(PackageError::InvalidManifest(
                "package kind must be \"controller\"".into(),
            ));
        }
        validate_identifier(&self.id)?;
        if self.name.trim().is_empty() || self.name.contains('\0') {
            return Err(PackageError::InvalidManifest(
                "controller name is empty or contains NUL".into(),
            ));
        }
        Version::parse(&self.version).map_err(|error| {
            PackageError::InvalidManifest(format!("invalid package version: {error}"))
        })?;
        let requirement = VersionReq::parse(&self.controller_api).map_err(|error| {
            PackageError::InvalidManifest(format!("invalid controller API requirement: {error}"))
        })?;
        let host_version = Version::parse(CONTROLLER_DRIVER_API_VERSION)
            .expect("constant controller API version is valid");
        if !requirement.matches(&host_version) {
            return Err(PackageError::IncompatibleApi {
                required: self.controller_api.clone(),
                available: CONTROLLER_DRIVER_API_VERSION.into(),
            });
        }
        match self.runtime.kind {
            DriverRuntimeKind::DeclarativeV1 if !self.runtime.entrypoints.is_empty() => {
                return Err(PackageError::InvalidManifest(
                    "declarative-v1 cannot declare binary entrypoints".into(),
                ));
            }
            DriverRuntimeKind::ProcessV1 | DriverRuntimeKind::WasmV1
                if self.runtime.entrypoints.is_empty() =>
            {
                return Err(PackageError::InvalidManifest(
                    "executable controller runtime has no platform entrypoints".into(),
                ));
            }
            _ => {}
        }
        for (target, entrypoint) in &self.runtime.entrypoints {
            validate_target(target)?;
            validate_relative_path(entrypoint)?;
        }
        self.permissions.validate(self.runtime.kind)?;
        if self.devices.is_empty() {
            return Err(PackageError::InvalidManifest(
                "controller package requires at least one device matcher".into(),
            ));
        }
        if self.runtime.kind == DriverRuntimeKind::DeclarativeV1 {
            if !self.surfaces.is_empty() {
                return Err(PackageError::InvalidManifest(
                    "declarative-v1 cannot own LITTLE or another display surface".into(),
                ));
            }
            if !self.settings.is_empty() {
                return Err(PackageError::InvalidManifest(
                    "declarative-v1 has no executable settings handler".into(),
                ));
            }
            if self.host_controls.is_empty()
                && self.host_actions.is_empty()
                && self.semantic_profile.is_none()
            {
                return Err(PackageError::InvalidManifest(
                    "declarative-v1 must declare at least one host or semantic MIDI mapping".into(),
                ));
            }
        } else if self.surfaces.is_empty() {
            return Err(PackageError::InvalidManifest(
                "executable controller package requires at least one display surface".into(),
            ));
        }
        for device in &self.devices {
            device.validate(self.runtime.kind)?;
        }
        for setting in &self.settings {
            setting.validate()?;
        }
        let mut setting_ids = BTreeSet::new();
        for setting in &self.settings {
            if !setting_ids.insert(&setting.id) {
                return Err(PackageError::InvalidManifest(format!(
                    "duplicate setting id {:?}",
                    setting.id
                )));
            }
        }
        let profile = ControllerProfile {
            id: self.id.clone(),
            name: self.name.clone(),
            driver_id: self.id.clone(),
            surfaces: self.surfaces.clone(),
            host_controls: self.host_controls.clone(),
            host_actions: self.host_actions.clone(),
            semantic_profile: self.semantic_profile.clone(),
        };
        if self.runtime.kind == DriverRuntimeKind::DeclarativeV1 {
            profile.validate_declarative()
        } else {
            profile.validate()
        }
        .map_err(PackageError::InvalidManifest)?;
        if let Some(integrity) = &self.integrity {
            if self.runtime.kind == DriverRuntimeKind::DeclarativeV1 && !integrity.sha256.is_empty()
            {
                return Err(PackageError::InvalidManifest(
                    "declarative-v1 has no executable artifacts to hash".into(),
                ));
            }
            for (target, digest) in &integrity.sha256 {
                if !self.runtime.entrypoints.contains_key(target) {
                    return Err(PackageError::InvalidManifest(format!(
                        "integrity digest references unknown target {target:?}"
                    )));
                }
                if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(PackageError::InvalidManifest(format!(
                        "invalid SHA-256 digest for {target:?}"
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn profile(&self) -> ControllerProfile {
        ControllerProfile {
            id: self.id.clone(),
            name: self.name.clone(),
            driver_id: self.id.clone(),
            surfaces: self.surfaces.clone(),
            host_controls: self.host_controls.clone(),
            host_actions: self.host_actions.clone(),
            semantic_profile: self.semantic_profile.clone(),
        }
    }

    pub fn entrypoint_for(&self, target: &str) -> Result<&str, PackageError> {
        self.runtime
            .entrypoints
            .get(target)
            .map(String::as_str)
            .ok_or_else(|| PackageError::MissingEntrypoint(target.into()))
    }

    pub fn is_declarative(&self) -> bool {
        self.runtime.kind == DriverRuntimeKind::DeclarativeV1
    }
}

/// Produces an immutable version of a controller manifest for a host-bundled
/// package. The build metadata changes whenever the source manifest or one of
/// the bundled artifacts changes, so a new RackForge build can install and
/// activate its matching controller without overwriting an existing version.
pub fn stamp_bundled_manifest(
    manifest_text: &str,
    artifacts: &[(&str, &[u8])],
) -> Result<String, PackageError> {
    let mut manifest: ControllerPackageManifest = toml::from_str(manifest_text)?;
    manifest.validate()?;

    let mut artifacts = artifacts.to_vec();
    artifacts.sort_by(|left, right| left.0.cmp(right.0));
    if artifacts.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(PackageError::InvalidManifest(
            "bundled controller contains duplicate artifact targets".into(),
        ));
    }

    let mut package_digest = Sha256::new();
    package_digest.update(manifest_text.as_bytes());
    for (target, bytes) in &artifacts {
        package_digest.update([0]);
        package_digest.update(target.as_bytes());
        package_digest.update([0]);
        package_digest.update(bytes);
    }
    let package_digest = format!("{:x}", package_digest.finalize());
    let mut version = Version::parse(&manifest.version).map_err(|error| {
        PackageError::InvalidManifest(format!("invalid package version: {error}"))
    })?;
    version.build = BuildMetadata::new(&format!("bundled.{}", &package_digest[..12]))
        .expect("a hexadecimal digest is valid SemVer build metadata");
    manifest.version = version.to_string();

    if !artifacts.is_empty() {
        let integrity = manifest.integrity.get_or_insert_with(|| ArtifactIntegrity {
            sha256: BTreeMap::new(),
        });
        for (target, bytes) in artifacts {
            integrity
                .sha256
                .insert(target.to_owned(), format!("{:x}", Sha256::digest(bytes)));
        }
    }
    manifest.validate()?;
    toml::to_string_pretty(&manifest).map_err(PackageError::TomlSerialize)
}

#[derive(Clone, Debug)]
pub struct ControllerPackage {
    root: PathBuf,
    manifest: ControllerPackageManifest,
}

impl ControllerPackage {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, PackageError> {
        let root = root.as_ref().to_path_buf();
        let metadata = fs::symlink_metadata(&root)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(PackageError::UnsafePackage(
                "controller package must be a real directory".into(),
            ));
        }
        let manifest_path = root.join(CONTROLLER_MANIFEST_FILE);
        let mut manifest_file = fs::File::open(&manifest_path)?;
        if manifest_file.metadata()?.len() > MAX_MANIFEST_BYTES {
            return Err(PackageError::UnsafePackage(
                "controller manifest is too large".into(),
            ));
        }
        let mut manifest_text = String::new();
        manifest_file.read_to_string(&mut manifest_text)?;
        let manifest: ControllerPackageManifest = toml::from_str(&manifest_text)?;
        manifest.validate()?;
        let package = Self { root, manifest };
        package.verify_artifacts()?;
        Ok(package)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &ControllerPackageManifest {
        &self.manifest
    }

    pub fn resolve_entrypoint(&self, target: &str) -> Result<PathBuf, PackageError> {
        let relative = self.manifest.entrypoint_for(target)?;
        let path = self.root.join(relative);
        let metadata = fs::symlink_metadata(&path).map_err(|_| {
            PackageError::MissingArtifact(path.strip_prefix(&self.root).unwrap_or(&path).into())
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(PackageError::UnsafePackage(format!(
                "entrypoint {:?} is not a regular file",
                path
            )));
        }
        Ok(path)
    }

    fn verify_artifacts(&self) -> Result<(), PackageError> {
        // A multi-platform manifest may declare targets this build never
        // produced: the Pi's package carries no Windows binary and a
        // Windows checkout carries no Linux one, and both are legitimate
        // installs OF THE SAME MANIFEST. What is REQUIRED is the artifact
        // for the platform installing it -- and integrity, when declared,
        // for every artifact that is actually present. Demanding every
        // declared target everywhere made a shared manifest impossible.
        let host = development_target();
        for target in self.manifest.runtime.entrypoints.keys() {
            let path = match self.resolve_entrypoint(target) {
                Ok(path) => path,
                Err(PackageError::MissingArtifact(missing)) if target != host => {
                    let _ = missing;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if let Some(expected) = self
                .manifest
                .integrity
                .as_ref()
                .and_then(|integrity| integrity.sha256.get(target))
            {
                let actual = sha256_file(&path)?;
                if !actual.eq_ignore_ascii_case(expected) {
                    return Err(PackageError::IntegrityMismatch {
                        target: target.clone(),
                        expected: expected.clone(),
                        actual,
                    });
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageTrust {
    Official,
    Certified,
    Community,
    Local,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRecord {
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    pub trust: PackageTrust,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct InstalledController {
    pub record: InstallRecord,
    pub package: ControllerPackage,
}

#[derive(Clone, Debug)]
pub struct PackageStore {
    root: PathBuf,
}

impl PackageStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn install_directory(
        &self,
        source: impl AsRef<Path>,
        trust: PackageTrust,
    ) -> Result<InstalledController, PackageError> {
        let source = ControllerPackage::open(source)?;
        fs::create_dir_all(self.root.join("packages"))?;
        fs::create_dir_all(self.root.join("active"))?;
        let id = &source.manifest.id;
        let version = &source.manifest.version;
        let destination = self.root.join("packages").join(id).join(version);
        if destination.exists() {
            let existing = ControllerPackage::open(&destination)?;
            if existing.manifest != source.manifest {
                return Err(PackageError::AlreadyInstalled {
                    id: id.clone(),
                    version: version.clone(),
                });
            }
            self.write_active_record(&InstallRecord {
                schema_version: INSTALL_RECORD_SCHEMA_VERSION,
                id: id.clone(),
                version: version.clone(),
                trust,
                enabled: true,
            })?;
            return self.resolve(id);
        }
        let staging = self.root.join(format!(
            ".staging-{}-{}-{}",
            id,
            version,
            std::process::id()
        ));
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        copy_package_tree(source.root(), &staging)?;
        let staged = ControllerPackage::open(&staging)?;
        if staged.manifest != source.manifest {
            let _ = fs::remove_dir_all(&staging);
            return Err(PackageError::UnsafePackage(
                "package changed while it was being installed".into(),
            ));
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&staging, &destination)?;
        let record = InstallRecord {
            schema_version: INSTALL_RECORD_SCHEMA_VERSION,
            id: id.clone(),
            version: version.clone(),
            trust,
            enabled: true,
        };
        if let Err(error) = self.write_active_record(&record) {
            let _ = fs::remove_dir_all(&destination);
            return Err(error);
        }
        self.resolve(id)
    }

    pub fn resolve(&self, id: &str) -> Result<InstalledController, PackageError> {
        let record = self.read_active_record(id)?;
        let package =
            ControllerPackage::open(self.root.join("packages").join(id).join(&record.version))?;
        if package.manifest.id != record.id || package.manifest.version != record.version {
            return Err(PackageError::UnsafePackage(
                "install record does not match controller package".into(),
            ));
        }
        Ok(InstalledController { record, package })
    }

    pub fn activate_version(
        &self,
        id: &str,
        version: &str,
    ) -> Result<InstalledController, PackageError> {
        validate_identifier(id)?;
        Version::parse(version).map_err(|error| {
            PackageError::InvalidManifest(format!("invalid package version: {error}"))
        })?;
        let current = self.read_active_record(id)?;
        let package = ControllerPackage::open(self.root.join("packages").join(id).join(version))?;
        if package.manifest.id != id || package.manifest.version != version {
            return Err(PackageError::UnsafePackage(
                "rollback target identity does not match its path".into(),
            ));
        }
        self.write_active_record(&InstallRecord {
            schema_version: INSTALL_RECORD_SCHEMA_VERSION,
            id: id.into(),
            version: version.into(),
            trust: current.trust,
            enabled: current.enabled,
        })?;
        self.resolve(id)
    }

    pub fn list(&self) -> Result<Vec<InstalledController>, PackageError> {
        let active = self.root.join("active");
        if !active.exists() {
            return Ok(Vec::new());
        }
        let mut controllers = Vec::new();
        for entry in fs::read_dir(active)? {
            let entry = entry?;
            if entry.file_type()?.is_symlink() || !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            controllers.push(self.resolve(id)?);
        }
        controllers.sort_by(|left, right| left.record.id.cmp(&right.record.id));
        Ok(controllers)
    }

    /// Resolves one approved/present MIDI input to a manifest-only
    /// controller. Disabled packages are ignored and ambiguous matches are
    /// rejected instead of silently assigning hardware to the wrong profile.
    pub fn resolve_declarative_input(
        &self,
        endpoint_name: &str,
    ) -> Result<Option<DeclarativeControllerBinding>, PackageError> {
        self.resolve_declarative_device(DeclarativeMidiDevice {
            endpoint_name,
            product_name: None,
            usb_vendor_id: None,
            usb_product_id: None,
        })
    }

    pub fn resolve_declarative_device(
        &self,
        device: DeclarativeMidiDevice<'_>,
    ) -> Result<Option<DeclarativeControllerBinding>, PackageError> {
        let endpoint_name = device.endpoint_name.trim();
        if endpoint_name.is_empty() || endpoint_name.contains('\0') {
            return Err(PackageError::InvalidEndpointName);
        }
        let mut matches = self
            .list()?
            .into_iter()
            .filter(|installed| {
                installed.record.enabled && installed.package.manifest().is_declarative()
            })
            .filter(|installed| {
                installed.package.manifest().devices.iter().any(|matcher| {
                    matcher.matches_declarative_device(DeclarativeMidiDevice {
                        endpoint_name,
                        ..device
                    })
                })
            })
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            matches.sort_by(|left, right| left.record.id.cmp(&right.record.id));
            return Err(PackageError::AmbiguousDeviceMatch {
                endpoint: endpoint_name.into(),
                controllers: matches
                    .iter()
                    .map(|installed| installed.record.id.clone())
                    .collect(),
            });
        }
        Ok(matches.pop().map(|installed| {
            let manifest = installed.package.manifest();
            DeclarativeControllerBinding {
                controller_id: manifest.id.clone(),
                controller_name: manifest.name.clone(),
                endpoint_name: endpoint_name.into(),
                host_controls: manifest.host_controls.clone(),
                host_actions: manifest.host_actions.clone(),
                semantic_profile: manifest.semantic_profile.clone(),
            }
        }))
    }

    fn write_active_record(&self, record: &InstallRecord) -> Result<(), PackageError> {
        let path = self.root.join("active").join(format!("{}.json", record.id));
        let temporary = path.with_extension(format!("json.new-{}", std::process::id()));
        let mut file = fs::File::create(&temporary)?;
        serde_json::to_writer_pretty(&mut file, record)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        Ok(())
    }

    fn read_active_record(&self, id: &str) -> Result<InstallRecord, PackageError> {
        validate_identifier(id)?;
        let record_path = self.root.join("active").join(format!("{id}.json"));
        let bytes = fs::read(&record_path)?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(PackageError::UnsafePackage(
                "controller install record is too large".into(),
            ));
        }
        let record: InstallRecord = serde_json::from_slice(&bytes)?;
        if record.schema_version != INSTALL_RECORD_SCHEMA_VERSION || record.id != id {
            return Err(PackageError::UnsafePackage(
                "controller install record is invalid".into(),
            ));
        }
        Version::parse(&record.version).map_err(|error| {
            PackageError::UnsafePackage(format!("invalid installed version: {error}"))
        })?;
        Ok(record)
    }
}

pub fn development_target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "aarch64") => "linux-aarch64",
        ("linux", "x86_64") => "linux-x86-64",
        ("windows", "x86_64") => "windows-x86-64",
        ("macos", "aarch64") => "macos-aarch64",
        ("macos", "x86_64") => "macos-x86-64",
        ("android", "aarch64") => "android-aarch64",
        _ => "unsupported",
    }
}

fn copy_package_tree(source: &Path, destination: &Path) -> Result<(), PackageError> {
    let mut files = 0_usize;
    let mut bytes = 0_u64;
    copy_directory(source, destination, &mut files, &mut bytes)
}

fn copy_directory(
    source: &Path,
    destination: &Path,
    files: &mut usize,
    bytes: &mut u64,
) -> Result<(), PackageError> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(PackageError::UnsafePackage(format!(
                "symbolic links are forbidden: {:?}",
                entry.path()
            )));
        }
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory(&entry.path(), &target, files, bytes)?;
        } else if file_type.is_file() {
            let length = entry.metadata()?.len();
            *files += 1;
            *bytes = bytes.saturating_add(length);
            if *files > MAX_PACKAGE_FILES
                || length > MAX_PACKAGE_FILE_BYTES
                || *bytes > MAX_PACKAGE_BYTES
            {
                return Err(PackageError::UnsafePackage(
                    "controller package exceeds installation limits".into(),
                ));
            }
            fs::copy(entry.path(), target)?;
        } else {
            return Err(PackageError::UnsafePackage(format!(
                "unsupported package entry {:?}",
                entry.path()
            )));
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, PackageError> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    io::copy(&mut file, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()))
}

fn validate_identifier(value: &str) -> Result<(), PackageError> {
    if value.is_empty()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        })
    {
        return Err(PackageError::InvalidManifest(format!(
            "invalid identifier {value:?}"
        )));
    }
    Ok(())
}

fn validate_target(value: &str) -> Result<(), PackageError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(PackageError::InvalidManifest(format!(
            "invalid target {value:?}"
        )));
    }
    Ok(())
}

// Target names are lowercase kebab-case ("windows-x86-64", never
// "windows-x86_64"): the validator above forbids underscores, and
// `development_target` follows the same convention.
fn validate_relative_path(value: &str) -> Result<(), PackageError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\\')
        || path.is_absolute()
        || path.components().any(|component| {
            !matches!(component, Component::Normal(_)) || component.as_os_str().is_empty()
        })
    {
        return Err(PackageError::InvalidManifest(format!(
            "unsafe package path {value:?}"
        )));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum PackageError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid TOML manifest: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("could not serialize TOML manifest: {0}")]
    TomlSerialize(toml::ser::Error),
    #[error("invalid JSON install record: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid controller manifest: {0}")]
    InvalidManifest(String),
    #[error("controller API {required} is incompatible with host API {available}")]
    IncompatibleApi { required: String, available: String },
    #[error("controller package has no entrypoint for {0}")]
    MissingEntrypoint(String),
    #[error("controller package is missing artifact {0:?}")]
    MissingArtifact(PathBuf),
    #[error("unsafe controller package: {0}")]
    UnsafePackage(String),
    #[error("artifact integrity mismatch for {target}: expected {expected}, got {actual}")]
    IntegrityMismatch {
        target: String,
        expected: String,
        actual: String,
    },
    #[error("controller {id} version {version} is already installed")]
    AlreadyInstalled { id: String, version: String },
    #[error("controller driver contract failed: {0}")]
    DriverContract(String),
    #[error("invalid MIDI endpoint name")]
    InvalidEndpointName,
    #[error(
        "MIDI endpoint {endpoint:?} matches more than one declarative controller: {controllers:?}"
    )]
    AmbiguousDeviceMatch {
        endpoint: String,
        controllers: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_controller_api::{
        GestureCapabilities, HostControlTarget, MidiControlChangeBinding, SurfaceQuality,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "rackforge-controller-package-{name}-{}-{}",
                std::process::id(),
                NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            ));
            if path.exists() {
                fs::remove_dir_all(&path).unwrap();
            }
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn manifest() -> ControllerPackageManifest {
        ControllerPackageManifest {
            schema_version: CONTROLLER_PACKAGE_SCHEMA_VERSION,
            kind: "controller".into(),
            id: "org.rackforge.example-controller".into(),
            name: "Example Controller".into(),
            version: "1.2.3".into(),
            controller_api: "^1.0".into(),
            runtime: DriverRuntime {
                kind: DriverRuntimeKind::ProcessV1,
                entrypoints: BTreeMap::from([(
                    "linux-aarch64".into(),
                    "bin/linux-aarch64/driver".into(),
                )]),
            },
            permissions: ControllerPermissions {
                midi_input: true,
                midi_output: true,
                sysex: true,
                usb_metadata: true,
                ..ControllerPermissions::default()
            },
            devices: vec![DeviceMatcher {
                id: "example.v1".into(),
                usb_vendor_id: 0x1234,
                usb_product_ids: vec![0x5678],
                product_names: vec!["Example".into()],
                endpoints: vec![
                    EndpointMatcher {
                        role: EndpointRole::SurfaceInput,
                        name_contains: vec!["example".into()],
                        name_contains_any: Vec::new(),
                        name_ends_with: Some("midi".into()),
                        exclude_contains: vec!["daw".into()],
                    },
                    EndpointMatcher {
                        role: EndpointRole::DisplayOutput,
                        name_contains: vec!["example".into()],
                        name_contains_any: Vec::new(),
                        name_ends_with: Some("midi".into()),
                        exclude_contains: vec!["daw".into()],
                    },
                ],
            }],
            surfaces: vec![SurfaceImplementation {
                layout_id: "little@1".into(),
                quality: SurfaceQuality::Native,
                priority: 0,
                viewport: Default::default(),
                gestures: GestureCapabilities {
                    soft_key_long_press: true,
                    emergency_home_chord: true,
                },
            }],
            host_controls: Vec::new(),
            host_actions: Vec::new(),
            semantic_profile: None,
            integrity: None,
            settings: Vec::new(),
        }
    }

    fn declarative_manifest(id: &str, endpoint: &str) -> ControllerPackageManifest {
        ControllerPackageManifest {
            schema_version: CONTROLLER_PACKAGE_SCHEMA_VERSION,
            kind: "controller".into(),
            id: id.into(),
            name: "Declarative MIDI Controller".into(),
            version: "1.0.0".into(),
            controller_api: "^1.0".into(),
            runtime: DriverRuntime {
                kind: DriverRuntimeKind::DeclarativeV1,
                entrypoints: BTreeMap::new(),
            },
            permissions: ControllerPermissions {
                midi_input: true,
                ..ControllerPermissions::default()
            },
            devices: vec![DeviceMatcher {
                id: "generic-midi.v1".into(),
                usb_vendor_id: 0x1234,
                usb_product_ids: vec![0x5678],
                product_names: Vec::new(),
                endpoints: vec![EndpointMatcher {
                    role: EndpointRole::PerformanceInput,
                    name_contains: vec![endpoint.into()],
                    name_contains_any: Vec::new(),
                    name_ends_with: None,
                    exclude_contains: Vec::new(),
                }],
            }],
            surfaces: Vec::new(),
            host_controls: vec![HostControlBinding {
                target: HostControlTarget::MasterLevel,
                midi_cc: MidiControlChangeBinding {
                    channel: 0,
                    controller: 7,
                },
            }],
            host_actions: Vec::new(),
            semantic_profile: None,
            integrity: None,
            settings: Vec::new(),
        }
    }

    #[test]
    fn validates_a_versioned_controller_package() {
        assert!(manifest().validate().is_ok());
        assert_eq!(manifest().profile().surfaces[0].layout_id, "little@1");
    }

    #[test]
    fn validates_manifest_only_declarative_controller() {
        let declarative = declarative_manifest("org.rackforge.generic-midi", "generic midi");
        declarative.validate().unwrap();
        assert!(declarative.runtime.entrypoints.is_empty());
        assert!(declarative.surfaces.is_empty());

        let mut unsafe_permissions = declarative.clone();
        unsafe_permissions.permissions.midi_output = true;
        assert!(unsafe_permissions.validate().is_err());

        let mut binary = declarative.clone();
        binary
            .runtime
            .entrypoints
            .insert("windows-x86-64".into(), "driver.exe".into());
        assert!(binary.validate().is_err());
    }

    #[test]
    fn declarative_resolution_is_enabled_case_insensitive_and_unambiguous() {
        fn install(store: &PackageStore, parent: &Path, manifest: &ControllerPackageManifest) {
            let root = parent.join(&manifest.id);
            fs::create_dir(&root).unwrap();
            fs::write(
                root.join(CONTROLLER_MANIFEST_FILE),
                toml::to_string_pretty(manifest).unwrap(),
            )
            .unwrap();
            store
                .install_directory(root, PackageTrust::Community)
                .unwrap();
        }

        let sources = TestDirectory::new("declarative-sources");
        let store_root = TestDirectory::new("declarative-store");
        let store = PackageStore::new(&store_root.0);
        install(
            &store,
            &sources.0,
            &declarative_manifest("org.rackforge.generic-midi", "generic midi"),
        );
        let resolved = store
            .resolve_declarative_input("Generic MIDI Port 1")
            .unwrap()
            .unwrap();
        assert_eq!(resolved.controller_id, "org.rackforge.generic-midi");
        assert_eq!(resolved.host_controls.len(), 1);
        assert!(
            store
                .resolve_declarative_device(DeclarativeMidiDevice {
                    endpoint_name: "Generic MIDI Port 1",
                    product_name: None,
                    usb_vendor_id: Some(0xabcd),
                    usb_product_id: Some(0x5678),
                })
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .resolve_declarative_input("Unrelated Keyboard")
                .unwrap()
                .is_none()
        );

        install(
            &store,
            &sources.0,
            &declarative_manifest("org.rackforge.generic-midi-two", "generic midi"),
        );
        assert!(matches!(
            store.resolve_declarative_input("Generic MIDI Port 1"),
            Err(PackageError::AmbiguousDeviceMatch { .. })
        ));
    }

    #[test]
    fn bundled_manifest_revision_tracks_manifest_and_driver_content() {
        let source = toml::to_string_pretty(&manifest()).unwrap();
        let first =
            stamp_bundled_manifest(&source, &[("linux-aarch64", b"first driver".as_slice())])
                .unwrap();
        let repeated =
            stamp_bundled_manifest(&source, &[("linux-aarch64", b"first driver".as_slice())])
                .unwrap();
        let changed =
            stamp_bundled_manifest(&source, &[("linux-aarch64", b"second driver".as_slice())])
                .unwrap();
        let first: ControllerPackageManifest = toml::from_str(&first).unwrap();
        let repeated: ControllerPackageManifest = toml::from_str(&repeated).unwrap();
        let changed: ControllerPackageManifest = toml::from_str(&changed).unwrap();

        assert_eq!(first.version, repeated.version);
        assert_ne!(first.version, changed.version);
        assert!(first.version.starts_with("1.2.3+bundled."));
        let expected_driver_digest = format!("{:x}", Sha256::digest(b"first driver"));
        assert_eq!(
            first
                .integrity
                .unwrap()
                .sha256
                .get("linux-aarch64")
                .map(String::as_str),
            Some(expected_driver_digest.as_str())
        );
    }

    #[test]
    fn rejects_path_escape_and_privileged_permissions() {
        let mut unsafe_path = manifest();
        unsafe_path
            .runtime
            .entrypoints
            .insert("windows-x86-64".into(), "../outside/driver.exe".into());
        assert!(matches!(
            unsafe_path.validate(),
            Err(PackageError::InvalidManifest(_))
        ));

        let mut firmware = manifest();
        firmware.permissions.firmware_write = true;
        assert!(matches!(
            firmware.validate(),
            Err(PackageError::InvalidManifest(_))
        ));
    }

    #[test]
    fn endpoint_matching_is_case_insensitive_and_excludes_auxiliary_ports() {
        let matcher = &manifest().devices[0].endpoints[0];
        assert!(matcher.matches("Example MIDI"));
        assert!(!matcher.matches("Example DAW MIDI"));
    }

    #[test]
    fn rejects_unsupported_api_and_ambiguous_surface_endpoints() {
        let mut incompatible = manifest();
        incompatible.controller_api = "^2.0".into();
        assert!(matches!(
            incompatible.validate(),
            Err(PackageError::IncompatibleApi { .. })
        ));

        let mut missing_output = manifest();
        missing_output.devices[0].endpoints.pop();
        assert!(matches!(
            missing_output.validate(),
            Err(PackageError::InvalidManifest(_))
        ));
    }

    #[test]
    fn store_installs_immutable_version_and_detects_artifact_tampering() {
        let source_root = TestDirectory::new("source");
        let entrypoint = source_root.0.join("bin/linux-aarch64/driver");
        fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
        fs::write(&entrypoint, b"known driver bytes").unwrap();
        let mut package_manifest = manifest();
        package_manifest.integrity = Some(ArtifactIntegrity {
            sha256: BTreeMap::from([("linux-aarch64".into(), sha256_file(&entrypoint).unwrap())]),
        });
        fs::write(
            source_root.0.join(CONTROLLER_MANIFEST_FILE),
            toml::to_string_pretty(&package_manifest).unwrap(),
        )
        .unwrap();

        let store_root = TestDirectory::new("store");
        let store = PackageStore::new(&store_root.0);
        let installed = store
            .install_directory(&source_root.0, PackageTrust::Local)
            .unwrap();
        assert_eq!(installed.record.id, package_manifest.id);
        assert_eq!(store.list().unwrap().len(), 1);
        assert_eq!(
            store
                .install_directory(&source_root.0, PackageTrust::Local)
                .unwrap()
                .record,
            installed.record
        );

        fs::write(&entrypoint, b"second driver version").unwrap();
        package_manifest.version = "2.0.0".into();
        package_manifest.integrity = Some(ArtifactIntegrity {
            sha256: BTreeMap::from([("linux-aarch64".into(), sha256_file(&entrypoint).unwrap())]),
        });
        fs::write(
            source_root.0.join(CONTROLLER_MANIFEST_FILE),
            toml::to_string_pretty(&package_manifest).unwrap(),
        )
        .unwrap();
        assert_eq!(
            store
                .install_directory(&source_root.0, PackageTrust::Local)
                .unwrap()
                .record
                .version,
            "2.0.0"
        );
        assert_eq!(
            store
                .activate_version(&package_manifest.id, "1.2.3")
                .unwrap()
                .record
                .version,
            "1.2.3"
        );

        let installed_entrypoint = installed
            .package
            .resolve_entrypoint("linux-aarch64")
            .unwrap();
        fs::write(installed_entrypoint, b"tampered").unwrap();
        assert!(matches!(
            store.resolve(&package_manifest.id),
            Err(PackageError::IntegrityMismatch { .. })
        ));
    }

    #[test]
    fn process_driver_identity_must_match_the_signed_manifest() {
        let package_manifest = manifest();
        let info = ProcessDriverInfo {
            protocol_version: PROCESS_DRIVER_PROTOCOL_VERSION,
            id: package_manifest.id.clone(),
            controller_api: CONTROLLER_DRIVER_API_VERSION.into(),
            layouts: vec!["little@1".into()],
            host_controls: Vec::new(),
            host_actions: Vec::new(),
            semantic_profile: package_manifest.semantic_profile.clone(),
        };
        info.validate_against(&package_manifest).unwrap();

        let mut impersonating = info;
        impersonating.id = "org.rackforge.other".into();
        assert!(matches!(
            impersonating.validate_against(&package_manifest),
            Err(PackageError::DriverContract(_))
        ));
    }
}
