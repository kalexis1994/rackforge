//! Signed repository catalogs and safe `.rfplugin` installation.
//!
//! This code intentionally performs no dynamic loading. Downloading and
//! installing a package is separate from activating native code in an audio
//! graph.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, VerifyingKey};
use rackforge_plugin_api::{
    PluginKind, PluginManifest, validate_branding_assets, validate_plugin_identifier,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;
use url::Url;
use zip::ZipArchive;

pub const REPOSITORY_SCHEMA_VERSION: u32 = 1;
pub const REPOSITORY_CONFIG_SCHEMA_VERSION: u32 = 1;
pub const MAX_INDEX_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_EXPANDED_PACKAGE_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_PACKAGE_ENTRIES: usize = 16_384;

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

/// Metadata obtained by fully extracting and validating a user-selected
/// `.rfplugin`, without making it an installed package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalPackageInspection {
    pub plugin_id: String,
    pub plugin_name: String,
    pub version: String,
    pub kind: PluginKind,
    pub platform: String,
    pub portable: bool,
    pub artifact_sha256: String,
    pub archive_bytes: u64,
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
        std::process::id(),
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
    if bytes.is_empty() || bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(RepositoryError::Integrity(
            "local artifact size is outside supported limits".into(),
        ));
    }
    let root = ensure_real_directory(store_root.as_ref())?;
    let packages_root = ensure_real_child(&root, "packages")?;
    let records_root = ensure_real_child(&root, "records")?;
    let serial = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
    let stage = root.join(format!(".install-local-{}-{serial}", std::process::id()));
    if stage.exists() {
        return Err(RepositoryError::UnsafeArchive(
            "staging path already exists".into(),
        ));
    }
    fs::create_dir(&stage)?;
    let result = (|| {
        extract_archive(bytes, &stage)?;
        let manifest = read_extracted_manifest(&stage)?;
        let platform = if manifest.portable_component().is_some() {
            "wasm-v1"
        } else {
            repository_platform_key()?
        };
        validate_extracted_payload(&stage, &manifest, platform)?;

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

/// Fully validates a user-selected `.rfplugin` and returns the identity of the
/// package that would be installed. The temporary extraction is always
/// removed, including when validation fails.
pub fn inspect_local_archive(
    store_root: impl AsRef<Path>,
    bytes: &[u8],
) -> Result<LocalPackageInspection, RepositoryError> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(RepositoryError::Integrity(
            "local artifact size is outside supported limits".into(),
        ));
    }
    let root = ensure_real_directory(store_root.as_ref())?;
    let serial = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
    let stage = root.join(format!(".inspect-local-{}-{serial}", std::process::id()));
    if stage.exists() {
        return Err(RepositoryError::UnsafeArchive(
            "inspection staging path already exists".into(),
        ));
    }
    fs::create_dir(&stage)?;
    let result = (|| {
        extract_archive(bytes, &stage)?;
        let manifest = read_extracted_manifest(&stage)?;
        let portable = manifest.portable_component().is_some();
        let platform = if portable {
            "wasm-v1"
        } else {
            repository_platform_key()?
        };
        validate_extracted_payload(&stage, &manifest, platform)?;
        Ok(LocalPackageInspection {
            plugin_id: manifest.id,
            plugin_name: manifest.name,
            version: manifest.version,
            kind: manifest.kind,
            platform: platform.into(),
            portable,
            artifact_sha256: hex_digest(Sha256::digest(bytes).as_slice()),
            archive_bytes: bytes.len() as u64,
        })
    })();
    if stage.exists() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
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

fn ensure_real_directory(path: &Path) -> Result<PathBuf, RepositoryError> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RepositoryError::UnsafeArchive(format!(
            "{} is not a real directory",
            path.display()
        )));
    }
    Ok(fs::canonicalize(path)?)
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
    let child = fs::canonicalize(child)?;
    if !child.starts_with(parent) {
        return Err(RepositoryError::UnsafeArchive(
            "installation path escaped its root".into(),
        ));
    }
    Ok(child)
}

fn extract_archive(bytes: &[u8], destination: &Path) -> Result<(), RepositoryError> {
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
        io::copy(&mut entry, &mut output)?;
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
    let canonical = fs::canonicalize(&payload_path)
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
        for metadata in [
            &component.runtime_descriptor,
            &component.parameter_schema,
            &component.preset_catalog,
        ] {
            let metadata_path = fs::canonicalize(root.join(metadata))
                .map_err(|error| RepositoryError::InvalidPackage(error.to_string()))?;
            if !metadata_path.starts_with(root) || !metadata_path.is_file() {
                return Err(RepositoryError::InvalidPackage(format!(
                    "portable metadata is missing or escaped the package: {metadata:?}"
                )));
            }
        }
    }
    Ok(())
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
    let temporary = path.with_extension(format!("tmp-{}-{serial}", std::process::id()));
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
capabilities = ["audio_output", "midi_input"]
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
            std::process::id(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let installed = install_archive(&root, &selected, &bytes).unwrap();
        assert!(installed.path.join("lib/synth.so").is_file());
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
            ("metadata/runtime.json", b"{}"),
            ("metadata/parameters.json", b"{}"),
            ("metadata/presets.json", b"{}"),
        ]);
        let mut selected = selected_for(&bytes);
        selected.artifact.platform = "wasm-v1".into();
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-portable-{}-{}",
            std::process::id(),
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
            ("metadata/runtime.json", b"{}"),
            ("metadata/parameters.json", b"{}"),
            ("metadata/presets.json", b"{}"),
        ]);
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-local-{}-{}",
            std::process::id(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let installed = install_local_archive(&root, &bytes).unwrap();
        assert_eq!(installed.record.plugin_id, "org.rackforge.synth");
        assert_eq!(installed.record.version, "1.2.3");
        assert_eq!(installed.record.platform, "wasm-v1");
        assert_eq!(installed.record.repository_id, "local");
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
    fn inspects_a_local_package_without_installing_it() {
        let bytes = archive(&[
            ("rackforge-plugin.toml", portable_manifest()),
            ("component.wasm", b"\0asm\x01\0\0\0"),
            ("metadata/runtime.json", b"{}"),
            ("metadata/parameters.json", b"{}"),
            ("metadata/presets.json", b"{}"),
        ]);
        let root = std::env::temp_dir().join(format!(
            "rackforge-repository-inspect-{}-{}",
            std::process::id(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let inspection = inspect_local_archive(&root, &bytes).unwrap();
        assert_eq!(inspection.plugin_id, "org.rackforge.synth");
        assert_eq!(inspection.plugin_name, "Test Synth");
        assert_eq!(inspection.version, "1.2.3");
        assert_eq!(inspection.kind, PluginKind::Instrument);
        assert_eq!(inspection.platform, "wasm-v1");
        assert!(inspection.portable);
        assert_eq!(inspection.archive_bytes, bytes.len() as u64);
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
            std::process::id(),
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
