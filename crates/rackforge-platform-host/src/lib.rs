#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

use anyhow::{Context, Result};
use rackforge_platform_api::{
    BinaryPlatform, FAST_AUDIO_BOOT_V1, HardwareProfileId, NETWORK_MANAGER_PROVIDER,
    PLATFORM_SCHEMA_VERSION, PlatformCapabilityBinding, PlatformCapabilityId,
    PlatformControlPayload, PlatformControlRequest, PlatformControlResponse, PlatformErrorCode,
    PlatformIdentity, PlatformOperation, PlatformProviderId, RASPBERRY_PI_PROVIDER,
    SYSTEM_TELEMETRY_V1, SYSTEMD_PROVIDER, SavedWifiConnection, VisibleWifiNetwork, WIFI_MANAGE_V1,
    WifiConnectionId, WifiPassphrase, WifiSsid, WifiStatus,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const DEVICE_TREE_MODEL: &str = "/sys/firmware/devicetree/base/model";
static NEXT_SECRET_FILE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnownHardwareProfile {
    RaspberryPi4,
    RaspberryPi5,
}

impl KnownHardwareProfile {
    pub const fn id(self) -> &'static str {
        match self {
            Self::RaspberryPi4 => "org.rackforge.hardware.raspberry-pi-4",
            Self::RaspberryPi5 => "org.rackforge.hardware.raspberry-pi-5",
        }
    }
}

pub fn detect_known_hardware(model: &str) -> Option<KnownHardwareProfile> {
    let model = model.trim_end_matches('\0').trim();
    if model.starts_with("Raspberry Pi 4 Model ") {
        Some(KnownHardwareProfile::RaspberryPi4)
    } else if model.starts_with("Raspberry Pi 5 Model ") {
        Some(KnownHardwareProfile::RaspberryPi5)
    } else {
        None
    }
}

pub fn detect_platform() -> Result<PlatformIdentity> {
    let binary_platform = BinaryPlatform::current()?;
    let hardware_model = read_trimmed(DEVICE_TREE_MODEL).ok();
    let known_hardware = hardware_model.as_deref().and_then(detect_known_hardware);
    let mut capabilities = Vec::new();

    if known_hardware.is_some() {
        capabilities.push(binding(FAST_AUDIO_BOOT_V1, SYSTEMD_PROVIDER)?);
        capabilities.push(binding(SYSTEM_TELEMETRY_V1, RASPBERRY_PI_PROVIDER)?);
    }
    if network_manager_is_available() && wireless_interface_exists(Path::new("/sys/class/net")) {
        capabilities.push(binding(WIFI_MANAGE_V1, NETWORK_MANAGER_PROVIDER)?);
    }

    let identity = PlatformIdentity {
        schema_version: PLATFORM_SCHEMA_VERSION,
        binary_platform,
        hardware_profile: known_hardware
            .map(KnownHardwareProfile::id)
            .map(HardwareProfileId::new)
            .transpose()?,
        hardware_model,
        capabilities,
    };
    identity.validate()?;
    Ok(identity)
}

#[cfg(unix)]
pub fn serve(socket: &Path) -> Result<()> {
    let identity = detect_platform()?;
    if let Ok(metadata) = fs::symlink_metadata(socket) {
        if !metadata.file_type().is_socket() {
            anyhow::bail!(
                "refusing to replace non-socket platform control path {}",
                socket.display()
            );
        }
        fs::remove_file(socket)
            .with_context(|| format!("removing stale socket {}", socket.display()))?;
    }
    if let Some(parent) = socket.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating platform socket directory {}", parent.display()))?;
    }
    let listener =
        UnixListener::bind(socket).with_context(|| format!("binding {}", socket.display()))?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o660))
        .with_context(|| format!("setting permissions on {}", socket.display()))?;
    println!(
        "PLATFORM_HOST_READY profile={} socket={}",
        identity
            .hardware_profile
            .as_ref()
            .map_or("generic", HardwareProfileId::as_str),
        socket.display()
    );

    for connection in listener.incoming() {
        let Ok(mut stream) = connection else {
            continue;
        };
        let response = handle_stream(&mut stream, &identity);
        let _ = serde_json::to_writer(&mut stream, &response);
        let _ = stream.write_all(b"\n");
        let _ = stream.flush();
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn serve(_socket: &Path) -> Result<()> {
    anyhow::bail!("platform control service currently requires Unix sockets")
}

fn handle_stream(stream: &mut impl Read, identity: &PlatformIdentity) -> PlatformControlResponse {
    let mut line = String::new();
    let read = BufReader::new(stream).take(16 * 1024).read_line(&mut line);
    let request = match read {
        Ok(0) => {
            return PlatformControlResponse::error(
                "unknown",
                PlatformErrorCode::InvalidRequest,
                "empty platform request",
            );
        }
        Ok(_) => match serde_json::from_str::<PlatformControlRequest>(&line) {
            Ok(request) => request,
            Err(error) => {
                return PlatformControlResponse::error(
                    "unknown",
                    PlatformErrorCode::InvalidRequest,
                    format!("invalid platform request: {error}"),
                );
            }
        },
        Err(error) => {
            return PlatformControlResponse::error(
                "unknown",
                PlatformErrorCode::InvalidRequest,
                format!("could not read platform request: {error}"),
            );
        }
    };
    if let Err(error) = request.validate() {
        return PlatformControlResponse::error(
            request.request_id,
            PlatformErrorCode::InvalidRequest,
            error.to_string(),
        );
    }
    dispatch_request(identity, request)
}

fn dispatch_request(
    identity: &PlatformIdentity,
    request: PlatformControlRequest,
) -> PlatformControlResponse {
    let result = match request.operation {
        PlatformOperation::GetIdentity => Ok(PlatformControlPayload::Identity(identity.clone())),
        PlatformOperation::GetWifiStatus => wifi_provider(identity)
            .and_then(NetworkManagerWifi::status)
            .map(PlatformControlPayload::WifiStatus),
        PlatformOperation::ListSavedWifi => wifi_provider(identity)
            .and_then(NetworkManagerWifi::saved_connections)
            .map(PlatformControlPayload::SavedWifi),
        PlatformOperation::ScanWifi => wifi_provider(identity)
            .and_then(NetworkManagerWifi::scan)
            .map(PlatformControlPayload::VisibleWifi),
        PlatformOperation::ActivateSavedWifi { connection_id } => wifi_provider(identity)
            .and_then(|provider| provider.activate_saved(&connection_id))
            .map(|_| PlatformControlPayload::Changed),
        PlatformOperation::ForgetSavedWifi { connection_id } => wifi_provider(identity)
            .and_then(|provider| provider.forget_saved(&connection_id))
            .map(|_| PlatformControlPayload::Changed),
        PlatformOperation::ConnectVisibleWifi { ssid, passphrase } => wifi_provider(identity)
            .and_then(|provider| provider.connect_visible(&ssid, passphrase.as_ref()))
            .map(|_| PlatformControlPayload::Changed),
        PlatformOperation::DisconnectWifi => wifi_provider(identity)
            .and_then(NetworkManagerWifi::disconnect)
            .map(|_| PlatformControlPayload::Changed),
        PlatformOperation::SetWifiEnabled { enabled } => wifi_provider(identity)
            .and_then(|provider| provider.set_enabled(enabled))
            .map(|_| PlatformControlPayload::Changed),
    };
    match result {
        Ok(payload) => PlatformControlResponse::success(request.request_id, payload),
        Err(error) => PlatformControlResponse::error(request.request_id, error.code, error.message),
    }
}

fn wifi_provider(identity: &PlatformIdentity) -> ProviderResult<NetworkManagerWifi> {
    match identity.binding(WIFI_MANAGE_V1) {
        Some(binding) if binding.provider.as_str() == NETWORK_MANAGER_PROVIDER => {
            NetworkManagerWifi::discover()
        }
        _ => Err(ProviderError::new(
            PlatformErrorCode::CapabilityUnavailable,
            "Wi-Fi management is unavailable on this platform",
        )),
    }
}

#[derive(Clone, Debug)]
struct NetworkManagerWifi {
    interface: String,
}

impl NetworkManagerWifi {
    fn discover() -> ProviderResult<Self> {
        let interface = fs::read_dir("/sys/class/net")
            .map_err(|error| {
                ProviderError::failure(format!("reading network interfaces: {error}"))
            })?
            .filter_map(std::result::Result::ok)
            .find(|entry| entry.path().join("wireless").is_dir())
            .and_then(|entry| entry.file_name().into_string().ok())
            .ok_or_else(|| {
                ProviderError::new(
                    PlatformErrorCode::CapabilityUnavailable,
                    "no wireless interface is available",
                )
            })?;
        Ok(Self { interface })
    }

    fn status(self) -> ProviderResult<WifiStatus> {
        let enabled = nmcli(&["-t", "-f", "WIFI", "general"])?.trim() == "enabled";
        let devices = nmcli(&[
            "-t",
            "--escape",
            "yes",
            "-f",
            "DEVICE,TYPE,STATE,CONNECTION",
            "device",
            "status",
        ])?;
        let device = parse_terse_rows(&devices)
            .into_iter()
            .find(|row| row.first().is_some_and(|device| device == &self.interface));
        let connected = device
            .as_ref()
            .and_then(|row| row.get(2))
            .is_some_and(|state| state == "connected");
        let connection_name = connected
            .then(|| device.as_ref().and_then(|row| row.get(3)).cloned())
            .flatten()
            .filter(|name| name != "--");
        let connection_id = if connected {
            active_connection_id(&self.interface)
        } else {
            None
        };
        let ssid = connection_id
            .as_ref()
            .and_then(|id| connection_ssid(id).ok())
            .flatten();
        let signal_percent = if connected {
            active_signal(&self.interface).ok().flatten()
        } else {
            None
        };
        let ipv4_addresses = if connected {
            nmcli(&["-g", "IP4.ADDRESS", "device", "show", &self.interface])
                .map(|output| {
                    output
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(WifiStatus {
            interface: self.interface,
            enabled,
            connected,
            connection_id,
            connection_name,
            ssid,
            signal_percent,
            ipv4_addresses,
        })
    }

    fn saved_connections(self) -> ProviderResult<Vec<SavedWifiConnection>> {
        let active_id = active_connection_id(&self.interface);
        let output = nmcli(&[
            "-t",
            "--escape",
            "yes",
            "-f",
            "NAME,UUID,TYPE",
            "connection",
            "show",
        ])?;
        let mut connections = Vec::new();
        for row in parse_terse_rows(&output) {
            let [name, uuid, kind] = row.as_slice() else {
                continue;
            };
            if kind != "802-11-wireless" && kind != "wifi" {
                continue;
            }
            let Ok(id) = WifiConnectionId::new(uuid) else {
                continue;
            };
            let ssid = connection_ssid(&id).ok().flatten();
            connections.push(SavedWifiConnection {
                active: active_id.as_ref() == Some(&id),
                id,
                name: name.clone(),
                ssid,
            });
        }
        connections.sort_by(|left, right| {
            right
                .active
                .cmp(&left.active)
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(connections)
    }

    fn scan(self) -> ProviderResult<Vec<VisibleWifiNetwork>> {
        let saved = self
            .clone()
            .saved_connections()?
            .into_iter()
            .filter_map(|connection| connection.ssid)
            .collect::<BTreeSet<_>>();
        let output = nmcli(&[
            "-t",
            "--escape",
            "yes",
            "-f",
            "SSID,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
            "--rescan",
            "yes",
            "ifname",
            &self.interface,
        ])?;
        let mut strongest = BTreeMap::<String, VisibleWifiNetwork>::new();
        for row in parse_terse_rows(&output) {
            let [ssid, signal, security] = row.as_slice() else {
                continue;
            };
            if ssid.is_empty() {
                continue;
            }
            let signal_percent = signal.parse::<u8>().unwrap_or(0).min(100);
            let candidate = VisibleWifiNetwork {
                ssid: ssid.clone(),
                signal_percent,
                secured: !security.is_empty() && security != "--",
                saved: saved.contains(ssid),
            };
            strongest
                .entry(ssid.clone())
                .and_modify(|existing| {
                    if candidate.signal_percent > existing.signal_percent {
                        *existing = candidate.clone();
                    }
                })
                .or_insert(candidate);
        }
        let mut networks = strongest.into_values().collect::<Vec<_>>();
        networks.sort_by(|left, right| {
            right
                .saved
                .cmp(&left.saved)
                .then_with(|| right.signal_percent.cmp(&left.signal_percent))
                .then_with(|| left.ssid.cmp(&right.ssid))
        });
        Ok(networks)
    }

    fn activate_saved(self, connection_id: &WifiConnectionId) -> ProviderResult<()> {
        let connection = self
            .clone()
            .saved_connections()?
            .into_iter()
            .find(|connection| &connection.id == connection_id)
            .ok_or_else(|| {
                ProviderError::new(
                    PlatformErrorCode::NotFound,
                    "saved Wi-Fi connection was not found",
                )
            })?;
        if connection.active {
            return Ok(());
        }
        nmcli(&[
            "--wait",
            "15",
            "connection",
            "up",
            "uuid",
            connection_id.as_str(),
            "ifname",
            &self.interface,
        ])
        .map(|_| ())
    }

    fn forget_saved(self, connection_id: &WifiConnectionId) -> ProviderResult<()> {
        let exists = self
            .clone()
            .saved_connections()?
            .iter()
            .any(|connection| &connection.id == connection_id);
        if !exists {
            return Err(ProviderError::new(
                PlatformErrorCode::NotFound,
                "saved Wi-Fi connection was not found",
            ));
        }
        nmcli(&["connection", "delete", "uuid", connection_id.as_str()]).map(|_| ())
    }

    fn connect_visible(
        self,
        ssid: &WifiSsid,
        passphrase: Option<&WifiPassphrase>,
    ) -> ProviderResult<()> {
        let network = self
            .clone()
            .scan()?
            .into_iter()
            .find(|network| network.ssid == ssid.as_str())
            .ok_or_else(|| {
                ProviderError::new(
                    PlatformErrorCode::NotFound,
                    "visible Wi-Fi network was not found",
                )
            })?;

        if network.saved
            && let Some(connection) = self
                .clone()
                .saved_connections()?
                .into_iter()
                .find(|connection| connection.ssid.as_deref() == Some(ssid.as_str()))
        {
            return self.activate_saved(&connection.id);
        }

        if network.secured {
            let passphrase = passphrase.ok_or_else(|| {
                ProviderError::new(
                    PlatformErrorCode::AuthenticationRequired,
                    "the Wi-Fi network requires a passphrase",
                )
            })?;
            self.connect_new_secured(ssid, passphrase)?;
        } else {
            nmcli(&[
                "--wait",
                "20",
                "device",
                "wifi",
                "connect",
                ssid.as_str(),
                "ifname",
                &self.interface,
            ])?;
        }
        Ok(())
    }

    fn connect_new_secured(
        &self,
        ssid: &WifiSsid,
        passphrase: &WifiPassphrase,
    ) -> ProviderResult<()> {
        let profile_name = format!(
            "RackForge-{}-{}",
            std::process::id(),
            NEXT_SECRET_FILE.fetch_add(1, Ordering::Relaxed)
        );
        nmcli(&[
            "connection",
            "add",
            "type",
            "wifi",
            "ifname",
            &self.interface,
            "con-name",
            &profile_name,
            "autoconnect",
            "yes",
            "ssid",
            ssid.as_str(),
            "--",
            "wifi-sec.key-mgmt",
            "wpa-psk",
        ])?;
        let uuid = nmcli(&[
            "-g",
            "connection.uuid",
            "connection",
            "show",
            "id",
            &profile_name,
        ])
        .and_then(|value| {
            WifiConnectionId::new(value.trim()).map_err(|error| {
                ProviderError::failure(format!(
                    "NetworkManager created an invalid connection id: {error}"
                ))
            })
        });
        let uuid = match uuid {
            Ok(uuid) => uuid,
            Err(error) => {
                let _ = nmcli(&["connection", "delete", "id", &profile_name]);
                return Err(error);
            }
        };
        let password_file = match NetworkManagerPasswordFile::create(passphrase) {
            Ok(file) => file,
            Err(error) => {
                let _ = nmcli(&["connection", "delete", "uuid", uuid.as_str()]);
                return Err(error);
            }
        };
        let activation = nmcli(&[
            "--wait",
            "20",
            "connection",
            "up",
            "uuid",
            uuid.as_str(),
            "ifname",
            &self.interface,
            "passwd-file",
            password_file.path().to_string_lossy().as_ref(),
        ]);
        drop(password_file);
        if let Err(error) = activation {
            let _ = nmcli(&["connection", "delete", "uuid", uuid.as_str()]);
            return Err(error);
        }
        Ok(())
    }

    fn disconnect(self) -> ProviderResult<()> {
        nmcli(&["device", "disconnect", &self.interface]).map(|_| ())
    }

    fn set_enabled(self, enabled: bool) -> ProviderResult<()> {
        nmcli(&["radio", "wifi", if enabled { "on" } else { "off" }]).map(|_| ())
    }
}

fn active_connection_id(interface: &str) -> Option<WifiConnectionId> {
    nmcli(&["-g", "GENERAL.CON-UUID", "device", "show", interface])
        .ok()
        .and_then(|uuid| WifiConnectionId::new(uuid.trim()).ok())
}

fn connection_ssid(id: &WifiConnectionId) -> ProviderResult<Option<String>> {
    let output = nmcli(&[
        "-g",
        "802-11-wireless.ssid",
        "connection",
        "show",
        "uuid",
        id.as_str(),
    ])?;
    Ok(output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned))
}

fn active_signal(interface: &str) -> ProviderResult<Option<u8>> {
    let output = nmcli(&[
        "-t",
        "--escape",
        "yes",
        "-f",
        "IN-USE,SIGNAL",
        "device",
        "wifi",
        "list",
        "ifname",
        interface,
    ])?;
    Ok(parse_terse_rows(&output).into_iter().find_map(|row| {
        (row.first().is_some_and(|active| active == "*"))
            .then(|| row.get(1)?.parse::<u8>().ok().map(|value| value.min(100)))
            .flatten()
    }))
}

fn nmcli(arguments: &[&str]) -> ProviderResult<String> {
    let output = Command::new("nmcli")
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            ProviderError::failure(format!("starting NetworkManager client: {error}"))
        })?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(ProviderError::failure(format!(
            "NetworkManager operation failed: {}",
            message.trim()
        )));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| ProviderError::failure("NetworkManager returned non-UTF-8 output"))
}

struct NetworkManagerPasswordFile {
    path: std::path::PathBuf,
}

impl NetworkManagerPasswordFile {
    fn create(passphrase: &WifiPassphrase) -> ProviderResult<Self> {
        let sequence = NEXT_SECRET_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::path::PathBuf::from(format!(
            "/run/rackforge/wifi-passwd-{}-{sequence}",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&path).map_err(|error| {
            ProviderError::failure(format!(
                "creating protected NetworkManager password file: {error}"
            ))
        })?;
        file.write_all(b"802-11-wireless-security.psk:")
            .and_then(|_| file.write_all(passphrase.expose().as_bytes()))
            .and_then(|_| file.write_all(b"\n"))
            .map_err(|error| {
                let _ = fs::remove_file(&path);
                ProviderError::failure(format!(
                    "writing protected NetworkManager password file: {error}"
                ))
            })?;
        drop(file);
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for NetworkManagerPasswordFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn parse_terse_rows(output: &str) -> Vec<Vec<String>> {
    output.lines().map(parse_terse_fields).collect()
}

fn parse_terse_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut escaped = false;
    for character in line.chars() {
        if escaped {
            field.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == ':' {
            fields.push(std::mem::take(&mut field));
        } else {
            field.push(character);
        }
    }
    if escaped {
        field.push('\\');
    }
    fields.push(field);
    fields
}

fn binding(capability: &str, provider: &str) -> Result<PlatformCapabilityBinding> {
    Ok(PlatformCapabilityBinding {
        capability: PlatformCapabilityId::new(capability)?,
        provider: PlatformProviderId::new(provider)?,
    })
}

fn read_trimmed(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let text = String::from_utf8(bytes).context("platform model is not UTF-8")?;
    Ok(text.trim_end_matches('\0').trim().to_owned())
}

fn network_manager_is_available() -> bool {
    Command::new("nmcli")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn wireless_interface_exists(root: &Path) -> bool {
    fs::read_dir(root).is_ok_and(|entries| {
        entries
            .filter_map(std::result::Result::ok)
            .any(|entry| entry.path().join("wireless").is_dir())
    })
}

type ProviderResult<T> = std::result::Result<T, ProviderError>;

#[derive(Debug)]
struct ProviderError {
    code: PlatformErrorCode,
    message: String,
}

impl ProviderError {
    fn new(code: PlatformErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn failure(message: impl Into<String>) -> Self {
        Self::new(PlatformErrorCode::ProviderFailure, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_raspberry_pi_models_without_using_revision_numbers() {
        assert_eq!(
            detect_known_hardware("Raspberry Pi 4 Model B Rev 1.4\0"),
            Some(KnownHardwareProfile::RaspberryPi4)
        );
        assert_eq!(
            detect_known_hardware("Raspberry Pi 5 Model B Rev 1.0"),
            Some(KnownHardwareProfile::RaspberryPi5)
        );
        assert_eq!(detect_known_hardware("Generic ARM64 board"), None);
    }

    #[test]
    fn wireless_detection_uses_interface_capability_not_a_fixed_name() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-platform-wireless-{}",
            std::process::id()
        ));
        let wireless = root.join("wl-custom").join("wireless");
        fs::create_dir_all(&wireless).unwrap();
        assert!(wireless_interface_exists(&root));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_nmcli_escaped_fields_without_invoking_a_shell() {
        assert_eq!(
            parse_terse_fields(r"Hotspot\: Stage:uuid-1:802-11-wireless"),
            vec!["Hotspot: Stage", "uuid-1", "802-11-wireless"]
        );
        assert_eq!(
            parse_terse_fields(r"Backslash\\Name:42"),
            vec![r"Backslash\Name", "42"]
        );
    }

    #[test]
    fn malformed_control_request_returns_a_typed_error() {
        let identity = PlatformIdentity::generic(BinaryPlatform::LinuxAarch64);
        let mut input =
            br#"{"schema_version":999,"request_id":"x","operation":{"op":"get_identity"}}
"#
            .as_slice();
        assert!(matches!(
            handle_stream(&mut input, &identity),
            PlatformControlResponse::Error {
                code: PlatformErrorCode::InvalidRequest,
                ..
            }
        ));
    }
}
