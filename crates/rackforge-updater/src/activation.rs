//! Crash-recoverable filesystem transaction. This always runs as the host user.
use anyhow::{Context, Result, bail, ensure};
use rackforge_platform_api::updates::{ActivationStatus, UpdatePhase};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const BINARIES: &[&str] = &[
    "rackforge-core",
    "rackforge-web",
    "rackforge-store",
    "rackforge-platform-host",
    "rackforge-updater",
    "rackforge-controller-host",
    "rackforge-arturia-keylab-essential-mk3-driver",
];
const COMPONENTS: &[&str] = &[
    "bin",
    "web",
    "controllers",
    "plugin-store",
    "config/audio.toml",
    "build-info.txt",
];
const MAX_EXPANDED: u64 = 6 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    pub root: PathBuf,
    pub user: String,
    pub group: String,
    pub service_prefix: String,
    pub runtime: PathBuf,
    pub web_port: u16,
}
impl AgentConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.root.is_absolute() && self.root.parent().is_some() && self.runtime.is_absolute(),
            "Invalid agent paths"
        );
        for name in [&self.user, &self.group, &self.service_prefix] {
            ensure!(
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
                "Invalid service identity"
            );
        }
        ensure!(
            self.user != "root" && self.web_port > 0,
            "The installer worker must run without root privileges"
        );
        Ok(())
    }
    pub fn units(&self) -> Vec<String> {
        [
            "audio.path",
            "controller-host.service",
            "web.service",
            "platform-host.service",
            "audio.service",
            "updater.service",
        ]
        .iter()
        .map(|suffix| format!("{}-{suffix}", self.service_prefix))
        .collect()
    }
    pub fn journal(&self) -> PathBuf {
        self.root.join("state/updates/activation.json")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Journal {
    pub schema_version: u32,
    pub transaction: String,
    pub status: ActivationStatus,
    pub previous_version: String,
    pub previous_build: String,
    pub units: Vec<String>,
    pub existed: BTreeSet<String>,
}

pub trait Services {
    fn snapshot(&self) -> Result<Vec<String>>;
    fn stop(&self) -> Result<()>;
    fn start(&self, units: &[String]) -> Result<()>;
    fn health(&self, version: &str, revision: &str, units: &[String], strict: bool) -> Result<()>;
}

pub fn installation_lock(config: &AgentConfig) -> Result<fs::File> {
    // The same flock used by the command-line Raspberry Pi installer.
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(config.root.join(".install.lock"))?;
    file.try_lock().map_err(|error| {
        anyhow::anyhow!("Another installation or recovery owns the installation lock: {error}")
    })?;
    Ok(file)
}

pub fn durable_json(path: &Path, value: &impl Serialize) -> Result<()> {
    fs::create_dir_all(path.parent().context("No state directory")?)?;
    let next = path.with_extension("json.new");
    let mut file = fs::File::create(&next)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    fs::rename(&next, path)?;
    sync_dir(path.parent().unwrap())
}
fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
pub fn installed(root: &Path) -> (String, String) {
    let fields = fs::read_to_string(root.join("build-info.txt")).unwrap_or_default();
    let field = |key: &str| {
        fields
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .unwrap_or("")
            .to_owned()
    };
    let version = field("version");
    (
        if version.is_empty() {
            env!("CARGO_PKG_VERSION").into()
        } else {
            version
        },
        field("revision"),
    )
}
pub fn read_journal(config: &AgentConfig) -> Result<Option<Journal>> {
    if !config.journal().exists() {
        return Ok(None);
    }
    let journal: Journal = serde_json::from_slice(&fs::read(config.journal())?)?;
    ensure!(
        journal.schema_version == 1
            && journal.transaction.len() == 16
            && journal.transaction.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid activation journal"
    );
    ensure!(
        journal
            .existed
            .iter()
            .all(|name| COMPONENTS.contains(&name.as_str())),
        "Invalid journal component"
    );
    ensure!(
        journal
            .units
            .iter()
            .all(|name| config.units().contains(name)),
        "Invalid journal services"
    );
    Ok(Some(journal))
}
pub fn status(config: &AgentConfig) -> Result<ActivationStatus> {
    if let Some(journal) = read_journal(config)? {
        return Ok(journal.status);
    }
    let (current_version, build) = installed(&config.root);
    Ok(ActivationStatus {
        phase: UpdatePhase::Idle,
        current_version,
        build,
        target_version: None,
        message: None,
    })
}
pub fn terminal(phase: &UpdatePhase) -> bool {
    matches!(
        phase,
        UpdatePhase::Idle | UpdatePhase::Installed | UpdatePhase::RolledBack
    )
}

fn transaction_dir(config: &AgentConfig, journal: &Journal) -> PathBuf {
    config.root.join(".host-updates").join(&journal.transaction)
}
fn rename(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination.parent().context("Missing destination parent")?)?;
    fs::rename(source, destination).with_context(|| {
        format!(
            "Cannot move {} to {}",
            source.display(),
            destination.display()
        )
    })?;
    sync_dir(source.parent().unwrap())?;
    sync_dir(destination.parent().unwrap())
}
pub fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(source)?;
    ensure!(
        !meta.file_type().is_symlink(),
        "Refusing to copy a symbolic link: {}",
        source.display()
    );
    if meta.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
        sync_dir(destination)?;
    } else {
        ensure!(meta.is_file(), "Unsupported runtime file");
        fs::create_dir_all(destination.parent().context("Missing destination parent")?)?;
        fs::copy(source, destination)?;
        fs::File::open(destination)?.sync_all()?;
    }
    Ok(())
}

fn sync_tree(path: &Path) -> Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            sync_tree(&entry?.path())?;
        }
        sync_dir(path)
    } else {
        fs::File::open(path)?.sync_all().map_err(Into::into)
    }
}
fn size_tree(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        !meta.file_type().is_symlink(),
        "Runtime snapshots do not support symbolic links"
    );
    if meta.is_dir() {
        fs::read_dir(path)?.try_fold(0u64, |sum, entry| Ok(sum + size_tree(&entry?.path())?))
    } else {
        Ok(meta.len())
    }
}
fn require_space(root: &Path, bytes: u64) -> Result<()> {
    #[cfg(unix)]
    {
        let output = Command::new("/usr/bin/df")
            .args(["-B1", "--output=avail"])
            .arg(root)
            .output()?;
        ensure!(
            output.status.success(),
            "Could not check free installation space"
        );
        let available: u64 = String::from_utf8(output.stdout)?
            .split_whitespace()
            .last()
            .context("No free space result")?
            .parse()?;
        ensure!(
            available > bytes + 64 * 1024 * 1024,
            "Not enough free space to retain the previous installation"
        );
    }
    #[cfg(not(unix))]
    let _ = (root, bytes);
    Ok(())
}

/// Reject links, devices, traversal and expansion bombs before writing any entry.
pub fn unpack(archive: &Path, destination: &Path) -> Result<()> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(archive)?));
    let mut total = 0u64;
    let mut names = BTreeSet::new();
    for (index, entry) in archive.entries()?.enumerate() {
        ensure!(index < 100_000, "Too many archive entries");
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let parts: Vec<_> = path.components().collect();
        ensure!(
            parts.iter().all(|p| matches!(p, Component::Normal(_)))
                && parts.first().is_some_and(|p| p.as_os_str() == "rackforge"),
            "Invalid archive path"
        );
        ensure!(names.insert(path.clone()), "Duplicate archive path");
        let kind = entry.header().entry_type();
        ensure!(
            kind.is_file() || kind.is_dir(),
            "Archive links and special files are forbidden"
        );
        total = total
            .checked_add(entry.size())
            .context("Expanded archive overflow")?;
        ensure!(total <= MAX_EXPANDED, "Expanded archive is too large");
        let target = destination.join(&path);
        if kind.is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            fs::create_dir_all(target.parent().unwrap())?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            std::io::copy(&mut entry, &mut file)?;
            file.sync_all()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(
                    &target,
                    fs::Permissions::from_mode(if entry.header().mode()? & 0o111 != 0 {
                        0o755
                    } else {
                        0o644
                    }),
                )?;
            }
        }
    }
    Ok(())
}
fn build_field(root: &Path, key: &str) -> Result<String> {
    fs::read_to_string(root.join("build-info.txt"))?
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")).map(str::to_owned))
        .with_context(|| format!("Build information missing {key}"))
}
fn validate_payload(payload: &Path, version: &str) -> Result<String> {
    ensure!(
        build_field(payload, "version")? == version,
        "Package version mismatch"
    );
    ensure!(
        build_field(payload, "architecture")?
            == if crate::target() == "linux-aarch64-services" {
                "linux-aarch64"
            } else {
                "linux-x86-64"
            },
        "Package architecture mismatch"
    );
    ensure!(
        build_field(payload, "edition")? == "standard",
        "Managed updates require Standard edition"
    );
    ensure!(
        fs::read_to_string(payload.join("config/update-public-key.txt"))?.trim()
            == crate::PUBLIC_KEY.trim(),
        "Package update key mismatch"
    );
    for binary in BINARIES {
        let mut header = [0u8; 20];
        fs::File::open(payload.join("target/release").join(binary))?.read_exact(&mut header)?;
        let machine = u16::from_le_bytes([header[18], header[19]]);
        ensure!(
            &header[..6] == b"\x7fELF\x02\x01"
                && machine
                    == if crate::target() == "linux-aarch64-services" {
                        183
                    } else {
                        62
                    },
            "Package binary {binary} has the wrong native architecture"
        );
    }
    ensure!(
        payload.join("web/dist/index.html").is_file(),
        "Package missing the web interface"
    );
    ensure!(payload.join("controller-packages/org.rackforge.arturia-keylab-essential-mk3.rfcontroller/rackforge-controller.toml").is_file(), "Package missing the LITTLE driver");
    let revision = build_field(payload, "revision")?;
    ensure!(
        revision.len() >= 7
            && revision.len() <= 40
            && revision.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid build revision"
    );
    let ui_revision = fs::read_to_string(payload.join("web/dist/ui-revision.txt"))?;
    ensure!(
        ui_revision.trim().len() >= 7 && revision.starts_with(ui_revision.trim()),
        "The packaged interface belongs to a different build"
    );
    Ok(revision)
}
pub fn command(command: &mut Command, timeout: Duration) -> Result<String> {
    // Bounded logs avoid a blocked pipe when a package installer is verbose.
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let output = std::env::temp_dir().join(format!(
        "rf-update-command-{}-{:x}.log",
        std::process::id(),
        u64::from_ne_bytes(random)
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&output)?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(file.try_clone()?)
        .stderr(file)
        .spawn()?;
    let deadline = Instant::now() + timeout;
    let exit = loop {
        if let Some(exit) = child.try_wait()? {
            break exit;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&output);
            bail!("Update command timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut text = String::new();
    fs::File::open(&output)?
        .take(64 * 1024)
        .read_to_string(&mut text)?;
    let _ = fs::remove_file(output);
    ensure!(exit.success(), "Update command failed: {text}");
    Ok(text)
}

fn prepare_mutable(config: &AgentConfig, payload: &Path, candidate: &Path) -> Result<()> {
    for name in ["controllers", "plugin-store"] {
        let source = config.root.join(name);
        if source.exists() {
            copy_tree(&source, &candidate.join(name))?;
        } else {
            fs::create_dir_all(candidate.join(name))?;
        }
    }
    let controller_host = candidate.join("bin/rackforge-controller-host");
    command(
        Command::new(&controller_host)
            .args(["install"])
            .arg(payload.join(
                "controller-packages/org.rackforge.arturia-keylab-essential-mk3.rfcontroller",
            ))
            .arg("--root")
            .arg(candidate.join("controllers"))
            .args(["--trust", "official"]),
        Duration::from_secs(60),
    )?;
    command(
        Command::new(&controller_host)
            .arg("install-catalog")
            .arg("--root")
            .arg(candidate.join("controllers")),
        Duration::from_secs(60),
    )?;
    preserve_controller_enablement(
        &config.root.join("controllers/active"),
        &candidate.join("controllers/active"),
    )?;
    let audio = config.root.join("config/audio.toml");
    let mut audio_text = fs::read_to_string(&audio).ok();
    let old_package = audio_text
        .as_ref()
        .and_then(|text| toml::from_str::<toml::Value>(text).ok())
        .and_then(|doc| doc.get("package")?.as_str().map(PathBuf::from));
    let old_id = old_package
        .as_ref()
        .and_then(|p| fs::read_to_string(p.join("rackforge-plugin.toml")).ok())
        .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
        .and_then(|doc| doc.get("id")?.as_str().map(str::to_owned));
    let bundles = payload.join("bundled-plugins");
    if bundles.is_dir() {
        for entry in fs::read_dir(bundles)? {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "rfplugin") {
                continue;
            }
            let output = command(
                Command::new(candidate.join("bin/rackforge-store"))
                    .arg("install-local")
                    .arg(&path)
                    .arg(candidate.join("plugin-store"))
                    .arg("--official"),
                Duration::from_secs(90),
            )?;
            let field = |key: &str| {
                output
                    .split_whitespace()
                    .find_map(|part| part.strip_prefix(key))
                    .map(str::to_owned)
            };
            let id = field("id=").context("Missing installed plugin identity")?;
            let version = field("version=").context("Missing installed plugin version")?;
            if !config.root.join("plugin-store/packages").join(&id).exists() {
                command(
                    Command::new(candidate.join("bin/rackforge-store"))
                        .arg("enable")
                        .arg(&id)
                        .arg(candidate.join("plugin-store")),
                    Duration::from_secs(15),
                )?;
            }
            if old_id.as_ref() == Some(&id)
                && let (Some(text), Some(old_path)) = (&mut audio_text, &old_package)
            {
                *text = text.replace(
                    old_path.to_str().context("Invalid startup package path")?,
                    config
                        .root
                        .join("plugin-store/packages")
                        .join(id)
                        .join(version)
                        .to_str()
                        .context("Invalid installed path")?,
                );
            }
        }
    }
    if let Some(text) = audio_text {
        fs::create_dir_all(candidate.join("config"))?;
        fs::write(candidate.join("config/audio.toml"), text)?;
    }
    Ok(())
}

fn preserve_controller_enablement(previous: &Path, candidate: &Path) -> Result<()> {
    if !previous.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(previous)? {
        let entry = entry?;
        if entry.path().extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let old: serde_json::Value = serde_json::from_slice(&fs::read(entry.path())?)?;
        let path = candidate.join(entry.file_name());
        if let Some(enabled) = old.get("enabled").and_then(serde_json::Value::as_bool)
            && path.is_file()
        {
            let mut updated: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
            updated["enabled"] = enabled.into();
            fs::write(path, serde_json::to_vec_pretty(&updated)?)?;
        }
    }
    Ok(())
}

pub fn activate(
    config: &AgentConfig,
    version: &str,
    services: &impl Services,
    progress: impl Fn(ActivationStatus),
) -> Result<()> {
    let _lock = installation_lock(config)?;
    let previous_transaction = read_journal(config)?;
    if let Some(previous) = &previous_transaction {
        ensure!(
            terminal(&previous.status.phase),
            "Recover the previous installation first"
        );
    }
    let version_parsed = semver::Version::parse(version)?;
    let (previous_version, previous_build) = installed(&config.root);
    ensure!(
        version_parsed > semver::Version::parse(&previous_version)?
            && version_parsed.pre.is_empty()
            && version_parsed.build.is_empty(),
        "The update must be a newer stable release"
    );
    let stage = config.root.join("state/updates").join(version);
    let metadata = fs::read(stage.join("updates.json"))?;
    let signature = fs::read_to_string(stage.join("updates.json.sig"))?;
    let manifest = crate::verify_manifest(&metadata, &signature, crate::PUBLIC_KEY)?;
    ensure!(
        manifest.version == version,
        "Staged release version mismatch"
    );
    let asset = manifest
        .targets
        .get(crate::target())
        .context("No archive for this platform")?;
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let transaction = random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mut journal = Journal {
        schema_version: 1,
        transaction,
        status: ActivationStatus {
            phase: UpdatePhase::Installing,
            current_version: previous_version.clone(),
            build: previous_build.clone(),
            target_version: Some(version.into()),
            message: Some("Preparing the signed runtime. Audio will stop briefly.".into()),
        },
        previous_version,
        previous_build,
        units: Vec::new(),
        existed: BTreeSet::new(),
    };
    let work = transaction_dir(config, &journal);
    fs::create_dir_all(&work)?;
    require_space(
        &config.root,
        asset.bytes * 2
            + size_tree(&config.root.join("plugin-store"))?
            + size_tree(&config.root.join("controllers"))?,
    )?;
    let archive = work.join("verified.tar.gz");
    crate::verify_archive(&stage.join(crate::asset_name(crate::target())?), asset)?;
    copy_tree(&stage.join(crate::asset_name(crate::target())?), &archive)?;
    crate::verify_archive(&archive, asset)?;
    unpack(&archive, &work.join("unpacked"))?;
    let payload = work.join("unpacked/rackforge");
    let revision = validate_payload(&payload, version)?;
    let candidate = work.join("candidate");
    fs::create_dir_all(&candidate)?;
    copy_tree(&payload.join("target/release"), &candidate.join("bin"))?;
    copy_tree(&payload.join("web/dist"), &candidate.join("web"))?;
    copy_tree(
        &payload.join("build-info.txt"),
        &candidate.join("build-info.txt"),
    )?;
    // From this durable point, any interruption must restore the previous runtime.
    journal.units = services.snapshot()?;
    durable_json(&config.journal(), &journal)?;
    progress(journal.status.clone());
    let result = (|| -> Result<()> {
        services.stop()?;
        prepare_mutable(config, &payload, &candidate)?;
        sync_tree(&candidate)?;
        for name in COMPONENTS {
            if config.root.join(name).exists() {
                journal.existed.insert((*name).into());
            }
        }
        durable_json(&config.journal(), &journal)?;
        for name in COMPONENTS {
            let live = config.root.join(name);
            if live.exists() {
                rename(&live, &work.join("backup").join(name))?;
            }
            let next = candidate.join(name);
            if next.exists() {
                rename(&next, &live)?;
            }
            #[cfg(feature = "test-support")]
            if std::env::var("RACKFORGE_UPDATE_FAILPOINT").ok().as_deref() == Some(*name) {
                std::process::exit(86);
            }
        }
        services.start(&journal.units)?;
        services.health(version, &revision, &journal.units, true)?;
        Ok(())
    })();
    if let Err(error) = result {
        journal.status.message = Some(format!(
            "Update failed: {error:#}. Restoring the previous installation."
        ));
        durable_json(&config.journal(), &journal)?;
        return recover_locked(config, services, progress);
    }
    journal.status.phase = UpdatePhase::Installed;
    journal.status.current_version = version.into();
    journal.status.build = revision;
    journal.status.message = Some(format!(
        "RackForge {version} was installed and its services passed the health checks."
    ));
    durable_json(&config.journal(), &journal)?;
    progress(journal.status);
    // Keep the immediately previous installation until another update commits.
    if let Some(previous) = previous_transaction {
        let old = transaction_dir(config, &previous);
        if old != work && old.exists() {
            let _ = fs::remove_dir_all(old);
        }
    }
    let _ = fs::remove_dir_all(work.join("unpacked"));
    let _ = fs::remove_file(work.join("verified.tar.gz"));
    Ok(())
}

/// File restoration is idempotent even if power is lost between two renames.
pub fn restore_files(config: &AgentConfig, journal: &Journal) -> Result<()> {
    let work = transaction_dir(config, journal);
    for name in COMPONENTS {
        let backup = work.join("backup").join(name);
        let live = config.root.join(name);
        if backup.exists() {
            if live.exists() {
                let failed = work.join("failed").join(name);
                if failed.exists() {
                    if failed.is_dir() {
                        fs::remove_dir_all(&failed)?;
                    } else {
                        fs::remove_file(&failed)?;
                    }
                }
                rename(&live, &failed)?;
            }
            rename(&backup, &live)?;
        } else if !journal.existed.contains(*name)
            && !work.join("candidate").join(name).exists()
            && live.exists()
        {
            // Only remove a newly introduced component once swapping started.
            if work.join("backup").exists() {
                rename(&live, &work.join("failed").join(name))?;
            }
        }
    }
    Ok(())
}
pub fn recover(
    config: &AgentConfig,
    services: &impl Services,
    progress: impl Fn(ActivationStatus),
) -> Result<()> {
    let _lock = installation_lock(config)?;
    recover_locked(config, services, progress)
}
fn recover_locked(
    config: &AgentConfig,
    services: &impl Services,
    progress: impl Fn(ActivationStatus),
) -> Result<()> {
    let Some(mut journal) = read_journal(config)? else {
        return Ok(());
    };
    if terminal(&journal.status.phase) {
        return Ok(());
    }
    journal.status.phase = UpdatePhase::Recovering;
    durable_json(&config.journal(), &journal)?;
    progress(journal.status.clone());
    let result = (|| -> Result<()> {
        services.stop()?;
        restore_files(config, &journal)?;
        services.start(&journal.units)?;
        services.health(
            &journal.previous_version,
            &journal.previous_build,
            &journal.units,
            false,
        )
    })();
    journal.status.current_version = journal.previous_version.clone();
    journal.status.build = journal.previous_build.clone();
    if let Err(error) = result {
        journal.status.phase = UpdatePhase::RecoveryFailed;
        journal.status.message = Some(format!("Automatic recovery needs attention: {error:#}"));
    } else {
        journal.status.phase = UpdatePhase::RolledBack;
        journal.status.message = Some(format!(
            "{} Previous RackForge {} restored and checked.",
            journal
                .status
                .message
                .unwrap_or_else(|| "An interrupted update was detected.".into()),
            journal.previous_version
        ));
    }
    durable_json(&config.journal(), &journal)?;
    progress(journal.status);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        config: AgentConfig,
    }
    impl Fixture {
        fn new() -> Self {
            let mut random = [0u8; 8];
            getrandom::fill(&mut random).unwrap();
            let root = std::env::temp_dir()
                .join(format!("rf-activation-{:x}", u64::from_ne_bytes(random)));
            fs::create_dir_all(&root).unwrap();
            Self {
                config: AgentConfig {
                    root,
                    user: "player".into(),
                    group: "player".into(),
                    service_prefix: "rackforge".into(),
                    runtime: "/run/rackforge-installer".into(),
                    web_port: 8787,
                },
            }
        }
        fn journal(&self) -> Journal {
            Journal {
                schema_version: 1,
                transaction: "0123456789abcdef".into(),
                status: ActivationStatus {
                    phase: UpdatePhase::Installing,
                    current_version: "0.1.39".into(),
                    build: "oldbuild".into(),
                    target_version: Some("0.1.40".into()),
                    message: None,
                },
                previous_version: "0.1.39".into(),
                previous_build: "oldbuild".into(),
                units: Vec::new(),
                existed: COMPONENTS.iter().map(|n| (*n).into()).collect(),
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.config.root);
        }
    }
    fn file(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fn contents(root: &Path, name: &str) -> PathBuf {
        if name.contains('.') {
            root.join(name)
        } else {
            root.join(name).join("content")
        }
    }

    #[test]
    fn every_interrupted_rename_restores_the_complete_previous_runtime_idempotently() {
        for cutoff in 0..=COMPONENTS.len() * 2 {
            let fixture = Fixture::new();
            let config = &fixture.config;
            let journal = fixture.journal();
            let work = transaction_dir(config, &journal);
            for name in COMPONENTS {
                file(&contents(&config.root, name), b"old");
                file(&contents(&work.join("candidate"), name), b"new");
            }
            file(&config.root.join("data/custom-preset.json"), b"my preset");
            file(&config.root.join("config/auth.json"), b"my pin state");
            durable_json(&config.journal(), &journal).unwrap();
            let mut steps = 0;
            for name in COMPONENTS {
                if steps >= cutoff {
                    break;
                }
                rename(&config.root.join(name), &work.join("backup").join(name)).unwrap();
                steps += 1;
                if steps >= cutoff {
                    break;
                }
                rename(&work.join("candidate").join(name), &config.root.join(name)).unwrap();
                steps += 1;
            }
            restore_files(config, &journal).unwrap();
            restore_files(config, &journal).unwrap();
            for name in COMPONENTS {
                assert_eq!(
                    fs::read(contents(&config.root, name)).unwrap(),
                    b"old",
                    "cutoff {cutoff}: {name}"
                );
            }
            assert_eq!(
                fs::read(config.root.join("data/custom-preset.json")).unwrap(),
                b"my preset"
            );
            assert_eq!(
                fs::read(config.root.join("config/auth.json")).unwrap(),
                b"my pin state"
            );
        }
    }
    #[test]
    fn interruption_while_preparing_does_not_remove_existing_files() {
        let fixture = Fixture::new();
        let mut journal = fixture.journal();
        journal.existed.clear();
        for name in COMPONENTS {
            file(&contents(&fixture.config.root, name), b"old");
        }
        restore_files(&fixture.config, &journal).unwrap();
        for name in COMPONENTS {
            assert_eq!(
                fs::read(contents(&fixture.config.root, name)).unwrap(),
                b"old"
            );
        }
    }
    #[test]
    fn rollback_removes_new_components_and_survives_another_interruption() {
        let fixture = Fixture::new();
        let mut journal = fixture.journal();
        journal.existed.remove("plugin-store");
        let work = transaction_dir(&fixture.config, &journal);
        file(&work.join("backup/bin/content"), b"old");
        file(&fixture.config.root.join("bin/content"), b"new");
        file(&fixture.config.root.join("plugin-store/content"), b"new");
        restore_files(&fixture.config, &journal).unwrap();
        restore_files(&fixture.config, &journal).unwrap();
        assert_eq!(
            fs::read(fixture.config.root.join("bin/content")).unwrap(),
            b"old"
        );
        assert!(!fixture.config.root.join("plugin-store").exists());
    }
    #[test]
    fn journal_cannot_select_another_path_or_service() {
        let fixture = Fixture::new();
        let mut journal = fixture.journal();
        journal.transaction = "../../outside".into();
        durable_json(&fixture.config.journal(), &journal).unwrap();
        assert!(read_journal(&fixture.config).is_err());
        journal.transaction = "0123456789abcdef".into();
        journal.units.push("sshd.service".into());
        durable_json(&fixture.config.journal(), &journal).unwrap();
        assert!(read_journal(&fixture.config).is_err());
    }
    #[test]
    fn activation_and_manual_installation_share_one_exclusive_lock() {
        let fixture = Fixture::new();
        let held = installation_lock(&fixture.config).unwrap();
        assert!(installation_lock(&fixture.config).is_err());
        drop(held);
        assert!(installation_lock(&fixture.config).is_ok());
    }
    #[test]
    fn updating_a_controller_does_not_reenable_it_or_discard_its_new_version() {
        let fixture = Fixture::new();
        let old = fixture.config.root.join("old");
        let new = fixture.config.root.join("new");
        file(
            &old.join("keyboard.json"),
            br#"{"version":"1.0.0","enabled":false}"#,
        );
        file(
            &new.join("keyboard.json"),
            br#"{"version":"1.1.0","enabled":true,"trust":"official"}"#,
        );
        preserve_controller_enablement(&old, &new).unwrap();
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(new.join("keyboard.json")).unwrap()).unwrap();
        assert_eq!(record["enabled"], false);
        assert_eq!(record["version"], "1.1.0");
        assert_eq!(record["trust"], "official");
    }
    #[test]
    fn signed_tar_still_cannot_create_symbolic_or_hard_links() {
        for kind in [tar::EntryType::Symlink, tar::EntryType::Link] {
            let fixture = Fixture::new();
            let path = fixture.config.root.join("bad.tar.gz");
            let gzip = flate2::write::GzEncoder::new(
                fs::File::create(&path).unwrap(),
                flate2::Compression::default(),
            );
            let mut archive = tar::Builder::new(gzip);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(kind);
            header.set_size(0);
            header.set_link_name("/etc/passwd").unwrap();
            header.set_mode(0o777);
            header.set_cksum();
            archive
                .append_data(&mut header, "rackforge/evil", std::io::empty())
                .unwrap();
            archive.into_inner().unwrap().finish().unwrap();
            assert!(unpack(&path, &fixture.config.root.join("unpacked")).is_err());
        }
    }
    struct FailingHealth;
    impl Services for FailingHealth {
        fn snapshot(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
        fn stop(&self) -> Result<()> {
            Ok(())
        }
        fn start(&self, _: &[String]) -> Result<()> {
            Ok(())
        }
        fn health(&self, _: &str, _: &str, _: &[String], _: bool) -> Result<()> {
            bail!("hardware unavailable")
        }
    }
    #[test]
    fn a_failed_recovery_is_persisted_instead_of_claiming_success() {
        let fixture = Fixture::new();
        let journal = fixture.journal();
        durable_json(&fixture.config.journal(), &journal).unwrap();
        recover(&fixture.config, &FailingHealth, |_| {}).unwrap();
        let status = status(&fixture.config).unwrap();
        assert_eq!(status.phase, UpdatePhase::RecoveryFailed);
        assert!(status.message.unwrap().contains("hardware unavailable"));
    }
}
