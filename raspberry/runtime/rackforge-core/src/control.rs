use anyhow::{Context, Result, bail};
use rackforge_control_api::{
    ControlRequest, ControlResponse, LiveSnapshot, MAX_CONTROL_MESSAGE_BYTES, decode_request,
    encode_line,
};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub enum ControlCommand {
    SelectSound {
        id: String,
        reply: SyncSender<Result<String, String>>,
    },
    BeginAudition {
        plugin_id: String,
        reply: SyncSender<Result<u64, String>>,
    },
    KeepAuditionAlive {
        lease_id: u64,
        reply: SyncSender<Result<u64, String>>,
    },
    EndAudition {
        lease_id: u64,
        reply: SyncSender<Result<(), String>>,
    },
}

pub struct ControlServer {
    _thread: JoinHandle<()>,
}

pub fn start(
    socket_path: &Path,
    snapshot: Arc<Mutex<LiveSnapshot>>,
    command_sender: Sender<ControlCommand>,
) -> Result<ControlServer> {
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating control directory {}", parent.display()))?;
    }
    if socket_path.exists() {
        if UnixStream::connect(socket_path).is_ok() {
            bail!(
                "another RackForge control server is active at {}",
                socket_path.display()
            );
        }
        let metadata = fs::symlink_metadata(socket_path)
            .with_context(|| format!("inspecting stale socket {}", socket_path.display()))?;
        if !metadata.file_type().is_socket() {
            bail!(
                "refusing to replace non-socket control path {}",
                socket_path.display()
            );
        }
        fs::remove_file(socket_path)
            .with_context(|| format!("removing stale socket {}", socket_path.display()))?;
    }

    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("binding control socket {}", socket_path.display()))?;
    fs::set_permissions(socket_path, fs::Permissions::from_mode(0o660)).with_context(|| {
        format!(
            "setting control socket permissions {}",
            socket_path.display()
        )
    })?;
    let path = socket_path.to_path_buf();
    let thread = thread::Builder::new()
        .name("rackforge-control".into())
        .spawn(move || serve(listener, path, snapshot, command_sender))
        .context("spawning RackForge control server")?;
    Ok(ControlServer { _thread: thread })
}

fn serve(
    listener: UnixListener,
    socket_path: PathBuf,
    snapshot: Arc<Mutex<LiveSnapshot>>,
    command_sender: Sender<ControlCommand>,
) {
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                if let Err(error) = handle_connection(stream, &snapshot, &command_sender) {
                    eprintln!("CONTROL_CLIENT_ERROR {error:#}");
                }
            }
            Err(error) => {
                eprintln!(
                    "CONTROL_ACCEPT_ERROR path={} error={error}",
                    socket_path.display()
                );
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn handle_connection(
    mut stream: UnixStream,
    snapshot: &Arc<Mutex<LiveSnapshot>>,
    command_sender: &Sender<ControlCommand>,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let mut bytes = Vec::new();
    BufReader::new(&stream)
        .take((MAX_CONTROL_MESSAGE_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .context("reading control request")?;
    if bytes.is_empty() || bytes.len() > MAX_CONTROL_MESSAGE_BYTES {
        return write_response(
            &mut stream,
            &ControlResponse::Error {
                message: "invalid control message size".into(),
            },
        );
    }
    let request = match decode_request(&bytes) {
        Ok(request) => request,
        Err(error) => {
            return write_response(
                &mut stream,
                &ControlResponse::Error {
                    message: format!("invalid control request: {error}"),
                },
            );
        }
    };
    let response = match request {
        ControlRequest::Snapshot => match snapshot.lock() {
            Ok(snapshot) => ControlResponse::Snapshot {
                snapshot: snapshot.clone(),
            },
            Err(_) => ControlResponse::Error {
                message: "live snapshot lock is poisoned".into(),
            },
        },
        ControlRequest::SelectSound { id } => {
            let (reply_sender, reply_receiver) = sync_channel(1);
            if command_sender
                .send(ControlCommand::SelectSound {
                    id,
                    reply: reply_sender,
                })
                .is_err()
            {
                ControlResponse::Error {
                    message: "LIVE audio engine is unavailable".into(),
                }
            } else {
                selection_response(reply_receiver)
            }
        }
        ControlRequest::BeginAudition { plugin_id } => {
            let (reply_sender, reply_receiver) = sync_channel(1);
            if command_sender
                .send(ControlCommand::BeginAudition {
                    plugin_id,
                    reply: reply_sender,
                })
                .is_err()
            {
                unavailable_response()
            } else {
                match reply_receiver.recv_timeout(Duration::from_secs(1)) {
                    Ok(Ok(lease_id)) => ControlResponse::AuditionGranted { lease_id },
                    Ok(Err(message)) => ControlResponse::Error { message },
                    Err(_) => ControlResponse::Error {
                        message: "LIVE did not grant audition focus in time".into(),
                    },
                }
            }
        }
        ControlRequest::KeepAuditionAlive { lease_id } => {
            let (reply_sender, reply_receiver) = sync_channel(1);
            if command_sender
                .send(ControlCommand::KeepAuditionAlive {
                    lease_id,
                    reply: reply_sender,
                })
                .is_err()
            {
                unavailable_response()
            } else {
                match reply_receiver.recv_timeout(Duration::from_secs(1)) {
                    Ok(Ok(lease_id)) => ControlResponse::AuditionKeptAlive { lease_id },
                    Ok(Err(message)) => ControlResponse::Error { message },
                    Err(_) => ControlResponse::Error {
                        message: "LIVE did not renew audition focus in time".into(),
                    },
                }
            }
        }
        ControlRequest::EndAudition { lease_id } => {
            let (reply_sender, reply_receiver) = sync_channel(1);
            if command_sender
                .send(ControlCommand::EndAudition {
                    lease_id,
                    reply: reply_sender,
                })
                .is_err()
            {
                unavailable_response()
            } else {
                match reply_receiver.recv_timeout(Duration::from_secs(1)) {
                    Ok(Ok(())) => ControlResponse::AuditionEnded,
                    Ok(Err(message)) => ControlResponse::Error { message },
                    Err(_) => ControlResponse::Error {
                        message: "LIVE did not release audition focus in time".into(),
                    },
                }
            }
        }
    };
    write_response(&mut stream, &response)
}

fn unavailable_response() -> ControlResponse {
    ControlResponse::Error {
        message: "LIVE audio engine is unavailable".into(),
    }
}

fn selection_response(receiver: Receiver<Result<String, String>>) -> ControlResponse {
    match receiver.recv_timeout(Duration::from_secs(1)) {
        Ok(Ok(selected_sound_id)) => ControlResponse::Ok { selected_sound_id },
        Ok(Err(message)) => ControlResponse::Error { message },
        Err(_) => ControlResponse::Error {
            message: "LIVE did not apply the sound selection in time".into(),
        },
    }
}

fn write_response(stream: &mut UnixStream, response: &ControlResponse) -> Result<()> {
    let bytes = encode_line(response).context("serializing control response")?;
    stream
        .write_all(&bytes)
        .context("writing control response")?;
    stream.flush().context("flushing control response")
}
