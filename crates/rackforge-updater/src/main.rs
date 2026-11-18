use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
#[cfg(unix)]
use rackforge_platform_api::updates::{UpdatePhase, UpdateRequest, UpdateState};
#[cfg(unix)]
use rackforge_updater::{ReleaseManifest, initial_state};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::{Arc, Mutex};

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["public-key"] => {
            println!("{}", rackforge_updater::PUBLIC_KEY.trim());
            Ok(())
        }
        ["serve", root, socket] => serve(PathBuf::from(root), Path::new(socket)),
        #[cfg(unix)]
        ["supervise", config] => rackforge_updater::installer::supervise(Path::new(config)),
        #[cfg(unix)]
        ["installer-worker", config] => rackforge_updater::installer::worker(Path::new(config)),
        ["status"] => client_request("status", None),
        ["check"] => client_request("check", None),
        ["stage", version] => client_request("stage", Some(version)),
        ["install", version] => client_request("install", Some(version)),
        ["recover"] => client_request("recover", None),
        ["offer-staged", version] => client_request("offer-staged", Some(version)),
        ["keygen", secret, public] => {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let key = SigningKey::from_bytes(&bytes);
            write_new(
                Path::new(secret),
                STANDARD.encode(key.to_bytes()).as_bytes(),
            )?;
            write_new(
                Path::new(public),
                STANDARD.encode(key.verifying_key().to_bytes()).as_bytes(),
            )?;
            println!("UPDATE_KEY_CREATED public={public}");
            Ok(())
        }
        ["sign", metadata, secret, output] => {
            let bytes = fs::read(metadata)?;
            let key: [u8; 32] = STANDARD
                .decode(fs::read_to_string(secret)?.trim())?
                .try_into()
                .map_err(|_| anyhow::anyhow!("Invalid signing key length"))?;
            let signature = STANDARD.encode(SigningKey::from_bytes(&key).sign(&bytes).to_bytes());
            // Refuse to sign a malformed manifest or a different application.
            rackforge_updater::verify_manifest(
                &bytes,
                &signature,
                &STANDARD.encode(SigningKey::from_bytes(&key).verifying_key().to_bytes()),
            )?;
            write_new(Path::new(output), signature.as_bytes())?;
            Ok(())
        }
        ["verify", metadata, signature, archive, platform] => {
            let manifest = rackforge_updater::verify_manifest(
                &fs::read(metadata)?,
                &fs::read_to_string(signature)?,
                rackforge_updater::PUBLIC_KEY,
            )?;
            let asset = manifest.targets.get(*platform).context("Missing target")?;
            rackforge_updater::verify_archive(Path::new(archive), asset)?;
            println!(
                "UPDATE_VERIFIED version={} platform={platform}",
                manifest.version
            );
            Ok(())
        }
        _ => bail!(
            "usage: rackforge-updater status|check|stage VERSION|install VERSION|offer-staged VERSION|recover|public-key | serve ROOT SOCKET | supervise CONFIG | keygen SECRET PUBLIC | sign MANIFEST SECRET SIGNATURE | verify MANIFEST SIGNATURE ARCHIVE PLATFORM"
        ),
    }
}

fn client_request(operation: &str, version: Option<&str>) -> Result<()> {
    #[cfg(unix)]
    {
        let request = match operation {
            "check" => UpdateRequest::Check,
            "stage" => UpdateRequest::Stage {
                version: version.context("Missing release version")?.into(),
            },
            "install" => UpdateRequest::Install {
                version: version.context("Missing release version")?.into(),
            },
            "recover" => UpdateRequest::Recover,
            "offer-staged" => UpdateRequest::UseStaged {
                version: version.context("Missing release version")?.into(),
            },
            _ => UpdateRequest::Status,
        };
        let state = rackforge_platform_api::updates::exchange(
            &rackforge_platform_api::updates::update_socket(),
            &request,
        )
        .map_err(|message| anyhow::anyhow!(message))?;
        println!("{}", serde_json::to_string_pretty(&state)?);
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (operation, version);
        bail!("The managed update service requires Linux")
    }
}

#[derive(Clone)]
#[cfg(unix)]
struct VerifiedRelease {
    manifest: ReleaseManifest,
    bytes: Vec<u8>,
    signature: String,
}
#[cfg(unix)]
struct Inner {
    state: UpdateState,
    found: Option<VerifiedRelease>,
}

#[cfg(unix)]
fn serve(root: PathBuf, socket: &Path) -> Result<()> {
    use std::io::{BufRead, BufReader, Read};
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::os::unix::net::UnixListener;
    use std::time::Duration;
    if let Ok(metadata) = fs::symlink_metadata(socket) {
        anyhow::ensure!(
            metadata.file_type().is_socket(),
            "Refusing to replace a non-socket update control path"
        );
        fs::remove_file(socket)?;
    }
    let listener = UnixListener::bind(socket)?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o660))?;
    let shared = Arc::new(Mutex::new(Inner {
        state: initial_state(),
        found: None,
    }));
    println!(
        "UPDATER_READY version={} socket={}",
        env!("CARGO_PKG_VERSION"),
        socket.display()
    );
    for connection in listener.incoming() {
        let Ok(mut stream) = connection else { continue };
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let mut line = String::new();
        let request = BufReader::new(&stream)
            .take(4096)
            .read_line(&mut line)
            .ok()
            .and_then(|_| serde_json::from_str::<UpdateRequest>(&line).ok());
        let response = match request {
            Some(request) => dispatch(Arc::clone(&shared), root.clone(), request),
            None => {
                let mut state = shared.lock().unwrap().state.clone();
                state.message = Some("Invalid update request".into());
                state
            }
        };
        if serde_json::to_writer(&mut stream, &response).is_ok() {
            let _ = stream.write_all(b"\n");
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn serve(_root: PathBuf, _socket: &Path) -> Result<()> {
    bail!("The managed updater service requires Unix sockets")
}

#[cfg(unix)]
fn dispatch(shared: Arc<Mutex<Inner>>, root: PathBuf, request: UpdateRequest) -> UpdateState {
    let mut inner = shared.lock().unwrap();
    let installer_socket = std::env::var_os("RACKFORGE_INSTALL_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| rackforge_platform_api::updates::INSTALL_SOCKET.into());
    if let Ok(status) =
        rackforge_updater::installer::exchange(&installer_socket, &UpdateRequest::Status)
    {
        inner.state.can_install = true;
        inner.state.current_version = status.current_version;
        inner.state.build = status.build;
        if matches!(
            status.phase,
            UpdatePhase::Installing | UpdatePhase::Recovering | UpdatePhase::RecoveryFailed
        ) || matches!(
            inner.state.phase,
            UpdatePhase::Idle | UpdatePhase::Installing | UpdatePhase::Recovering
        ) {
            inner.state.phase = status.phase;
            inner.state.message = status.message;
        }
    } else {
        inner.state.can_install = false;
    }
    if matches!(request, UpdateRequest::Recover) && !inner.state.busy() {
        match rackforge_updater::installer::exchange(&installer_socket, &request) {
            Ok(status) => {
                inner.state.phase = status.phase;
                inner.state.message = status.message;
            }
            Err(error) => {
                inner.state.phase = UpdatePhase::Failed;
                inner.state.message = Some(format!("{error:#}"));
            }
        }
        return inner.state.clone();
    }
    if matches!(request, UpdateRequest::Status) || inner.state.busy() {
        return inner.state.clone();
    }
    if let UpdateRequest::UseStaged { version } = &request {
        let result = (|| -> Result<VerifiedRelease> {
            let parsed = semver::Version::parse(version)?;
            anyhow::ensure!(
                parsed.pre.is_empty() && parsed.build.is_empty(),
                "Use a stable release version"
            );
            let stage = root.join("state/updates").join(version);
            let bytes = fs::read(stage.join("updates.json"))?;
            let signature = fs::read_to_string(stage.join("updates.json.sig"))?;
            let manifest = rackforge_updater::verify_manifest(
                &bytes,
                &signature,
                rackforge_updater::PUBLIC_KEY,
            )?;
            anyhow::ensure!(manifest.version == *version, "Cached version mismatch");
            let offer = rackforge_updater::release_offer(
                &manifest,
                &inner.state.current_version,
                &inner.state.platform,
            )?
            .context("The cached release is not newer")?;
            rackforge_updater::verify_archive(
                &stage.join(rackforge_updater::asset_name(&inner.state.platform)?),
                &manifest.targets[&inner.state.platform],
            )?;
            inner.state.available = Some(offer);
            Ok(VerifiedRelease {
                manifest,
                bytes,
                signature,
            })
        })();
        match result {
            Ok(release) => {
                inner.found = Some(release);
                inner.state.phase = UpdatePhase::Ready;
                inner.state.message = Some("The offline signed update is ready to install".into());
            }
            Err(error) => {
                inner.state.phase = UpdatePhase::Failed;
                inner.state.message = Some(format!("{error:#}"));
            }
        }
        return inner.state.clone();
    }
    let stage = match &request {
        UpdateRequest::Stage { version } | UpdateRequest::Install { version } => {
            inner.found.clone().filter(|r| {
                r.manifest.version == *version
                    && inner
                        .state
                        .available
                        .as_ref()
                        .is_some_and(|offer| offer.version == *version)
            })
        }
        _ => None,
    };
    let install = matches!(request, UpdateRequest::Install { .. });
    if install && !inner.state.can_install {
        let mut state = inner.state.clone();
        state.phase = UpdatePhase::Failed;
        state.message = Some("The installation agent is not available".into());
        return state;
    }
    if matches!(
        request,
        UpdateRequest::Stage { .. } | UpdateRequest::Install { .. }
    ) && stage.is_none()
    {
        let mut state = inner.state.clone();
        state.phase = UpdatePhase::Failed;
        state.message = Some("Check for this release before downloading it".into());
        return state;
    }
    if !inner.state.can_stage {
        inner.state.phase = UpdatePhase::Unsupported;
        inner.state.message =
            Some("Signed updates are not configured for this managed host".into());
        return inner.state.clone();
    }
    inner.state.phase = if stage.is_some() {
        UpdatePhase::Downloading
    } else {
        UpdatePhase::Checking
    };
    inner.state.message = None;
    inner.state.downloaded_bytes = 0;
    if stage.is_none() {
        inner.found = None;
        inner.state.available = None;
    }
    let response = inner.state.clone();
    drop(inner);
    let preparing = stage.is_some();
    std::thread::spawn(move || {
        let result: Result<()> = (|| {
            if let Some(release) = stage {
                let platform = shared.lock().unwrap().state.platform.clone();
                rackforge_updater::stage_release(
                    &root,
                    &release.manifest,
                    &release.bytes,
                    &release.signature,
                    &platform,
                    |bytes| shared.lock().unwrap().state.downloaded_bytes = bytes,
                )?;
                let mut inner = shared.lock().unwrap();
                if install {
                    let status = rackforge_updater::installer::exchange(
                        &installer_socket,
                        &UpdateRequest::Install {
                            version: release.manifest.version,
                        },
                    )?;
                    inner.state.phase = status.phase;
                    inner.state.message = status.message;
                } else {
                    inner.state.phase = UpdatePhase::Ready;
                    inner.state.message =
                        Some("The signed package is verified and ready to install.".into());
                }
            } else {
                let state = shared.lock().unwrap().state.clone();
                let (manifest, offer, bytes, signature) =
                    rackforge_updater::check_release(&state.current_version, &state.platform)?;
                let mut inner = shared.lock().unwrap();
                inner.state.phase = if offer.is_some() {
                    UpdatePhase::Available
                } else {
                    UpdatePhase::UpToDate
                };
                inner.state.available = offer;
                inner.found = Some(VerifiedRelease {
                    manifest,
                    bytes,
                    signature,
                });
            }
            Ok(())
        })();
        if let Err(error) = result {
            let mut inner = shared.lock().unwrap();
            if !preparing
                && matches!(
                    error.downcast_ref::<ureq::Error>(),
                    Some(ureq::Error::StatusCode(404))
                )
            {
                inner.state.phase = UpdatePhase::NotPublished;
                inner.state.message =
                    Some("No signed RackForge update has been published yet.".into());
            } else {
                inner.state.phase = UpdatePhase::Failed;
                inner.state.message = Some(format!("{error:#}"));
            }
        }
    });
    response
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn staging_requires_a_newer_verified_offer_and_never_accepts_an_up_to_date_release() {
        let mut state = initial_state();
        state.phase = UpdatePhase::UpToDate;
        let shared = Arc::new(Mutex::new(Inner {
            state,
            found: Some(VerifiedRelease {
                manifest: ReleaseManifest {
                    schema_version: 1,
                    product: rackforge_updater::PRODUCT.into(),
                    version: "0.1.0".into(),
                    notes: String::new(),
                    targets: BTreeMap::new(),
                },
                bytes: Vec::new(),
                signature: String::new(),
            }),
        }));
        let result = dispatch(
            Arc::clone(&shared),
            PathBuf::from("/unused"),
            UpdateRequest::Stage {
                version: "0.1.0".into(),
            },
        );
        assert_eq!(result.phase, UpdatePhase::Failed);
        assert!(result.message.unwrap().contains("Check for this release"));
        assert_eq!(shared.lock().unwrap().state.phase, UpdatePhase::UpToDate);
    }
    #[test]
    fn concurrent_checks_do_not_start_another_background_job() {
        let mut state = initial_state();
        state.phase = UpdatePhase::Checking;
        let shared = Arc::new(Mutex::new(Inner {
            state: state.clone(),
            found: None,
        }));
        assert_eq!(
            dispatch(shared, PathBuf::from("/unused"), UpdateRequest::Check),
            state
        );
    }
}
