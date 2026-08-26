//! Signed repository catalogs and safe `.rfplugin` installation.
//!
//! This code intentionally performs no dynamic loading. Downloading and
//! installing a package is separate from activating native code in an audio
//! graph.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, VerifyingKey};
use rackforge_plugin_api::{
    ParameterSchema, PluginKind, PluginManifest, PresetCatalog, RuntimeDescriptor,
    validate_branding_asset, validate_branding_assets, validate_plugin_identifier,
};
use semver::Version;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use thiserror::Error;
use url::Url;
use zip::ZipArchive;

pub const REPOSITORY_SCHEMA_VERSION: u32 = 1;
pub const REPOSITORY_CONFIG_SCHEMA_VERSION: u32 = 1;
pub const MAX_INDEX_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_EXPANDED_PACKAGE_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_PACKAGE_ENTRIES: usize = 16_384;
const MAX_PORTABLE_METADATA_BYTES: u64 = 1024 * 1024;
const PLUGIN_ACTIVATION_SCHEMA_VERSION: u32 = 1;
const PLUGIN_ACTIVATION_FILE: &str = "activation.json";

static TEMP_SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryFile {
    pub schema_version: u32,
    pub repositories: Vec<RepositoryConfig>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub public_key: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub allow_insecure_http: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryIndex {
    pub schema_version: u32,
    pub repository_id: String,
    pub name: String,
    pub generated_at: String,
    pub plugins: Vec<RepositoryPlugin>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryPlugin {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    pub releases: Vec<PluginRelease>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRelease {
    pub version: String,
    pub published_at: String,
    pub artifacts: Vec<PluginArtifact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginArtifact {
    pub platform: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationRecord {
    pub schema_version: u32,
    pub plugin_id: String,
    pub version: String,
    pub platform: String,
    pub repository_id: String,
    pub artifact_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedRepository {
    pub config: RepositoryConfig,
    pub index_url: Url,
    pub index: RepositoryIndex,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedArtifact {
    pub repository_id: String,
    pub plugin: RepositoryPlugin,
    pub release: PluginRelease,
    pub artifact: PluginArtifact,
    pub url: Url,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledPackage {
    pub path: PathBuf,
    pub record: InstallationRecord,
    pub already_installed: bool,
}

/// Persistent package enablement, deliberately separate from installation.
///
/// A missing document means a pre-enable/disable RackForge installation and
/// therefore keeps every existing package enabled. As soon as the first new
/// package is installed, the current package set is materialized and the new
/// package remains disabled until the user explicitly activates it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PluginActivationDocument {
    schema_version: u32,
    enabled_plugins: BTreeSet<String>,
}

/// Returns whether an installed managed package is enabled for host loading.
/// Legacy stores without an activation document preserve their old behavior.
pub fn plugin_is_enabled(
    store_root: impl AsRef<Path>,
    plugin_id: &str,
) -> Result<bool, RepositoryError> {
    validate_plugin_identifier(plugin_id)
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    let root = ensure_real_directory(store_root.as_ref())?;
    Ok(read_plugin_activation_document(&root)?
        .is_none_or(|document| document.enabled_plugins.contains(plugin_id)))
}

/// Enables or disables one managed package without changing its immutable
/// installation. The first mutation migrates legacy stores by enabling every
/// package that was already installed.
pub fn set_plugin_enabled(
    store_root: impl AsRef<Path>,
    plugin_id: &str,
    enabled: bool,
) -> Result<(), RepositoryError> {
    validate_plugin_identifier(plugin_id)
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    let root = ensure_real_directory(store_root.as_ref())?;
    let mut document = activation_document_or_legacy_default(&root)?;
    if enabled {
        document.enabled_plugins.insert(plugin_id.to_owned());
    } else {
        document.enabled_plugins.remove(plugin_id);
    }
    write_json_atomic(&root.join(PLUGIN_ACTIVATION_FILE), &document)
}

fn read_plugin_activation_document(
    root: &Path,
) -> Result<Option<PluginActivationDocument>, RepositoryError> {
    let path = root.join(PLUGIN_ACTIVATION_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let document: PluginActivationDocument = serde_json::from_slice(&bytes)
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    if document.schema_version != PLUGIN_ACTIVATION_SCHEMA_VERSION {
        return Err(RepositoryError::InvalidPackage(format!(
            "unsupported plugin activation schema {}",
            document.schema_version
        )));
    }
    for plugin_id in &document.enabled_plugins {
        validate_plugin_identifier(plugin_id)
            .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    }
    Ok(Some(document))
}

fn activation_document_or_legacy_default(
    root: &Path,
) -> Result<PluginActivationDocument, RepositoryError> {
    if let Some(document) = read_plugin_activation_document(root)? {
        return Ok(document);
    }
    Ok(PluginActivationDocument {
        schema_version: PLUGIN_ACTIVATION_SCHEMA_VERSION,
        enabled_plugins: installed_plugin_ids(root)?,
    })
}

fn installed_plugin_ids(root: &Path) -> Result<BTreeSet<String>, RepositoryError> {
    let packages_root = ensure_real_child(root, "packages")?;
    let mut ids = BTreeSet::new();
    for entry in fs::read_dir(packages_root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let plugin_id = entry.file_name().to_string_lossy().into_owned();
        if validate_plugin_identifier(&plugin_id).is_ok() {
            ids.insert(plugin_id);
        }
    }
    Ok(ids)
}

fn prepare_new_plugin_disabled(root: &Path) -> Result<(), RepositoryError> {
    if read_plugin_activation_document(root)?.is_none() {
        let document = activation_document_or_legacy_default(root)?;
        write_json_atomic(&root.join(PLUGIN_ACTIVATION_FILE), &document)?;
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UninstalledPackage {
    pub plugin_id: String,
    pub removed_versions: usize,
    /// A renamed tombstone remains when the operating system still has a
    /// plugin binary open. It is outside the package catalog and can be
    /// cleaned after the host exits without making the plugin visible again.
    pub cleanup_pending: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PluginUserDataRemovalOptions {
    pub presets: bool,
    pub plugin_data: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PluginUserDataRemoval {
    pub preset_files_removed: usize,
    pub data_namespaces_removed: usize,
}

/// Retries deletion of packages that were atomically removed from discovery
/// while a host process still had a native module mapped.
pub fn cleanup_uninstall_tombstones(
    store_root: impl AsRef<Path>,
) -> Result<usize, RepositoryError> {
    let root = ensure_real_directory(store_root.as_ref())?;
    let trash_root = ensure_real_child(&root, ".uninstall")?;
    let mut removed = 0;
    for entry in fs::read_dir(&trash_root)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        if fs::remove_dir_all(entry.path()).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Metadata obtained by fully extracting and validating a user-selected
/// `.rfplugin`, without making it an installed package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalPackageInspection {
    pub plugin_id: String,
    pub plugin_name: String,
    pub vendor: String,
    pub version: String,
    pub description: Option<String>,
    pub kind: PluginKind,
    pub platform: String,
    pub portable: bool,
    pub artifact_sha256: String,
    pub archive_bytes: u64,
    pub branding: Option<LocalPackageBrandingPreview>,
}

/// Branding bytes extracted from a fully validated package for a host-owned
/// installation preview. The browser never receives an archive path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalPackageBrandingPreview {
    pub banner_png: Vec<u8>,
    pub background_color: Option<String>,
    pub accent_color: Option<String>,
}

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("invalid repository configuration: {0}")]
    InvalidConfig(String),
    #[error("repository transport rejected: {0}")]
    UnsafeTransport(String),
    #[error("repository request failed: {0}")]
    Request(String),
    #[error("repository response exceeded {0} bytes")]
    ResponseTooLarge(usize),
    #[error("invalid repository signature")]
    InvalidSignature,
    #[error("invalid repository catalog: {0}")]
    InvalidCatalog(String),
    #[error("plugin {0:?} was not found in the repository")]
    PluginNotFound(String),
    #[error("plugin {plugin:?} has no artifact for {platform:?}")]
    PlatformUnavailable { plugin: String, platform: String },
    #[error("artifact integrity check failed: {0}")]
    Integrity(String),
    #[error("unsafe plugin archive: {0}")]
    UnsafeArchive(String),
    #[error("invalid plugin package: {0}")]
    InvalidPackage(String),
    #[error("immutable plugin version already exists with different contents")]
    ImmutableConflict,
    #[error("plugin installation was cancelled")]
    InstallationCancelled,
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

fn enabled_by_default() -> bool {
    true
}

impl RepositoryFile {
    pub fn parse_toml(bytes: &[u8]) -> Result<Self, RepositoryError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|error| RepositoryError::InvalidConfig(error.to_string()))?;
        let file: Self = toml::from_str(text)
            .map_err(|error| RepositoryError::InvalidConfig(error.to_string()))?;
        file.validate()?;
        Ok(file)
    }

    pub fn validate(&self) -> Result<(), RepositoryError> {
        if self.schema_version != REPOSITORY_CONFIG_SCHEMA_VERSION {
            return Err(RepositoryError::InvalidConfig(format!(
                "unsupported schema {}",
                self.schema_version
            )));
        }
        let mut ids = BTreeSet::new();
        for repository in &self.repositories {
            repository.validate()?;
            if !ids.insert(repository.id.as_str()) {
                return Err(RepositoryError::InvalidConfig(format!(
                    "duplicate repository {:?}",
                    repository.id
                )));
            }
        }
        Ok(())
    }
}

impl RepositoryConfig {
    pub fn validate(&self) -> Result<Url, RepositoryError> {
        validate_dotted_identifier(&self.id)
            .map_err(|message| RepositoryError::InvalidConfig(message.into()))?;
        validate_text("repository name", &self.name, 128)
            .map_err(RepositoryError::InvalidConfig)?;
        decode_public_key(&self.public_key)?;
        let url = Url::parse(&self.base_url)
            .map_err(|error| RepositoryError::InvalidConfig(error.to_string()))?;
        if url.username() != "" || url.password().is_some() || url.fragment().is_some() {
            return Err(RepositoryError::InvalidConfig(
                "repository URL cannot contain credentials or a fragment".into(),
            ));
        }
        match url.scheme() {
            "https" => {}
            "http" if self.allow_insecure_http => {}
            "http" => {
                return Err(RepositoryError::UnsafeTransport(
                    "HTTP requires allow_insecure_http = true".into(),
                ));
            }
            scheme => {
                return Err(RepositoryError::UnsafeTransport(format!(
                    "unsupported URL scheme {scheme:?}"
                )));
            }
        }
        Ok(url)
    }
}

impl RepositoryIndex {
    pub fn validate(&self, expected_id: &str) -> Result<(), RepositoryError> {
        if self.schema_version != REPOSITORY_SCHEMA_VERSION {
            return Err(RepositoryError::InvalidCatalog(format!(
                "unsupported schema {}",
                self.schema_version
            )));
        }
        if self.repository_id != expected_id {
            return Err(RepositoryError::InvalidCatalog(
                "catalog identity does not match the configured repository".into(),
            ));
        }
        validate_text("repository name", &self.name, 128)
            .map_err(RepositoryError::InvalidCatalog)?;
        validate_text("generated_at", &self.generated_at, 64)
            .map_err(RepositoryError::InvalidCatalog)?;
        let mut plugin_ids = BTreeSet::new();
        for plugin in &self.plugins {
            validate_plugin_identifier(&plugin.id)
                .map_err(|error| RepositoryError::InvalidCatalog(error.to_string()))?;
            if !plugin_ids.insert(plugin.id.as_str()) {
                return Err(RepositoryError::InvalidCatalog(format!(
                    "duplicate plugin {:?}",
                    plugin.id
                )));
            }
            validate_text("plugin name", &plugin.name, 128)
                .map_err(RepositoryError::InvalidCatalog)?;
            validate_text("plugin summary", &plugin.summary, 1024)
                .map_err(RepositoryError::InvalidCatalog)?;
            validate_text("plugin license", &plugin.license, 128)
                .map_err(RepositoryError::InvalidCatalog)?;
            if plugin.releases.is_empty() {
                return Err(RepositoryError::InvalidCatalog(format!(
                    "plugin {:?} has no releases",
                    plugin.id
                )));
            }
            let mut versions = BTreeSet::new();
            for release in &plugin.releases {
                Version::parse(&release.version).map_err(|error| {
                    RepositoryError::InvalidCatalog(format!(
                        "invalid version {:?}: {error}",
                        release.version
                    ))
                })?;
                if !versions.insert(release.version.as_str()) {
                    return Err(RepositoryError::InvalidCatalog(format!(
                        "duplicate release {:?}",
                        release.version
                    )));
                }
                validate_text("published_at", &release.published_at, 64)
                    .map_err(RepositoryError::InvalidCatalog)?;
                if release.artifacts.is_empty() {
                    return Err(RepositoryError::InvalidCatalog(format!(
                        "release {:?} has no artifacts",
                        release.version
                    )));
                }
                let mut platforms = BTreeSet::new();
                for artifact in &release.artifacts {
                    validate_simple_identifier(&artifact.platform)
                        .map_err(|message| RepositoryError::InvalidCatalog(message.into()))?;
                    if !platforms.insert(artifact.platform.as_str()) {
                        return Err(RepositoryError::InvalidCatalog(format!(
                            "duplicate platform {:?}",
                            artifact.platform
                        )));
                    }
                    if artifact.size == 0 || artifact.size > MAX_PACKAGE_BYTES {
                        return Err(RepositoryError::InvalidCatalog(
                            "artifact size is outside supported limits".into(),
                        ));
                    }
                    validate_sha256(&artifact.sha256)?;
                    validate_artifact_url(&artifact.url)?;
                }
            }
        }
        Ok(())
    }
}

pub fn verify_catalog(
    config: &RepositoryConfig,
    index_bytes: &[u8],
    signature_bytes: &[u8],
) -> Result<RepositoryIndex, RepositoryError> {
    config.validate()?;
    if index_bytes.len() > MAX_INDEX_BYTES {
        return Err(RepositoryError::ResponseTooLarge(MAX_INDEX_BYTES));
    }
    let key = decode_public_key(&config.public_key)?;
    let signature_text = std::str::from_utf8(signature_bytes)
        .map_err(|_| RepositoryError::InvalidSignature)?
        .trim();
    let signature_bytes = STANDARD
        .decode(signature_text)
        .map_err(|_| RepositoryError::InvalidSignature)?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| RepositoryError::InvalidSignature)?;
    key.verify_strict(index_bytes, &signature)
        .map_err(|_| RepositoryError::InvalidSignature)?;
    let index: RepositoryIndex = serde_json::from_slice(index_bytes)
        .map_err(|error| RepositoryError::InvalidCatalog(error.to_string()))?;
    index.validate(&config.id)?;
    Ok(index)
}

pub fn fetch_repository(config: &RepositoryConfig) -> Result<VerifiedRepository, RepositoryError> {
    let base_url = config.validate()?;
    let index_url = directory_url(&base_url)?
        .join("v1/index.json")
        .map_err(|error| {
            RepositoryError::InvalidConfig(format!("cannot form catalog URL: {error}"))
        })?;
    let signature_url = directory_url(&base_url)?
        .join("v1/index.json.sig")
        .map_err(|error| {
            RepositoryError::InvalidConfig(format!("cannot form signature URL: {error}"))
        })?;
    let index_bytes = download_limited(&index_url, MAX_INDEX_BYTES)?;
    let signature_bytes = download_limited(&signature_url, 1024)?;
    let index = verify_catalog(config, &index_bytes, &signature_bytes)?;
    Ok(VerifiedRepository {
        config: config.clone(),
        index_url,
        index,
    })
}

impl VerifiedRepository {
    pub fn select(
        &self,
        plugin_id: &str,
        requested_version: Option<&str>,
        platform: &str,
    ) -> Result<SelectedArtifact, RepositoryError> {
        let plugin = self
            .index
            .plugins
            .iter()
            .find(|plugin| plugin.id == plugin_id)
            .ok_or_else(|| RepositoryError::PluginNotFound(plugin_id.into()))?;
        let mut candidates: Vec<_> = plugin
            .releases
            .iter()
            .filter(|release| requested_version.is_none_or(|value| release.version == value))
            .filter_map(|release| {
                release
                    .artifacts
                    .iter()
                    .find(|artifact| artifact.platform == "wasm-v1")
                    .or_else(|| {
                        release
                            .artifacts
                            .iter()
                            .find(|artifact| artifact.platform == platform)
                    })
                    .and_then(|artifact| {
                        Version::parse(&release.version)
                            .ok()
                            .map(|version| (version, release, artifact))
                    })
            })
            .collect();
        candidates.sort_by(|left, right| right.0.cmp(&left.0));
        let (_, release, artifact) =
            candidates
                .first()
                .ok_or_else(|| RepositoryError::PlatformUnavailable {
                    plugin: plugin_id.into(),
                    platform: platform.into(),
                })?;
        let url = self.index_url.join(&artifact.url).map_err(|error| {
            RepositoryError::InvalidCatalog(format!("invalid artifact URL: {error}"))
        })?;
        if url.origin() != self.index_url.origin() {
            return Err(RepositoryError::UnsafeTransport(
                "cross-origin artifact URL rejected".into(),
            ));
        }
        Ok(SelectedArtifact {
            repository_id: self.index.repository_id.clone(),
            plugin: plugin.clone(),
            release: (*release).clone(),
            artifact: (*artifact).clone(),
            url,
        })
    }

    pub fn download(&self, selected: &SelectedArtifact) -> Result<Vec<u8>, RepositoryError> {
        let limit = usize::try_from(selected.artifact.size)
            .map_err(|_| RepositoryError::Integrity("artifact is too large".into()))?;
        let bytes = download_limited(&selected.url, limit)?;
        verify_artifact_bytes(&selected.artifact, &bytes)?;
        Ok(bytes)
    }
}

pub fn verify_artifact_bytes(
    artifact: &PluginArtifact,
    bytes: &[u8],
) -> Result<(), RepositoryError> {
    if bytes.len() as u64 != artifact.size {
        return Err(RepositoryError::Integrity(format!(
            "expected {} bytes, received {}",
            artifact.size,
            bytes.len()
        )));
    }
    let digest = hex_digest(Sha256::digest(bytes).as_slice());
    if digest != artifact.sha256 {
        return Err(RepositoryError::Integrity(
            "SHA-256 digest does not match signed catalog".into(),
        ));
    }
    Ok(())
}

pub fn install_archive(
    store_root: impl AsRef<Path>,
    selected: &SelectedArtifact,
    bytes: &[u8],
) -> Result<InstalledPackage, RepositoryError> {
    verify_artifact_bytes(&selected.artifact, bytes)?;
    let root = ensure_real_directory(store_root.as_ref())?;
    let packages_root = ensure_real_child(&root, "packages")?;
    let records_root = ensure_real_child(&root, "records")?;
    prepare_new_plugin_disabled(&root)?;
    let plugin_root = ensure_real_child(&packages_root, &selected.plugin.id)?;
    let plugin_records = ensure_real_child(&records_root, &selected.plugin.id)?;
    let destination = plugin_root.join(&selected.release.version);
    let record_path = plugin_records.join(format!("{}.json", selected.release.version));
    let record = InstallationRecord {
        schema_version: 1,
        plugin_id: selected.plugin.id.clone(),
        version: selected.release.version.clone(),
        platform: selected.artifact.platform.clone(),
        repository_id: selected_repository_id(selected),
        artifact_sha256: selected.artifact.sha256.clone(),
    };

    if destination.exists() {
        let existing = read_installation_record(&record_path)?;
        if existing == record {
            return Ok(InstalledPackage {
                path: destination,
                record,
                already_installed: true,
            });
        }
        return Err(RepositoryError::ImmutableConflict);
    }

    let serial = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
    let stage = root.join(format!(
        ".install-{}-{}-{serial}",
        writer_discriminator(),
        selected.plugin.id
    ));
    if stage.exists() {
        return Err(RepositoryError::UnsafeArchive(
            "staging path already exists".into(),
        ));
    }
    fs::create_dir(&stage)?;
    let result = (|| {
        extract_archive(bytes, &stage)?;
        validate_extracted_package(&stage, selected)?;
        fs::rename(&stage, &destination)?;
        if let Err(error) = write_json_atomic(&record_path, &record) {
            // The destination was created by this transaction and has already
            // passed archive safety checks. Without its record it is not a
            // valid immutable installation, so roll it back completely.
            let _ = fs::remove_dir_all(&destination);
            return Err(error);
        }
        Ok(InstalledPackage {
            path: destination,
            record,
            already_installed: false,
        })
    })();
    if stage.exists() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

/// Installs a user-selected `.rfplugin` without requiring a signed repository
/// catalog. Archive safety, manifest validation and immutable version semantics
/// are identical to repository installations; the record explicitly marks the
/// package as local.
pub fn install_local_archive(
    store_root: impl AsRef<Path>,
    bytes: &[u8],
) -> Result<InstalledPackage, RepositoryError> {
    install_local_archive_cancellable(store_root, bytes, &AtomicBool::new(false))
}

/// Installs a local package while allowing the host to cancel before the
/// immutable package is committed. Extraction checks the flag between chunks,
/// and the staging directory is removed on every cancellation path.
pub fn install_local_archive_cancellable(
    store_root: impl AsRef<Path>,
    bytes: &[u8],
    cancelled: &AtomicBool,
) -> Result<InstalledPackage, RepositoryError> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(RepositoryError::Integrity(
            "local artifact size is outside supported limits".into(),
        ));
    }
    ensure_installation_not_cancelled(cancelled)?;
    let root = ensure_real_directory(store_root.as_ref())?;
    let packages_root = ensure_real_child(&root, "packages")?;
    let records_root = ensure_real_child(&root, "records")?;
    let serial = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
    let stage = root.join(format!(
        ".install-local-{}-{serial}",
        writer_discriminator()
    ));
    if stage.exists() {
        return Err(RepositoryError::UnsafeArchive(
            "staging path already exists".into(),
        ));
    }
    fs::create_dir(&stage)?;
    let result = (|| {
        extract_archive_cancellable(bytes, &stage, cancelled)?;
        ensure_installation_not_cancelled(cancelled)?;
        let manifest = read_extracted_manifest(&stage)?;
        let platform = if manifest.portable_component().is_some() {
            "wasm-v1"
        } else {
            repository_platform_key()?
        };
        validate_extracted_payload(&stage, &manifest, platform)?;
        ensure_installation_not_cancelled(cancelled)?;
        prepare_new_plugin_disabled(&root)?;

        let plugin_root = ensure_real_child(&packages_root, &manifest.id)?;
        let plugin_records = ensure_real_child(&records_root, &manifest.id)?;
        let destination = plugin_root.join(&manifest.version);
        let record_path = plugin_records.join(format!("{}.json", manifest.version));
        let record = InstallationRecord {
            schema_version: 1,
            plugin_id: manifest.id,
            version: manifest.version,
            platform: platform.into(),
            repository_id: "local".into(),
            artifact_sha256: hex_digest(Sha256::digest(bytes).as_slice()),
        };

        if destination.exists() {
            let existing = read_installation_record(&record_path)?;
            if existing == record {
                return Ok(InstalledPackage {
                    path: destination,
                    record,
                    already_installed: true,
                });
            }
            return Err(RepositoryError::ImmutableConflict);
        }

        // Rename is the transaction commit. Cancellation is deliberately
        // checked immediately before it; after this point the record must be
        // written so discovery can never observe a half-installed package.
        ensure_installation_not_cancelled(cancelled)?;
        fs::rename(&stage, &destination)?;
        if let Err(error) = write_json_atomic(&record_path, &record) {
            let _ = fs::remove_dir_all(&destination);
            return Err(error);
        }
        Ok(InstalledPackage {
            path: destination,
            record,
            already_installed: false,
        })
    })();
    if stage.exists() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

/// Removes every managed version and installation record for one plugin.
///
/// Plugin-owned data is deliberately outside `plugin-store` and is never
/// touched here. Package directories are renamed out of the catalog before
/// deletion, so a mapped native binary on Windows cannot leave a half-visible
/// installation behind.
pub fn uninstall_plugin(
    store_root: impl AsRef<Path>,
    plugin_id: &str,
) -> Result<UninstalledPackage, RepositoryError> {
    validate_plugin_identifier(plugin_id)
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    let root = ensure_real_directory(store_root.as_ref())?;
    let packages_root = ensure_real_child(&root, "packages")?;
    let records_root = ensure_real_child(&root, "records")?;
    let trash_root = ensure_real_child(&root, ".uninstall")?;
    let package_root = packages_root.join(plugin_id);
    let record_root = records_root.join(plugin_id);

    let package_metadata = match fs::symlink_metadata(&package_root) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => metadata,
        Ok(_) => {
            return Err(RepositoryError::UnsafeArchive(
                "managed plugin path is not a real directory".into(),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(RepositoryError::PluginNotFound(plugin_id.into()));
        }
        Err(error) => return Err(error.into()),
    };
    let _ = package_metadata;
    let removed_versions = fs::read_dir(&package_root)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .count();

    let serial = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
    let package_tombstone = trash_root.join(format!(
        "{}-{serial}-packages-{plugin_id}",
        writer_discriminator()
    ));
    let record_tombstone = trash_root.join(format!(
        "{}-{serial}-records-{plugin_id}",
        writer_discriminator()
    ));
    if package_tombstone.exists() || record_tombstone.exists() {
        return Err(RepositoryError::UnsafeArchive(
            "plugin uninstall staging path already exists".into(),
        ));
    }

    fs::rename(&package_root, &package_tombstone)?;
    let records_moved = if record_root.exists() {
        let metadata = fs::symlink_metadata(&record_root)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            let _ = fs::rename(&package_tombstone, &package_root);
            return Err(RepositoryError::UnsafeArchive(
                "managed plugin record path is not a real directory".into(),
            ));
        }
        if let Err(error) = fs::rename(&record_root, &record_tombstone) {
            let _ = fs::rename(&package_tombstone, &package_root);
            return Err(error.into());
        }
        true
    } else {
        false
    };

    let mut cleanup_pending = fs::remove_dir_all(&package_tombstone).is_err();
    if records_moved && fs::remove_dir_all(&record_tombstone).is_err() {
        cleanup_pending = true;
    }
    if let Some(mut document) = read_plugin_activation_document(&root)? {
        document.enabled_plugins.remove(plugin_id);
        write_json_atomic(&root.join(PLUGIN_ACTIVATION_FILE), &document)?;
    }
    Ok(UninstalledPackage {
        plugin_id: plugin_id.into(),
        removed_versions,
        cleanup_pending,
    })
}

/// Removes user-owned state selected explicitly during plugin uninstall.
///
/// RackForge presets and private plugin data are separate namespaces. State
/// blobs are intentionally retained because racks and songs may still refer
/// to them even after their named presets are removed. The `resources`
/// namespace is an Android compatibility layout; current desktop and Linux
/// hosts keep the same data under `plugins`.
pub fn remove_plugin_user_data(
    data_root: impl AsRef<Path>,
    plugin_id: &str,
    options: PluginUserDataRemovalOptions,
) -> Result<PluginUserDataRemoval, RepositoryError> {
    validate_plugin_identifier(plugin_id)
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    if !options.presets && !options.plugin_data {
        return Ok(PluginUserDataRemoval::default());
    }

    let data_root = data_root.as_ref();
    let metadata = match fs::symlink_metadata(data_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(PluginUserDataRemoval::default());
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RepositoryError::UnsafeArchive(
            "plugin data root is not a real directory".into(),
        ));
    }
    let data_root = resolve_existing(data_root)?;
    let mut targets = Vec::new();
    if options.presets
        && let Some(target) = inspect_owned_namespace(
            &data_root,
            &data_root.join("states").join("presets").join(plugin_id),
        )?
    {
        targets.push((target.0, true, target.1));
    }
    if options.plugin_data {
        for namespace in ["plugins", "addons", "resources"] {
            if let Some(target) =
                inspect_owned_namespace(&data_root, &data_root.join(namespace).join(plugin_id))?
            {
                targets.push((target.0, false, target.1));
            }
        }
    }

    let mut removed = PluginUserDataRemoval::default();
    for (target, presets, file_count) in targets {
        fs::remove_dir_all(target)?;
        if presets {
            removed.preset_files_removed += file_count;
        } else {
            removed.data_namespaces_removed += 1;
        }
    }
    Ok(removed)
}

fn inspect_owned_namespace(
    data_root: &Path,
    namespace: &Path,
) -> Result<Option<(PathBuf, usize)>, RepositoryError> {
    let metadata = match fs::symlink_metadata(namespace) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RepositoryError::UnsafeArchive(format!(
            "plugin user-data namespace is not a real directory: {}",
            namespace.display()
        )));
    }
    let canonical = resolve_existing(namespace)?;
    if canonical == data_root || !canonical.starts_with(data_root) {
        return Err(RepositoryError::UnsafeArchive(
            "plugin user-data namespace escaped its root".into(),
        ));
    }
    let files_removed = inspect_owned_tree(&canonical)?;
    Ok(Some((canonical, files_removed)))
}

fn inspect_owned_tree(path: &Path) -> Result<usize, RepositoryError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(RepositoryError::UnsafeArchive(format!(
            "plugin user data cannot contain symbolic links: {}",
            path.display()
        )));
    }
    if metadata.is_file() {
        return Ok(1);
    }
    if !metadata.is_dir() {
        return Err(RepositoryError::UnsafeArchive(format!(
            "plugin user data contains an unsupported entry: {}",
            path.display()
        )));
    }
    let mut files_removed = 0;
    for entry in fs::read_dir(path)? {
        files_removed += inspect_owned_tree(&entry?.path())?;
    }
    Ok(files_removed)
}

/// Validates the structure and executable metadata of a user-selected
/// `.rfplugin` and returns the identity of the package that would be installed.
///
/// Preview deliberately does not inflate packaged resources. A SoundFont or
/// sample library can be hundreds of megabytes after decompression, and a
/// browser host would otherwise duplicate all of it merely to paint a
/// confirmation dialog. Installation still extracts every byte, enforces CRCs
/// and repeats the complete payload validation before anything is committed.
pub fn inspect_local_archive(
    store_root: impl AsRef<Path>,
    bytes: &[u8],
) -> Result<LocalPackageInspection, RepositoryError> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(RepositoryError::Integrity(
            "local artifact size is outside supported limits".into(),
        ));
    }
    // Preserve the existing contract that the caller's store root is real and
    // writable without creating a package staging tree.
    ensure_real_directory(store_root.as_ref())?;
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| RepositoryError::UnsafeArchive(error.to_string()))?;
    validate_archive_layout(&mut archive)?;
    let manifest = read_archive_manifest(&mut archive)?;
    let portable = manifest.portable_component().is_some();
    let platform = if portable {
        "wasm-v1"
    } else {
        repository_platform_key()?
    };
    validate_archive_payload(&mut archive, &manifest, platform)?;
    let branding = read_archive_branding(&mut archive, &manifest)?;
    Ok(LocalPackageInspection {
        plugin_id: manifest.id,
        plugin_name: manifest.name,
        vendor: manifest.vendor,
        version: manifest.version,
        description: manifest.description,
        kind: manifest.kind,
        platform: platform.into(),
        portable,
        artifact_sha256: hex_digest(Sha256::digest(bytes).as_slice()),
        archive_bytes: bytes.len() as u64,
        branding,
    })
}

fn validate_archive_layout<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<(), RepositoryError> {
    if archive.is_empty() || archive.len() > MAX_PACKAGE_ENTRIES {
        return Err(RepositoryError::UnsafeArchive(
            "archive entry count is outside supported limits".into(),
        ));
    }
    let mut expanded = 0_u64;
    let mut paths = BTreeSet::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| RepositoryError::UnsafeArchive(error.to_string()))?;
        if entry.name().contains('\\') {
            return Err(RepositoryError::UnsafeArchive(
                "backslashes are forbidden in archive paths".into(),
            ));
        }
        let relative = entry.enclosed_name().ok_or_else(|| {
            RepositoryError::UnsafeArchive("archive path escapes package root".into())
        })?;
        if relative.as_os_str().is_empty() || !paths.insert(relative.to_path_buf()) {
            return Err(RepositoryError::UnsafeArchive(
                "empty or duplicate archive path".into(),
            ));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(RepositoryError::UnsafeArchive(
                "symbolic links are forbidden in plugin packages".into(),
            ));
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or_else(|| RepositoryError::UnsafeArchive("expanded size overflow".into()))?;
        if expanded > MAX_EXPANDED_PACKAGE_BYTES {
            return Err(RepositoryError::UnsafeArchive(
                "expanded package exceeds supported limit".into(),
            ));
        }
    }
    Ok(())
}

fn read_archive_bytes<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    relative: &str,
    maximum: u64,
    label: &str,
) -> Result<Vec<u8>, RepositoryError> {
    let entry = archive.by_name(relative).map_err(|_| {
        RepositoryError::InvalidPackage(format!("{label} is missing: {relative:?}"))
    })?;
    if entry.is_dir() || entry.size() == 0 || entry.size() > maximum {
        return Err(RepositoryError::InvalidPackage(format!(
            "{label} size is outside the supported limit"
        )));
    }
    let capacity = usize::try_from(entry.size()).map_err(|_| {
        RepositoryError::InvalidPackage(format!("{label} is too large for this host"))
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    entry
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| RepositoryError::InvalidPackage(format!("reading {label}: {error}")))?;
    if bytes.len() as u64 > maximum {
        return Err(RepositoryError::InvalidPackage(format!(
            "{label} size is outside the supported limit"
        )));
    }
    Ok(bytes)
}

fn read_archive_manifest<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<PluginManifest, RepositoryError> {
    let bytes = read_archive_bytes(
        archive,
        "rackforge-plugin.toml",
        1024 * 1024,
        "plugin manifest",
    )?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    let manifest: PluginManifest =
        toml::from_str(text).map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    manifest
        .validate()
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    Ok(manifest)
}

fn read_archive_metadata<R: Read + Seek, T: DeserializeOwned>(
    archive: &mut ZipArchive<R>,
    relative: &str,
    label: &str,
) -> Result<T, RepositoryError> {
    let bytes = read_archive_bytes(archive, relative, MAX_PORTABLE_METADATA_BYTES, label)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| RepositoryError::InvalidPackage(format!("parsing {label}: {error}")))
}

fn validate_archive_payload<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    manifest: &PluginManifest,
    platform: &str,
) -> Result<(), RepositoryError> {
    if platform != "wasm-v1" {
        let path = manifest.binaries.get(platform).ok_or_else(|| {
            RepositoryError::InvalidPackage("package has no binary for this platform".into())
        })?;
        archive
            .by_name(path)
            .map_err(|_| RepositoryError::InvalidPackage("plugin executable is missing".into()))?;
        return Ok(());
    }
    let component = manifest.portable_component().ok_or_else(|| {
        RepositoryError::InvalidPackage("portable artifact has no wasm-v1 component".into())
    })?;
    let mut executable = archive
        .by_name(&component.path)
        .map_err(|_| RepositoryError::InvalidPackage("plugin executable is missing".into()))?;
    if executable.is_dir() || executable.size() < 4 {
        return Err(RepositoryError::InvalidPackage(
            "portable component is not a WebAssembly binary".into(),
        ));
    }
    let mut magic = [0_u8; 4];
    executable.read_exact(&mut magic).map_err(|error| {
        RepositoryError::InvalidPackage(format!("reading portable component: {error}"))
    })?;
    drop(executable);
    if &magic != b"\0asm" {
        return Err(RepositoryError::InvalidPackage(
            "portable component is not a WebAssembly binary".into(),
        ));
    }
    let runtime: RuntimeDescriptor =
        read_archive_metadata(archive, &component.runtime_descriptor, "runtime descriptor")?;
    runtime.validate_against(manifest).map_err(|error| {
        RepositoryError::InvalidPackage(format!("invalid runtime descriptor: {error}"))
    })?;
    let parameters: ParameterSchema =
        read_archive_metadata(archive, &component.parameter_schema, "parameter schema")?;
    parameters.validate().map_err(|error| {
        RepositoryError::InvalidPackage(format!("invalid parameter schema: {error}"))
    })?;
    let presets: PresetCatalog =
        read_archive_metadata(archive, &component.preset_catalog, "preset catalog")?;
    presets.validate().map_err(|error| {
        RepositoryError::InvalidPackage(format!("invalid preset catalog: {error}"))
    })?;
    Ok(())
}

fn read_archive_branding<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    manifest: &PluginManifest,
) -> Result<Option<LocalPackageBrandingPreview>, RepositoryError> {
    let Some(branding) = &manifest.branding else {
        return Ok(None);
    };
    let mut banner_png = None;
    for (kind, relative) in branding.assets() {
        let bytes = read_archive_bytes(
            archive,
            relative,
            kind.max_file_bytes() as u64,
            "branding asset",
        )?;
        validate_branding_asset(kind, &bytes)
            .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
        if relative == branding.banner {
            banner_png = Some(bytes);
        }
    }
    Ok(Some(LocalPackageBrandingPreview {
        banner_png: banner_png.expect("validated branding always includes its banner"),
        background_color: branding.background_color.clone(),
        accent_color: branding.accent_color.clone(),
    }))
}

pub fn repository_platform_key() -> Result<&'static str, RepositoryError> {
    if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Ok("linux-aarch64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok("linux-x86_64")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok("windows-x86_64")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok("macos-aarch64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Ok("macos-x86_64")
    } else {
        Err(RepositoryError::InvalidConfig(format!(
            "unsupported platform {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )))
    }
}

fn selected_repository_id(selected: &SelectedArtifact) -> String {
    selected.repository_id.clone()
}

fn download_limited(url: &Url, limit: usize) -> Result<Vec<u8>, RepositoryError> {
    let mut response = ureq::get(url.as_str())
        .call()
        .map_err(|error| RepositoryError::Request(error.to_string()))?;
    response
        .body_mut()
        .with_config()
        .limit(limit.saturating_add(1) as u64)
        .read_to_vec()
        .map_err(|error| RepositoryError::Request(error.to_string()))
        .and_then(|bytes| {
            if bytes.len() > limit {
                Err(RepositoryError::ResponseTooLarge(limit))
            } else {
                Ok(bytes)
            }
        })
}

fn decode_public_key(value: &str) -> Result<VerifyingKey, RepositoryError> {
    let bytes = STANDARD
        .decode(value.trim())
        .map_err(|_| RepositoryError::InvalidConfig("invalid public key encoding".into()))?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| RepositoryError::InvalidConfig("public key must be 32 bytes".into()))?;
    let key = VerifyingKey::from_bytes(&bytes)
        .map_err(|_| RepositoryError::InvalidConfig("invalid Ed25519 public key".into()))?;
    if key.is_weak() {
        return Err(RepositoryError::InvalidConfig(
            "weak Ed25519 public key rejected".into(),
        ));
    }
    Ok(key)
}

fn directory_url(url: &Url) -> Result<Url, RepositoryError> {
    let mut url = url.clone();
    url.set_query(None);
    let path = url.path();
    if !path.ends_with('/') {
        url.set_path(&format!("{path}/"));
    }
    Ok(url)
}

fn validate_artifact_url(value: &str) -> Result<(), RepositoryError> {
    if value.is_empty() || value.len() > 2048 || value.contains('\\') {
        return Err(RepositoryError::InvalidCatalog(
            "invalid artifact URL".into(),
        ));
    }
    let reference = Url::parse("https://repository.invalid/v1/index.json")
        .expect("static URL is valid")
        .join(value)
        .map_err(|error| RepositoryError::InvalidCatalog(error.to_string()))?;
    if reference.scheme() != "https" || reference.fragment().is_some() {
        return Err(RepositoryError::InvalidCatalog(
            "artifact URL must be an HTTP path without a fragment".into(),
        ));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), RepositoryError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(RepositoryError::InvalidCatalog(
            "SHA-256 must be 64 lowercase hexadecimal characters".into(),
        ));
    }
    Ok(())
}

fn validate_text(field: &str, value: &str, maximum: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(format!("invalid {field}"));
    }
    Ok(())
}

fn validate_dotted_identifier(value: &str) -> Result<(), &'static str> {
    if value.len() > 128
        || !value.contains('.')
        || value
            .split('.')
            .any(|part| validate_simple_identifier(part).is_err())
    {
        return Err("invalid dotted identifier");
    }
    Ok(())
}

fn validate_simple_identifier(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > 64
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err("invalid identifier");
    }
    Ok(())
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

/// Distinguishes the scratch paths of writers that could be working in the
/// same store at once.
///
/// Off the web those are other RackForge processes, so the process id is the
/// right answer. A browser has no process id — asking for one aborts — and no
/// second writer either, since a page's storage is private to it, so the
/// serial each call already carries is enough on its own.
#[cfg(not(target_arch = "wasm32"))]
fn writer_discriminator() -> u64 {
    u64::from(std::process::id())
}

#[cfg(target_arch = "wasm32")]
fn writer_discriminator() -> u64 {
    0
}

/// Resolves a path so the containment checks around it mean something.
///
/// Off the web that is `fs::canonicalize`, which resolves symlinks and `..`
/// before a path is compared against a root. WASI has neither a working
/// directory to resolve against nor `realpath`, but it also gives the guest no
/// way to reach outside the directory the embedder preopened: containment is
/// enforced by the runtime before any path is opened. The fallback therefore
/// normalises lexically and leaves the rest to the sandbox.
fn resolve_existing(path: &Path) -> Result<PathBuf, RepositoryError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Ok(fs::canonicalize(path)?)
    }
    #[cfg(target_arch = "wasm32")]
    {
        fs::symlink_metadata(path)?;
        let mut resolved = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    if !resolved.pop() {
                        return Err(RepositoryError::UnsafeArchive(format!(
                            "{} escapes the RackForge data root",
                            path.display()
                        )));
                    }
                }
                other => resolved.push(other.as_os_str()),
            }
        }
        Ok(resolved)
    }
}

fn ensure_real_directory(path: &Path) -> Result<PathBuf, RepositoryError> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RepositoryError::UnsafeArchive(format!(
            "{} is not a real directory",
            path.display()
        )));
    }
    resolve_existing(path)
}

fn ensure_real_child(parent: &Path, name: &str) -> Result<PathBuf, RepositoryError> {
    if name.is_empty()
        || name.contains(['/', '\\'])
        || Path::new(name)
            .components()
            .any(|part| part != Component::Normal(name.as_ref()))
    {
        return Err(RepositoryError::UnsafeArchive(
            "unsafe installation path".into(),
        ));
    }
    let child = parent.join(name);
    if !child.exists() {
        fs::create_dir(&child)?;
    }
    let metadata = fs::symlink_metadata(&child)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RepositoryError::UnsafeArchive(
            "installation path is not a real directory".into(),
        ));
    }
    let child = resolve_existing(&child)?;
    if !child.starts_with(parent) {
        return Err(RepositoryError::UnsafeArchive(
            "installation path escaped its root".into(),
        ));
    }
    Ok(child)
}

fn extract_archive(bytes: &[u8], destination: &Path) -> Result<(), RepositoryError> {
    extract_archive_cancellable(bytes, destination, &AtomicBool::new(false))
}

fn ensure_installation_not_cancelled(cancelled: &AtomicBool) -> Result<(), RepositoryError> {
    if cancelled.load(Ordering::Acquire) {
        Err(RepositoryError::InstallationCancelled)
    } else {
        Ok(())
    }
}

fn extract_archive_cancellable(
    bytes: &[u8],
    destination: &Path,
    cancelled: &AtomicBool,
) -> Result<(), RepositoryError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| RepositoryError::UnsafeArchive(error.to_string()))?;
    if archive.is_empty() || archive.len() > MAX_PACKAGE_ENTRIES {
        return Err(RepositoryError::UnsafeArchive(
            "archive entry count is outside supported limits".into(),
        ));
    }
    let mut expanded = 0_u64;
    let mut paths = BTreeSet::new();
    for index in 0..archive.len() {
        ensure_installation_not_cancelled(cancelled)?;
        let mut entry = archive
            .by_index(index)
            .map_err(|error| RepositoryError::UnsafeArchive(error.to_string()))?;
        if entry.name().contains('\\') {
            return Err(RepositoryError::UnsafeArchive(
                "backslashes are forbidden in archive paths".into(),
            ));
        }
        let relative = entry.enclosed_name().ok_or_else(|| {
            RepositoryError::UnsafeArchive("archive path escapes package root".into())
        })?;
        if relative.as_os_str().is_empty() || !paths.insert(relative.to_path_buf()) {
            return Err(RepositoryError::UnsafeArchive(
                "empty or duplicate archive path".into(),
            ));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(RepositoryError::UnsafeArchive(
                "symbolic links are forbidden in plugin packages".into(),
            ));
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or_else(|| RepositoryError::UnsafeArchive("expanded size overflow".into()))?;
        if expanded > MAX_EXPANDED_PACKAGE_BYTES {
            return Err(RepositoryError::UnsafeArchive(
                "expanded package exceeds supported limit".into(),
            ));
        }
        let target = destination.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        let parent = target
            .parent()
            .ok_or_else(|| RepositoryError::UnsafeArchive("archive entry has no parent".into()))?;
        fs::create_dir_all(parent)?;
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&target)?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            ensure_installation_not_cancelled(cancelled)?;
            let read = entry.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read])?;
        }
        output.sync_all()?;
    }
    Ok(())
}

fn validate_extracted_package(
    root: &Path,
    selected: &SelectedArtifact,
) -> Result<(), RepositoryError> {
    let manifest = read_extracted_manifest(root)?;
    if manifest.id != selected.plugin.id || manifest.version != selected.release.version {
        return Err(RepositoryError::InvalidPackage(
            "manifest identity does not match signed catalog".into(),
        ));
    }
    validate_extracted_payload(root, &manifest, &selected.artifact.platform)
}

fn read_extracted_manifest(root: &Path) -> Result<PluginManifest, RepositoryError> {
    let manifest_path = root.join("rackforge-plugin.toml");
    let mut text = String::new();
    File::open(&manifest_path)
        .and_then(|file| file.take(1024 * 1024).read_to_string(&mut text))
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    let manifest: PluginManifest = toml::from_str(&text)
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    manifest
        .validate()
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    validate_branding_assets(&manifest, root)
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    Ok(manifest)
}

fn validate_extracted_payload(
    root: &Path,
    manifest: &PluginManifest,
    platform: &str,
) -> Result<(), RepositoryError> {
    let payload = if platform == "wasm-v1" {
        manifest
            .portable_component()
            .map(|component| component.path.as_str())
            .ok_or_else(|| {
                RepositoryError::InvalidPackage("portable artifact has no wasm-v1 component".into())
            })?
    } else {
        manifest
            .binaries
            .get(platform)
            .map(String::as_str)
            .ok_or_else(|| {
                RepositoryError::InvalidPackage("package has no binary for this platform".into())
            })?
    };
    let payload_path = root.join(payload);
    let canonical = resolve_existing(&payload_path)
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    if !canonical.starts_with(root) || !canonical.is_file() {
        return Err(RepositoryError::InvalidPackage(
            "plugin executable is missing or escaped the package".into(),
        ));
    }
    if platform == "wasm-v1" {
        let bytes = fs::read(&canonical)
            .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
        if !bytes.starts_with(b"\0asm") {
            return Err(RepositoryError::InvalidPackage(
                "portable component is not a WebAssembly binary".into(),
            ));
        }
        let component = manifest.portable_component().expect("validated above");
        let runtime: RuntimeDescriptor =
            read_portable_metadata(root, &component.runtime_descriptor, "runtime descriptor")?;
        runtime.validate_against(manifest).map_err(|error| {
            RepositoryError::InvalidPackage(format!("invalid runtime descriptor: {error}"))
        })?;
        let parameters: ParameterSchema =
            read_portable_metadata(root, &component.parameter_schema, "parameter schema")?;
        parameters.validate().map_err(|error| {
            RepositoryError::InvalidPackage(format!("invalid parameter schema: {error}"))
        })?;
        let presets: PresetCatalog =
            read_portable_metadata(root, &component.preset_catalog, "preset catalog")?;
        presets.validate().map_err(|error| {
            RepositoryError::InvalidPackage(format!("invalid preset catalog: {error}"))
        })?;
    }
    Ok(())
}

fn read_portable_metadata<T: DeserializeOwned>(
    root: &Path,
    relative: &str,
    label: &str,
) -> Result<T, RepositoryError> {
    let path = resolve_existing(&root.join(relative)).map_err(|error| {
        RepositoryError::InvalidPackage(format!("{label} is unavailable: {error}"))
    })?;
    let metadata = fs::metadata(&path).map_err(|error| {
        RepositoryError::InvalidPackage(format!("{label} is unavailable: {error}"))
    })?;
    if !path.starts_with(root) || !metadata.is_file() {
        return Err(RepositoryError::InvalidPackage(format!(
            "{label} is missing or escaped the package: {relative:?}"
        )));
    }
    if metadata.len() == 0 || metadata.len() > MAX_PORTABLE_METADATA_BYTES {
        return Err(RepositoryError::InvalidPackage(format!(
            "{label} size is outside the supported limit"
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(&path)
        .and_then(|file| {
            file.take(MAX_PORTABLE_METADATA_BYTES.saturating_add(1))
                .read_to_end(&mut bytes)
        })
        .map_err(|error| RepositoryError::InvalidPackage(format!("reading {label}: {error}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| RepositoryError::InvalidPackage(format!("parsing {label}: {error}")))
}

fn read_installation_record(path: &Path) -> Result<InstallationRecord, RepositoryError> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), RepositoryError> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
    bytes.push(b'\n');
    let serial = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("tmp-{}-{serial}", writer_discriminator()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7_u8; 32])
    }

    fn config() -> RepositoryConfig {
        RepositoryConfig {
            id: "org.rackforge.test".into(),
            name: "RackForge Test".into(),
            base_url: "http://127.0.0.1:8788/".into(),
            public_key: STANDARD.encode(signing_key().verifying_key().as_bytes()),
            enabled: true,
            allow_insecure_http: true,
        }
    }

    fn index() -> RepositoryIndex {
        RepositoryIndex {
            schema_version: 1,
            repository_id: "org.rackforge.test".into(),
            name: "RackForge Test".into(),
            generated_at: "2026-07-31T00:00:00Z".into(),
            plugins: vec![RepositoryPlugin {
                id: "org.rackforge.synth".into(),
                name: "Test Synth".into(),
                summary: "Test instrument".into(),
                license: "GPL-2.0-or-later".into(),
                homepage: None,
                releases: vec![PluginRelease {
                    version: "1.2.3".into(),
                    published_at: "2026-07-31T00:00:00Z".into(),
                    artifacts: vec![PluginArtifact {
                        platform: "linux-aarch64".into(),
                        url: "../packages/synth.rfplugin".into(),
                        size: 4,
                        sha256: hex_digest(Sha256::digest(b"test").as_slice()),
                    }],
                }],
            }],
        }
    }

    fn selected_for(bytes: &[u8]) -> SelectedArtifact {
        SelectedArtifact {
            repository_id: "org.rackforge.test".into(),
            plugin: RepositoryPlugin {
                id: "org.rackforge.synth".into(),
                name: "Test Synth".into(),
                summary: "Test instrument".into(),
                license: "GPL-2.0-or-later".into(),
                homepage: None,
                releases: Vec::new(),
            },
            release: PluginRelease {
                version: "1.2.3".into(),
                published_at: "2026-07-31T00:00:00Z".into(),
                artifacts: Vec::new(),
            },
            artifact: PluginArtifact {
                platform: "linux-aarch64".into(),
                url: "../packages/synth.rfplugin".into(),
                size: bytes.len() as u64,
                sha256: hex_digest(Sha256::digest(bytes).as_slice()),
            },
            url: Url::parse("http://127.0.0.1:8788/packages/synth.rfplugin").unwrap(),
        }
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let cursor = Cursor::new(&mut bytes);
            let mut archive = ZipWriter::new(cursor);
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .unix_permissions(0o644);
            for (name, content) in entries {
                archive.start_file(*name, options).unwrap();
                archive.write_all(content).unwrap();
            }
            archive.finish().unwrap();
        }
        bytes
    }

    fn test_manifest() -> &'static [u8] {
        br#"schema_version = 1
id = "org.rackforge.synth"
name = "Test Synth"
vendor = "RackForge Test"
version = "1.2.3"
kind = "instrument"
state_version = 1
capabilities = ["audio_output", "midi_input"]
ui_layouts = ["little@1"]

[api]
major = 1
minor = 5

[binaries]
linux-aarch64 = "lib/synth.so"
"#
    }

    fn portable_manifest() -> &'static [u8] {
        br#"schema_version = 1
id = "org.rackforge.synth"
name = "Test Synth"
vendor = "RackForge Test"
version = "1.2.3"
kind = "instrument"
state_version = 1
capabilities = ["audio_output", "midi_input", "presets"]
ui_layouts = ["little@1"]

[api]
major = 1
minor = 5

[component]
abi = "wasm-v1"
path = "component.wasm"
runtime_descriptor = "metadata/runtime.json"
parameter_schema = "metadata/parameters.json"
preset_catalog = "metadata/presets.json"
"#
    }

    fn portable_runtime() -> &'static [u8] {
        br#"{"schema_version":1,"id":"org.rackforge.synth","version":"1.2.3","state_version":1}"#
    }

    fn portable_parameters() -> &'static [u8] {
        br#"{"schema_version":1,"pages":[],"parameters":[]}"#
    }

    fn portable_presets() -> &'static [u8] {
        br#"{"schema_version":1,"banks":[{"id":"factory","name":"Factory","order":0}],"presets":[{"id":"factory.default","name":"Default","bank":"factory","order":0,"tags":[]}] }"#
    }

    #[test]
    fn verifies_exact_catalog_bytes_and_rejects_tampering() {
        let bytes = serde_json::to_vec(&index()).unwrap();
        let signature = STANDARD.encode(signing_key().sign(&bytes).to_bytes());
        let verified = verify_catalog(&config(), &bytes, signature.as_bytes()).unwrap();
        assert_eq!(verified.repository_id, "org.rackforge.test");

        let mut tampered = bytes;
        tampered.push(b' ');
        assert!(matches!(
            verify_catalog(&config(), &tampered, signature.as_bytes()),
            Err(RepositoryError::InvalidSignature)
        ));
    }

    #[test]
    fn selects_latest_compatible_release() {
        let repository = VerifiedRepository {
            config: config(),
            index_url: Url::parse("http://127.0.0.1:8788/v1/index.json").unwrap(),
            index: index(),
        };
        let selected = repository
            .select("org.rackforge.synth", None, "linux-aarch64")
            .unwrap();
        assert_eq!(selected.release.version, "1.2.3");
        assert_eq!(
            selected.url.as_str(),
            "http://127.0.0.1:8788/packages/synth.rfplugin"
        );
    }

    #[test]
    fn prefers_one_portable_artifact_on_every_host() {
        let mut catalog = index();
        catalog.plugins[0].releases[0].artifacts.insert(
            0,
            PluginArtifact {
                platform: "wasm-v1".into(),
                url: "../packages/synth-portable.rfplugin".into(),
                size: 8,
                sha256: hex_digest(Sha256::digest(b"portable").as_slice()),
            },
        );
        let repository = VerifiedRepository {
            config: config(),
            index_url: Url::parse("http://127.0.0.1:8788/v1/index.json").unwrap(),
            index: catalog,
        };
        let selected = repository
            .select("org.rackforge.synth", None, "linux-aarch64")
            .unwrap();
        assert_eq!(selected.artifact.platform, "wasm-v1");
        assert!(selected.url.as_str().ends_with("synth-portable.rfplugin"));
    }

    #[test]
    fn refuses_plain_http_without_explicit_lan_permission() {
        let mut repository = config();
        repository.allow_insecure_http = false;
        assert!(matches!(
            repository.validate(),
            Err(RepositoryError::UnsafeTransport(_))
        ));
    }

    #[test]
    fn installs_an_immutable_verified_package_without_activating_it() {
        let bytes = archive(&[
            ("rackforge-plugin.toml", test_manifest()),
            ("lib/synth.so", b"native-test-binary"),
        ]);
        let selected = selected_for(&bytes);
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-install-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let installed = install_archive(&root, &selected, &bytes).unwrap();
        assert!(installed.path.join("lib/synth.so").is_file());
        assert!(!plugin_is_enabled(&root, "org.rackforge.synth").unwrap());
        assert!(!root.join("active").exists());
        let repeated = install_archive(&root, &selected, &bytes).unwrap();
        assert!(repeated.already_installed);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installs_a_verified_portable_package() {
        let wasm = b"\0asm\x01\0\0\0";
        let bytes = archive(&[
            ("rackforge-plugin.toml", portable_manifest()),
            ("component.wasm", wasm),
            ("metadata/runtime.json", portable_runtime()),
            ("metadata/parameters.json", portable_parameters()),
            ("metadata/presets.json", portable_presets()),
        ]);
        let mut selected = selected_for(&bytes);
        selected.artifact.platform = "wasm-v1".into();
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-portable-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let installed = install_archive(&root, &selected, &bytes).unwrap();
        assert_eq!(
            fs::read(installed.path.join("component.wasm")).unwrap(),
            wasm
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installs_a_local_portable_package_with_an_explicit_local_record() {
        let wasm = b"\0asm\x01\0\0\0";
        let bytes = archive(&[
            ("rackforge-plugin.toml", portable_manifest()),
            ("component.wasm", wasm),
            ("metadata/runtime.json", portable_runtime()),
            ("metadata/parameters.json", portable_parameters()),
            ("metadata/presets.json", portable_presets()),
        ]);
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-local-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let installed = install_local_archive(&root, &bytes).unwrap();
        assert_eq!(installed.record.plugin_id, "org.rackforge.synth");
        assert_eq!(installed.record.version, "1.2.3");
        assert_eq!(installed.record.platform, "wasm-v1");
        assert_eq!(installed.record.repository_id, "local");
        assert!(!plugin_is_enabled(&root, "org.rackforge.synth").unwrap());
        assert_eq!(
            fs::read(installed.path.join("component.wasm")).unwrap(),
            wasm
        );
        assert!(
            install_local_archive(&root, &bytes)
                .unwrap()
                .already_installed
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancelled_local_install_never_commits_or_leaves_staging_data() {
        let wasm = b"\0asm\x01\0\0\0";
        let bytes = archive(&[
            ("rackforge-plugin.toml", portable_manifest()),
            ("component.wasm", wasm),
            ("metadata/runtime.json", portable_runtime()),
            ("metadata/parameters.json", portable_parameters()),
            ("metadata/presets.json", portable_presets()),
        ]);
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-cancelled-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let cancelled = AtomicBool::new(true);

        let error = install_local_archive_cancellable(&root, &bytes, &cancelled).unwrap_err();

        assert!(matches!(error, RepositoryError::InstallationCancelled));
        assert!(!root.join("packages/org.rackforge.synth/1.2.3").exists());
        if root.exists() {
            assert!(fs::read_dir(&root).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".install-local-")
            }));
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn rejects_a_portable_package_with_invalid_typed_metadata() {
        let wasm = b"\0asm\x01\0\0\0";
        let invalid_parameters = br#"{
          "schema_version": 1,
          "pages": [{"id":"main","name":"Main"}],
          "parameters": [{
            "index": 0,
            "id": "cutoff",
            "name": "Cutoff",
            "page": "main",
            "kind": {"type":"float","minimum":0.0,"maximum":1.0,"default":0.5,"step":0.01},
            "suggested_control": "slider"
          }]
        }"#;
        let bytes = archive(&[
            ("rackforge-plugin.toml", portable_manifest()),
            ("component.wasm", wasm),
            ("metadata/runtime.json", portable_runtime()),
            ("metadata/parameters.json", invalid_parameters),
            ("metadata/presets.json", portable_presets()),
        ]);
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-invalid-metadata-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));

        let error = install_local_archive(&root, &bytes).unwrap_err();
        assert!(
            error.to_string().contains("unknown variant `slider`"),
            "{error}"
        );
        assert!(!root.join("packages/org.rackforge.synth/1.2.3").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uninstalls_every_managed_version_but_preserves_plugin_data() {
        let wasm = b"\0asm\x01\0\0\0";
        let bytes = archive(&[
            ("rackforge-plugin.toml", portable_manifest()),
            ("component.wasm", wasm),
            ("metadata/runtime.json", portable_runtime()),
            ("metadata/parameters.json", portable_parameters()),
            ("metadata/presets.json", portable_presets()),
        ]);
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-uninstall-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        install_local_archive(&root, &bytes).unwrap();
        set_plugin_enabled(&root, "org.rackforge.synth", true).unwrap();
        let data = root.join("data/plugins/org.rackforge.synth/user-preset.bin");
        fs::create_dir_all(data.parent().unwrap()).unwrap();
        fs::write(&data, b"user state").unwrap();

        let removed = uninstall_plugin(&root, "org.rackforge.synth").unwrap();
        assert_eq!(removed.plugin_id, "org.rackforge.synth");
        assert_eq!(removed.removed_versions, 1);
        assert!(!removed.cleanup_pending);
        assert!(!root.join("packages/org.rackforge.synth").exists());
        assert!(!root.join("records/org.rackforge.synth").exists());
        assert!(!plugin_is_enabled(&root, "org.rackforge.synth").unwrap());
        assert_eq!(fs::read(data).unwrap(), b"user state");
        assert!(matches!(
            uninstall_plugin(&root, "org.rackforge.synth"),
            Err(RepositoryError::PluginNotFound(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plugin_user_data_cleanup_keeps_unselected_data_and_shared_state_blobs() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-user-data-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let plugin_id = "org.rackforge.synth";
        let preset = root.join(format!("states/presets/{plugin_id}/stage.json"));
        let state_blob = root.join("states/blobs/aa/aa.rfstate");
        let resource = root.join(format!("plugins/{plugin_id}/firmware/m1.bin"));
        let android_resource = root.join(format!("resources/{plugin_id}/pcm.resource"));
        let other_plugin = root.join("plugins/org.rackforge.other/keep.bin");
        for path in [
            &preset,
            &state_blob,
            &resource,
            &android_resource,
            &other_plugin,
        ] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"owned").unwrap();
        }

        let presets = remove_plugin_user_data(
            &root,
            plugin_id,
            PluginUserDataRemovalOptions {
                presets: true,
                plugin_data: false,
            },
        )
        .unwrap();
        assert_eq!(presets.preset_files_removed, 1);
        assert_eq!(presets.data_namespaces_removed, 0);
        assert!(!preset.exists());
        assert!(state_blob.is_file());
        assert!(resource.is_file());

        let data = remove_plugin_user_data(
            &root,
            plugin_id,
            PluginUserDataRemovalOptions {
                presets: false,
                plugin_data: true,
            },
        )
        .unwrap();
        assert_eq!(data.preset_files_removed, 0);
        assert_eq!(data.data_namespaces_removed, 2);
        assert!(!resource.exists());
        assert!(!android_resource.exists());
        assert!(state_blob.is_file());
        assert!(other_plugin.is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_plugin_store_keeps_existing_packages_enabled() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-activation-legacy-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("packages/org.rackforge.first/1.0.0")).unwrap();

        assert!(plugin_is_enabled(&root, "org.rackforge.first").unwrap());
        assert!(!root.join(PLUGIN_ACTIVATION_FILE).exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preparing_an_install_preserves_legacy_packages_but_not_the_new_one() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-activation-install-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("packages/org.rackforge.legacy/1.0.0")).unwrap();

        let canonical_root = ensure_real_directory(&root).unwrap();
        prepare_new_plugin_disabled(&canonical_root).unwrap();
        fs::create_dir_all(root.join("packages/org.rackforge.new/1.0.0")).unwrap();

        assert!(plugin_is_enabled(&root, "org.rackforge.legacy").unwrap());
        assert!(!plugin_is_enabled(&root, "org.rackforge.new").unwrap());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn activation_can_be_toggled_without_changing_installed_files() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-activation-toggle-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let package = root.join("packages/org.rackforge.synth/1.0.0/plugin.bin");
        fs::create_dir_all(package.parent().unwrap()).unwrap();
        fs::write(&package, b"immutable package").unwrap();

        set_plugin_enabled(&root, "org.rackforge.synth", false).unwrap();
        assert!(!plugin_is_enabled(&root, "org.rackforge.synth").unwrap());
        assert_eq!(fs::read(&package).unwrap(), b"immutable package");

        set_plugin_enabled(&root, "org.rackforge.synth", true).unwrap();
        assert!(plugin_is_enabled(&root, "org.rackforge.synth").unwrap());
        assert_eq!(fs::read(&package).unwrap(), b"immutable package");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plugin_user_data_cleanup_does_nothing_without_explicit_options() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-user-data-preserve-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let resource = root.join("plugins/org.rackforge.synth/firmware.bin");
        fs::create_dir_all(resource.parent().unwrap()).unwrap();
        fs::write(&resource, b"owned").unwrap();

        assert_eq!(
            remove_plugin_user_data(
                &root,
                "org.rackforge.synth",
                PluginUserDataRemovalOptions::default(),
            )
            .unwrap(),
            PluginUserDataRemoval::default()
        );
        assert!(resource.is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uninstall_rejects_unsafe_plugin_identifiers() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-uninstall-unsafe-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(uninstall_plugin(&root, "../escape").is_err());
        if root.exists() {
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn cleanup_removes_only_real_uninstall_tombstone_directories() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-uninstall-cleanup-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let tombstone = root.join(".uninstall/previous-host-package");
        fs::create_dir_all(&tombstone).unwrap();
        fs::write(tombstone.join("mapped-plugin.dll"), b"old").unwrap();
        fs::write(root.join(".uninstall/README"), b"do not follow files").unwrap();

        assert_eq!(cleanup_uninstall_tombstones(&root).unwrap(), 1);
        assert!(!tombstone.exists());
        assert!(root.join(".uninstall/README").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inspects_a_local_package_without_installing_it() {
        let bytes = archive(&[
            ("rackforge-plugin.toml", portable_manifest()),
            ("component.wasm", b"\0asm\x01\0\0\0"),
            ("metadata/runtime.json", portable_runtime()),
            ("metadata/parameters.json", portable_parameters()),
            ("metadata/presets.json", portable_presets()),
        ]);
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-inspect-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let inspection = inspect_local_archive(&root, &bytes).unwrap();
        assert_eq!(inspection.plugin_id, "org.rackforge.synth");
        assert_eq!(inspection.plugin_name, "Test Synth");
        assert_eq!(inspection.vendor, "RackForge Test");
        assert_eq!(inspection.version, "1.2.3");
        assert_eq!(inspection.description, None);
        assert_eq!(inspection.kind, PluginKind::Instrument);
        assert_eq!(inspection.platform, "wasm-v1");
        assert!(inspection.portable);
        assert_eq!(inspection.archive_bytes, bytes.len() as u64);
        assert_eq!(inspection.branding, None);
        assert_eq!(
            inspection.artifact_sha256,
            hex_digest(Sha256::digest(&bytes).as_slice())
        );
        assert!(!root.join("packages").exists());
        assert!(
            fs::read_dir(&root).unwrap().next().is_none(),
            "inspection staging directory must be removed"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_archive_path_traversal() {
        let bytes = archive(&[("../escaped", b"bad")]);
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-traversal-{}-{}",
            writer_discriminator(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(matches!(
            install_archive(&root, &selected_for(&bytes), &bytes),
            Err(RepositoryError::UnsafeArchive(_))
        ));
        assert!(!root.join("escaped").exists());
        let _ = fs::remove_dir_all(root);
    }
}
