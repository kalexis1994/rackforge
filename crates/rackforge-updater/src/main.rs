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
        ["serve", root, socket] => serve(PathBuf::from(root), Path::new(socket)),
        ["status"] => client_request("status", None),
        ["check"] => client_request("check", None),
        ["stage", version] => client_request("stage", Some(version)),
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
            "usage: rackforge-updater status|check|stage VERSION | serve ROOT SOCKET | keygen SECRET PUBLIC | sign MANIFEST SECRET SIGNATURE | verify MANIFEST SIGNATURE ARCHIVE PLATFORM"
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
            _ => UpdateRequest::Status,
        };
        let state = rackforge_platform_api::updates::exchange(
            Path::new(rackforge_platform_api::updates::UPDATE_SOCKET),
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
    if matches!(request, UpdateRequest::Status) || inner.state.busy() {
        return inner.state.clone();
    }
    let stage = match &request {
        UpdateRequest::Stage { version } => inner.found.clone().filter(|r| {
            r.manifest.version == *version
                && inner
                    .state
                    .available
                    .as_ref()
                    .is_some_and(|offer| offer.version == *version)
        }),
        _ => None,
    };
    if matches!(request, UpdateRequest::Stage { .. }) && stage.is_none() {
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
                inner.state.phase = UpdatePhase::Ready;
                inner.state.message = Some("The signed package is verified and ready. Activation is not yet supported from the interface.".into());
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
