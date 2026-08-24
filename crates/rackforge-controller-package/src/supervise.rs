//! Driver supervision as a library.
//!
//! This loop used to live inside the `rackforge-controller-host` CLI, which
//! made the CLI the only host that could run controller packages. Every
//! platform host (Raspberry Pi service, the desktop app, Android) needs the
//! same behavior — enumerate enabled packages, spawn each driver, restart
//! with backoff when one dies — so it lives here and the CLI delegates.

use crate::{
    DriverRuntimeKind, InstalledController, PackageStore, PackageTrust, development_target,
};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const GRACEFUL_SHUTDOWN_POLL: Duration = Duration::from_millis(25);

/// How a host runs the supervision loop.
pub struct SuperviseOptions {
    /// Community-trust packages refuse to run unless the person explicitly
    /// allowed them.
    pub allow_community: bool,
    /// Extra environment for every spawned driver. This is how a host hands
    /// its children the way back in — e.g. `RACKFORGE_CONTROL_ADDR` for a
    /// TCP control endpoint on hosts without a Unix socket.
    pub extra_env: Vec<(String, String)>,
    /// Set to true to wind the loop down; drivers are killed and reaped.
    pub shutdown: Arc<AtomicBool>,
}

pub fn ensure_executable(
    installed: &InstalledController,
    allow_community: bool,
) -> Result<(), String> {
    let id = &installed.record.id;
    if !installed.record.enabled {
        return Err(format!("controller {id:?} is disabled"));
    }
    if installed.record.trust == PackageTrust::Community && !allow_community {
        return Err(format!(
            "controller {id:?} is community code; enable community packages after reviewing it"
        ));
    }
    if installed.package.manifest().runtime.kind != DriverRuntimeKind::ProcessV1 {
        return Err(format!(
            "runtime {:?} is not available in this host build",
            installed.package.manifest().runtime.kind
        ));
    }
    Ok(())
}

/// Builds the command for one driver invocation with the package contract's
/// environment (id, package root, trust) plus the host's extras.
pub fn controller_command(
    installed: &InstalledController,
    arguments: &[String],
    extra_env: &[(String, String)],
) -> Result<Command, String> {
    let target = development_target();
    if target == "unsupported" {
        return Err(format!(
            "unsupported controller host platform {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
    }
    let entrypoint = installed
        .package
        .resolve_entrypoint(target)
        .map_err(|error| {
            format!(
                "resolving {target} entrypoint for {:?}: {error}",
                installed.record.id
            )
        })?;
    let mut command = Command::new(entrypoint);
    command
        .args(arguments)
        .current_dir(installed.package.root())
        .env("RACKFORGE_CONTROLLER_ID", &installed.record.id)
        .env(
            "RACKFORGE_CONTROLLER_PACKAGE",
            installed.package.root().as_os_str(),
        )
        .env(
            "RACKFORGE_CONTROLLER_TRUST",
            format!("{:?}", installed.record.trust).to_ascii_lowercase(),
        )
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    Ok(command)
}

struct ManagedController {
    installed: InstalledController,
    child: Option<Child>,
    restart_at: Instant,
}

fn stop_controllers(managed: &mut [ManagedController]) {
    // Closing the supervisor pipe is the portable shutdown request promised
    // to controller processes. It gives drivers a chance to restore hardware
    // state (OLED, LEDs, presets) before their MIDI handles disappear.
    for controller in managed.iter_mut() {
        if let Some(child) = &mut controller.child {
            child.stdin.take();
        }
    }

    let deadline = Instant::now() + GRACEFUL_SHUTDOWN_TIMEOUT;
    loop {
        let mut running = false;
        for controller in managed.iter_mut() {
            let Some(child) = &mut controller.child else {
                continue;
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!(
                        "CONTROLLER_STOPPED_GRACEFULLY id={} status={status}",
                        controller.installed.record.id
                    );
                    controller.child = None;
                }
                Ok(None) | Err(_) => running = true,
            }
        }
        if !running || Instant::now() >= deadline {
            break;
        }
        thread::sleep(GRACEFUL_SHUTDOWN_POLL);
    }

    for controller in managed {
        if let Some(mut child) = controller.child.take() {
            eprintln!(
                "CONTROLLER_STOP_FORCED id={} reason=grace-timeout",
                controller.installed.record.id
            );
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Runs every enabled, executable controller package under `root` until
/// `shutdown` is raised. Returns the number of packages it supervised (zero
/// is not an error: a host with no controllers installed simply has nothing
/// to do, unlike the CLI, whose user asked for something explicit).
pub fn supervise(root: &Path, options: &SuperviseOptions) -> Result<usize, String> {
    let mut managed = PackageStore::new(root)
        .list()
        .map_err(|error| format!("listing controller store: {error}"))?
        .into_iter()
        .filter(|installed| installed.record.enabled)
        .filter_map(
            |installed| match ensure_executable(&installed, options.allow_community) {
                Ok(()) => Some(ManagedController {
                    installed,
                    child: None,
                    restart_at: Instant::now(),
                }),
                Err(error) => {
                    eprintln!(
                        "CONTROLLER_SKIPPED id={} reason={error}",
                        installed.record.id
                    );
                    None
                }
            },
        )
        .collect::<Vec<_>>();
    let count = managed.len();
    if count == 0 {
        return Ok(0);
    }
    println!(
        "CONTROLLER_HOST_READY packages={count} root={}",
        root.display()
    );
    while !options.shutdown.load(Ordering::Relaxed) {
        let now = Instant::now();
        for controller in &mut managed {
            if let Some(child) = &mut controller.child {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        eprintln!(
                            "CONTROLLER_EXITED id={} status={status}; restarting",
                            controller.installed.record.id
                        );
                        controller.child = None;
                        controller.restart_at = now + Duration::from_secs(1);
                    }
                    Ok(None) => continue,
                    Err(error) => {
                        eprintln!(
                            "CONTROLLER_WAIT_FAILED id={} error={error}; restarting",
                            controller.installed.record.id
                        );
                        controller.child = None;
                        controller.restart_at = now + Duration::from_secs(1);
                    }
                }
            }
            if controller.child.is_none() && now >= controller.restart_at {
                // Each driver also learns where ITS settings live: the host
                // writes `state/<id>/settings.toml` in the store, the driver
                // watches the file and applies changes to the hardware.
                let mut env = options.extra_env.clone();
                // The child watches this pipe: when the supervisor (or its
                // whole host process) dies, the pipe closes and the driver
                // exits instead of holding MIDI ports as an orphan.
                env.push(("RACKFORGE_SUPERVISOR_PIPE".into(), "1".into()));
                env.push((
                    "RACKFORGE_CONTROLLER_SETTINGS".into(),
                    root.join("state")
                        .join(&controller.installed.record.id)
                        .join("settings.toml")
                        .to_string_lossy()
                        .into_owned(),
                ));
                match controller_command(
                    &controller.installed,
                    &["serve".into(), "--execute".into()],
                    &env,
                )
                .and_then(|mut command| {
                    command.stdin(Stdio::piped());
                    command.spawn().map_err(|error| error.to_string())
                }) {
                    Ok(child) => {
                        println!(
                            "CONTROLLER_STARTED id={} pid={}",
                            controller.installed.record.id,
                            child.id()
                        );
                        controller.child = Some(child);
                    }
                    Err(error) => {
                        eprintln!(
                            "CONTROLLER_START_FAILED id={} error={error}",
                            controller.installed.record.id
                        );
                        controller.restart_at = now + Duration::from_secs(2);
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
    stop_controllers(&mut managed);
    Ok(count)
}
