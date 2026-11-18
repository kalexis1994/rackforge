//! Signed release metadata. No downloaded code is executed by this service.
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, VerifyingKey};
use rackforge_platform_api::updates::{ReleaseOffer, UpdatePhase, UpdateState};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const PRODUCT: &str = "org.rackforge.host";
pub const REPOSITORY: &str = "https://github.com/kalexis1994/rackforge";
pub const PUBLIC_KEY: &str = include_str!("../../../config/update-public-key.txt");
pub const MAX_METADATA: u64 = 128 * 1024;
pub const MAX_ARCHIVE: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    pub schema_version: u32,
    pub product: String,
    pub version: String,
    pub notes: String,
    pub targets: BTreeMap<String, ReleaseAsset>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseAsset {
    pub url: String,
    pub bytes: u64,
    pub sha256: String,
}

pub fn target() -> &'static str {
    if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "linux-aarch64-services"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-x86_64-services"
    } else {
        "unsupported"
    }
}

pub fn asset_name(target: &str) -> Result<&'static str> {
    match target {
        "linux-aarch64-services" => Ok("RackForge-RaspberryPi-arm64.tar.gz"),
        "linux-x86_64-services" => Ok("RackForge-Linux-x86_64.tar.gz"),
        _ => bail!("Unsupported managed-host platform"),
    }
}

pub fn verify_manifest(bytes: &[u8], signature: &str, key: &str) -> Result<ReleaseManifest> {
    ensure!(
        bytes.len() as u64 <= MAX_METADATA,
        "Update metadata is too large"
    );
    let key: [u8; 32] = STANDARD
        .decode(key.trim())
        .context("Invalid update public key")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid update public key length"))?;
    let signature = Signature::from_slice(
        &STANDARD
            .decode(signature.trim())
            .context("Invalid update signature")?,
    )?;
    VerifyingKey::from_bytes(&key)?
        .verify_strict(bytes, &signature)
        .context("Update signature verification failed")?;
    let manifest: ReleaseManifest = serde_json::from_slice(bytes)?;
    ensure!(
        manifest.schema_version == 1 && manifest.product == PRODUCT,
        "Not a RackForge host release"
    );
    let version = Version::parse(&manifest.version)?;
    ensure!(
        version.pre.is_empty() && version.build.is_empty(),
        "Stable updates require a stable release version"
    );
    ensure!(
        manifest.notes.len() <= 16 * 1024,
        "Release notes are too large"
    );
    ensure!(
        !manifest.targets.is_empty(),
        "Release has no installation targets"
    );
    for (target, asset) in &manifest.targets {
        let name = asset_name(target)?;
        ensure!(
            asset.url == format!("{REPOSITORY}/releases/download/v{version}/{name}"),
            "Unexpected update download address"
        );
        ensure!(
            asset.bytes > 0 && asset.bytes <= MAX_ARCHIVE,
            "Invalid archive size"
        );
        ensure!(
            asset.sha256.len() == 64 && asset.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid archive digest"
        );
    }
    Ok(manifest)
}

pub fn initial_state() -> UpdateState {
    UpdateState {
        current_version: env!("CARGO_PKG_VERSION").into(),
        build: option_env!("RACKFORGE_REVISION")
            .unwrap_or("unknown")
            .into(),
        platform: target().into(),
        phase: UpdatePhase::Idle,
        available: None,
        message: None,
        downloaded_bytes: 0,
        can_stage: target() != "unsupported" && !PUBLIC_KEY.trim().is_empty(),
    }
}

fn agent(timeout_seconds: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_seconds)))
        .timeout_connect(Some(Duration::from_secs(15)))
        .https_only(true)
        .build()
        .into()
}

fn fetch_small(url: &str, limit: u64) -> Result<Vec<u8>> {
    let mut response = agent(30).get(url).header("User-Agent", "RackForge-Updater").header("Cache-Control", "no-cache").call()
        .context("Could not retrieve signed update metadata. Check the connection and whether a signed release has been published.")?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Update metadata exceeds its size limit"
    );
    Ok(bytes)
}

pub fn check_release(
    current: &str,
    platform: &str,
) -> Result<(ReleaseManifest, Option<ReleaseOffer>, Vec<u8>, String)> {
    asset_name(platform)?;
    let bytes = fetch_small(
        &format!("{REPOSITORY}/releases/latest/download/updates.json"),
        MAX_METADATA,
    )?;
    // This untrusted field only selects a fixed repository path. It is never
    // an offer until the exact bytes have passed signature verification.
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let version = Version::parse(
        value["version"]
            .as_str()
            .context("Release has no version")?,
    )?;
    let signature = String::from_utf8(fetch_small(
        &format!("{REPOSITORY}/releases/download/v{version}/updates.json.sig"),
        1024,
    )?)?;
    let manifest = verify_manifest(&bytes, &signature, PUBLIC_KEY)?;
    let offer = release_offer(&manifest, current, platform)?;
    Ok((manifest, offer, bytes, signature))
}

pub fn release_offer(
    manifest: &ReleaseManifest,
    current: &str,
    platform: &str,
) -> Result<Option<ReleaseOffer>> {
    let asset = manifest
        .targets
        .get(platform)
        .context("This release has no package for this platform")?;
    Ok(
        (Version::parse(&manifest.version)? > Version::parse(current)?).then(|| ReleaseOffer {
            version: manifest.version.clone(),
            notes: manifest.notes.clone(),
            download_bytes: asset.bytes,
        }),
    )
}

pub fn verify_archive(path: &Path, asset: &ReleaseAsset) -> Result<()> {
    ensure!(
        fs::metadata(path)?.len() == asset.bytes,
        "Archive size does not match the signed release"
    );
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut block = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut block)?;
        if n == 0 {
            break;
        }
        digest.update(&block[..n]);
    }
    ensure!(
        format!("{:x}", digest.finalize()).eq_ignore_ascii_case(&asset.sha256),
        "Archive digest does not match the signed release"
    );
    Ok(())
}

pub fn stage_release(
    root: &Path,
    manifest: &ReleaseManifest,
    metadata: &[u8],
    signature: &str,
    platform: &str,
    progress: impl Fn(u64),
) -> Result<PathBuf> {
    // Revalidate immediately before using any download address or filesystem name.
    let verified = verify_manifest(metadata, signature, PUBLIC_KEY)?;
    ensure!(
        verified.version == manifest.version,
        "The selected release changed"
    );
    let asset = verified
        .targets
        .get(platform)
        .context("Missing update target")?;
    let stage = root.join("state/updates").join(&verified.version);
    fs::create_dir_all(&stage)?;
    let partial = stage.join("download.part");
    let archive = stage.join(asset_name(platform)?);
    if archive.exists() && verify_archive(&archive, asset).is_ok() {
        fs::write(stage.join("updates.json"), metadata)?;
        fs::write(stage.join("updates.json.sig"), signature)?;
        progress(asset.bytes);
        return Ok(archive);
    }
    let _ = fs::remove_file(&partial);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&partial)?;
    let result = (|| -> Result<()> {
        let mut response = agent(300)
            .get(&asset.url)
            .header("User-Agent", "RackForge-Updater")
            .call()?;
        let mut reader = response.body_mut().as_reader();
        let mut total = 0u64;
        let mut block = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut block)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            ensure!(total <= asset.bytes, "Download exceeds the signed size");
            file.write_all(&block[..n])?;
            progress(total);
        }
        file.sync_all()?;
        verify_archive(&partial, asset)?;
        fs::write(stage.join("updates.json"), metadata)?;
        fs::write(stage.join("updates.json.sig"), signature)?;
        fs::rename(&partial, &archive)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result?;
    Ok(archive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn candidate() -> ReleaseManifest {
        ReleaseManifest {
            schema_version: 1,
            product: PRODUCT.into(),
            version: "0.1.40".into(),
            notes: "A new host".into(),
            targets: BTreeMap::from([(
                "linux-aarch64-services".into(),
                ReleaseAsset {
                    url: format!(
                        "{REPOSITORY}/releases/download/v0.1.40/RackForge-RaspberryPi-arm64.tar.gz"
                    ),
                    bytes: 5,
                    sha256: format!("{:x}", Sha256::digest(b"hello")),
                },
            )]),
        }
    }

    fn signed(manifest: &ReleaseManifest) -> (Vec<u8>, String, String) {
        // A test-only key, distinct from the public release key compiled above.
        let key = SigningKey::from_bytes(&[31; 32]);
        let bytes = serde_json::to_vec(manifest).unwrap();
        let signature = STANDARD.encode(key.sign(&bytes).to_bytes());
        (
            bytes,
            signature,
            STANDARD.encode(key.verifying_key().to_bytes()),
        )
    }

    #[test]
    fn authentic_metadata_selects_only_a_newer_compatible_release() {
        let (bytes, signature, key) = signed(&candidate());
        let manifest = verify_manifest(&bytes, &signature, &key).unwrap();
        assert_eq!(
            release_offer(&manifest, "0.1.9", "linux-aarch64-services")
                .unwrap()
                .unwrap()
                .version,
            "0.1.40"
        );
        assert!(
            release_offer(&manifest, "0.1.40", "linux-aarch64-services")
                .unwrap()
                .is_none()
        );
        assert!(
            release_offer(&manifest, "0.2.0", "linux-aarch64-services")
                .unwrap()
                .is_none()
        );
        assert!(release_offer(&manifest, "0.1.39", "linux-x86_64-services").is_err());
    }

    #[test]
    fn changing_a_byte_or_using_another_key_rejects_the_release() {
        let (mut bytes, signature, key) = signed(&candidate());
        bytes[1] ^= 1;
        assert!(verify_manifest(&bytes, &signature, &key).is_err());
        let (bytes, signature, _) = signed(&candidate());
        let other_key =
            STANDARD.encode(SigningKey::from_bytes(&[32; 32]).verifying_key().to_bytes());
        assert!(verify_manifest(&bytes, &signature, &other_key).is_err());
        assert!(verify_manifest(&bytes, "", &key).is_err());
        assert!(verify_manifest(&bytes, &signature, "").is_err());
    }

    #[test]
    fn even_a_valid_signature_cannot_authorize_another_product_or_address() {
        for modification in 0..6 {
            let mut manifest = candidate();
            match modification {
                0 => manifest.product = "org.other.application".into(),
                1 => manifest.version = "0.1.40-alpha.1".into(),
                2 => manifest.schema_version = 2,
                3 => {
                    manifest
                        .targets
                        .get_mut("linux-aarch64-services")
                        .unwrap()
                        .url = "https://example.com/installer.sh".into()
                }
                4 => {
                    manifest
                        .targets
                        .get_mut("linux-aarch64-services")
                        .unwrap()
                        .bytes = MAX_ARCHIVE + 1
                }
                _ => {
                    manifest
                        .targets
                        .get_mut("linux-aarch64-services")
                        .unwrap()
                        .sha256 = "bad".into()
                }
            }
            let (bytes, signature, key) = signed(&manifest);
            assert!(
                verify_manifest(&bytes, &signature, &key).is_err(),
                "case {modification}"
            );
        }
    }

    #[test]
    fn archives_require_both_signed_size_and_digest() {
        let path = std::env::temp_dir().join(format!("rf-update-digest-{}", std::process::id()));
        let manifest = candidate();
        let asset = &manifest.targets["linux-aarch64-services"];
        fs::write(&path, b"hello").unwrap();
        assert!(verify_archive(&path, asset).is_ok());
        fs::write(&path, b"other").unwrap();
        assert!(verify_archive(&path, asset).is_err());
        fs::write(&path, b"hello!").unwrap();
        assert!(verify_archive(&path, asset).is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn request_protocol_never_accepts_a_download_url_or_command() {
        use rackforge_platform_api::updates::UpdateRequest;
        assert!(
            serde_json::from_str::<UpdateRequest>(
                r#"{"op":"stage","version":"0.1.40","url":"https://example.com"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<UpdateRequest>(r#"{"op":"execute","command":"sh"}"#).is_err()
        );
    }
}
