//! Software information and signed-release checking, shared by Web and LITTLE.
use serde::{Deserialize, Serialize};

pub const UPDATE_SOCKET: &str = "/run/rackforge-updater/control.sock";
pub const INSTALL_SOCKET: &str = "/run/rackforge-installer/control.sock";
pub const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn update_socket() -> std::path::PathBuf {
    std::env::var_os("RACKFORGE_UPDATE_SOCKET")
        .map(Into::into)
        .unwrap_or_else(|| UPDATE_SOCKET.into())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Ready,
    Installing,
    Recovering,
    Installed,
    RolledBack,
    RecoveryFailed,
    Failed,
    NotPublished,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseOffer {
    pub version: String,
    pub notes: String,
    pub download_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateState {
    pub current_version: String,
    pub build: String,
    pub platform: String,
    pub phase: UpdatePhase,
    pub available: Option<ReleaseOffer>,
    pub message: Option<String>,
    pub downloaded_bytes: u64,
    /// Checked metadata and archives can be prepared before activation is supported.
    pub can_stage: bool,
    #[serde(default)]
    pub can_install: bool,
}

impl UpdateState {
    pub fn unsupported(version: impl Into<String>, platform: impl Into<String>) -> Self {
        Self {
            current_version: version.into(),
            build: String::new(),
            platform: platform.into(),
            phase: UpdatePhase::Unsupported,
            available: None,
            message: Some("Software updates are unavailable on this host.".into()),
            downloaded_bytes: 0,
            can_stage: false,
            can_install: false,
        }
    }
    pub fn busy(&self) -> bool {
        matches!(
            self.phase,
            UpdatePhase::Checking
                | UpdatePhase::Downloading
                | UpdatePhase::Installing
                | UpdatePhase::Recovering
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum UpdateRequest {
    Status,
    Check,
    /// Selects an already verified offer, never a client-supplied URL or path.
    Stage {
        version: String,
    },
    Install {
        version: String,
    },
    Recover,
    /// Offline CLI preparation; the cached metadata and archive are reverified.
    UseStaged {
        version: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivationStatus {
    pub phase: UpdatePhase,
    pub current_version: String,
    pub build: String,
    pub target_version: Option<String>,
    pub message: Option<String>,
}

#[cfg(unix)]
pub fn exchange(socket: &std::path::Path, request: &UpdateRequest) -> Result<UpdateState, String> {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(socket).map_err(|e| e.to_string())?;
    let timeout = Some(std::time::Duration::from_secs(2));
    stream
        .set_read_timeout(timeout)
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(timeout)
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut stream, request).map_err(|e| e.to_string())?;
    stream.write_all(b"\n").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .take(64 * 1024)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&line).map_err(|e| e.to_string())
}
