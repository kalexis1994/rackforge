use anyhow::{Context, Result, bail};
#[cfg(target_os = "linux")]
use rackforge_control_api::{ControlRequest, ControlResponse};
use rackforge_controller_package::{
    ControllerPackage, DriverRuntimeKind, InstalledController, PackageStore, PackageTrust,
    ProcessDriverInfo,
};
#[cfg(target_os = "linux")]
use rackforge_session_api::{ClientId, CommandEnvelope, SessionCommand};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "linux")]
static NEXT_DECLARATIVE_COMMAND_ID: AtomicU64 = AtomicU64::new(1);

enum HostCommand {
    Verify {
        package: PathBuf,
    },
    Install {
        package: PathBuf,
        root: PathBuf,
        trust: PackageTrust,
    },
    List {
        root: PathBuf,
    },
    Activate {
        id: String,
        version: String,
        root: PathBuf,
    },
    Serve {
        root: PathBuf,
        allow_community: bool,
    },
    RestoreAll {
        root: PathBuf,
        allow_community: bool,
    },
    Conformance {
        id: String,
        root: PathBuf,
        allow_community: bool,
    },
    Exec {
        id: String,
        root: PathBuf,
        allow_community: bool,
        arguments: Vec<String>,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match parse_args(env::args().skip(1))? {
        HostCommand::Verify { package } => {
            let package = ControllerPackage::open(&package)
                .with_context(|| format!("verifying controller package {}", package.display()))?;
            println!(
                "CONTROLLER_PACKAGE_OK id={} version={} runtime={:?}",
                package.manifest().id,
                package.manifest().version,
                package.manifest().runtime.kind
            );
        }
        HostCommand::Install {
            package,
            root,
            trust,
        } => {
            let installed = PackageStore::new(&root)
                .install_directory(&package, trust)
                .with_context(|| {
                    format!(
                        "installing controller package {} into {}",
                        package.display(),
                        root.display()
                    )
                })?;
            println!(
                "CONTROLLER_INSTALLED id={} version={} trust={:?} root={}",
                installed.record.id,
                installed.record.version,
                installed.record.trust,
                installed.package.root().display()
            );
        }
        HostCommand::List { root } => {
            let controllers = PackageStore::new(&root).list()?;
            if controllers.is_empty() {
                println!("NO_CONTROLLERS_INSTALLED root={}", root.display());
            }
            for controller in controllers {
                println!(
                    "CONTROLLER id={} version={} trust={:?} enabled={} runtime={:?}",
                    controller.record.id,
                    controller.record.version,
                    controller.record.trust,
                    controller.record.enabled,
                    controller.package.manifest().runtime.kind
                );
            }
        }
        HostCommand::Activate { id, version, root } => {
            let installed = PackageStore::new(&root)
                .activate_version(&id, &version)
                .with_context(|| {
                    format!(
                        "activating controller package {id:?} version {version:?} in {}",
                        root.display()
                    )
                })?;
            println!(
                "CONTROLLER_ACTIVATED id={} version={} trust={:?}",
                installed.record.id, installed.record.version, installed.record.trust
            );
        }
        HostCommand::Serve {
            root,
            allow_community,
        } => serve_controllers(&root, allow_community)?,
        HostCommand::RestoreAll {
            root,
            allow_community,
        } => restore_all(&root, allow_community)?,
        HostCommand::Conformance {
            id,
            root,
            allow_community,
        } => verify_conformance(&root, &id, allow_community)?,
        HostCommand::Exec {
            id,
            root,
            allow_community,
            arguments,
        } => execute_controller(&root, &id, allow_community, &arguments)?,
    }
    Ok(())
}

fn execute_controller(
    root: &Path,
    id: &str,
    allow_community: bool,
    arguments: &[String],
) -> Result<()> {
    let installed = PackageStore::new(root)
        .resolve(id)
        .with_context(|| format!("resolving installed controller {id:?}"))?;
    ensure_executable(&installed, allow_community)?;
    let status = controller_command(&installed, arguments)?
        .status()
        .with_context(|| format!("launching controller {id:?}"))?;
    if !status.success() {
        bail!(
            "controller {id:?} exited with {}",
            status
                .code()
                .map_or_else(|| "a signal".into(), |code| format!("code {code}"))
        );
    }
    Ok(())
}

fn serve_controllers(root: &Path, allow_community: bool) -> Result<()> {
    let startup = rackforge_startup::StartupTimeline::new("controller-host");
    #[cfg(target_os = "linux")]
    let declarative_packages = PackageStore::new(root)
        .list()?
        .into_iter()
        .filter(|installed| {
            installed.record.enabled
                && installed.package.manifest().runtime.kind == DriverRuntimeKind::DeclarativeV1
        })
        .count();
    #[cfg(not(target_os = "linux"))]
    let declarative_packages = 0;
    let executable = PackageStore::new(root)
        .list()?
        .into_iter()
        .filter(|installed| {
            installed.record.enabled
                && installed.package.manifest().runtime.kind == DriverRuntimeKind::ProcessV1
        })
        .count();
    if executable == 0 {
        if declarative_packages == 0 {
            bail!(
                "no active controller packages are available in {}",
                root.display()
            );
        }
        #[cfg(target_os = "linux")]
        let mut declarative = DeclarativeControllers::default();
        #[cfg(target_os = "linux")]
        if let Err(error) = declarative.refresh(root) {
            eprintln!("DECLARATIVE_CONTROLLER_INITIAL_REGISTER_FAILED error={error:#}");
        }
        startup.advance(rackforge_startup::StartupPhase::ControlReady)?;
        if let Err(error) = rackforge_startup::notify_service_ready(
            "RackForge controller host completed initial discovery",
        ) {
            eprintln!("SYSTEMD_READY_FAILED error={error}");
        }
        println!(
            "CONTROLLER_HOST_READY declarative={} root={}",
            declarative_packages,
            root.display()
        );
        loop {
            #[cfg(target_os = "linux")]
            if let Err(error) = declarative.refresh(root) {
                eprintln!("DECLARATIVE_CONTROLLER_REFRESH_FAILED error={error:#}");
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }
    #[cfg(target_os = "linux")]
    if declarative_packages > 0 {
        let declarative_root = root.to_path_buf();
        std::thread::Builder::new()
            .name("rackforge-declarative-controllers".into())
            .spawn(move || {
                let mut declarative = DeclarativeControllers::default();
                loop {
                    if let Err(error) = declarative.refresh(&declarative_root) {
                        eprintln!("DECLARATIVE_CONTROLLER_REFRESH_FAILED error={error:#}");
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            })?;
    }
    // The loop itself lives in the package crate now, shared with every
    // platform host; the CLI's contract stays: zero runnable packages is an
    // error, because the user explicitly asked to serve.
    let options = rackforge_controller_package::supervise::SuperviseOptions {
        allow_community,
        extra_env: Vec::new(),
        shutdown: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        on_ready: Some(std::sync::Arc::new(move || {
            let _ = startup.advance(rackforge_startup::StartupPhase::ControlReady);
            if let Err(error) = rackforge_startup::notify_service_ready(
                "RackForge controller host completed initial driver launch",
            ) {
                eprintln!("SYSTEMD_READY_FAILED error={error}");
            }
        })),
    };
    let count = rackforge_controller_package::supervise::supervise(root, &options)
        .map_err(|error| anyhow::anyhow!(error))?;
    if count == 0 && declarative_packages == 0 {
        bail!(
            "no executable controller packages are active in {}",
            root.display()
        );
    }
    Ok(())
}

/// The MIDI inputs seen so far and what their devices said: each device is
/// asked which model it is once per connection, and hears its package's
/// connect messages once.
#[cfg(target_os = "linux")]
#[derive(Default)]
struct DeclarativeControllers {
    endpoints: std::collections::BTreeMap<String, SeenEndpoint>,
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct SeenEndpoint {
    identity: Option<rackforge_controller_package::IdentityReply>,
    /// The packages whose connect messages went to this endpoint.
    sent: std::collections::BTreeSet<String>,
}

#[cfg(target_os = "linux")]
impl DeclarativeControllers {
    fn refresh(&mut self, root: &Path) -> Result<usize> {
        use midir::MidiInput;

        let midi = MidiInput::new("rackforge-declarative-controller-discovery")?;
        let names = midi
            .ports()
            .iter()
            .map(|port| midi.port_name(port))
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        drop(midi);
        // A device unplugged is asked again when it comes back.
        self.endpoints.retain(|name, _| names.contains(name));
        let store = PackageStore::new(root);
        for name in &names {
            if self.endpoints.contains_key(name) {
                continue;
            }
            // Only a device some package claims by name is asked: to choose
            // between packages, or to confirm the one.
            let claimed = !matches!(store.resolve_declarative_input(name), Ok(None));
            let identity = if claimed { query_identity(name) } else { None };
            if let Some(identity) = &identity {
                println!(
                    "DECLARATIVE_CONTROLLER_IDENTITY endpoint={name:?} manufacturer={:02X?} family={:#06x} model={:#06x}",
                    identity.manufacturer, identity.family, identity.model
                );
            }
            self.endpoints.insert(
                name.clone(),
                SeenEndpoint {
                    identity,
                    sent: Default::default(),
                },
            );
        }
        let mut registered = std::collections::BTreeSet::new();
        for (endpoint_name, seen) in &mut self.endpoints {
            let Some(binding) =
                store.resolve_identified_input(endpoint_name, seen.identity.as_ref())?
            else {
                continue;
            };
            if !registered.insert(binding.controller_id.clone()) {
                bail!(
                    "declarative controller {} matches more than one MIDI input; refine its endpoint matcher",
                    binding.controller_id
                );
            }
            register_declarative_controller(endpoint_name, &binding)?;
            if !binding.on_connect.is_empty() && seen.sent.insert(binding.controller_id.clone()) {
                match send_to_endpoint(endpoint_name, &binding.on_connect) {
                    Ok(()) => println!(
                        "DECLARATIVE_CONTROLLER_ON_CONNECT id={} endpoint={endpoint_name:?} messages={}",
                        binding.controller_id,
                        binding.on_connect.len()
                    ),
                    Err(error) => eprintln!(
                        "DECLARATIVE_CONTROLLER_ON_CONNECT_FAILED id={} endpoint={endpoint_name:?} error={error:#}",
                        binding.controller_id
                    ),
                }
            }
        }
        Ok(registered.len())
    }
}

/// Asks the device behind an input which model it is, and waits briefly for
/// its Identity Reply on that input. ALSA lets this listen beside the
/// engine, which keeps its own subscription to the port.
#[cfg(target_os = "linux")]
fn query_identity(endpoint_name: &str) -> Option<rackforge_controller_package::IdentityReply> {
    use midir::{Ignore, MidiInput};
    use std::sync::mpsc;

    let mut input = MidiInput::new("rackforge-controller-identity").ok()?;
    input.ignore(Ignore::None);
    let port = input
        .ports()
        .into_iter()
        .find(|port| input.port_name(port).as_deref() == Ok(endpoint_name))?;
    let (sender, receiver) = mpsc::channel();
    let _connection = input
        .connect(
            &port,
            "rackforge-controller-identity",
            move |_, message, _| {
                if let Some(reply) = rackforge_controller_package::IdentityReply::parse(message) {
                    let _ = sender.send(reply);
                }
            },
            (),
        )
        .ok()?;
    if let Err(error) = send_to_endpoint(
        endpoint_name,
        &[rackforge_controller_package::IDENTITY_REQUEST.to_vec()],
    ) {
        eprintln!(
            "DECLARATIVE_CONTROLLER_IDENTITY_UNASKED endpoint={endpoint_name:?} error={error:#}"
        );
        return None;
    }
    receiver
        .recv_timeout(std::time::Duration::from_millis(
            rackforge_controller_package::IDENTITY_REPLY_WINDOW_MS,
        ))
        .ok()
}

/// Writes messages to the output port of the device behind an input: ALSA
/// names a device port's two directions alike.
#[cfg(target_os = "linux")]
fn send_to_endpoint(endpoint_name: &str, messages: &[Vec<u8>]) -> Result<()> {
    use midir::MidiOutput;

    let output = MidiOutput::new("rackforge-controller-output")?;
    let port = output
        .ports()
        .into_iter()
        .find(|port| output.port_name(port).as_deref() == Ok(endpoint_name))
        .with_context(|| format!("no MIDI output answers to {endpoint_name:?}"))?;
    let mut connection = output
        .connect(&port, "rackforge-controller-output")
        .map_err(|error| anyhow::anyhow!("opening MIDI output {endpoint_name:?}: {error}"))?;
    for message in messages {
        connection
            .send(message)
            .map_err(|error| anyhow::anyhow!("writing to {endpoint_name:?}: {error}"))?;
        // A device may need a moment between mode changes.
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn register_declarative_controller(
    endpoint_name: &str,
    binding: &rackforge_controller_package::DeclarativeControllerBinding,
) -> Result<()> {
    let command = SessionCommand::RegisterHostBindings {
        controller_id: binding.controller_id.clone(),
        // Declarative input remains part of the normal musical stream.
        // Process-driver reservations are consuming by design, so do not
        // reuse them here merely to publish the semantic profile.
        controls: Vec::new(),
        actions: Vec::new(),
        midi_source_name: Some(endpoint_name.to_owned()),
        semantic_profile: binding.semantic_profile.clone(),
        identified: binding.identified,
    };
    let request = ControlRequest::Dispatch {
        envelope: CommandEnvelope::new(
            ClientId::new(format!("controller.{}", binding.controller_id))
                .map_err(anyhow::Error::msg)?,
            NEXT_DECLARATIVE_COMMAND_ID.fetch_add(1, Ordering::Relaxed),
            command,
        ),
    };
    let endpoint = rackforge_control_api::transport::endpoint_from_env(default_control_socket)?;
    match rackforge_control_api::transport::exchange(&endpoint, &request)? {
        ControlResponse::CommandApplied { .. } => {
            println!(
                "DECLARATIVE_CONTROLLER_REGISTERED id={} endpoint={:?}",
                binding.controller_id, endpoint_name
            );
            Ok(())
        }
        ControlResponse::Error { message, .. } => {
            bail!(
                "registering declarative controller {}: {message}",
                binding.controller_id
            )
        }
        response => bail!(
            "registering declarative controller {} returned {response:?}",
            binding.controller_id
        ),
    }
}

#[cfg(target_os = "linux")]
fn default_control_socket() -> PathBuf {
    env::var_os("RACKFORGE_ROOT")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join("rackforge")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("state/live-control.sock")
}

fn restore_all(root: &Path, allow_community: bool) -> Result<()> {
    let mut failures = Vec::new();
    for installed in PackageStore::new(root).list()? {
        if !installed.record.enabled {
            continue;
        }
        if installed.package.manifest().runtime.kind == DriverRuntimeKind::DeclarativeV1 {
            continue;
        }
        if let Err(error) = ensure_executable(&installed, allow_community).and_then(|()| {
            let status = controller_command(&installed, &["restore".into(), "--execute".into()])?
                .status()?;
            if status.success() {
                Ok(())
            } else {
                bail!("restore exited with {status}")
            }
        }) {
            failures.push(format!("{}: {error:#}", installed.record.id));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        bail!("controller restore failures: {}", failures.join("; "))
    }
}

fn verify_conformance(root: &Path, id: &str, allow_community: bool) -> Result<()> {
    let installed = PackageStore::new(root)
        .resolve(id)
        .with_context(|| format!("resolving installed controller {id:?}"))?;
    if installed.package.manifest().runtime.kind == DriverRuntimeKind::DeclarativeV1 {
        let profile = installed.package.manifest().profile();
        println!(
            "CONTROLLER_CONFORMANCE_OK id={} version={} runtime=declarative-v1 inputs={} mappings={}",
            installed.record.id,
            installed.record.version,
            installed.package.manifest().inputs.len(),
            profile.host_controls.len()
                + profile.host_actions.len()
                + profile
                    .semantic_profile
                    .as_ref()
                    .map_or(0, |profile| profile.controls.len())
        );
        return Ok(());
    }
    ensure_executable(&installed, allow_community)?;
    let output = controller_command(&installed, &["driver-info".into()])?
        .stdout(Stdio::piped())
        .output()
        .with_context(|| format!("querying driver info for {id:?}"))?;
    if !output.status.success() {
        bail!("driver-info for {id:?} exited with {}", output.status);
    }
    let info: ProcessDriverInfo = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("decoding driver-info for {id:?}"))?;
    info.validate_against(installed.package.manifest())?;
    let status = controller_command(&installed, &["self-test".into()])?
        .status()
        .with_context(|| format!("running self-test for {id:?}"))?;
    if !status.success() {
        bail!("self-test for {id:?} exited with {status}");
    }
    println!(
        "CONTROLLER_CONFORMANCE_OK id={} version={} layouts={}",
        installed.record.id,
        installed.record.version,
        info.layouts.join(",")
    );
    Ok(())
}

fn ensure_executable(installed: &InstalledController, allow_community: bool) -> Result<()> {
    rackforge_controller_package::supervise::ensure_executable(installed, allow_community)
        .map_err(|error| anyhow::anyhow!(error))
}

fn controller_command(installed: &InstalledController, arguments: &[String]) -> Result<Command> {
    rackforge_controller_package::supervise::controller_command(installed, arguments, &[])
        .map_err(|error| anyhow::anyhow!(error))
}

fn parse_args(arguments: impl Iterator<Item = String>) -> Result<HostCommand> {
    let arguments = arguments.collect::<Vec<_>>();
    let Some(command) = arguments.first().map(String::as_str) else {
        bail!(usage());
    };
    match command {
        "verify" => {
            if arguments.len() != 2 {
                bail!(usage());
            }
            Ok(HostCommand::Verify {
                package: PathBuf::from(&arguments[1]),
            })
        }
        "install" => {
            if arguments.len() < 2 {
                bail!(usage());
            }
            let package = PathBuf::from(&arguments[1]);
            let mut root = default_store_root();
            let mut trust = PackageTrust::Local;
            let mut index = 2;
            while index < arguments.len() {
                match arguments[index].as_str() {
                    "--root" => {
                        index += 1;
                        root = arguments
                            .get(index)
                            .context("--root requires a directory")?
                            .into();
                    }
                    "--trust" => {
                        index += 1;
                        trust =
                            parse_trust(arguments.get(index).context("--trust requires a value")?)?;
                    }
                    _ => bail!(usage()),
                }
                index += 1;
            }
            Ok(HostCommand::Install {
                package,
                root,
                trust,
            })
        }
        "list" => {
            let mut root = default_store_root();
            if arguments.len() == 3 && arguments[1] == "--root" {
                root = arguments[2].clone().into();
            } else if arguments.len() != 1 {
                bail!(usage());
            }
            Ok(HostCommand::List { root })
        }
        "activate" => {
            if arguments.len() < 3 {
                bail!(usage());
            }
            let id = arguments[1].clone();
            let version = arguments[2].clone();
            let mut root = default_store_root();
            if arguments.len() == 5 && arguments[3] == "--root" {
                root = arguments[4].clone().into();
            } else if arguments.len() != 3 {
                bail!(usage());
            }
            Ok(HostCommand::Activate { id, version, root })
        }
        "serve" | "restore-all" => {
            let mut root = default_store_root();
            let mut allow_community = false;
            let mut index = 1;
            while index < arguments.len() {
                match arguments[index].as_str() {
                    "--root" => {
                        index += 1;
                        root = arguments
                            .get(index)
                            .context("--root requires a directory")?
                            .into();
                    }
                    "--allow-community" => allow_community = true,
                    _ => bail!(usage()),
                }
                index += 1;
            }
            if command == "serve" {
                Ok(HostCommand::Serve {
                    root,
                    allow_community,
                })
            } else {
                Ok(HostCommand::RestoreAll {
                    root,
                    allow_community,
                })
            }
        }
        "exec" | "conformance" => {
            let Some(id) = arguments.get(1) else {
                bail!(usage());
            };
            let mut root = default_store_root();
            let mut allow_community = false;
            let mut index = 2;
            let mut driver_arguments = Vec::new();
            while index < arguments.len() {
                match arguments[index].as_str() {
                    "--root" => {
                        index += 1;
                        root = arguments
                            .get(index)
                            .context("--root requires a directory")?
                            .into();
                    }
                    "--allow-community" => allow_community = true,
                    "--" if command == "exec" => {
                        driver_arguments.extend_from_slice(&arguments[index + 1..]);
                        break;
                    }
                    _ => bail!(usage()),
                }
                index += 1;
            }
            if command == "exec" {
                Ok(HostCommand::Exec {
                    id: id.clone(),
                    root,
                    allow_community,
                    arguments: driver_arguments,
                })
            } else {
                Ok(HostCommand::Conformance {
                    id: id.clone(),
                    root,
                    allow_community,
                })
            }
        }
        _ => bail!(usage()),
    }
}

fn parse_trust(value: &str) -> Result<PackageTrust> {
    match value {
        "official" => Ok(PackageTrust::Official),
        "certified" => Ok(PackageTrust::Certified),
        "community" => Ok(PackageTrust::Community),
        "local" => Ok(PackageTrust::Local),
        _ => bail!("trust must be official, certified, community or local"),
    }
}

fn default_store_root() -> PathBuf {
    env::var_os("RACKFORGE_CONTROLLER_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("RACKFORGE_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("rackforge")
                })
                .join("controllers")
        })
}

fn usage() -> &'static str {
    "usage:
  rackforge-controller-host verify PACKAGE.rfcontroller
  rackforge-controller-host install PACKAGE.rfcontroller [--root DIR] [--trust LEVEL]
  rackforge-controller-host list [--root DIR]
  rackforge-controller-host activate ID VERSION [--root DIR]
  rackforge-controller-host serve [--root DIR] [--allow-community]
  rackforge-controller-host restore-all [--root DIR] [--allow-community]
  rackforge-controller-host conformance ID [--root DIR] [--allow-community]
  rackforge-controller-host exec ID [--root DIR] [--allow-community] -- DRIVER_ARGS..."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_keeps_host_options_separate_from_driver_arguments() {
        let command = parse_args(
            [
                "exec",
                "org.rackforge.example",
                "--root",
                "/tmp/controllers",
                "--allow-community",
                "--",
                "serve",
                "--execute",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        let HostCommand::Exec {
            id,
            root,
            allow_community,
            arguments,
        } = command
        else {
            panic!("expected exec");
        };
        assert_eq!(id, "org.rackforge.example");
        assert_eq!(root, PathBuf::from("/tmp/controllers"));
        assert!(allow_community);
        assert_eq!(arguments, ["serve", "--execute"]);
    }

    #[test]
    fn activate_parses_an_explicit_immutable_version() {
        let command = parse_args(
            [
                "activate",
                "org.rackforge.example",
                "1.2.3",
                "--root",
                "/tmp/controllers",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        let HostCommand::Activate { id, version, root } = command else {
            panic!("expected activate");
        };
        assert_eq!(id, "org.rackforge.example");
        assert_eq!(version, "1.2.3");
        assert_eq!(root, PathBuf::from("/tmp/controllers"));
    }

    #[test]
    fn install_defaults_to_local_trust() {
        let command = parse_args(
            ["install", "example.rfcontroller"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        let HostCommand::Install { trust, .. } = command else {
            panic!("expected install");
        };
        assert_eq!(trust, PackageTrust::Local);
    }

    #[test]
    fn serve_is_hardware_agnostic_and_accepts_only_store_options() {
        let command = parse_args(
            ["serve", "--root", "/tmp/controllers"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        let HostCommand::Serve {
            root,
            allow_community,
        } = command
        else {
            panic!("expected serve");
        };
        assert_eq!(root, PathBuf::from("/tmp/controllers"));
        assert!(!allow_community);
    }
}
