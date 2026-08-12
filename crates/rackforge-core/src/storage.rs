use anyhow::{Context, Result, bail};
use rackforge_plugin_api::{PreparedProgram, ProgramDocument, validate_plugin_identifier};
use std::ffi::OsStr;
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component as PathComponent, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const RECOMMENDED_PROGRAM_SUFFIX: &str = ".rackforge-program.json";
const PLUGINS_DIRECTORY: &str = "plugins";
const LEGACY_DATA_DIRECTORY: &str = "addons";

static TEMP_SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginDirectory {
    pub root: PathBuf,
}

#[derive(Clone, Debug)]
pub struct PluginStorage {
    data_root: PathBuf,
}

impl PluginStorage {
    pub fn new(data_root: impl Into<PathBuf>) -> Self {
        Self {
            data_root: data_root.into(),
        }
    }

    pub fn ensure_plugin(&self, plugin_id: &str) -> Result<PluginDirectory> {
        validate_plugin_identifier(plugin_id).context("validating plugin namespace")?;
        let root = ensure_root(&self.data_root)?;
        let plugins = ensure_plugins_directory(&root)?;
        let plugin = ensure_child_directory(&plugins, OsStr::new(plugin_id))?;
        Ok(PluginDirectory { root: plugin })
    }

    pub fn ensure_directory(&self, plugin_id: &str, relative: &Path) -> Result<PathBuf> {
        let plugin = self.ensure_plugin(plugin_id)?;
        ensure_relative_directory(&plugin.root, relative)
    }

    pub fn write_atomic(&self, plugin_id: &str, relative: &Path, bytes: &[u8]) -> Result<PathBuf> {
        let plugin = self.ensure_plugin(plugin_id)?;
        let destination = prepare_file_path(&plugin.root, relative)?;
        reject_symlink(&destination)?;

        let serial = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
        let file_name = destination
            .file_name()
            .context("plugin storage path has no file name")?
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
                    .context("plugin storage file has no parent")?,
            )?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result?;
        Ok(destination)
    }

    /// Copies an already-authorized regular file into a plugin namespace and
    /// commits it atomically. The source name and path never become part of
    /// the private resource layout.
    pub fn copy_file_atomic(
        &self,
        plugin_id: &str,
        relative: &Path,
        source: &Path,
    ) -> Result<PathBuf> {
        let metadata = fs::symlink_metadata(source)
            .with_context(|| format!("inspecting resource source {}", source.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("resource source must be a regular file");
        }
        let plugin = self.ensure_plugin(plugin_id)?;
        let destination = prepare_file_path(&plugin.root, relative)?;
        reject_symlink(&destination)?;
        let serial = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
        let file_name = destination
            .file_name()
            .context("plugin storage path has no file name")?
            .to_string_lossy();
        let temporary =
            destination.with_file_name(format!(".{file_name}.tmp-{}-{serial}", std::process::id()));
        let copy_result = (|| -> Result<()> {
            let copied = fs::copy(source, &temporary).with_context(|| {
                format!(
                    "copying resource {} into {}",
                    source.display(),
                    temporary.display()
                )
            })?;
            if copied != metadata.len() {
                bail!("resource source changed while it was being copied");
            }
            OpenOptions::new()
                .write(true)
                .open(&temporary)?
                .sync_all()?;
            replace_file(&temporary, &destination)?;
            sync_directory(
                destination
                    .parent()
                    .context("plugin storage file has no parent")?,
            )?;
            Ok(())
        })();
        if copy_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        copy_result?;
        Ok(destination)
    }

    pub fn read(&self, plugin_id: &str, relative: &Path) -> Result<Vec<u8>> {
        let plugin = self.ensure_plugin(plugin_id)?;
        let path = resolve_existing_file(&plugin.root, relative)?;
        fs::read(&path).with_context(|| format!("reading {}", path.display()))
    }

    pub fn save_program(&self, relative: &Path, program: &ProgramDocument) -> Result<PathBuf> {
        program.validate().context("validating program")?;
        let mut bytes = serde_json::to_vec_pretty(program).context("serializing program")?;
        bytes.push(b'\n');
        self.write_atomic(&program.plugin_id, relative, &bytes)
    }

    /// Persists a plugin-prepared Program and every small derived artifact it
    /// owns. Artifacts are written first and the canonical Program document is
    /// committed last, so readers never observe a new document before its
    /// derived files are available.
    pub fn save_prepared_program(&self, prepared: &PreparedProgram) -> Result<PathBuf> {
        prepared.validate().context("validating prepared program")?;
        for artifact in &prepared.artifacts {
            self.write_atomic(
                &prepared.document.plugin_id,
                Path::new(&artifact.storage_path),
                &artifact.bytes,
            )
            .with_context(|| format!("saving program artifact {}", artifact.storage_path))?;
        }
        self.save_program(Path::new(&prepared.storage_path), &prepared.document)
    }

    pub fn load_program(&self, plugin_id: &str, relative: &Path) -> Result<ProgramDocument> {
        let bytes = self.read(plugin_id, relative)?;
        let program: ProgramDocument =
            serde_json::from_slice(&bytes).context("parsing program document")?;
        program.validate().context("validating program document")?;
        if program.plugin_id != plugin_id {
            bail!("program identity does not match its plugin namespace");
        }
        Ok(program)
    }

    /// Loads every saved program in a plugin namespace in deterministic path
    /// order. Portable plugins cannot read host storage themselves, so the
    /// host uses this to reinstall their program library for each instance.
    pub fn list_programs(&self, plugin_id: &str) -> Result<Vec<ProgramDocument>> {
        let plugin = self.ensure_plugin(plugin_id)?;
        let mut paths = Vec::new();
        collect_program_paths(&plugin.root, &mut paths)?;
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let relative = path
                    .strip_prefix(&plugin.root)
                    .context("saved program escaped its plugin namespace")?;
                self.load_program(plugin_id, relative)
                    .with_context(|| format!("loading saved program {}", path.display()))
            })
            .collect()
    }
}

fn collect_program_paths(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("reading program directory {}", directory.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            bail!(
                "program storage cannot contain a symlink: {}",
                entry.path().display()
            );
        }
        if file_type.is_dir() {
            collect_program_paths(&entry.path(), paths)?;
        } else if file_type.is_file()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(RECOMMENDED_PROGRAM_SUFFIX))
        {
            paths.push(entry.path());
        }
    }
    Ok(())
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

fn ensure_plugins_directory(root: &Path) -> Result<PathBuf> {
    let plugins = root.join(PLUGINS_DIRECTORY);
    let legacy = root.join(LEGACY_DATA_DIRECTORY);

    if !plugins.exists() {
        match fs::symlink_metadata(&legacy) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("legacy plugin data directory cannot be a symlink")
            }
            Ok(metadata) if !metadata.is_dir() => {
                bail!("legacy plugin data path is not a directory")
            }
            Ok(_) => {
                fs::rename(&legacy, &plugins).with_context(|| {
                    format!(
                        "migrating legacy plugin data {} to {}",
                        legacy.display(),
                        plugins.display()
                    )
                })?;
                sync_directory(root)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("inspecting {}", legacy.display()));
            }
        }
    }

    let plugins = ensure_child_directory(root, OsStr::new(PLUGINS_DIRECTORY))?;
    merge_legacy_plugin_directories(root, &plugins)?;
    Ok(plugins)
}

fn merge_legacy_plugin_directories(root: &Path, plugins: &Path) -> Result<()> {
    let legacy = root.join(LEGACY_DATA_DIRECTORY);
    let metadata = match fs::symlink_metadata(&legacy) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("inspecting {}", legacy.display()));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("legacy plugin data root must be a real directory");
    }

    for entry in fs::read_dir(&legacy)
        .with_context(|| format!("reading legacy plugin data {}", legacy.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            bail!(
                "legacy plugin data cannot contain a symlink: {}",
                entry.path().display()
            );
        }
        if !file_type.is_dir() {
            bail!(
                "legacy plugin data contains a non-directory entry: {}",
                entry.path().display()
            );
        }
        let destination = plugins.join(entry.file_name());
        if destination.exists() {
            let destination_type = fs::symlink_metadata(&destination)?.file_type();
            if destination_type.is_symlink() || !destination_type.is_dir() {
                bail!(
                    "plugin data destination is not a real directory: {}",
                    destination.display()
                );
            }
            let source_is_empty = fs::read_dir(entry.path())?.next().is_none();
            let destination_is_empty = fs::read_dir(&destination)?.next().is_none();
            if source_is_empty {
                fs::remove_dir(entry.path())?;
                continue;
            }
            if destination_is_empty {
                fs::remove_dir(&destination)?;
            } else {
                bail!(
                    "both legacy and current plugin data contain {:?}; merge them manually",
                    entry.file_name()
                );
            }
        }
        fs::rename(entry.path(), &destination).with_context(|| {
            format!(
                "migrating legacy plugin directory {} to {}",
                entry.path().display(),
                destination.display()
            )
        })?;
    }
    sync_directory(plugins)?;
    if fs::read_dir(&legacy)?.next().is_none() {
        fs::remove_dir(&legacy)
            .with_context(|| format!("removing empty legacy directory {}", legacy.display()))?;
        sync_directory(root)?;
    }
    Ok(())
}

fn validate_relative_path(path: &Path, allow_empty: bool) -> Result<Vec<&OsStr>> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            PathComponent::Normal(part) if !part.is_empty() => parts.push(part),
            _ => bail!(
                "plugin storage path must contain only normal relative components: {}",
                path.display()
            ),
        }
    }
    if parts.is_empty() && !allow_empty {
        bail!("plugin storage path cannot be empty");
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
        bail!("storage directory escaped its plugin namespace");
    }
    Ok(canonical)
}

fn prepare_file_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    let parts = validate_relative_path(relative, false)?;
    let (file_name, directories) = parts
        .split_last()
        .context("plugin storage path has no file name")?;
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
            bail!("plugin storage path cannot traverse a symlink");
        }
    }
    if !current.is_file() {
        bail!(
            "plugin storage path is not a regular file: {}",
            current.display()
        );
    }
    let canonical =
        fs::canonicalize(&current).with_context(|| format!("resolving {}", current.display()))?;
    if !canonical.starts_with(root) {
        bail!("plugin storage file escaped its plugin namespace");
    }
    Ok(canonical)
}

fn reject_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "plugin storage file cannot be a symlink: {}",
                path.display()
            )
        }
        Ok(metadata) if !metadata.is_file() => {
            bail!(
                "plugin storage path is not a regular file: {}",
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
    fn plugin_controls_its_internal_layout() {
        let root = temporary_root();
        let storage = PluginStorage::new(&root);
        let plugin = storage.ensure_plugin("org.rackforge.roland-scva").unwrap();
        assert!(plugin.root.is_dir());
        assert_eq!(
            fs::read_dir(&plugin.root).unwrap().count(),
            0,
            "RackForge must not impose folders inside a plugin namespace"
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
        assert!(plugin.root.join("patches/live").is_dir());

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
    fn copies_authorized_resources_atomically_into_private_storage() {
        let root = temporary_root();
        let source = root.with_extension("source.bin");
        fs::write(&source, b"recognized-rom").unwrap();
        let storage = PluginStorage::new(&root);
        let destination = storage
            .copy_file_atomic(
                "org.rackforge.roland-scva",
                Path::new("roms/wave.bin"),
                &source,
            )
            .unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"recognized-rom");
        assert!(destination.ends_with(Path::new("roms/wave.bin")));
        fs::remove_file(source).unwrap();
        fs::remove_dir_all(fs::canonicalize(root).unwrap()).unwrap();
    }

    #[test]
    fn saves_prepared_program_artifacts_in_the_plugin_namespace() {
        let root = temporary_root();
        let storage = PluginStorage::new(&root);
        let prepared = PreparedProgram {
            schema_version: rackforge_plugin_api::PROGRAM_EDIT_SCHEMA_VERSION,
            storage_path: "programs/user.test.rackforge-program.json".into(),
            preview_sound_id: "custom.user.test".into(),
            document: program("Artifact test"),
            artifacts: vec![rackforge_plugin_api::ProgramArtifact {
                storage_path: "exports/user.test.syx".into(),
                media_type: "audio/midi".into(),
                bytes: vec![0xf0, 0x42, 0xf7],
            }],
        };

        storage.save_prepared_program(&prepared).unwrap();
        assert_eq!(
            storage
                .read(
                    "org.rackforge.roland-scva",
                    Path::new("exports/user.test.syx")
                )
                .unwrap(),
            vec![0xf0, 0x42, 0xf7]
        );
        assert_eq!(
            storage
                .load_program(
                    "org.rackforge.roland-scva",
                    Path::new("programs/user.test.rackforge-program.json")
                )
                .unwrap()
                .name,
            "Artifact test"
        );
        fs::remove_dir_all(fs::canonicalize(root).unwrap()).unwrap();
    }

    #[test]
    fn lists_saved_program_documents_recursively_and_ignores_other_files() {
        let root = temporary_root();
        let storage = PluginStorage::new(&root);
        storage
            .save_program(
                Path::new("programs/b.rackforge-program.json"),
                &program("Second"),
            )
            .unwrap();
        storage
            .save_program(
                Path::new("programs/nested/a.rackforge-program.json"),
                &program("First"),
            )
            .unwrap();
        storage
            .write_atomic(
                "org.rackforge.roland-scva",
                Path::new("programs/readme.txt"),
                b"not a program",
            )
            .unwrap();

        let programs = storage.list_programs("org.rackforge.roland-scva").unwrap();
        assert_eq!(programs.len(), 2);
        assert_eq!(programs[0].name, "Second");
        assert_eq!(programs[1].name, "First");
        fs::remove_dir_all(fs::canonicalize(root).unwrap()).unwrap();
    }

    #[test]
    fn migrates_the_legacy_data_root_without_losing_plugin_data() {
        let root = temporary_root();
        let legacy_plugin = root
            .join(LEGACY_DATA_DIRECTORY)
            .join("org.rackforge.roland-scva");
        fs::create_dir_all(legacy_plugin.join("custom")).unwrap();
        fs::write(legacy_plugin.join("custom/program.json"), b"preserved").unwrap();
        let legacy_bank = root.join(LEGACY_DATA_DIRECTORY).join("rf-dls/banks");
        fs::create_dir_all(&legacy_bank).unwrap();
        fs::write(legacy_bank.join("gm.dls"), b"bank").unwrap();

        let storage = PluginStorage::new(&root);
        let plugin = storage.ensure_plugin("org.rackforge.roland-scva").unwrap();

        assert_eq!(
            fs::read(plugin.root.join("custom/program.json")).unwrap(),
            b"preserved"
        );
        assert_eq!(
            fs::read(root.join("plugins/rf-dls/banks/gm.dls")).unwrap(),
            b"bank"
        );
        assert!(!root.join(LEGACY_DATA_DIRECTORY).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn merges_legacy_namespaces_when_the_plugin_root_already_exists() {
        let root = temporary_root();
        fs::create_dir_all(root.join("plugins/already.present")).unwrap();
        fs::create_dir_all(
            root.join(LEGACY_DATA_DIRECTORY)
                .join("org.rackforge.roland-scva"),
        )
        .unwrap();

        PluginStorage::new(&root)
            .ensure_plugin("org.rackforge.roland-scva")
            .unwrap();

        assert!(root.join("plugins/already.present").is_dir());
        assert!(root.join("plugins/org.rackforge.roland-scva").is_dir());
        assert!(!root.join(LEGACY_DATA_DIRECTORY).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_to_overwrite_conflicting_legacy_plugin_data() {
        let root = temporary_root();
        let current = root.join("plugins/org.rackforge.roland-scva");
        let legacy = root
            .join(LEGACY_DATA_DIRECTORY)
            .join("org.rackforge.roland-scva");
        fs::create_dir_all(&current).unwrap();
        fs::create_dir_all(&legacy).unwrap();
        fs::write(current.join("current.json"), b"current").unwrap();
        fs::write(legacy.join("legacy.json"), b"legacy").unwrap();

        let error = PluginStorage::new(&root)
            .ensure_plugin("org.rackforge.roland-scva")
            .unwrap_err();

        assert!(error.to_string().contains("merge them manually"));
        assert_eq!(fs::read(current.join("current.json")).unwrap(), b"current");
        assert_eq!(fs::read(legacy.join("legacy.json")).unwrap(), b"legacy");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_namespace_and_relative_path_escape() {
        let root = temporary_root();
        let storage = PluginStorage::new(&root);
        assert!(storage.ensure_plugin("../escape").is_err());
        assert!(!root.exists());

        storage.ensure_plugin("org.rackforge.roland-scva").unwrap();
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
