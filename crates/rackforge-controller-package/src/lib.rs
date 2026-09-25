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

pub mod feedback;
pub mod inputs;
pub mod supervise;
pub mod user;

pub use feedback::{
    IDENTITY_REPLY_WINDOW_MS, IDENTITY_REQUEST, IdentityReply, OnConnectMessage, OnConnectPort,
    OutputState, OutputSummary,
};
pub use inputs::{
    ButtonReport, ControllerInput, EncoderEncoding, InputAction, InputKind, InputMessage,
    InputMidi, InputRole, SysexIdentity,
};
pub use user::{USER_CONTROLLER_PREFIX, UserControllerRequest};

/// The schema that describes mappings only, each repeating its MIDI message.
pub const CONTROLLER_PACKAGE_SCHEMA_VERSION: u32 = 1;
/// The layered schema: identity, the controller's inputs, the meanings given
/// to them, and optionally a driver (docs/architecture/controller-packages-v2.md).
pub const CONTROLLER_PACKAGE_SCHEMA_VERSION_2: u32 = 2;
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
    /// Declarative only: the output a package's `on_connect` messages with
    /// `to = "setup_output"` go to. Some controllers change mode only
    /// through their DAW port, which is not the port their controls send on.
    SetupOutput,
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
    /// Schema 2: the Identity Reply the device answers with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sysex_identity: Option<SysexIdentity>,
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
        if let Some(identity) = &self.sysex_identity {
            identity.validate().map_err(|error| {
                PackageError::InvalidManifest(format!("device {:?}: {error}", self.id))
            })?;
        }
        let roles = self
            .endpoints
            .iter()
            .map(|endpoint| endpoint.role)
            .collect::<BTreeSet<_>>();
        if runtime != DriverRuntimeKind::DeclarativeV1 && roles.contains(&EndpointRole::SetupOutput)
        {
            return Err(PackageError::InvalidManifest(
                "a driver opens its own ports; setup_output is for declarative packages".into(),
            ));
        }
        if self
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.role == EndpointRole::SetupOutput)
            .count()
            > 1
        {
            return Err(PackageError::InvalidManifest(
                "a device declares at most one setup_output endpoint".into(),
            ));
        }
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

    /// The endpoint a package's setup messages go to, when it declares one.
    pub fn setup_output(&self) -> Option<&EndpointMatcher> {
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.role == EndpointRole::SetupOutput)
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

/// What a package may do. A permission left out is not granted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
    fn validate(
        self,
        runtime: DriverRuntimeKind,
        on_connect: &[OnConnectMessage],
    ) -> Result<(), PackageError> {
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
        if runtime == DriverRuntimeKind::DeclarativeV1 {
            if self.usb_metadata {
                return Err(PackageError::InvalidManifest(
                    "declarative-v1 cannot request USB metadata".into(),
                ));
            }
            // A declarative package sends only its on_connect messages, and
            // asks for exactly what they need: the player reads the request
            // as what the package will do.
            let sends = !on_connect.is_empty();
            if self.midi_output != sends {
                return Err(PackageError::InvalidManifest(if sends {
                    "a package that sends on_connect messages asks for midi_output".into()
                } else {
                    "declarative-v1 asks for MIDI output only to send on_connect messages".into()
                }));
            }
            let sysex = feedback::sends_sysex(on_connect);
            if self.sysex != sysex {
                return Err(PackageError::InvalidManifest(if sysex {
                    "a package that sends SysEx on connect asks for sysex".into()
                } else {
                    "declarative-v1 asks for SysEx only when an on_connect message is SysEx".into()
                }));
            }
        }
        if runtime != DriverRuntimeKind::DeclarativeV1 && !self.midi_output {
            return Err(PackageError::InvalidManifest(
                "executable controller packages require MIDI output".into(),
            ));
        }
        Ok(())
    }
}

/// Which of the MIDI outputs a host sees is the setup output of the device
/// found on `input_name`: one the package's `setup_output` endpoint matches,
/// and of those the one whose name shares the most with the input's -- two
/// units of one model differ by the device ID in their names. Windows wraps
/// the input's whole name ("MIDIOUT2 (LCXL3 1 MIDI)"); macOS and ALSA share
/// its beginning ("LCXL3 1 (DAW In)"). Two outputs alike to the last
/// character are not guessed between.
pub fn setup_output_port(
    matcher: &EndpointMatcher,
    input_name: &str,
    outputs: &[String],
) -> Option<String> {
    let input = input_name.to_ascii_lowercase();
    let shared = |output: &str| {
        let output = output.to_ascii_lowercase();
        if !input.is_empty() && output.contains(&input) {
            return usize::MAX;
        }
        output
            .chars()
            .zip(input.chars())
            .take_while(|(left, right)| left == right)
            .count()
    };
    let mut candidates = outputs
        .iter()
        .filter(|output| matcher.matches(output))
        .map(|output| (shared(output), output))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.0.cmp(&left.0));
    match candidates.as_slice() {
        [] => None,
        [(_, only)] => Some((*only).clone()),
        [(best, output), (next, _), ..] if best > next => Some((*output).clone()),
        _ => None,
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
    /// Whether the device's Identity Reply matched the package's.
    pub identified: bool,
    pub output_state: OutputState,
    /// What to send when the controller connects: empty until allowed.
    pub on_connect: Vec<Vec<u8>>,
    /// What to send to the device's setup output when it connects, and how
    /// to find that output: empty until allowed.
    pub setup_output: Option<EndpointMatcher>,
    pub setup_messages: Vec<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclarativeMidiDevice<'a> {
    pub endpoint_name: &'a str,
    pub product_name: Option<&'a str>,
    pub usb_vendor_id: Option<u16>,
    pub usb_product_id: Option<u16>,
    /// The device's answer to the Identity Request, when it gave one.
    pub identity: Option<&'a IdentityReply>,
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
        // A device that said which model it is is not another model, however
        // alike their port names.
        if let (Some(expected), Some(reply)) = (&self.sysex_identity, device.identity)
            && !expected.matches(reply)
        {
            return false;
        }
        true
    }

    fn identifies(&self, device: DeclarativeMidiDevice<'_>) -> bool {
        self.sysex_identity.is_some()
            && device.identity.is_some()
            && self.matches_declarative_device(device)
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

/// Schema 2 leaves the runtime out of a package that is data only.
pub(crate) fn declarative_runtime() -> DriverRuntime {
    DriverRuntime {
        kind: DriverRuntimeKind::DeclarativeV1,
        entrypoints: BTreeMap::new(),
    }
}

/// Schema 2 leaves the permissions out of a package that only listens.
pub(crate) fn input_only_permissions() -> ControllerPermissions {
    ControllerPermissions {
        midi_input: true,
        ..ControllerPermissions::default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerPackageManifest {
    pub schema_version: u32,
    pub kind: String,
    pub id: String,
    pub name: String,
    /// Schema 2: who makes the hardware.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    pub version: String,
    pub controller_api: String,
    #[serde(default = "declarative_runtime")]
    pub runtime: DriverRuntime,
    #[serde(default = "input_only_permissions")]
    pub permissions: ControllerPermissions,
    pub devices: Vec<DeviceMatcher>,
    /// Schema 2: every physical control, with the message it sends.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<ControllerInput>,
    /// Schema 2: semantic roles, by input.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<InputRole>,
    /// Schema 2: host actions, by input.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<InputAction>,
    /// Schema 2: the stable MIDI source identity the roles speak for,
    /// `controller.<id>` when left out. A package moving from schema 1 keeps
    /// its old one, so what a player learnt on its controls stays linked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    /// Schema 2, layer 4: messages sent to the controller when it connects,
    /// once the player allows it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on_connect: Vec<OnConnectMessage>,
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
        let profile = manifest.profile();
        if self.host_controls != profile.host_controls {
            return Err(PackageError::DriverContract(
                "driver reserved host controls do not match the manifest".into(),
            ));
        }
        if self.host_actions != profile.host_actions {
            return Err(PackageError::DriverContract(
                "driver reserved host actions do not match the manifest".into(),
            ));
        }
        if self.semantic_profile != profile.semantic_profile {
            return Err(PackageError::DriverContract(
                "driver semantic control profile does not match the manifest".into(),
            ));
        }
        Ok(())
    }
}

impl ControllerPackageManifest {
    /// The controls that fill a slot of the plugins' control layouts, in
    /// the package's order.
    pub fn slotted_inputs(&self) -> Vec<rackforge_midi_api::control_layout::SlottedInput> {
        self.inputs
            .iter()
            .filter_map(ControllerInput::slotted_input)
            .collect()
    }

    /// Whether the package reads the MIDI input named so as its performance
    /// input: a host plays and reads such a port whatever it is called, even
    /// a DAW port another keyboard's would be left alone.
    pub fn claims_performance_input(&self, endpoint_name: &str) -> bool {
        self.devices.iter().any(|device| {
            device.endpoints.iter().any(|endpoint| {
                endpoint.role == EndpointRole::PerformanceInput && endpoint.matches(endpoint_name)
            })
        })
    }

    /// The controller as a player knows it: its maker, then its name, once.
    pub fn display_name(&self) -> String {
        match &self.vendor {
            Some(vendor) if !self.name.starts_with(vendor.as_str()) => {
                format!("{vendor} {}", self.name)
            }
            _ => self.name.clone(),
        }
    }

    pub fn validate(&self) -> Result<(), PackageError> {
        match self.schema_version {
            CONTROLLER_PACKAGE_SCHEMA_VERSION => self.validate_schema_1_fields()?,
            CONTROLLER_PACKAGE_SCHEMA_VERSION_2 => self.validate_schema_2_fields()?,
            other => {
                return Err(PackageError::InvalidManifest(format!(
                    "unsupported controller package schema {other}"
                )));
            }
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
        self.permissions
            .validate(self.runtime.kind, &self.on_connect)?;
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
            // Schema 1 exists only through its mappings. Schema 2 exists
            // through its inputs: a player's M-Audio with named knobs and
            // no roles is a complete package, whose knobs they map.
            if self.schema_version == CONTROLLER_PACKAGE_SCHEMA_VERSION
                && self.host_controls.is_empty()
                && self.host_actions.is_empty()
                && self.semantic_profile.is_none()
            {
                return Err(PackageError::InvalidManifest(
                    "declarative-v1 must declare at least one host or semantic MIDI mapping".into(),
                ));
            }
            if self.schema_version == CONTROLLER_PACKAGE_SCHEMA_VERSION_2 && self.inputs.is_empty()
            {
                return Err(PackageError::InvalidManifest(
                    "a declarative schema 2 controller declares at least one input".into(),
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
        let profile = self.profile();
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

    /// The runtime profile, the same shape for either schema: schema 2's
    /// roles and actions are lowered onto the inputs they name. Hosts read
    /// mappings from here, never from the manifest's own fields.
    pub fn profile(&self) -> ControllerProfile {
        let (host_actions, semantic_profile) =
            if self.schema_version == CONTROLLER_PACKAGE_SCHEMA_VERSION_2 {
                let source_id = self
                    .source_id
                    .clone()
                    .unwrap_or_else(|| format!("controller.{}", self.id));
                (
                    inputs::lower_actions(&self.inputs, &self.actions),
                    inputs::lower_roles(&source_id, &self.inputs, &self.roles),
                )
            } else {
                (self.host_actions.clone(), self.semantic_profile.clone())
            };
        ControllerProfile {
            id: self.id.clone(),
            name: self.name.clone(),
            driver_id: self.id.clone(),
            surfaces: self.surfaces.clone(),
            host_controls: self.host_controls.clone(),
            host_actions,
            semantic_profile,
        }
    }

    /// Schema 1 has none of schema 2's fields: an older package that carries
    /// one is a mistake, not a hybrid.
    fn validate_schema_1_fields(&self) -> Result<(), PackageError> {
        if self.vendor.is_some()
            || !self.inputs.is_empty()
            || !self.roles.is_empty()
            || !self.actions.is_empty()
            || self.source_id.is_some()
            || !self.on_connect.is_empty()
            || self
                .devices
                .iter()
                .any(|device| device.sysex_identity.is_some())
        {
            return Err(PackageError::InvalidManifest(
                "vendor, inputs, roles, actions, source_id, on_connect and sysex_identity \
                 need schema_version = 2"
                    .into(),
            ));
        }
        Ok(())
    }

    /// Schema 2 names inputs instead of repeating messages: schema 1's
    /// mapping fields are replaced by `roles` and `actions`.
    fn validate_schema_2_fields(&self) -> Result<(), PackageError> {
        if !self.host_controls.is_empty()
            || !self.host_actions.is_empty()
            || self.semantic_profile.is_some()
        {
            return Err(PackageError::InvalidManifest(
                "schema 2 gives meanings to inputs with roles and actions, not host_controls, \
                 host_actions or semantic_profile"
                    .into(),
            ));
        }
        if let Some(vendor) = &self.vendor
            && (vendor.trim().is_empty() || vendor.contains('\0') || vendor.chars().count() > 64)
        {
            return Err(PackageError::InvalidManifest(
                "vendor is empty, too long or contains NUL".into(),
            ));
        }
        if let Some(source_id) = &self.source_id {
            validate_identifier(source_id)?;
        }
        if !self.on_connect.is_empty() && !self.is_declarative() {
            return Err(PackageError::InvalidManifest(
                "a driver sends its controller's messages itself; on_connect is for \
                 declarative packages"
                    .into(),
            ));
        }
        feedback::validate_on_connect(&self.on_connect).map_err(PackageError::InvalidManifest)?;
        // A setup output and the messages for it come together: one without
        // the other is a package that says something it cannot do.
        let sends_setup = self
            .on_connect
            .iter()
            .any(|message| message.to == OnConnectPort::SetupOutput);
        let has_setup_output = self
            .devices
            .iter()
            .all(|device| device.setup_output().is_some());
        let declares_setup_output = self
            .devices
            .iter()
            .any(|device| device.setup_output().is_some());
        if sends_setup && !has_setup_output {
            return Err(PackageError::InvalidManifest(
                "on_connect messages to the setup output need every device to declare one".into(),
            ));
        }
        if declares_setup_output && !sends_setup {
            return Err(PackageError::InvalidManifest(
                "a setup_output endpoint is only for on_connect messages sent to it".into(),
            ));
        }
        inputs::validate_inputs(&self.inputs, &self.roles, &self.actions)
            .map_err(PackageError::InvalidManifest)
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

    /// The controls an editor shows for this controller, and the meanings the
    /// package gives them. A schema 2 package declares both. A schema 1
    /// package names no control, so each message it binds becomes an unnamed
    /// input -- "CC 74" -- carrying the role or action it had.
    pub fn editor_inputs(&self) -> EditorInputs {
        if self.schema_version == CONTROLLER_PACKAGE_SCHEMA_VERSION_2 {
            return EditorInputs {
                inputs: self.inputs.clone(),
                roles: self.roles.clone(),
                actions: self.actions.clone(),
            };
        }
        let mut editor = EditorInputs::default();
        let mut seen = BTreeSet::new();
        let mut input_for = |editor: &mut EditorInputs,
                             channel: u8,
                             controller: u8,
                             kind: InputKind,
                             button: Option<ButtonReport>|
         -> String {
            let id = format!("cc-{}-{controller}", channel + 1);
            if seen.insert((channel, controller)) {
                editor.inputs.push(ControllerInput {
                    id: id.clone(),
                    name: format!("CC {controller}"),
                    kind,
                    group: None,
                    midi: InputMidi {
                        channel,
                        cc: Some(controller),
                        ..InputMidi::default()
                    },
                    button,
                    encoder: None,
                    slot: None,
                });
            }
            id
        };
        if let Some(profile) = &self.semantic_profile {
            for control in &profile.controls {
                let kind = match control.mode {
                    rackforge_controller_api::SemanticControlMode::Relative => InputKind::Encoder,
                    rackforge_controller_api::SemanticControlMode::Absolute => InputKind::Knob,
                };
                let input = input_for(
                    &mut editor,
                    control.midi_cc.channel,
                    control.midi_cc.controller,
                    kind,
                    None,
                );
                editor.roles.push(InputRole {
                    input,
                    role: control.role.clone(),
                    invert: control.invert,
                    mode: control.mode,
                });
            }
        }
        // Schema 1 declares its host actions as Control Changes only.
        for (action, midi_cc) in self
            .host_actions
            .iter()
            .filter_map(|action| Some((action, action.midi_cc?)))
        {
            let input = input_for(
                &mut editor,
                midi_cc.channel,
                midi_cc.controller,
                InputKind::Button,
                Some(ButtonReport {
                    press: midi_cc.press_value,
                    release: midi_cc.release_value,
                    press_only: false,
                    latching: false,
                }),
            );
            editor.actions.push(InputAction {
                input,
                target: action.target,
            });
        }
        for control in &self.host_controls {
            input_for(
                &mut editor,
                control.midi_cc.channel,
                control.midi_cc.controller,
                InputKind::Fader,
                None,
            );
        }
        editor
    }
}

/// A controller's controls and the package's meanings for them, for editing.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditorInputs {
    pub inputs: Vec<ControllerInput>,
    pub roles: Vec<InputRole>,
    pub actions: Vec<InputAction>,
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
    /// The version whose on_connect messages the player allowed. A new
    /// version asks again: what it sends may have changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_allowed_version: Option<String>,
}

#[derive(Clone, Debug)]
pub struct InstalledController {
    pub record: InstallRecord,
    pub package: ControllerPackage,
}

impl InstalledController {
    /// Whether RackForge sends this package's messages to its controller.
    /// RackForge's own packages and certified ones are allowed as installed.
    pub fn output_state(&self) -> OutputState {
        if self.package.manifest().on_connect.is_empty() {
            OutputState::None
        } else if matches!(
            self.record.trust,
            PackageTrust::Official | PackageTrust::Certified
        ) || self.record.output_allowed_version.as_deref() == Some(&self.record.version)
        {
            OutputState::Allowed
        } else {
            OutputState::Asked
        }
    }

    /// What the package asks to send, as the catalog shows it to the player.
    pub fn output_summary(&self) -> OutputSummary {
        OutputSummary {
            state: self.output_state(),
            messages: self
                .package
                .manifest()
                .on_connect
                .iter()
                .map(|message| message.message.clone())
                .collect(),
            sysex: self.package.manifest().permissions.sysex,
        }
    }

    /// The messages to send to the controller's own port when it connects:
    /// none until allowed.
    pub fn on_connect_messages(&self) -> Vec<Vec<u8>> {
        self.messages_to(OnConnectPort::Input)
    }

    /// The messages to send to the device's setup output when it connects:
    /// none until allowed.
    pub fn setup_messages(&self) -> Vec<Vec<u8>> {
        self.messages_to(OnConnectPort::SetupOutput)
    }

    fn messages_to(&self, port: OnConnectPort) -> Vec<Vec<u8>> {
        if self.output_state() != OutputState::Allowed {
            return Vec::new();
        }
        self.package
            .manifest()
            .on_connect
            .iter()
            .filter(|message| message.to == port)
            .filter_map(|message| message.bytes().ok())
            .collect()
    }
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
            // The same package again: what the player allowed it still holds.
            let output_allowed_version = self
                .read_active_record(id)
                .ok()
                .and_then(|record| record.output_allowed_version);
            self.write_active_record(&InstallRecord {
                schema_version: INSTALL_RECORD_SCHEMA_VERSION,
                id: id.clone(),
                version: version.clone(),
                trust,
                enabled: true,
                output_allowed_version,
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
        let output_allowed_version = self
            .read_active_record(id)
            .ok()
            .and_then(|record| record.output_allowed_version);
        let record = InstallRecord {
            schema_version: INSTALL_RECORD_SCHEMA_VERSION,
            id: id.clone(),
            version: version.clone(),
            trust,
            enabled: true,
            output_allowed_version,
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
            output_allowed_version: current.output_allowed_version,
        })?;
        self.resolve(id)
    }

    /// Records whether the player lets the active version of a package send
    /// its on_connect messages.
    pub fn allow_output(&self, id: &str, allow: bool) -> Result<InstalledController, PackageError> {
        let mut record = self.read_active_record(id)?;
        record.output_allowed_version = allow.then(|| record.version.clone());
        self.write_active_record(&record)?;
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
        self.resolve_identified_input(endpoint_name, None)
    }

    /// As [`Self::resolve_declarative_input`], with the device's answer to
    /// the Identity Request when the host asked and it replied.
    pub fn resolve_identified_input(
        &self,
        endpoint_name: &str,
        identity: Option<&IdentityReply>,
    ) -> Result<Option<DeclarativeControllerBinding>, PackageError> {
        self.resolve_declarative_device(DeclarativeMidiDevice {
            endpoint_name,
            product_name: None,
            usb_vendor_id: None,
            usb_product_id: None,
            identity,
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
        let device = DeclarativeMidiDevice {
            endpoint_name,
            ..device
        };
        let identifies = |installed: &InstalledController| {
            installed
                .package
                .manifest()
                .devices
                .iter()
                .any(|matcher| matcher.identifies(device))
        };
        let mut matches = self
            .list()?
            .into_iter()
            .filter(|installed| {
                installed.record.enabled && installed.package.manifest().is_declarative()
            })
            .filter(|installed| {
                installed
                    .package
                    .manifest()
                    .devices
                    .iter()
                    .any(|matcher| matcher.matches_declarative_device(device))
            })
            .collect::<Vec<_>>();
        // Several packages claim the port name: the one that knows the
        // device's Identity Reply is the one it belongs to.
        if matches.len() > 1 && matches.iter().any(identifies) {
            matches.retain(identifies);
        }
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
            let profile = manifest.profile();
            DeclarativeControllerBinding {
                controller_id: manifest.id.clone(),
                controller_name: manifest.name.clone(),
                endpoint_name: endpoint_name.into(),
                host_controls: profile.host_controls,
                host_actions: profile.host_actions,
                semantic_profile: profile.semantic_profile,
                identified: identifies(&installed),
                output_state: installed.output_state(),
                on_connect: installed.on_connect_messages(),
                setup_output: manifest
                    .devices
                    .iter()
                    .find(|matcher| matcher.matches_declarative_device(device))
                    .and_then(DeviceMatcher::setup_output)
                    .cloned(),
                setup_messages: installed.setup_messages(),
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
            vendor: None,
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
                sysex_identity: None,
            }],
            inputs: Vec::new(),
            roles: Vec::new(),
            actions: Vec::new(),
            source_id: None,
            on_connect: Vec::new(),
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
            vendor: None,
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
                sysex_identity: None,
            }],
            inputs: Vec::new(),
            roles: Vec::new(),
            actions: Vec::new(),
            source_id: None,
            on_connect: Vec::new(),
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
                    identity: None,
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

    const SCHEMA_2: &str = r#"
schema_version = 2
kind = "controller"
id = "user.m-audio-oxygen-49"
name = "Oxygen 49"
vendor = "M-Audio"
version = "1.0.0"
controller_api = "^1.0"

[[devices]]
id = "oxygen-49"

[[devices.endpoints]]
role = "performance_input"
name_contains = ["oxygen 49"]

[devices.sysex_identity]
manufacturer = [0x00, 0x01, 0x05]
family = 0x27
model = 1

[[inputs]]
id = "knob-1"
name = "Knob 1"
kind = "knob"
group = "Knobs"
midi = { cc = 74, channel = 0 }

[[inputs]]
id = "button-1"
name = "Button 1"
kind = "button"
midi = { cc = 119, channel = 0 }
button = { press = 127, release = 0 }

[[inputs]]
id = "pad-1"
name = "Pad 1"
kind = "pad"
midi = { note = 36, channel = 9 }

[[roles]]
input = "knob-1"
role = "synth.filter.cutoff"

[[actions]]
input = "button-1"
target = "keyboard_parts"
"#;

    #[test]
    fn a_schema_2_package_is_data_its_hosts_already_run() {
        let manifest: ControllerPackageManifest = toml::from_str(SCHEMA_2).unwrap();
        manifest.validate().unwrap();
        // Left out, the runtime is declarative and the package only listens.
        assert!(manifest.is_declarative());
        assert!(!manifest.permissions.midi_output && manifest.permissions.midi_input);
        assert_eq!(manifest.inputs.len(), 3);

        let profile = manifest.profile();
        let semantic = profile.semantic_profile.as_ref().unwrap();
        assert_eq!(semantic.source_id, "controller.user.m-audio-oxygen-49");
        assert_eq!(semantic.controls[0].midi_cc.controller, 74);
        assert_eq!(profile.host_actions.len(), 1);
        assert_eq!(
            profile.host_actions[0].reserved_control_change(),
            Some((0, 119))
        );
        assert!(matches!(
            profile.declarative_input(&[0xb0, 119, 127]),
            Some(rackforge_controller_api::DeclarativeControllerInput::HostAction { .. })
        ));
    }

    #[test]
    fn a_schema_2_package_survives_stamping() {
        let stamped = stamp_bundled_manifest(SCHEMA_2, &[]).unwrap();
        let manifest: ControllerPackageManifest = toml::from_str(&stamped).unwrap();
        manifest.validate().unwrap();
        assert_eq!(manifest.inputs.len(), 3);
        assert_eq!(manifest.roles.len(), 1);
    }

    #[test]
    fn named_inputs_alone_make_a_package() {
        let mut manifest: ControllerPackageManifest = toml::from_str(SCHEMA_2).unwrap();
        manifest.roles.clear();
        manifest.actions.clear();
        manifest.validate().unwrap();
        manifest.inputs.clear();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn each_schema_keeps_to_its_own_fields() {
        let mut schema_2: ControllerPackageManifest = toml::from_str(SCHEMA_2).unwrap();
        schema_2.host_controls.push(HostControlBinding {
            target: HostControlTarget::MasterLevel,
            midi_cc: MidiControlChangeBinding {
                channel: 0,
                controller: 7,
            },
        });
        assert!(schema_2.validate().is_err());

        let mut schema_1 = declarative_manifest("org.rackforge.generic-midi", "generic midi");
        schema_1.vendor = Some("Example".into());
        assert!(schema_1.validate().is_err());

        let mut future: ControllerPackageManifest = toml::from_str(SCHEMA_2).unwrap();
        future.schema_version = 3;
        assert!(future.validate().is_err());

        schema_1 = declarative_manifest("org.rackforge.generic-midi", "generic midi");
        schema_1.on_connect.push(OnConnectMessage {
            message: "B0 7F 00".into(),
            to: OnConnectPort::Input,
        });
        schema_1.permissions.midi_output = true;
        assert!(schema_1.validate().is_err());
    }

    fn sending(messages: &[&str]) -> ControllerPackageManifest {
        let mut manifest: ControllerPackageManifest = toml::from_str(SCHEMA_2).unwrap();
        manifest.on_connect = messages
            .iter()
            .map(|message| OnConnectMessage {
                message: (*message).into(),
                to: OnConnectPort::Input,
            })
            .collect();
        manifest
    }

    #[test]
    fn a_package_asks_for_exactly_the_output_its_messages_need() {
        // Sends, but asks for nothing.
        let mut manifest = sending(&["B0 7F 00"]);
        assert!(manifest.validate().is_err());
        manifest.permissions.midi_output = true;
        manifest.validate().unwrap();
        // Asks for SysEx it never sends.
        manifest.permissions.sysex = true;
        assert!(manifest.validate().is_err());

        let mut manifest = sending(&["F0 00 20 6B 7F 42 02 00 40 50 01 F7"]);
        manifest.permissions.midi_output = true;
        assert!(manifest.validate().is_err());
        manifest.permissions.sysex = true;
        manifest.validate().unwrap();

        // Asks for output with nothing to send.
        let mut manifest = sending(&[]);
        manifest.permissions.midi_output = true;
        assert!(manifest.validate().is_err());

        let mut manifest = sending(&["F0 00 20 F7", "F8"]);
        manifest.permissions.midi_output = true;
        manifest.permissions.sysex = true;
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn nothing_is_sent_until_the_player_allows_this_version() {
        let sources = TestDirectory::new("output-sources");
        let store_root = TestDirectory::new("output-store");
        let store = PackageStore::new(&store_root.0);
        let install = |manifest: &ControllerPackageManifest, trust| {
            let root = sources.0.join(&manifest.version);
            fs::create_dir_all(&root).unwrap();
            fs::write(
                root.join(CONTROLLER_MANIFEST_FILE),
                toml::to_string_pretty(manifest).unwrap(),
            )
            .unwrap();
            store.install_directory(root, trust).unwrap()
        };

        let mut manifest = sending(&["F0 7D 01 F7"]);
        manifest.permissions.midi_output = true;
        manifest.permissions.sysex = true;
        let installed = install(&manifest, PackageTrust::Local);
        assert_eq!(installed.output_state(), OutputState::Asked);
        let binding = store
            .resolve_declarative_input("Oxygen 49 MIDI 1")
            .unwrap()
            .unwrap();
        assert_eq!(binding.output_state, OutputState::Asked);
        assert!(binding.on_connect.is_empty());

        store.allow_output(&manifest.id, true).unwrap();
        let binding = store
            .resolve_declarative_input("Oxygen 49 MIDI 1")
            .unwrap()
            .unwrap();
        assert_eq!(binding.output_state, OutputState::Allowed);
        assert_eq!(binding.on_connect, vec![vec![0xf0, 0x7d, 0x01, 0xf7]]);

        // Installing the same version again keeps the answer; a new version
        // asks again.
        assert_eq!(
            install(&manifest, PackageTrust::Local).output_state(),
            OutputState::Allowed
        );
        manifest.version = "1.1.0".into();
        manifest.on_connect[0].message = "F0 7D 02 F7".into();
        assert_eq!(
            install(&manifest, PackageTrust::Local).output_state(),
            OutputState::Asked
        );

        // RackForge's own packages are allowed as installed.
        manifest.version = "1.2.0".into();
        assert_eq!(
            install(&manifest, PackageTrust::Official).output_state(),
            OutputState::Allowed
        );
        store.allow_output(&manifest.id, false).unwrap();
    }

    /// A Launch Control XL 3 changes mode only through its DAW port: the
    /// package names that port as its setup output, and the messages for it
    /// go there, not to the port its controls send on.
    #[test]
    fn setup_messages_go_to_the_setup_output_the_package_names() {
        let sources = TestDirectory::new("setup-sources");
        let store_root = TestDirectory::new("setup-store");
        let store = PackageStore::new(&store_root.0);

        let mut manifest = sending(&["9F 0B 7F", "B6 1E 1D", "9F 0B 00"]);
        for message in &mut manifest.on_connect {
            message.to = OnConnectPort::SetupOutput;
        }
        manifest.permissions.midi_output = true;
        // Messages for a setup output the device does not declare.
        assert!(manifest.validate().is_err());
        manifest.devices[0].endpoints.push(EndpointMatcher {
            role: EndpointRole::SetupOutput,
            name_contains: vec!["oxygen 49".into()],
            name_contains_any: vec!["midi 2".into()],
            name_ends_with: None,
            exclude_contains: Vec::new(),
        });
        manifest.validate().unwrap();
        // A setup output with nothing to send to it.
        let mut idle = manifest.clone();
        idle.on_connect.clear();
        idle.permissions.midi_output = false;
        assert!(idle.validate().is_err());

        let root = sources.0.join("package");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(CONTROLLER_MANIFEST_FILE),
            toml::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        store
            .install_directory(root, PackageTrust::Official)
            .unwrap();
        let binding = store
            .resolve_declarative_input("Oxygen 49 MIDI 1")
            .unwrap()
            .unwrap();
        assert!(binding.on_connect.is_empty());
        assert_eq!(
            binding.setup_messages,
            vec![
                vec![0x9f, 0x0b, 0x7f],
                vec![0xb6, 0x1e, 0x1d],
                vec![0x9f, 0x0b, 0x00]
            ]
        );
        let setup = binding.setup_output.as_ref().unwrap();
        // Two units, told apart by their device ID; the input's own port is
        // not its setup output.
        let outputs = [
            "Oxygen 49 MIDI 1".to_owned(),
            "Oxygen 49 MIDI 2".to_owned(),
            "Oxygen 49 #2 MIDI 2".to_owned(),
        ];
        assert_eq!(
            setup_output_port(setup, "Oxygen 49 MIDI 1", &outputs).as_deref(),
            Some("Oxygen 49 MIDI 2")
        );
        assert_eq!(
            setup_output_port(setup, "Oxygen 49 #2 MIDI 1", &outputs).as_deref(),
            Some("Oxygen 49 #2 MIDI 2")
        );
        assert_eq!(setup_output_port(setup, "Oxygen 49 MIDI 1", &[]), None);

        // Windows names the second port after the whole first one.
        let windows = EndpointMatcher {
            role: EndpointRole::SetupOutput,
            name_contains: vec!["lcxl3".into()],
            name_contains_any: vec!["midiout2".into(), "port 2".into(), "daw".into()],
            name_ends_with: None,
            exclude_contains: Vec::new(),
        };
        let outputs = [
            "LCXL3 1 MIDI".to_owned(),
            "MIDIOUT2 (LCXL3 1 MIDI)".to_owned(),
            "LCXL3 2 MIDI".to_owned(),
            "MIDIOUT2 (LCXL3 2 MIDI)".to_owned(),
        ];
        assert_eq!(
            setup_output_port(&windows, "LCXL3 2 MIDI", &outputs).as_deref(),
            Some("MIDIOUT2 (LCXL3 2 MIDI)")
        );
        let mac = ["LCXL3 1 (DAW In)".to_owned(), "LCXL3 2 (DAW In)".to_owned()];
        assert_eq!(
            setup_output_port(&windows, "LCXL3 1 (MIDI Out)", &mac).as_deref(),
            Some("LCXL3 1 (DAW In)")
        );
    }

    #[test]
    fn an_identity_reply_tells_apart_models_that_share_a_port_name() {
        let sources = TestDirectory::new("identity-sources");
        let store_root = TestDirectory::new("identity-store");
        let store = PackageStore::new(&store_root.0);
        let install = |manifest: &ControllerPackageManifest| {
            let root = sources.0.join(&manifest.id);
            fs::create_dir_all(&root).unwrap();
            fs::write(
                root.join(CONTROLLER_MANIFEST_FILE),
                toml::to_string_pretty(manifest).unwrap(),
            )
            .unwrap();
            store
                .install_directory(root, PackageTrust::Community)
                .unwrap();
        };
        // The Oxygen 49 of SCHEMA_2, family 0x27 model 1, and a second
        // generation under the same port name, model 2.
        let first: ControllerPackageManifest = toml::from_str(SCHEMA_2).unwrap();
        let mut second = first.clone();
        second.id = "user.m-audio-oxygen-49-mk2".into();
        second.devices[0].sysex_identity.as_mut().unwrap().model = 2;
        install(&first);
        install(&second);

        let reply = |model: u8| {
            IdentityReply::parse(&[
                0xf0, 0x7e, 0x7f, 0x06, 0x02, 0x00, 0x01, 0x05, 0x27, 0x00, model, 0x00, 0x01,
                0x00, 0x00, 0x00, 0xf7,
            ])
            .unwrap()
        };
        let port = "Oxygen 49:Oxygen 49 MIDI 1 28:0";
        // Without an answer, the name alone cannot choose.
        assert!(matches!(
            store.resolve_declarative_input(port),
            Err(PackageError::AmbiguousDeviceMatch { .. })
        ));
        let binding = store
            .resolve_identified_input(port, Some(&reply(2)))
            .unwrap()
            .unwrap();
        assert_eq!(binding.controller_id, "user.m-audio-oxygen-49-mk2");
        assert!(binding.identified);
        let binding = store
            .resolve_identified_input(port, Some(&reply(1)))
            .unwrap()
            .unwrap();
        assert_eq!(binding.controller_id, "user.m-audio-oxygen-49");

        // A third model neither package knows is neither of them.
        assert!(
            store
                .resolve_identified_input(port, Some(&reply(3)))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_schema_1_package_shows_the_messages_it_binds_as_inputs() {
        let mut schema_1 = declarative_manifest("org.rackforge.generic-midi", "generic midi");
        schema_1
            .host_actions
            .push(HostActionBinding::control_change(
                rackforge_controller_api::HostActionTarget::KeyboardParts,
                rackforge_controller_api::MidiButtonBinding {
                    channel: 0,
                    controller: 119,
                    press_value: 127,
                    release_value: 0,
                },
            ));
        let editor = schema_1.editor_inputs();
        // The master fader it reserves, and the part key.
        assert_eq!(editor.inputs.len(), 2);
        assert!(editor.inputs.iter().any(|input| input.id == "cc-1-7"));
        assert_eq!(editor.actions[0].input, "cc-1-119");
        let part = editor
            .inputs
            .iter()
            .find(|input| input.id == "cc-1-119")
            .unwrap();
        assert_eq!(part.kind, InputKind::Button);

        let schema_2: ControllerPackageManifest = toml::from_str(SCHEMA_2).unwrap();
        assert_eq!(schema_2.editor_inputs().inputs, schema_2.inputs);
    }

    #[test]
    fn the_generic_example_is_a_valid_package() {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/controllers/generic-midi");
        let package = ControllerPackage::open(&root).unwrap();
        assert!(package.manifest().is_declarative());
    }
}
