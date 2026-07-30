use crate::session::SharedSessionStore;
use anyhow::{Context, Result, bail};
use rackforge_control_api::{
    ControlErrorCode, ControlRequest, ControlResponse, MAX_CONTROL_MESSAGE_BYTES, decode_request,
    encode_line,
};
use rackforge_session_api::{
    AuditionEndReason, CommandEnvelope, CommandRef, InstanceId, Revision, SESSION_SCHEMA_VERSION,
    SessionCommand, SessionEvent,
};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const AUDIO_COMMAND_TIMEOUT: Duration = Duration::from_secs(1);
const AUDITION_LEASE_TIMEOUT: Duration = Duration::from_secs(15);
const AUDITION_WATCHDOG_PERIOD: Duration = Duration::from_millis(250);

pub enum AudioControlCommand {
    SelectSound {
        instance_id: InstanceId,
        sound_id: String,
        reply: SyncSender<Result<(), String>>,
    },
    BeginAudition {
        instance_id: InstanceId,
        previous_sound_id: Option<String>,
        reply: SyncSender<Result<u64, String>>,
    },
    KeepAuditionAlive {
        lease_id: u64,
        reply: SyncSender<Result<(), String>>,
    },
    EndAudition {
        lease_id: u64,
        reply: SyncSender<Result<(), String>>,
    },
}

struct LeaseDeadline {
    lease_id: u64,
    deadline: Instant,
}

struct ControlFailure {
    code: ControlErrorCode,
    message: String,
    current_revision: Option<Revision>,
}

impl ControlFailure {
    fn into_response(self) -> ControlResponse {
        error_response(self.code, self.message, self.current_revision)
    }
}

struct ControlContext {
    store: SharedSessionStore,
    audio_sender: SyncSender<AudioControlCommand>,
    dispatch_lock: Mutex<()>,
    lease_deadline: Mutex<Option<LeaseDeadline>>,
}

pub struct ControlServer {
    _server_thread: JoinHandle<()>,
    _watchdog_thread: JoinHandle<()>,
}

pub fn start(
    socket_path: &Path,
    store: SharedSessionStore,
    audio_sender: SyncSender<AudioControlCommand>,
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

    let context = Arc::new(ControlContext {
        store,
        audio_sender,
        dispatch_lock: Mutex::new(()),
        lease_deadline: Mutex::new(None),
    });
    let path = socket_path.to_path_buf();
    let server_context = Arc::clone(&context);
    let server_thread = thread::Builder::new()
        .name("rackforge-control".into())
        .spawn(move || serve(listener, path, server_context))
        .context("spawning RackForge control server")?;
    let watchdog_thread = thread::Builder::new()
        .name("rackforge-audition-watchdog".into())
        .spawn(move || audition_watchdog(context))
        .context("spawning RackForge audition watchdog")?;
    Ok(ControlServer {
        _server_thread: server_thread,
        _watchdog_thread: watchdog_thread,
    })
}

fn serve(listener: UnixListener, socket_path: PathBuf, context: Arc<ControlContext>) {
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                if let Err(error) = handle_connection(stream, &context) {
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

fn handle_connection(mut stream: UnixStream, context: &Arc<ControlContext>) -> Result<()> {
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
            &error_response(
                ControlErrorCode::InvalidRequest,
                "invalid control message size",
                None,
            ),
        );
    }
    let request = match decode_request(&bytes) {
        Ok(request) => request,
        Err(error) => {
            return write_response(
                &mut stream,
                &error_response(
                    ControlErrorCode::InvalidRequest,
                    format!("invalid control request: {error}"),
                    None,
                ),
            );
        }
    };
    let response = match request {
        ControlRequest::Snapshot => match context.store.lock() {
            Ok(store) => ControlResponse::Snapshot {
                snapshot: store.snapshot(),
            },
            Err(_) => internal_error("session store lock is poisoned", None),
        },
        ControlRequest::Events { after_revision } => match context.store.lock() {
            Ok(store) => match store.events_after(after_revision) {
                Ok(events) => ControlResponse::Events {
                    current_revision: store.state().revision,
                    events,
                },
                Err(error) => error_response(
                    ControlErrorCode::Conflict,
                    error.to_string(),
                    Some(store.state().revision),
                ),
            },
            Err(_) => internal_error("session store lock is poisoned", None),
        },
        ControlRequest::Dispatch { envelope } => dispatch_command(context, envelope),
    };
    write_response(&mut stream, &response)
}

fn dispatch_command(context: &Arc<ControlContext>, envelope: CommandEnvelope) -> ControlResponse {
    if envelope.schema_version != SESSION_SCHEMA_VERSION || envelope.command_id == 0 {
        return error_response(
            ControlErrorCode::InvalidRequest,
            format!(
                "invalid command envelope schema={} id={}",
                envelope.schema_version, envelope.command_id
            ),
            current_revision(context),
        );
    }
    let _dispatch_guard = match context.dispatch_lock.lock() {
        Ok(guard) => guard,
        Err(_) => {
            return internal_error(
                "command dispatch lock is poisoned",
                current_revision(context),
            );
        }
    };
    let snapshot = match context.store.lock() {
        Ok(store) => store.snapshot(),
        Err(_) => return internal_error("session store lock is poisoned", None),
    };
    if let Some(expected) = envelope.expected_revision
        && expected != snapshot.revision
    {
        return error_response(
            ControlErrorCode::Conflict,
            format!(
                "expected revision {}, current revision is {}",
                expected.get(),
                snapshot.revision.get()
            ),
            Some(snapshot.revision),
        );
    }
    let command_ref = CommandRef {
        client_id: envelope.client_id,
        command_id: envelope.command_id,
    };

    match envelope.command {
        SessionCommand::SelectSound {
            instance_id,
            sound_id,
        } => {
            let instance = match require_active_instance(&snapshot, &instance_id) {
                Ok(instance) => instance,
                Err(failure) => return failure.into_response(),
            };
            if !instance.sounds.iter().any(|sound| sound.id == sound_id) {
                return error_response(
                    ControlErrorCode::NotFound,
                    format!("unknown sound {sound_id:?} for instance {instance_id}"),
                    Some(snapshot.revision),
                );
            }
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::SelectSound {
                    instance_id: instance_id.clone(),
                    sound_id: sound_id.clone(),
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "apply sound selection") {
                Ok(()) => record_command_event(
                    context,
                    command_ref,
                    SessionEvent::SoundSelected {
                        instance_id,
                        sound_id,
                    },
                ),
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::BeginAudition { instance_id } => {
            let instance = match require_active_instance(&snapshot, &instance_id) {
                Ok(instance) => instance,
                Err(failure) => return failure.into_response(),
            };
            if snapshot.audition.is_some() {
                return error_response(
                    ControlErrorCode::Conflict,
                    "audition focus is already leased",
                    Some(snapshot.revision),
                );
            }
            let previous_sound_id = instance.selected_sound_id.clone();
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::BeginAudition {
                    instance_id: instance_id.clone(),
                    previous_sound_id: previous_sound_id.clone(),
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "grant audition focus") {
                Ok(lease_id) => {
                    let response = record_command_event(
                        context,
                        command_ref,
                        SessionEvent::AuditionStarted {
                            lease_id,
                            instance_id,
                            previous_sound_id,
                        },
                    );
                    if matches!(response, ControlResponse::CommandApplied { .. }) {
                        set_lease_deadline(context, Some(lease_id));
                    }
                    response
                }
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::KeepAuditionAlive { lease_id } => {
            if snapshot
                .audition
                .as_ref()
                .is_none_or(|audition| audition.lease_id != lease_id)
            {
                return error_response(
                    ControlErrorCode::NotFound,
                    "audition lease is missing or no longer valid",
                    Some(snapshot.revision),
                );
            }
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::KeepAuditionAlive {
                    lease_id,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "renew audition focus") {
                Ok(()) => {
                    set_lease_deadline(context, Some(lease_id));
                    ControlResponse::CommandApplied {
                        client_id: command_ref.client_id,
                        command_id: command_ref.command_id,
                        revision: snapshot.revision,
                        events: Vec::new(),
                    }
                }
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::EndAudition { lease_id } => {
            let audition = match snapshot
                .audition
                .as_ref()
                .filter(|audition| audition.lease_id == lease_id)
            {
                Some(audition) => audition,
                None => {
                    return error_response(
                        ControlErrorCode::NotFound,
                        "audition lease is missing or no longer valid",
                        Some(snapshot.revision),
                    );
                }
            };
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::EndAudition {
                    lease_id,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "release audition focus") {
                Ok(()) => {
                    set_lease_deadline(context, None);
                    record_command_event(
                        context,
                        command_ref,
                        SessionEvent::AuditionEnded {
                            lease_id,
                            instance_id: audition.instance_id.clone(),
                            restored_sound_id: audition.previous_sound_id.clone(),
                            reason: AuditionEndReason::Released,
                        },
                    )
                }
                Err(failure) => failure.into_response(),
            }
        }
    }
}

fn require_active_instance<'a>(
    snapshot: &'a rackforge_session_api::SessionState,
    instance_id: &InstanceId,
) -> Result<&'a rackforge_session_api::AddonInstanceState, ControlFailure> {
    let instance = snapshot.instance(instance_id).ok_or_else(|| {
        control_failure(
            ControlErrorCode::NotFound,
            format!("unknown instance {instance_id}"),
            Some(snapshot.revision),
        )
    })?;
    if snapshot.active_instance_id.as_ref() != Some(instance_id) {
        return Err(control_failure(
            ControlErrorCode::Rejected,
            format!("instance {instance_id} is not active"),
            Some(snapshot.revision),
        ));
    }
    Ok(instance)
}

fn send_audio(
    context: &ControlContext,
    command: AudioControlCommand,
) -> Result<(), ControlFailure> {
    match context.audio_sender.try_send(command) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(_)) => Err(control_failure(
            ControlErrorCode::Unavailable,
            "LIVE audio command queue is full",
            current_revision(context),
        )),
        Err(TrySendError::Disconnected(_)) => Err(control_failure(
            ControlErrorCode::Unavailable,
            "LIVE audio engine is unavailable",
            current_revision(context),
        )),
    }
}

fn receive_audio<T>(
    receiver: Receiver<Result<T, String>>,
    action: &str,
) -> Result<T, ControlFailure> {
    match receiver.recv_timeout(AUDIO_COMMAND_TIMEOUT) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(message)) => Err(control_failure(ControlErrorCode::Rejected, message, None)),
        Err(_) => Err(control_failure(
            ControlErrorCode::Timeout,
            format!("LIVE did not {action} in time"),
            None,
        )),
    }
}

fn record_command_event(
    context: &ControlContext,
    command: CommandRef,
    event: SessionEvent,
) -> ControlResponse {
    match context.store.lock() {
        Ok(mut store) => match store.record(Some(command.clone()), event) {
            Ok(event) => ControlResponse::CommandApplied {
                client_id: command.client_id,
                command_id: command.command_id,
                revision: event.revision,
                events: vec![event],
            },
            Err(error) => internal_error(error.to_string(), Some(store.state().revision)),
        },
        Err(_) => internal_error("session store lock is poisoned", None),
    }
}

fn audition_watchdog(context: Arc<ControlContext>) {
    loop {
        thread::sleep(AUDITION_WATCHDOG_PERIOD);
        let expired_lease = match context.lease_deadline.lock() {
            Ok(deadline) => deadline
                .as_ref()
                .filter(|deadline| Instant::now() >= deadline.deadline)
                .map(|deadline| deadline.lease_id),
            Err(_) => {
                eprintln!("AUDITION_WATCHDOG_ERROR lease deadline lock is poisoned");
                return;
            }
        };
        let Some(lease_id) = expired_lease else {
            continue;
        };
        let _dispatch_guard = match context.dispatch_lock.lock() {
            Ok(guard) => guard,
            Err(_) => {
                eprintln!("AUDITION_WATCHDOG_ERROR dispatch lock is poisoned");
                return;
            }
        };
        if !lease_is_expired(&context, lease_id) {
            continue;
        }
        let snapshot = match context.store.lock() {
            Ok(store) => store.snapshot(),
            Err(_) => {
                eprintln!("AUDITION_WATCHDOG_ERROR session store lock is poisoned");
                return;
            }
        };
        let Some(audition) = snapshot
            .audition
            .as_ref()
            .filter(|audition| audition.lease_id == lease_id)
        else {
            set_lease_deadline(&context, None);
            continue;
        };
        let (reply_sender, reply_receiver) = sync_channel(1);
        if send_audio(
            &context,
            AudioControlCommand::EndAudition {
                lease_id,
                reply: reply_sender,
            },
        )
        .is_err()
        {
            continue;
        }
        if receive_audio(reply_receiver, "expire audition focus").is_err() {
            continue;
        }
        set_lease_deadline(&context, None);
        match context.store.lock() {
            Ok(mut store) => match store.record(
                None,
                SessionEvent::AuditionEnded {
                    lease_id,
                    instance_id: audition.instance_id.clone(),
                    restored_sound_id: audition.previous_sound_id.clone(),
                    reason: AuditionEndReason::Expired,
                },
            ) {
                Ok(event) => println!(
                    "AUDITION_EXPIRED lease={lease_id} revision={}",
                    event.revision.get()
                ),
                Err(error) => eprintln!("AUDITION_WATCHDOG_ERROR {error:#}"),
            },
            Err(_) => {
                eprintln!("AUDITION_WATCHDOG_ERROR session store lock is poisoned");
                return;
            }
        }
    }
}

fn lease_is_expired(context: &ControlContext, lease_id: u64) -> bool {
    context.lease_deadline.lock().is_ok_and(|deadline| {
        deadline.as_ref().is_some_and(|deadline| {
            deadline.lease_id == lease_id && Instant::now() >= deadline.deadline
        })
    })
}

fn set_lease_deadline(context: &ControlContext, lease_id: Option<u64>) {
    match context.lease_deadline.lock() {
        Ok(mut deadline) => {
            *deadline = lease_id.map(|lease_id| LeaseDeadline {
                lease_id,
                deadline: Instant::now() + AUDITION_LEASE_TIMEOUT,
            });
        }
        Err(_) => eprintln!("AUDITION_WATCHDOG_ERROR lease deadline lock is poisoned"),
    }
}

fn current_revision(context: &ControlContext) -> Option<Revision> {
    context
        .store
        .lock()
        .ok()
        .map(|store| store.state().revision)
}

fn error_response(
    code: ControlErrorCode,
    message: impl Into<String>,
    current_revision: Option<Revision>,
) -> ControlResponse {
    ControlResponse::Error {
        code,
        message: message.into(),
        current_revision,
    }
}

fn control_failure(
    code: ControlErrorCode,
    message: impl Into<String>,
    current_revision: Option<Revision>,
) -> ControlFailure {
    ControlFailure {
        code,
        message: message.into(),
        current_revision,
    }
}

fn internal_error(
    message: impl Into<String>,
    current_revision: Option<Revision>,
) -> ControlResponse {
    error_response(ControlErrorCode::Internal, message, current_revision)
}

fn write_response(stream: &mut UnixStream, response: &ControlResponse) -> Result<()> {
    let bytes = encode_line(response).context("serializing control response")?;
    stream
        .write_all(&bytes)
        .context("writing control response")?;
    stream.flush().context("flushing control response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionStore;
    use rackforge_session_api::{
        AddonInstanceState, ClientId, DEFAULT_LIVE_INSTANCE_ID, DEFAULT_LIVE_SESSION_ID, SessionId,
        SessionState, SoundSummary,
    };
    use std::sync::mpsc::sync_channel;

    fn context() -> (Arc<ControlContext>, Receiver<AudioControlCommand>) {
        let instance_id = InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).unwrap();
        let state = SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            session_id: SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            revision: Revision::ZERO,
            active_instance_id: Some(instance_id.clone()),
            instances: vec![AddonInstanceState {
                instance_id,
                addon_id: "org.rackforge.rf-dls".into(),
                addon_name: "RF-DLS".into(),
                ui_layouts: vec!["little@1".into()],
                sounds: vec![SoundSummary {
                    id: "piano".into(),
                    name: "Piano".into(),
                    bank: None,
                    detail: None,
                }],
                selected_sound_id: Some("piano".into()),
            }],
            audition: None,
        };
        let (sender, receiver) = sync_channel(4);
        (
            Arc::new(ControlContext {
                store: SessionStore::shared(state).unwrap(),
                audio_sender: sender,
                dispatch_lock: Mutex::new(()),
                lease_deadline: Mutex::new(None),
            }),
            receiver,
        )
    }

    #[test]
    fn rejects_stale_commands_before_touching_audio() {
        let (context, receiver) = context();
        let instance_id = InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).unwrap();
        let response = dispatch_command(
            &context,
            CommandEnvelope {
                schema_version: SESSION_SCHEMA_VERSION,
                client_id: ClientId::new("test.control").unwrap(),
                command_id: 1,
                expected_revision: Some(Revision::ZERO.next().unwrap()),
                command: SessionCommand::SelectSound {
                    instance_id,
                    sound_id: "piano".into(),
                },
            },
        );
        assert!(matches!(
            response,
            ControlResponse::Error {
                code: ControlErrorCode::Conflict,
                ..
            }
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn applies_audio_command_then_publishes_a_session_event() {
        let (context, receiver) = context();
        let audio = thread::spawn(move || match receiver.recv().unwrap() {
            AudioControlCommand::SelectSound {
                instance_id,
                sound_id,
                reply,
            } => {
                assert_eq!(instance_id.as_str(), DEFAULT_LIVE_INSTANCE_ID);
                assert_eq!(sound_id, "piano");
                reply.send(Ok(())).unwrap();
            }
            _ => panic!("unexpected audio command"),
        });
        let instance_id = InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).unwrap();
        let response = dispatch_command(
            &context,
            CommandEnvelope::new(
                ClientId::new("test.control").unwrap(),
                7,
                SessionCommand::SelectSound {
                    instance_id,
                    sound_id: "piano".into(),
                },
            ),
        );
        let ControlResponse::CommandApplied {
            client_id,
            command_id,
            revision,
            events,
        } = response
        else {
            panic!("command was not applied");
        };
        assert_eq!(client_id.as_str(), "test.control");
        assert_eq!(command_id, 7);
        assert_eq!(revision.get(), 1);
        assert_eq!(events.len(), 1);
        let store = context.store.lock().unwrap();
        assert_eq!(store.snapshot().revision.get(), 1);
        assert_eq!(store.events_after(Revision::ZERO).unwrap(), events);
        audio.join().unwrap();
    }
}
