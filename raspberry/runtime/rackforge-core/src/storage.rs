use anyhow::{Context, Result, bail};
use rackforge_plugin_api::{ProgramDocument, validate_plugin_identifier};
use std::ffi::OsStr;
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component as PathComponent, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const RECOMMENDED_PROGRAM_SUFFIX: &str = ".rackforge-program.json";

static TEMP_SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddonDirectory {
    pub root: PathBuf,
}

#[derive(Clone, Debug)]
pub struct AddonStorage {
    data_root: PathBuf,
}

impl AddonStorage {
    pub fn new(data_root: impl Into<PathBuf>) -> Self {
        Self {
            data_root: data_root.into(),
        }
    }

    pub fn ensure_addon(&self, plugin_id: &str) -> Result<AddonDirectory> {
        validate_plugin_identifier(plugin_id).context("validating addon namespace")?;
        let root = ensure_root(&self.data_root)?;
        let addons = ensure_child_directory(&root, OsStr::new("addons"))?;
        let addon = ensure_child_directory(&addons, OsStr::new(plugin_id))?;
        Ok(AddonDirectory { root: addon })
    }

    pub fn ensure_directory(&self, plugin_id: &str, relative: &Path) -> Result<PathBuf> {
        let addon = self.ensure_addon(plugin_id)?;
        ensure_relative_directory(&addon.root, relative)
    }

    pub fn write_atomic(&self, plugin_id: &str, relative: &Path, bytes: &[u8]) -> Result<PathBuf> {
        let addon = self.ensure_addon(plugin_id)?;
        let destination = prepare_file_path(&addon.root, relative)?;
        reject_symlink(&destination)?;

        let serial = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
        let file_name = destination
            .file_name()
            .context("addon storage path has no file name")?
            .to_string_lossy();
        let temporary =
            destination.with_file_name(format!(".{file_name}.tmp-{}-{serial}", std::process::id()));
        let write_result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .with_context(|| format!("creating {}", temporary.display()))?;
            file.write_all(bytes)
                .with_context(|| format!("writing {}", temporary.display()))?;
            file.sync_all()
                .with_context(|| format!("syncing {}", temporary.display()))?;
            drop(file);
            replace_file(&temporary, &destination)?;
            sync_directory(
                destination
                    .parent()
                    .context("addon storage file has no parent")?,
            )?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result?;
        Ok(destination)
    }

    pub fn read(&self, plugin_id: &str, relative: &Path) -> Result<Vec<u8>> {
        let addon = self.ensure_addon(plugin_id)?;
        let path = resolve_existing_file(&addon.root, relative)?;
        fs::read(&path).with_context(|| format!("reading {}", path.display()))
    }

    pub fn save_program(&self, relative: &Path, program: &ProgramDocument) -> Result<PathBuf> {
        program.validate().context("validating program")?;
        let mut bytes = serde_json::to_vec_pretty(program).context("serializing program")?;
        bytes.push(b'\n');
        self.write_atomic(&program.plugin_id, relative, &bytes)
    }

    pub fn load_program(&self, plugin_id: &str, relative: &Path) -> Result<ProgramDocument> {
        let bytes = self.read(plugin_id, relative)?;
        let program: ProgramDocument =
            serde_json::from_slice(&bytes).context("parsing program document")?;
        program.validate().context("validating program document")?;
        if program.plugin_id != plugin_id {
            bail!("program identity does not match its addon namespace");
        }
        Ok(program)
    }
}

fn ensure_root(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("RackForge data root cannot be empty");
    }
    fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))?;
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("inspecting {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("RackForge data root must be a real directory");
    }
    fs::canonicalize(path).with_context(|| format!("resolving {}", path.display()))
}

fn validate_relative_path(path: &Path, allow_empty: bool) -> Result<Vec<&OsStr>> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            PathComponent::Normal(part) if !part.is_empty() => parts.push(part),
            _ => bail!(
                "addon storage path must contain only normal relative components: {}",
                path.display()
            ),
        }
    }
    if parts.is_empty() && !allow_empty {
        bail!("addon storage path cannot be empty");
    }
    Ok(parts)
}

fn ensure_relative_directory(root: &Path, relative: &Path) -> Result<PathBuf> {
    let parts = validate_relative_path(relative, true)?;
    let mut directory = root.to_path_buf();
    for part in parts {
        directory = ensure_child_directory(&directory, part)?;
    }
    Ok(directory)
}

fn ensure_child_directory(parent: &Path, name: &OsStr) -> Result<PathBuf> {
    let child = parent.join(name);
    match fs::symlink_metadata(&child) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("storage directory cannot be a symlink: {}", child.display())
        }
        Ok(metadata) if !metadata.is_dir() => {
            bail!("storage path is not a directory: {}", child.display())
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&child).with_context(|| format!("creating {}", child.display()))?;
        }
        Err(error) => return Err(error).with_context(|| format!("inspecting {}", child.display())),
    }
    let canonical =
        fs::canonicalize(&child).with_context(|| format!("resolving {}", child.display()))?;
    if !canonical.starts_with(parent) {
        bail!("storage directory escaped its addon namespace");
    }
    Ok(canonical)
}

fn prepare_file_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    let parts = validate_relative_path(relative, false)?;
    let (file_name, directories) = parts
        .split_last()
        .context("addon storage path has no file name")?;
    let mut parent = root.to_path_buf();
    for directory in directories {
        parent = ensure_child_directory(&parent, directory)?;
    }
    Ok(parent.join(file_name))
}

fn resolve_existing_file(root: &Path, relative: &Path) -> Result<PathBuf> {
    let parts = validate_relative_path(relative, false)?;
    let mut current = root.to_path_buf();
    for part in parts {
        current.push(part);
        let metadata = fs::symlink_metadata(&current)
            .with_context(|| format!("inspecting {}", current.display()))?;
        if metadata.file_type().is_symlink() {
            bail!("addon storage path cannot traverse a symlink");
        }
    }
    if !current.is_file() {
        bail!(
            "addon storage path is not a regular file: {}",
            current.display()
        );
    }
    let canonical =
        fs::canonicalize(&current).with_context(|| format!("resolving {}", current.display()))?;
    if !canonical.starts_with(root) {
        bail!("addon storage file escaped its addon namespace");
    }
    Ok(canonical)
}

fn reject_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("addon storage file cannot be a symlink: {}", path.display())
        }
        Ok(metadata) if !metadata.is_file() => {
            bail!(
                "addon storage path is not a regular file: {}",
                path.display()
            )
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspecting {}", path.display())),
    }
}

#[cfg(unix)]
fn replace_file(temporary: &Path, destination: &Path) -> Result<()> {
    fs::rename(temporary, destination).with_context(|| {
        format!(
            "atomically replacing {} with {}",
            destination.display(),
            temporary.display()
        )
    })
}

#[cfg(not(unix))]
fn replace_file(temporary: &Path, destination: &Path) -> Result<()> {
    if !destination.exists() {
        return fs::rename(temporary, destination)
            .with_context(|| format!("installing {}", destination.display()));
    }
    let backup = destination.with_extension("previous-rackforge-data");
    reject_symlink(&backup)?;
    if backup.exists() {
        fs::remove_file(&backup).with_context(|| format!("removing {}", backup.display()))?;
    }
    fs::rename(destination, &backup)
        .with_context(|| format!("backing up {}", destination.display()))?;
    if let Err(error) = fs::rename(temporary, destination) {
        let _ = fs::rename(&backup, destination);
        return Err(error).with_context(|| format!("installing {}", destination.display()));
    }
    fs::remove_file(&backup).with_context(|| format!("removing {}", backup.display()))
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .with_context(|| format!("syncing directory {}", path.display()))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_plugin_api::PROGRAM_SCHEMA_VERSION;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SERIAL: AtomicU64 = AtomicU64::new(0);

    fn temporary_root() -> PathBuf {
        let serial = TEST_SERIAL.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rackforge-storage-test-{}-{serial}",
            std::process::id()
        ))
    }

    fn program(name: &str) -> ProgramDocument {
        ProgramDocument {
            schema_version: PROGRAM_SCHEMA_VERSION,
            id: "user.piano-strings".into(),
            name: name.into(),
            plugin_id: "org.rackforge.roland-scva".into(),
            plugin_version: "0.1.0".into(),
            plugin_state_version: 2,
            payload_version: 1,
            category: Some("Layered".into()),
            tags: vec!["Piano".into()],
            payload: json!({"layers": []}),
        }
    }

    #[test]
    fn addon_controls_its_internal_layout() {
        let root = temporary_root();
        let storage = AddonStorage::new(&root);
        let addon = storage.ensure_addon("org.rackforge.roland-scva").unwrap();
        assert!(addon.root.is_dir());
        assert_eq!(
            fs::read_dir(&addon.root).unwrap().count(),
            0,
            "RackForge must not impose folders inside an addon namespace"
        );

        let path = Path::new("patches/live/piano-strings.json");
        storage.save_program(path, &program("First name")).unwrap();
        storage
            .save_program(path, &program("Updated name"))
            .unwrap();
        let loaded = storage
            .load_program("org.rackforge.roland-scva", path)
            .unwrap();
        assert_eq!(loaded.name, "Updated name");
        assert!(addon.root.join("patches/live").is_dir());

        storage
            .write_atomic(
                "org.rackforge.roland-scva",
                Path::new("indexes/sounds.db"),
                b"plugin-owned",
            )
            .unwrap();
        assert_eq!(
            storage
                .read("org.rackforge.roland-scva", Path::new("indexes/sounds.db"))
                .unwrap(),
            b"plugin-owned"
        );

        let absolute = fs::canonicalize(&root).unwrap();
        let temporary = fs::canonicalize(std::env::temp_dir()).unwrap();
        assert!(absolute.starts_with(temporary));
        fs::remove_dir_all(absolute).unwrap();
    }

    #[test]
    fn rejects_namespace_and_relative_path_escape() {
        let root = temporary_root();
        let storage = AddonStorage::new(&root);
        assert!(storage.ensure_addon("../escape").is_err());
        assert!(!root.exists());

        storage.ensure_addon("org.rackforge.roland-scva").unwrap();
        assert!(
            storage
                .write_atomic("org.rackforge.roland-scva", Path::new("../outside"), b"no")
                .is_err()
        );
        let absolute = fs::canonicalize(&root).unwrap();
        let temporary = fs::canonicalize(std::env::temp_dir()).unwrap();
        assert!(absolute.starts_with(temporary));
        fs::remove_dir_all(absolute).unwrap();
    }
}
