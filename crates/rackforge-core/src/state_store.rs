use anyhow::{Context, Result, bail};
use rackforge_plugin_api::{
    HOST_PRESET_SCHEMA_VERSION, HostPreset, HostPresetSummary,
    PLUGIN_STATE_REFERENCE_SCHEMA_VERSION, PluginStateReference, validate_plugin_identifier,
    validate_preset_id, validate_preset_name,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_PLUGIN_STATE_BYTES: usize = 1024 * 1024;
const STATE_DIRECTORY: &str = "states";
const PRESET_DIRECTORY: &str = "presets";

#[derive(Debug)]
pub struct PluginStateStore {
    root: Option<PathBuf>,
    memory_blobs: BTreeMap<String, Vec<u8>>,
    memory_presets: BTreeMap<(String, String), HostPreset>,
}

impl PluginStateStore {
    pub fn new(data_root: Option<&Path>) -> Result<Self> {
        let root = data_root.map(|root| root.join(STATE_DIRECTORY));
        if let Some(root) = &root {
            ensure_real_directory(root)?;
            ensure_real_directory(&root.join("blobs"))?;
            ensure_real_directory(&root.join(PRESET_DIRECTORY))?;
        }
        Ok(Self {
            root,
            memory_blobs: BTreeMap::new(),
            memory_presets: BTreeMap::new(),
        })
    }

    pub fn put(
        &mut self,
        plugin_id: &str,
        plugin_version: &str,
        state_version: u32,
        selected_sound_id: Option<String>,
        bytes: &[u8],
    ) -> Result<PluginStateReference> {
        validate_plugin_identifier(plugin_id).context("validating plugin state owner")?;
        if bytes.is_empty() || bytes.len() > MAX_PLUGIN_STATE_BYTES {
            bail!(
                "plugin state size {} is outside 1..={MAX_PLUGIN_STATE_BYTES}",
                bytes.len()
            );
        }
        let digest = state_digest(plugin_id, plugin_version, state_version, bytes);
        let reference = PluginStateReference {
            schema_version: PLUGIN_STATE_REFERENCE_SCHEMA_VERSION,
            plugin_id: plugin_id.into(),
            plugin_version: plugin_version.into(),
            state_version,
            blob_sha256: digest.clone(),
            byte_length: u32::try_from(bytes.len()).context("plugin state length")?,
            selected_sound_id,
        };
        reference
            .validate()
            .context("validating stored plugin state")?;
        if let Some(root) = &self.root {
            let directory = blob_directory(root, &digest)?;
            let destination = directory.join(format!("{digest}.rfstate"));
            if destination.exists() {
                let existing = read_regular_file(&destination, MAX_PLUGIN_STATE_BYTES)?;
                if existing != bytes {
                    bail!("plugin state hash collision for {digest}");
                }
            } else {
                write_new_atomic(&destination, bytes)?;
            }
        } else {
            self.memory_blobs
                .entry(digest.clone())
                .or_insert_with(|| bytes.to_vec());
        }
        Ok(reference)
    }

    pub fn read(&self, reference: &PluginStateReference) -> Result<Vec<u8>> {
        reference
            .validate()
            .context("validating plugin state reference")?;
        let bytes = if let Some(root) = &self.root {
            let path = blob_directory(root, &reference.blob_sha256)?
                .join(format!("{}.rfstate", reference.blob_sha256));
            read_regular_file(&path, MAX_PLUGIN_STATE_BYTES)?
        } else {
            self.memory_blobs
                .get(&reference.blob_sha256)
                .cloned()
                .context("plugin state blob is missing")?
        };
        if bytes.len() != reference.byte_length as usize {
            bail!("plugin state length does not match its reference");
        }
        let digest = state_digest(
            &reference.plugin_id,
            &reference.plugin_version,
            reference.state_version,
            &bytes,
        );
        if digest != reference.blob_sha256 {
            bail!("plugin state checksum does not match its reference");
        }
        Ok(bytes)
    }

    pub fn list_presets(&self, plugin_id: &str) -> Result<Vec<HostPresetSummary>> {
        validate_plugin_identifier(plugin_id).context("validating preset plugin")?;
        let mut presets = self.load_presets(plugin_id)?;
        presets.sort_by(|left, right| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(presets.iter().map(HostPresetSummary::from).collect())
    }

    pub fn preset(&self, plugin_id: &str, preset_id: &str) -> Result<HostPreset> {
        validate_plugin_identifier(plugin_id).context("validating preset plugin")?;
        validate_preset_id(preset_id).context("validating preset id")?;
        let preset = if let Some(root) = &self.root {
            let path = preset_directory(root, plugin_id)?.join(format!("{preset_id}.json"));
            let bytes = read_regular_file(&path, 64 * 1024)?;
            serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?
        } else {
            self.memory_presets
                .get(&(plugin_id.into(), preset_id.into()))
                .cloned()
                .context("RackForge preset does not exist")?
        };
        preset.validate().context("validating RackForge preset")?;
        if preset.plugin_id != plugin_id {
            bail!("RackForge preset belongs to a different plugin");
        }
        Ok(preset)
    }

    pub fn save_preset(&mut self, name: &str, state: PluginStateReference) -> Result<HostPreset> {
        validate_preset_name(name)?;
        state.validate()?;
        let name = name.trim();
        let existing = self
            .load_presets(&state.plugin_id)?
            .into_iter()
            .find(|preset| preset.name.eq_ignore_ascii_case(name));
        let now = unix_milliseconds()?;
        let id = existing
            .as_ref()
            .map(|preset| preset.id.clone())
            .unwrap_or_else(|| unique_preset_id(name, &state, now));
        let preset = HostPreset {
            schema_version: HOST_PRESET_SCHEMA_VERSION,
            id: id.clone(),
            name: name.into(),
            plugin_id: state.plugin_id.clone(),
            created_unix_ms: existing
                .as_ref()
                .map(|preset| preset.created_unix_ms)
                .unwrap_or(now),
            updated_unix_ms: now,
            state,
        };
        preset.validate()?;
        if let Some(root) = &self.root {
            let destination = preset_directory(root, &preset.plugin_id)?.join(format!("{id}.json"));
            let mut bytes = serde_json::to_vec_pretty(&preset)?;
            bytes.push(b'\n');
            write_replace_atomic(&destination, &bytes)?;
        } else {
            self.memory_presets
                .insert((preset.plugin_id.clone(), id), preset.clone());
        }
        Ok(preset)
    }

    pub fn rename_preset(
        &mut self,
        plugin_id: &str,
        preset_id: &str,
        name: &str,
    ) -> Result<HostPreset> {
        validate_plugin_identifier(plugin_id).context("validating preset plugin")?;
        validate_preset_id(preset_id).context("validating preset id")?;
        validate_preset_name(name)?;
        let name = name.trim();
        if self
            .load_presets(plugin_id)?
            .into_iter()
            .any(|preset| preset.id != preset_id && preset.name.eq_ignore_ascii_case(name))
        {
            bail!("a RackForge preset with that name already exists");
        }
        let mut preset = self.preset(plugin_id, preset_id)?;
        preset.name = name.into();
        preset.updated_unix_ms = unix_milliseconds()?;
        preset.validate()?;
        if let Some(root) = &self.root {
            let destination = preset_directory(root, plugin_id)?.join(format!("{preset_id}.json"));
            let mut bytes = serde_json::to_vec_pretty(&preset)?;
            bytes.push(b'\n');
            write_replace_atomic(&destination, &bytes)?;
        } else {
            self.memory_presets
                .insert((plugin_id.into(), preset_id.into()), preset.clone());
        }
        Ok(preset)
    }

    pub fn delete_preset(&mut self, plugin_id: &str, preset_id: &str) -> Result<HostPreset> {
        validate_plugin_identifier(plugin_id).context("validating preset plugin")?;
        validate_preset_id(preset_id).context("validating preset id")?;
        let preset = self.preset(plugin_id, preset_id)?;
        if let Some(root) = &self.root {
            let path = preset_directory(root, plugin_id)?.join(format!("{preset_id}.json"));
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("inspecting {}", path.display()))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                bail!(
                    "preset storage path is not a regular file: {}",
                    path.display()
                );
            }
            fs::remove_file(&path).with_context(|| format!("deleting {}", path.display()))?;
            sync_parent(&path)?;
        } else {
            self.memory_presets
                .remove(&(plugin_id.into(), preset_id.into()));
        }
        Ok(preset)
    }

    fn load_presets(&self, plugin_id: &str) -> Result<Vec<HostPreset>> {
        if let Some(root) = &self.root {
            let directory = preset_directory(root, plugin_id)?;
            let mut paths = Vec::new();
            for entry in fs::read_dir(&directory)? {
                let entry = entry?;
                let file_type = entry.file_type()?;
                if file_type.is_symlink() || !file_type.is_file() {
                    bail!("preset storage contains a non-regular file");
                }
                if entry.path().extension().and_then(|value| value.to_str()) == Some("json") {
                    paths.push(entry.path());
                }
            }
            paths.sort();
            paths
                .into_iter()
                .map(|path| {
                    let bytes = read_regular_file(&path, 64 * 1024)?;
                    let preset: HostPreset = serde_json::from_slice(&bytes)
                        .with_context(|| format!("parsing {}", path.display()))?;
                    preset.validate()?;
                    if preset.plugin_id != plugin_id {
                        bail!("preset {} is stored under the wrong plugin", preset.id);
                    }
                    Ok(preset)
                })
                .collect()
        } else {
            Ok(self
                .memory_presets
                .iter()
                .filter(|((owner, _), _)| owner == plugin_id)
                .map(|(_, preset)| preset.clone())
                .collect())
        }
    }
}

fn state_digest(plugin_id: &str, plugin_version: &str, state_version: u32, bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"rackforge-plugin-state-v1\0");
    hash.update(plugin_id.as_bytes());
    hash.update([0]);
    hash.update(plugin_version.as_bytes());
    hash.update([0]);
    hash.update(state_version.to_le_bytes());
    hash.update(bytes);
    format!("{:x}", hash.finalize())
}

fn blob_directory(root: &Path, digest: &str) -> Result<PathBuf> {
    let prefix = digest.get(..2).context("invalid plugin state digest")?;
    let directory = root.join("blobs").join(prefix);
    ensure_real_directory(&directory)?;
    Ok(directory)
}

fn preset_directory(root: &Path, plugin_id: &str) -> Result<PathBuf> {
    validate_plugin_identifier(plugin_id).context("validating preset namespace")?;
    let directory = root.join(PRESET_DIRECTORY).join(plugin_id);
    ensure_real_directory(&directory)?;
    Ok(directory)
}

fn ensure_real_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!(
            "state storage path must be a real directory: {}",
            path.display()
        );
    }
    Ok(())
}

fn read_regular_file(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("inspecting {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "state storage path is not a regular file: {}",
            path.display()
        );
    }
    if metadata.len() == 0 || metadata.len() > maximum as u64 {
        bail!("state storage file has invalid size: {}", path.display());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)?
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn write_new_atomic(destination: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = temporary_path(destination);
    let mut file = secure_open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, destination)
        .with_context(|| format!("publishing {}", destination.display()))?;
    sync_parent(destination)
}

fn write_replace_atomic(destination: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = temporary_path(destination);
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    let mut file = secure_open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    replace_file(&temporary, destination)?;
    sync_parent(destination)
}

fn secure_open(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    options
        .open(path)
        .with_context(|| format!("creating {}", path.display()))
}

/// Names the scratch file an atomic write goes through.
fn temporary_path(destination: &Path) -> PathBuf {
    let name = destination
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    destination.with_file_name(format!(
        ".{name}.tmp.{}",
        crate::storage::writer_discriminator()
    ))
}

#[cfg(unix)]
fn replace_file(temporary: &Path, destination: &Path) -> Result<()> {
    fs::rename(temporary, destination).context("atomically replacing state document")
}

#[cfg(not(unix))]
fn replace_file(temporary: &Path, destination: &Path) -> Result<()> {
    if !destination.exists() {
        return fs::rename(temporary, destination).context("installing state document");
    }
    let backup = destination.with_extension("previous-rackforge-state");
    if backup.exists() {
        fs::remove_file(&backup)?;
    }
    fs::rename(destination, &backup)?;
    if let Err(error) = fs::rename(temporary, destination) {
        let _ = fs::rename(&backup, destination);
        return Err(error).context("installing state document");
    }
    fs::remove_file(backup)?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<()> {
    File::open(path.parent().context("state file has no parent")?)?
        .sync_all()
        .context("syncing state directory")
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<()> {
    Ok(())
}

fn unix_milliseconds() -> Result<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

fn unique_preset_id(name: &str, state: &PluginStateReference, now: u64) -> String {
    let mut slug = name
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    slug = slug.trim_matches('-').chars().take(48).collect();
    if slug.is_empty() {
        slug = "preset".into();
    }
    let mut hash = Sha256::new();
    hash.update(name.as_bytes());
    hash.update(state.blob_sha256.as_bytes());
    hash.update(now.to_le_bytes());
    let suffix = format!("{:x}", hash.finalize());
    format!("{slug}-{}", &suffix[..12])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "rackforge-state-store-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn state_and_named_preset_round_trip_in_memory() {
        let mut store = PluginStateStore::new(None).unwrap();
        let state = store
            .put(
                "org.rackforge.rf-dls",
                "0.1.0",
                3,
                Some("dls.piano".into()),
                b"opaque plugin bytes",
            )
            .unwrap();
        assert_eq!(store.read(&state).unwrap(), b"opaque plugin bytes");
        let preset = store.save_preset("Warm Piano", state).unwrap();
        assert_eq!(
            store.preset("org.rackforge.rf-dls", &preset.id).unwrap(),
            preset
        );
        assert_eq!(store.list_presets("org.rackforge.rf-dls").unwrap().len(), 1);

        let renamed = store
            .rename_preset("org.rackforge.rf-dls", &preset.id, "Stage Piano")
            .unwrap();
        assert_eq!(renamed.name, "Stage Piano");
        assert_eq!(renamed.id, preset.id);
        assert_eq!(renamed.state, preset.state);
        let deleted = store
            .delete_preset("org.rackforge.rf-dls", &preset.id)
            .unwrap();
        assert_eq!(deleted.id, preset.id);
        assert!(
            store
                .list_presets("org.rackforge.rf-dls")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn disk_store_filters_presets_and_detects_blob_tampering() {
        let root = temporary_root();
        let mut store = PluginStateStore::new(Some(&root)).unwrap();
        let first = store
            .put("org.rackforge.rf-dls", "0.1.0", 3, None, b"first state")
            .unwrap();
        let second = store
            .put("org.rackforge.rf-dls", "0.1.0", 3, None, b"second state")
            .unwrap();
        let original = store.save_preset("Stage", first).unwrap();
        let updated = store.save_preset("stage", second.clone()).unwrap();
        assert_eq!(original.id, updated.id);
        assert_eq!(store.list_presets("org.rackforge.rf-dls").unwrap().len(), 1);
        assert!(
            store
                .list_presets("org.rackforge.unrelated")
                .unwrap()
                .is_empty()
        );

        let blob = root
            .join(STATE_DIRECTORY)
            .join("blobs")
            .join(&second.blob_sha256[..2])
            .join(format!("{}.rfstate", second.blob_sha256));
        fs::write(blob, b"tampered").unwrap();
        assert!(store.read(&second).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
