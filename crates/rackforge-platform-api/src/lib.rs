use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use thiserror::Error;
use zeroize::Zeroize;

pub const PLATFORM_SCHEMA_VERSION: u32 = 1;
pub const PLATFORM_CONTROL_SCHEMA_VERSION: u32 = 1;
pub const PLATFORM_CONTROL_SOCKET_NAME: &str = "platform-control.sock";

pub const WIFI_MANAGE_V1: &str = "system.network.wifi.manage.v1";
pub const FAST_AUDIO_BOOT_V1: &str = "system.boot.fast-audio.v1";
pub const SYSTEM_TELEMETRY_V1: &str = "system.telemetry.v1";

pub const NETWORK_MANAGER_PROVIDER: &str = "org.rackforge.provider.network-manager";
pub const SYSTEMD_PROVIDER: &str = "org.rackforge.provider.systemd";
pub const RASPBERRY_PI_PROVIDER: &str = "org.rackforge.provider.raspberry-pi";

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, PlatformError> {
                let value = value.into();
                validate_identifier(&value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

string_id!(HardwareProfileId);
string_id!(PlatformCapabilityId);
string_id!(PlatformProviderId);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WifiConnectionId(String);

impl WifiConnectionId {
    pub fn new(value: impl Into<String>) -> Result<Self, PlatformError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        {
            return Err(PlatformError::InvalidWifiConnectionId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WifiConnectionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WifiSsid(String);

impl WifiSsid {
    pub fn new(value: impl Into<String>) -> Result<Self, PlatformError> {
        let value = value.into();
        validate_wifi_ssid(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WifiSsid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WifiPassphrase(String);

impl WifiPassphrase {
    pub fn new(value: impl Into<String>) -> Result<Self, PlatformError> {
        let value = value.into();
        validate_wifi_passphrase(&value)?;
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for WifiPassphrase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WifiPassphrase([REDACTED])")
    }
}

impl Drop for WifiPassphrase {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BinaryPlatform {
    LinuxAarch64,
    LinuxX86_64,
    WindowsX86_64,
}

impl BinaryPlatform {
    pub const fn key(self) -> &'static str {
        match self {
            Self::LinuxAarch64 => "linux-aarch64",
            Self::LinuxX86_64 => "linux-x86_64",
            Self::WindowsX86_64 => "windows-x86_64",
        }
    }

    pub fn current() -> Result<Self, PlatformError> {
        if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            Ok(Self::LinuxAarch64)
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Ok(Self::LinuxX86_64)
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Ok(Self::WindowsX86_64)
        } else {
            Err(PlatformError::UnsupportedBinaryPlatform {
                os: std::env::consts::OS,
                architecture: std::env::consts::ARCH,
            })
        }
    }
}

impl fmt::Display for BinaryPlatform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformCapabilityBinding {
    pub capability: PlatformCapabilityId,
    pub provider: PlatformProviderId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformIdentity {
    pub schema_version: u32,
    pub binary_platform: BinaryPlatform,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_profile: Option<HardwareProfileId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_model: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<PlatformCapabilityBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformControlRequest {
    pub schema_version: u32,
    pub request_id: String,
    pub operation: PlatformOperation,
}

impl PlatformControlRequest {
    pub fn validate(&self) -> Result<(), PlatformError> {
        if self.schema_version != PLATFORM_CONTROL_SCHEMA_VERSION {
            return Err(PlatformError::UnsupportedControlSchema(self.schema_version));
        }
        validate_request_id(&self.request_id)?;
        match &self.operation {
            PlatformOperation::ActivateSavedWifi { connection_id }
            | PlatformOperation::ForgetSavedWifi { connection_id } => {
                WifiConnectionId::new(connection_id.as_str())?;
            }
            PlatformOperation::ConnectVisibleWifi { ssid, passphrase } => {
                validate_wifi_ssid(ssid.as_str())?;
                if let Some(passphrase) = passphrase {
                    validate_wifi_passphrase(passphrase.expose())?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlatformOperation {
    GetIdentity,
    GetWifiStatus,
    ListSavedWifi,
    ScanWifi,
    ActivateSavedWifi {
        connection_id: WifiConnectionId,
    },
    ForgetSavedWifi {
        connection_id: WifiConnectionId,
    },
    ConnectVisibleWifi {
        ssid: WifiSsid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        passphrase: Option<WifiPassphrase>,
    },
    DisconnectWifi,
    SetWifiEnabled {
        enabled: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WifiStatus {
    pub interface: String,
    pub enabled: bool,
    pub connected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<WifiConnectionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_percent: Option<u8>,
    #[serde(default)]
    pub ipv4_addresses: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedWifiConnection {
    pub id: WifiConnectionId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    pub active: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisibleWifiNetwork {
    pub ssid: String,
    pub signal_percent: u8,
    pub secured: bool,
    pub saved: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", content = "value", rename_all = "snake_case")]
pub enum PlatformControlPayload {
    Identity(PlatformIdentity),
    WifiStatus(WifiStatus),
    SavedWifi(Vec<SavedWifiConnection>),
    VisibleWifi(Vec<VisibleWifiNetwork>),
    Changed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PlatformControlResponse {
    Ok {
        schema_version: u32,
        request_id: String,
        payload: PlatformControlPayload,
    },
    Error {
        schema_version: u32,
        request_id: String,
        code: PlatformErrorCode,
        message: String,
    },
}

impl PlatformControlResponse {
    pub fn success(request_id: impl Into<String>, payload: PlatformControlPayload) -> Self {
        Self::Ok {
            schema_version: PLATFORM_CONTROL_SCHEMA_VERSION,
            request_id: request_id.into(),
            payload,
        }
    }

    pub fn error(
        request_id: impl Into<String>,
        code: PlatformErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self::Error {
            schema_version: PLATFORM_CONTROL_SCHEMA_VERSION,
            request_id: request_id.into(),
            code,
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformErrorCode {
    InvalidRequest,
    CapabilityUnavailable,
    ProviderFailure,
    NotFound,
    Busy,
    AuthenticationRequired,
}

impl PlatformIdentity {
    pub fn generic(binary_platform: BinaryPlatform) -> Self {
        Self {
            schema_version: PLATFORM_SCHEMA_VERSION,
            binary_platform,
            hardware_profile: None,
            hardware_model: None,
            capabilities: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), PlatformError> {
        if self.schema_version != PLATFORM_SCHEMA_VERSION {
            return Err(PlatformError::UnsupportedSchema(self.schema_version));
        }
        if self
            .hardware_model
            .as_ref()
            .is_some_and(|model| model.trim().is_empty())
        {
            return Err(PlatformError::EmptyHardwareModel);
        }
        let mut capabilities = BTreeSet::new();
        for binding in &self.capabilities {
            if !capabilities.insert(binding.capability.as_str()) {
                return Err(PlatformError::DuplicateCapability(
                    binding.capability.to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn binding(&self, capability: &str) -> Option<&PlatformCapabilityBinding> {
        self.capabilities
            .iter()
            .find(|binding| binding.capability.as_str() == capability)
    }

    pub fn supports(&self, capability: &str) -> bool {
        self.binding(capability).is_some()
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum PlatformError {
    #[error("unsupported platform schema version {0}")]
    UnsupportedSchema(u32),
    #[error("unsupported platform control schema version {0}")]
    UnsupportedControlSchema(u32),
    #[error("invalid platform identifier {0:?}")]
    InvalidIdentifier(String),
    #[error("unsupported binary platform {os}-{architecture}")]
    UnsupportedBinaryPlatform {
        os: &'static str,
        architecture: &'static str,
    },
    #[error("hardware model cannot be blank")]
    EmptyHardwareModel,
    #[error("platform capability {0:?} is declared more than once")]
    DuplicateCapability(String),
    #[error("invalid platform request id {0:?}")]
    InvalidRequestId(String),
    #[error("invalid Wi-Fi connection id {0:?}")]
    InvalidWifiConnectionId(String),
    #[error("invalid Wi-Fi SSID")]
    InvalidWifiSsid,
    #[error("invalid Wi-Fi passphrase")]
    InvalidWifiPassphrase,
}

fn validate_identifier(value: &str) -> Result<(), PlatformError> {
    if value.is_empty()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        })
    {
        return Err(PlatformError::InvalidIdentifier(value.into()));
    }
    Ok(())
}

fn validate_request_id(value: &str) -> Result<(), PlatformError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    {
        return Err(PlatformError::InvalidRequestId(value.into()));
    }
    Ok(())
}

fn validate_wifi_ssid(value: &str) -> Result<(), PlatformError> {
    if value.is_empty()
        || value.len() > 32
        || value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
    {
        return Err(PlatformError::InvalidWifiSsid);
    }
    Ok(())
}

fn validate_wifi_passphrase(value: &str) -> Result<(), PlatformError> {
    if value.is_empty()
        || value.len() > 128
        || value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
    {
        return Err(PlatformError::InvalidWifiPassphrase);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(capability: &str, provider: &str) -> PlatformCapabilityBinding {
        PlatformCapabilityBinding {
            capability: PlatformCapabilityId::new(capability).unwrap(),
            provider: PlatformProviderId::new(provider).unwrap(),
        }
    }

    #[test]
    fn binary_platform_is_separate_from_hardware_profile() {
        let identity = PlatformIdentity {
            schema_version: PLATFORM_SCHEMA_VERSION,
            binary_platform: BinaryPlatform::LinuxAarch64,
            hardware_profile: Some(
                HardwareProfileId::new("org.rackforge.hardware.raspberry-pi-4").unwrap(),
            ),
            hardware_model: Some("Raspberry Pi 4 Model B Rev 1.4".into()),
            capabilities: vec![binding(FAST_AUDIO_BOOT_V1, SYSTEMD_PROVIDER)],
        };
        assert_eq!(identity.binary_platform.key(), "linux-aarch64");
        assert_eq!(
            identity.hardware_profile.unwrap().as_str(),
            "org.rackforge.hardware.raspberry-pi-4"
        );
    }

    #[test]
    fn capabilities_resolve_to_allowlisted_providers() {
        let identity = PlatformIdentity {
            schema_version: PLATFORM_SCHEMA_VERSION,
            binary_platform: BinaryPlatform::LinuxAarch64,
            hardware_profile: None,
            hardware_model: None,
            capabilities: vec![binding(WIFI_MANAGE_V1, NETWORK_MANAGER_PROVIDER)],
        };
        assert!(identity.supports(WIFI_MANAGE_V1));
        assert_eq!(
            identity.binding(WIFI_MANAGE_V1).unwrap().provider.as_str(),
            NETWORK_MANAGER_PROVIDER
        );
        assert_eq!(identity.validate(), Ok(()));
    }

    #[test]
    fn duplicate_capability_is_rejected_even_with_another_provider() {
        let identity = PlatformIdentity {
            schema_version: PLATFORM_SCHEMA_VERSION,
            binary_platform: BinaryPlatform::LinuxAarch64,
            hardware_profile: None,
            hardware_model: None,
            capabilities: vec![
                binding(WIFI_MANAGE_V1, NETWORK_MANAGER_PROVIDER),
                binding(WIFI_MANAGE_V1, "org.rackforge.provider.other"),
            ],
        };
        assert_eq!(
            identity.validate(),
            Err(PlatformError::DuplicateCapability(WIFI_MANAGE_V1.into()))
        );
    }

    #[test]
    fn serialized_identity_is_versioned_and_human_readable() {
        let identity = PlatformIdentity::generic(BinaryPlatform::LinuxX86_64);
        let json = serde_json::to_value(identity).unwrap();
        assert_eq!(json["schema_version"], PLATFORM_SCHEMA_VERSION);
        assert_eq!(json["binary_platform"], "linux-x86-64");
    }

    #[test]
    fn control_requests_are_versioned_and_do_not_accept_arbitrary_commands() {
        let request = PlatformControlRequest {
            schema_version: PLATFORM_CONTROL_SCHEMA_VERSION,
            request_id: "display-42".into(),
            operation: PlatformOperation::ActivateSavedWifi {
                connection_id: WifiConnectionId::new("a7c1550e-e62c-3985-b50a-82489b8027cb")
                    .unwrap(),
            },
        };
        assert_eq!(request.validate(), Ok(()));
        let json = serde_json::to_value(request).unwrap();
        assert_eq!(json["operation"]["op"], "activate_saved_wifi");
        assert!(json["operation"].get("command").is_none());
    }

    #[test]
    fn visible_wifi_credentials_are_typed_and_debug_redacted() {
        let passphrase = WifiPassphrase::new("stage-secret").unwrap();
        let request = PlatformControlRequest {
            schema_version: PLATFORM_CONTROL_SCHEMA_VERSION,
            request_id: "display-43".into(),
            operation: PlatformOperation::ConnectVisibleWifi {
                ssid: WifiSsid::new("PHONE HOTSPOT").unwrap(),
                passphrase: Some(passphrase),
            },
        };
        assert_eq!(request.validate(), Ok(()));
        assert!(!format!("{request:?}").contains("stage-secret"));
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["operation"]["op"], "connect_visible_wifi");
        assert_eq!(json["operation"]["ssid"], "PHONE HOTSPOT");
        assert_eq!(json["operation"]["passphrase"], "stage-secret");
    }

    #[test]
    fn deserialized_wifi_values_are_revalidated() {
        let request: PlatformControlRequest = serde_json::from_value(serde_json::json!({
            "schema_version": PLATFORM_CONTROL_SCHEMA_VERSION,
            "request_id": "display-44",
            "operation": {
                "op": "connect_visible_wifi",
                "ssid": "bad\nssid",
                "passphrase": "secret"
            }
        }))
        .unwrap();
        assert_eq!(request.validate(), Err(PlatformError::InvalidWifiSsid));
    }
}
