use anyhow::{Context, Result, bail};
use rackforge_controller_package::{
    ControllerPackage, DriverRuntimeKind, InstalledController, PackageStore, PackageTrust,
    ProcessDriverInfo, development_target,
};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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

struct ManagedController {
    installed: InstalledController,
    child: Option<Child>,
    restart_at: Instant,
}

fn serve_controllers(root: &Path, allow_community: bool) -> Result<()> {
    let mut managed = PackageStore::new(root)
        .list()?
        .into_iter()
        .filter(|installed| installed.record.enabled)
        .filter_map(
            |installed| match ensure_executable(&installed, allow_community) {
                Ok(()) => Some(ManagedController {
                    installed,
                    child: None,
                    restart_at: Instant::now(),
                }),
                Err(error) => {
                    eprintln!(
                        "CONTROLLER_SKIPPED id={} reason={error:#}",
                        installed.record.id
                    );
                    None
                }
            },
        )
        .collect::<Vec<_>>();
    if managed.is_empty() {
        bail!(
            "no executable controller packages are active in {}",
            root.display()
        );
    }
    println!(
        "CONTROLLER_HOST_READY packages={} root={}",
        managed.len(),
        root.display()
    );
    loop {
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
                match controller_command(
                    &controller.installed,
                    &["serve".into(), "--execute".into()],
                )
                .and_then(|mut command| command.spawn().map_err(anyhow::Error::from))
                {
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
                            "CONTROLLER_START_FAILED id={} error={error:#}",
                            controller.installed.record.id
                        );
                        controller.restart_at = now + Duration::from_secs(2);
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn restore_all(root: &Path, allow_community: bool) -> Result<()> {
    let mut failures = Vec::new();
    for installed in PackageStore::new(root).list()? {
        if !installed.record.enabled {
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
    let id = &installed.record.id;
    if !installed.record.enabled {
        bail!("controller {id:?} is disabled");
    }
    if installed.record.trust == PackageTrust::Community && !allow_community {
        bail!("controller {id:?} is community code; pass --allow-community after reviewing it");
    }
    if installed.package.manifest().runtime.kind != DriverRuntimeKind::ProcessV1 {
        bail!(
            "runtime {:?} is not available in this host build",
            installed.package.manifest().runtime.kind
        );
    }
    Ok(())
}

fn controller_command(installed: &InstalledController, arguments: &[String]) -> Result<Command> {
    let target = development_target();
    if target == "unsupported" {
        bail!(
            "unsupported controller host platform {}-{}",
            env::consts::OS,
            env::consts::ARCH
        );
    }
    let entrypoint = installed
        .package
        .resolve_entrypoint(target)
        .with_context(|| {
            format!(
                "resolving {target} entrypoint for {:?}",
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
    Ok(command)
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
                .unwrap_or_else(|| PathBuf::from("/home/kalex/rackforge"))
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
