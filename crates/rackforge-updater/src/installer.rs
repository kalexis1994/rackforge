//! Independent installer and a narrowly scoped root service supervisor.
use crate::activation::{self, AgentConfig, Services};
use anyhow::{Context, Result, bail, ensure};
use rackforge_platform_api::updates::{ActivationStatus, UpdatePhase, UpdateRequest};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::{UnixDatagram, UnixListener, UnixStream},
    },
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum ServiceRequest {
    Snapshot,
    Stop,
    Start { units: Vec<String> },
    Ready,
}
#[derive(Serialize, Deserialize)]
struct ServiceResponse {
    units: Vec<String>,
    error: Option<String>,
}

fn notify_ready() -> Result<()> {
    if let Some(path) = std::env::var_os("NOTIFY_SOCKET") {
        UnixDatagram::unbound()?.send_to(b"READY=1", path)?;
    }
    Ok(())
}
fn listener(path: &Path) -> Result<UnixListener> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        use std::os::unix::fs::FileTypeExt;
        ensure!(
            meta.file_type().is_socket(),
            "Refusing to replace a non-socket control path"
        );
        fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o660))?;
    Ok(listener)
}
fn read_request<T: serde::de::DeserializeOwned>(stream: &UnixStream) -> Result<T> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut bytes = String::new();
    BufReader::new(stream).take(4096).read_line(&mut bytes)?;
    Ok(serde_json::from_str(&bytes)?)
}
fn send(stream: &mut UnixStream, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *stream, value)?;
    stream.write_all(b"\n")?;
    Ok(())
}
fn config(path: &Path) -> Result<AgentConfig> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
        "Installer configuration must be owned and writable only by root"
    );
    let config: AgentConfig = serde_json::from_slice(&fs::read(path)?)?;
    config.validate()?;
    Ok(config)
}

/// Root never extracts packages or executes code from the user-owned runtime.
/// It only starts/stops the units selected by its root-owned configuration.
pub fn supervise(path: &Path) -> Result<()> {
    let config = config(path)?;
    let executable = std::env::current_exe()?;
    let meta = fs::metadata(&executable)?;
    ensure!(
        meta.uid() == 0 && meta.mode() & 0o022 == 0,
        "The update agent must be installed as a root-owned executable"
    );
    let server = listener(&config.runtime.join("services.sock"))?;
    server.set_nonblocking(true)?;
    let mut child = Command::new("/usr/bin/setpriv")
        .arg("--reuid")
        .arg(&config.user)
        .arg("--regid")
        .arg(&config.group)
        .arg("--init-groups")
        .arg(executable)
        .arg("installer-worker")
        .arg(path)
        .env_remove("NOTIFY_SOCKET")
        .stdin(Stdio::null())
        .spawn()?;
    loop {
        ensure!(
            child.try_wait()?.is_none(),
            "Installer worker exited; restarting the recovery supervisor"
        );
        let (mut stream, _) = match server.accept() {
            Ok(value) => value,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let result = (|| -> Result<Vec<String>> {
            match read_request::<ServiceRequest>(&stream)? {
                ServiceRequest::Snapshot => Ok(config
                    .units()
                    .into_iter()
                    .filter(|unit| unit_active(unit))
                    .collect()),
                ServiceRequest::Stop => {
                    activation::command(
                        Command::new("/usr/bin/systemctl")
                            .arg("stop")
                            .args(config.units()),
                        Duration::from_secs(90),
                    )?;
                    Ok(Vec::new())
                }
                ServiceRequest::Start { units } => {
                    ensure!(
                        units.iter().all(|unit| config.units().contains(unit)),
                        "Unknown managed service"
                    );
                    if !units.is_empty() {
                        // Queue starts, without waiting for this Type=notify unit itself.
                        activation::command(
                            Command::new("/usr/bin/systemctl")
                                .args(["--no-block", "start"])
                                .args(&units),
                            Duration::from_secs(10),
                        )?;
                    }
                    Ok(Vec::new())
                }
                ServiceRequest::Ready => {
                    notify_ready()?;
                    Ok(Vec::new())
                }
            }
        })();
        let response = match result {
            Ok(units) => ServiceResponse { units, error: None },
            Err(error) => ServiceResponse {
                units: Vec::new(),
                error: Some(format!("{error:#}")),
            },
        };
        let _ = send(&mut stream, &response);
    }
}
fn unit_active(unit: &str) -> bool {
    Command::new("/usr/bin/systemctl")
        .args(["is-active", "--quiet", unit])
        .status()
        .is_ok_and(|status| status.success())
}
struct SystemServices {
    config: AgentConfig,
}
impl SystemServices {
    fn exchange(&self, request: ServiceRequest) -> Result<Vec<String>> {
        let mut stream = UnixStream::connect(self.config.runtime.join("services.sock"))?;
        stream.set_read_timeout(Some(Duration::from_secs(95)))?;
        send(&mut stream, &request)?;
        let mut line = String::new();
        BufReader::new(stream)
            .take(16 * 1024)
            .read_line(&mut line)?;
        let response: ServiceResponse = serde_json::from_str(&line)?;
        if let Some(error) = response.error {
            bail!("{error}");
        }
        Ok(response.units)
    }
}
impl Services for SystemServices {
    fn snapshot(&self) -> Result<Vec<String>> {
        self.exchange(ServiceRequest::Snapshot)
    }
    fn stop(&self) -> Result<()> {
        self.exchange(ServiceRequest::Stop).map(|_| ())
    }
    fn start(&self, units: &[String]) -> Result<()> {
        self.exchange(ServiceRequest::Start {
            units: units.to_vec(),
        })
        .map(|_| ())
    }
    fn health(&self, version: &str, revision: &str, units: &[String], strict: bool) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(45);
        let mut stable = 0;
        while Instant::now() < deadline {
            let ready = (|| -> Result<()> {
                ensure!(
                    units.iter().all(|unit| unit_active(unit)),
                    "Some managed services are not active"
                );
                if units.contains(&format!("{}-web.service", self.config.service_prefix)) {
                    let port = fs::read_to_string(self.config.root.join("config/rackforge.toml"))
                        .ok()
                        .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
                        .and_then(|doc| doc.get("web")?.get("port")?.as_integer())
                        .and_then(|port| u16::try_from(port).ok())
                        .filter(|port| *port != 0)
                        .unwrap_or(self.config.web_port);
                    let mut response = ureq::Agent::config_builder()
                        .timeout_global(Some(Duration::from_secs(2)))
                        .build()
                        .new_agent()
                        .get(format!("http://127.0.0.1:{}/api/v1/health", port))
                        .call()?;
                    let mut bytes = Vec::new();
                    response
                        .body_mut()
                        .as_reader()
                        .take(64 * 1024)
                        .read_to_end(&mut bytes)?;
                    let health: serde_json::Value = serde_json::from_slice(&bytes)?;
                    ensure!(health["status"] == "ok", "Web health check failed");
                    if units.contains(&format!("{}-audio.service", self.config.service_prefix)) {
                        ensure!(health["core_connected"] == true, "Core did not reconnect");
                        let core = rackforge_control_api::transport::exchange(
                            &rackforge_control_api::transport::ControlEndpoint::Unix(
                                self.config.root.join("state/live-control.sock"),
                            ),
                            &rackforge_control_api::ControlRequest::Snapshot,
                        )?;
                        ensure!(
                            matches!(
                                core,
                                rackforge_control_api::ControlResponse::Snapshot { .. }
                            ),
                            "Core did not answer its health request"
                        );
                    }
                    if strict {
                        ensure!(
                            health["version"] == version,
                            "The web service is running a different version"
                        );
                        let actual = health["revision"].as_str().unwrap_or("");
                        ensure!(
                            actual.len() >= 7
                                && (revision.starts_with(actual) || actual.starts_with(revision)),
                            "The web service is running a different build"
                        );
                    }
                }
                Ok(())
            })()
            .is_ok();
            stable = if ready { stable + 1 } else { 0 };
            if stable >= 5 {
                return Ok(());
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        bail!("Managed services did not remain healthy within 45 seconds")
    }
}

pub fn worker(path: &Path) -> Result<()> {
    let config = config(path)?;
    ensure!(
        fs::metadata("/proc/self")?.uid() != 0,
        "Installer filesystem operations must never run as root"
    );
    let services = SystemServices {
        config: config.clone(),
    };
    // Restore interrupted swaps before systemd allows the managed units to start.
    let pending = activation::read_journal(&config)?;
    if let Some(journal) = pending
        .as_ref()
        .filter(|j| !activation::terminal(&j.status.phase))
    {
        activation::restore_files(&config, journal)?;
    }
    let initial = activation::status(&config)?;
    let state = Arc::new(Mutex::new(initial));
    let socket = config.runtime.join("control.sock");
    let server = listener(&socket)?;
    services.exchange(ServiceRequest::Ready)?;
    let shared = Arc::clone(&state);
    let initial_recovery = pending.is_some_and(|j| !activation::terminal(&j.status.phase));
    if initial_recovery {
        let config = config.clone();
        std::thread::spawn(move || {
            let services = SystemServices {
                config: config.clone(),
            };
            if let Err(error) =
                activation::recover(&config, &services, |value| *shared.lock().unwrap() = value)
            {
                let mut state = shared.lock().unwrap();
                state.phase = UpdatePhase::RecoveryFailed;
                state.message = Some(format!("{error:#}"));
            }
        });
    }
    for connection in server.incoming() {
        let Ok(mut stream) = connection else { continue };
        let request = read_request::<UpdateRequest>(&stream);
        let mut shared = state.lock().unwrap();
        if let Ok(request) = request {
            let busy = matches!(
                shared.phase,
                UpdatePhase::Installing | UpdatePhase::Recovering
            );
            if !busy
                && matches!(
                    request,
                    UpdateRequest::Install { .. } | UpdateRequest::Recover
                )
            {
                let allowed = !matches!(request, UpdateRequest::Install { .. })
                    || activation::terminal(&shared.phase)
                    || shared.phase == UpdatePhase::Failed;
                if allowed {
                    shared.phase = if matches!(request, UpdateRequest::Recover) {
                        UpdatePhase::Recovering
                    } else {
                        UpdatePhase::Installing
                    };
                    let config = config.clone();
                    let shared = Arc::clone(&state);
                    std::thread::spawn(move || {
                        let services = SystemServices {
                            config: config.clone(),
                        };
                        let progress = |value| *shared.lock().unwrap() = value;
                        let result = match request {
                            UpdateRequest::Install { version } => {
                                activation::activate(&config, &version, &services, progress)
                            }
                            _ => activation::recover(&config, &services, progress),
                        };
                        if let Err(error) = result {
                            let mut shared = shared.lock().unwrap();
                            shared.phase = UpdatePhase::Failed;
                            shared.message = Some(format!("{error:#}"));
                        } else if let Ok(status) = activation::status(&config) {
                            *shared.lock().unwrap() = status;
                        }
                    });
                }
            }
        }
        let _ = send(&mut stream, &*shared);
    }
    Ok(())
}

pub fn exchange(path: &Path, request: &UpdateRequest) -> Result<ActivationStatus> {
    let mut stream =
        UnixStream::connect(path).context("The installation agent is not available")?;
    send(&mut stream, request)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut line = String::new();
    BufReader::new(stream)
        .take(64 * 1024)
        .read_line(&mut line)?;
    Ok(serde_json::from_str(&line)?)
}
