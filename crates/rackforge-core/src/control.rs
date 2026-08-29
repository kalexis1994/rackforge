use crate::PluginStorage;
use crate::performance::PerformanceRepository;
use crate::rack_graph::{
    CompiledAudioSource, CompiledRackSlot, compile_instrument_definition, compile_instrument_rack,
};
use crate::session::SharedSessionStore;
use crate::session_checkpoint::SessionCheckpointStore;
use crate::{
    CompiledParameterLink, SemanticParameterLinkContext, compile_semantic_parameter_links,
};
use crate::{
    IsolatedPluginStateEditor, LoadedPlugin, PluginInstance, PluginStateStore,
    validate_state_reference,
};
use anyhow::{Context, Result, bail};
use rackforge_audio_api::{AudioOutputDocument, AudioOutputProfile, AudioOutputState, OutputMeter};
use rackforge_control_api::{
    ControlErrorCode, ControlRequest, ControlResponse, MAX_CONTROL_MESSAGE_BYTES,
    MidiLearnCandidate, MidiSourceStatus, PluginParameterValue, VirtualMidiMessage, decode_request,
    encode_line,
};
#[cfg(test)]
use rackforge_midi_api::MidiSourceDescriptor;
use rackforge_midi_api::{
    IngressMidiEvent, MidiMessageKind, MidiPacket, MidiSourceRegistry, ParameterLink,
    ParameterLinkMessage,
};
#[cfg(test)]
use rackforge_performance_api::PerformanceLibrary;
use rackforge_performance_api::{
    LibraryRevision, PERFORMANCE_SNAPSHOT_SCHEMA_VERSION, PerformanceEdit, PerformanceSnapshot,
    RackDefinition, RackId, RackKeyboardParts, RackMidiTransform,
};
use rackforge_plugin_api::abi::MidiEventV1;
use rackforge_plugin_api::{
    Capability, ParameterSchema, PluginManifest, PreparedProgram, PresetCatalog, ProgramDocument,
    ProgramEditRequest, ProgramEditorView, ProgramFieldEditRequest, ResourceKind,
    SurfaceActivationRequest, SurfaceActivationResponse,
};
use rackforge_session_api::{
    AuditionEndReason, ClientId, CommandEnvelope, CommandRef, HostActionBinding,
    HostControlBinding, InstanceId, MasterLevel, MasterPan, ProgramDraftState, Revision,
    SESSION_SCHEMA_VERSION, SemanticControlProfile, SessionCommand, SessionEvent, SoundSummary,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const AUDIO_COMMAND_TIMEOUT: Duration = Duration::from_secs(1);
const AUDIO_RECONFIGURE_TIMEOUT: Duration = Duration::from_secs(8);
const AUDITION_LEASE_TIMEOUT: Duration = Duration::from_secs(15);
const AUDITION_WATCHDOG_PERIOD: Duration = Duration::from_millis(250);
pub const MAX_ACTIVE_RACK_SLOTS: usize = 8;
pub const MAX_EVENTS_PER_BLOCK: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RackSlotStateLoad {
    Default,
    Opaque(Vec<u8>),
    LegacyPreset(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RackMidiStageRuntimeSpec {
    pub transform: RackMidiTransform,
    pub keyboard_parts: Option<RackKeyboardParts>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RackSlotRuntimeSpec {
    pub slot_id: String,
    pub plugin_id: String,
    pub state: RackSlotStateLoad,
    pub midi_stages: Vec<RackMidiStageRuntimeSpec>,
    pub audio_sources: Vec<CompiledAudioSource>,
    pub sends_to_main: bool,
    pub level_per_mille: u16,
    pub pan_per_mille: i16,
}

pub enum AudioControlCommand {
    InjectVirtualMidi {
        events: Vec<IngressMidiEvent>,
    },
    Sequencer {
        command: rackforge_control_api::SequencerCommand,
        reply: SyncSender<Result<(), String>>,
    },
    SequencerStatus {
        reply: SyncSender<Result<rackforge_control_api::SequencerStatusV1, String>>,
    },
    SequencerCaptureTake {
        lane: u8,
        reply: SyncSender<Result<Vec<rackforge_control_api::CapturedNoteV1>, String>>,
    },
    ApplyAudioOutput {
        profile: AudioOutputProfile,
        reply: SyncSender<Result<AudioOutputState, String>>,
    },
    RegisterHostControls {
        controller_id: String,
        bindings: Vec<HostControlBinding>,
        reply: SyncSender<Result<(), String>>,
    },
    RegisterHostBindings {
        controller_id: String,
        controls: Vec<HostControlBinding>,
        actions: Vec<HostActionBinding>,
        reply: SyncSender<Result<(), String>>,
    },
    SetMasterLevel {
        level: MasterLevel,
        reply: SyncSender<Result<(), String>>,
    },
    SetMasterPan {
        pan: MasterPan,
        reply: SyncSender<Result<(), String>>,
    },
    SetRenderMode {
        mode: rackforge_session_api::SurfaceMode,
        reply: SyncSender<Result<(), String>>,
    },
    EmergencyStop {
        reply: SyncSender<Result<(), String>>,
    },
    SelectPlugin {
        instance_id: InstanceId,
        reply: SyncSender<Result<(), String>>,
    },
    SelectSound {
        instance_id: InstanceId,
        sound_id: String,
        reply: SyncSender<Result<(), String>>,
    },
    ActivateRack {
        rack_id: String,
        instance_id: InstanceId,
        slots: Vec<RackSlotRuntimeSpec>,
        prepared_slots: Option<Vec<PreparedRackSlot>>,
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
    BeginProgramEdit {
        instance_id: InstanceId,
        request: ProgramEditRequest,
        previous_sound_id: Option<String>,
        reply: SyncSender<Result<(u64, PreparedProgram, ProgramEditorView), String>>,
    },
    ReplaceProgramDraft {
        instance_id: InstanceId,
        document: ProgramDocument,
        reply: SyncSender<Result<(PreparedProgram, ProgramEditorView), String>>,
    },
    EditProgramDraftField {
        instance_id: InstanceId,
        request: ProgramFieldEditRequest,
        reply: SyncSender<Result<(PreparedProgram, ProgramEditorView), String>>,
    },
    InstallProgram {
        instance_id: InstanceId,
        prepared: PreparedProgram,
        reply: SyncSender<Result<PresetCatalog, String>>,
    },
    ActivateSurface {
        instance_id: InstanceId,
        request: SurfaceActivationRequest,
        reply: SyncSender<Result<SurfaceActivationResponse, String>>,
    },
    CaptureState {
        instance_id: InstanceId,
        reply: SyncSender<Result<Vec<u8>, String>>,
    },
    RestoreState {
        instance_id: InstanceId,
        bytes: Vec<u8>,
        reply: SyncSender<Result<(), String>>,
    },
    MaterializeState {
        instance_id: InstanceId,
        program_id: Option<String>,
        reply: SyncSender<Result<Vec<u8>, String>>,
    },
    PluginParameters {
        instance_id: InstanceId,
        reply: SyncSender<Result<(ParameterSchema, Vec<PluginParameterValue>), String>>,
    },
    SetPluginParameter {
        instance_id: InstanceId,
        parameter_index: u32,
        value: f64,
        reply: SyncSender<Result<f64, String>>,
    },
    ReplaceParameterLinks {
        links: Vec<CompiledParameterLink>,
        reply: SyncSender<Result<(), String>>,
    },
    ReplaceStandaloneVoice {
        instance_id: InstanceId,
        instance: PreparedPluginInstance,
        reply: SyncSender<Result<(), String>>,
    },
}

pub struct PreparedPluginInstance(pub PluginInstance<'static>);

// SAFETY: this wrapper is constructed only for a portable wasm-v1 instance.
// Ownership moves from the control worker to the single audio loop.
unsafe impl Send for PreparedPluginInstance {}

pub struct PreparedParallelUnits(pub crate::parallel_render::ParallelUnits<'static>);

// SAFETY: `ParallelUnits::create` admits only portable wasm-v1 instances.
// Ownership moves once from the control worker to the audio loop, which
// hands each unit to at most one pool worker per block.
unsafe impl Send for PreparedParallelUnits {}

pub struct PreparedRackSlot {
    pub slot_id: String,
    pub midi_stages: Vec<RackMidiStageRuntimeSpec>,
    pub audio_sources: Vec<CompiledAudioSource>,
    pub sends_to_main: bool,
    pub level_per_mille: u16,
    pub pan_per_mille: i16,
    pub plugin: &'static LoadedPlugin,
    pub instance: PreparedPluginInstance,
    pub parallel: Option<PreparedParallelUnits>,
    pub input_channels: usize,
    pub input: Vec<f32>,
    pub output: Vec<f32>,
    pub events: Vec<MidiEventV1>,
    pub parameter_events: Vec<rackforge_plugin_api::abi::ParameterEventV1>,
}

// SAFETY: `PreparedRackSlot` is produced only from `PortableControlPlugin`,
// which admits RackForge's immutable wasm-v1 backend. The instance has not
// been published anywhere else and ownership moves once to the audio loop.
unsafe impl Send for PreparedRackSlot {}

#[derive(Clone, Copy)]
pub struct PortableControlPlugin(&'static LoadedPlugin);

impl PortableControlPlugin {
    pub fn new(plugin: &'static LoadedPlugin) -> Option<Self> {
        plugin
            .manifest()
            .portable_component()
            .is_some()
            .then_some(Self(plugin))
    }
}

// SAFETY: PortableControlPlugin can contain only RackForge's immutable
// wasm-v1 backend. Native ABI host pointers are rejected by the constructor.
unsafe impl Send for PortableControlPlugin {}
unsafe impl Sync for PortableControlPlugin {}

struct LeaseDeadline {
    lease_id: u64,
    deadline: Instant,
}

struct MidiLearnState {
    id: u64,
    candidate: Option<MidiLearnCandidate>,
}

#[derive(Clone, Default)]
struct VirtualMidiClientState {
    notes: BTreeSet<(u8, u8)>,
    channels: BTreeSet<u8>,
    source: Option<rackforge_midi_api::MidiSourceKey>,
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

#[derive(Clone)]
struct RegisteredSemanticProfile {
    profile: SemanticControlProfile,
    runtime_source_id: Option<rackforge_midi_api::MidiSourceId>,
    runtime_source_name: Option<String>,
}

struct ControlContext {
    store: SharedSessionStore,
    audio_sender: SyncSender<AudioControlCommand>,
    audio_state: Arc<Mutex<AudioOutputState>>,
    output_meter: Arc<OutputMeter>,
    audio_state_path: PathBuf,
    performance_repository: Arc<Mutex<PerformanceRepository>>,
    state_store: Arc<Mutex<PluginStateStore>>,
    plugin_manifests: BTreeMap<String, PluginManifest>,
    portable_plugins: BTreeMap<String, PortableControlPlugin>,
    semantic_profiles: Mutex<BTreeMap<String, RegisteredSemanticProfile>>,
    midi_sources: MidiSourceRegistry,
    connected_midi_sources: Arc<Mutex<BTreeSet<u32>>>,
    midi_observer: Mutex<Receiver<IngressMidiEvent>>,
    midi_learn: Mutex<Option<MidiLearnState>>,
    dynamic_resources: Mutex<BTreeMap<InstanceId, BTreeMap<String, PathBuf>>>,
    virtual_midi: Mutex<BTreeMap<ClientId, VirtualMidiClientState>>,
    plugin_sample_rate: f64,
    plugin_maximum_frames: u32,
    plugin_output_channels: u32,
    storage: Option<PluginStorage>,
    checkpoint: Option<SessionCheckpointStore>,
    dispatch_lock: Mutex<()>,
    lease_deadline: Mutex<Option<LeaseDeadline>>,
    next_draft_id: AtomicU64,
    next_midi_learn_id: AtomicU64,
}

pub struct ControlServer {
    _server_thread: JoinHandle<()>,
    _watchdog_thread: JoinHandle<()>,
}

pub struct ControlServerOptions {
    pub store: SharedSessionStore,
    pub audio_sender: SyncSender<AudioControlCommand>,
    pub audio_state: Arc<Mutex<AudioOutputState>>,
    pub output_meter: Arc<OutputMeter>,
    pub audio_state_path: PathBuf,
    pub performance_repository: Arc<Mutex<PerformanceRepository>>,
    pub state_store: Arc<Mutex<PluginStateStore>>,
    pub plugin_manifests: BTreeMap<String, PluginManifest>,
    pub portable_plugins: BTreeMap<String, PortableControlPlugin>,
    pub midi_sources: MidiSourceRegistry,
    pub midi_observer: Receiver<IngressMidiEvent>,
    pub connected_midi_sources: Arc<Mutex<BTreeSet<u32>>>,
    pub plugin_sample_rate: f64,
    pub plugin_maximum_frames: u32,
    pub plugin_output_channels: u32,
    pub storage: Option<PluginStorage>,
    pub checkpoint: Option<SessionCheckpointStore>,
}

pub fn start(socket_path: &Path, options: ControlServerOptions) -> Result<ControlServer> {
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
        store: options.store,
        audio_sender: options.audio_sender,
        audio_state: options.audio_state,
        output_meter: options.output_meter,
        audio_state_path: options.audio_state_path,
        performance_repository: options.performance_repository,
        state_store: options.state_store,
        plugin_manifests: options.plugin_manifests,
        portable_plugins: options.portable_plugins,
        semantic_profiles: Mutex::new(BTreeMap::new()),
        midi_sources: options.midi_sources,
        connected_midi_sources: options.connected_midi_sources,
        midi_observer: Mutex::new(options.midi_observer),
        midi_learn: Mutex::new(None),
        dynamic_resources: Mutex::new(BTreeMap::new()),
        virtual_midi: Mutex::new(BTreeMap::new()),
        plugin_sample_rate: options.plugin_sample_rate,
        plugin_maximum_frames: options.plugin_maximum_frames,
        plugin_output_channels: options.plugin_output_channels,
        storage: options.storage,
        checkpoint: options.checkpoint,
        dispatch_lock: Mutex::new(()),
        lease_deadline: Mutex::new(None),
        next_draft_id: AtomicU64::new(1),
        next_midi_learn_id: AtomicU64::new(1),
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
                snapshot: Box::new(store.snapshot()),
            },
            Err(_) => internal_error("session store lock is poisoned", None),
        },
        ControlRequest::AudioSnapshot => match context.audio_state.lock() {
            Ok(snapshot) => ControlResponse::AudioSnapshot {
                snapshot: Box::new(snapshot.clone()),
            },
            Err(_) => internal_error("audio state lock is poisoned", current_revision(context)),
        },
        ControlRequest::OutputMeter => output_meter_response(context),
        ControlRequest::PerformanceSnapshot => {
            match (context.store.lock(), context.performance_repository.lock()) {
                (Ok(store), Ok(repository)) => {
                    let snapshot = PerformanceSnapshot {
                        schema_version: PERFORMANCE_SNAPSHOT_SCHEMA_VERSION,
                        revision: repository.revision(),
                        library: repository.library().clone(),
                        live: store.state().live.clone(),
                    };
                    match snapshot.validate() {
                        Ok(()) => ControlResponse::PerformanceSnapshot {
                            snapshot: Box::new(snapshot),
                        },
                        Err(error) => internal_error(
                            format!("invalid performance snapshot: {error}"),
                            Some(store.state().revision),
                        ),
                    }
                }
                (Err(_), _) => internal_error("session store lock is poisoned", None),
                (_, Err(_)) => internal_error("performance repository lock is poisoned", None),
            }
        }
        ControlRequest::PluginPresets { plugin_id } => list_plugin_presets(context, &plugin_id),
        ControlRequest::SavePluginPreset { instance_id, name } => {
            save_plugin_preset(context, &instance_id, &name)
        }
        ControlRequest::LoadPluginPreset {
            instance_id,
            preset_id,
        } => load_plugin_preset(context, &instance_id, &preset_id),
        ControlRequest::RenamePluginPreset {
            plugin_id,
            preset_id,
            name,
        } => rename_plugin_preset(context, &plugin_id, &preset_id, &name),
        ControlRequest::DeletePluginPreset {
            plugin_id,
            preset_id,
        } => delete_plugin_preset(context, &plugin_id, &preset_id),
        ControlRequest::PluginPreset {
            plugin_id,
            preset_id,
        } => get_plugin_preset(context, &plugin_id, &preset_id),
        ControlRequest::ExportPluginPreset {
            plugin_id,
            preset_id,
        } => export_plugin_preset(context, &plugin_id, &preset_id),
        ControlRequest::InspectPluginPreset {
            target_plugin_id,
            file,
        } => inspect_plugin_preset(context, &target_plugin_id, &file),
        ControlRequest::ImportPluginPreset {
            target_plugin_id,
            file,
            conflict_policy,
        } => import_plugin_preset(context, &target_plugin_id, &file, conflict_policy),
        ControlRequest::ExportLiveShow { name } => export_live_show(context, &name),
        ControlRequest::InspectLiveShow { file } => inspect_live_show_request(context, &file),
        ControlRequest::ImportLiveShow { file } => import_live_show(context, &file),
        ControlRequest::MaterializePluginState {
            plugin_id,
            sound_id,
        } => materialize_plugin_state(context, &plugin_id, sound_id),
        ControlRequest::PluginParameters { instance_id } => {
            plugin_parameters(context, &instance_id)
        }
        ControlRequest::SetPluginParameter {
            instance_id,
            parameter_index,
            value,
        } => set_plugin_parameter(context, &instance_id, parameter_index, value),
        ControlRequest::PluginStateParameters { state } => plugin_state_parameters(context, &state),
        ControlRequest::SetPluginStateParameter {
            state,
            parameter_index,
            value,
        } => set_plugin_state_parameter(context, &state, parameter_index, value),
        ControlRequest::MidiSources => {
            let connected = context.connected_midi_sources.lock().ok();
            let sources: Vec<MidiSourceStatus> = context
                .midi_sources
                .descriptors()
                .filter(|source| source.id.as_str() != "rackforge.virtual.touch")
                .cloned()
                .map(|source| {
                    let key = context.midi_sources.resolve_optional(&source.id);
                    MidiSourceStatus {
                        source,
                        connected: key.is_some_and(|key| {
                            connected
                                .as_ref()
                                .is_some_and(|present| present.contains(&key.get()))
                        }),
                    }
                })
                .collect();
            ControlResponse::MidiSources { sources }
        }
        ControlRequest::BeginMidiLearn {
            instance_id,
            parameter_index,
        } => begin_midi_learn(context, &instance_id, parameter_index),
        ControlRequest::MidiLearnStatus { learn_id } => midi_learn_status(context, learn_id),
        ControlRequest::CancelMidiLearn { learn_id } => cancel_midi_learn(context, learn_id),
        ControlRequest::LoadPluginResource {
            plugin_id,
            instance_id,
            resource_id,
            path,
            persist,
            preview,
        } => load_plugin_resource(
            context,
            &plugin_id,
            &instance_id,
            &resource_id,
            &path,
            persist,
            preview,
        ),
        ControlRequest::ClearPluginResource {
            plugin_id,
            instance_id,
            resource_id,
        } => clear_plugin_resource(context, &plugin_id, &instance_id, &resource_id),
        ControlRequest::EditPerformance {
            expected_revision,
            edit,
        } => edit_performance(context, expected_revision, edit),
        ControlRequest::ApplyAudioOutput { profile } => apply_audio_output(context, profile),
        ControlRequest::VirtualMidi {
            client_id,
            source_name,
            message,
        } => accept_virtual_midi(context, client_id, source_name, message),
        ControlRequest::ReleaseVirtualMidi { client_id } => {
            release_virtual_midi(context, client_id)
        }
        ControlRequest::Sequencer { command } => sequencer_command(context, command),
        ControlRequest::SequencerStatus => sequencer_status(context),
        ControlRequest::SequencerCaptureTake { lane } => sequencer_capture_take(context, lane),
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

fn sequencer_command(
    context: &ControlContext,
    command: rackforge_control_api::SequencerCommand,
) -> ControlResponse {
    let (reply_sender, reply_receiver) = sync_channel(1);
    if let Err(failure) = send_audio(
        context,
        AudioControlCommand::Sequencer {
            command,
            reply: reply_sender,
        },
    ) {
        return failure.into_response();
    }
    match receive_audio(reply_receiver, "apply sequencer command") {
        Ok(()) => ControlResponse::SequencerAccepted,
        Err(failure) => failure.into_response(),
    }
}

fn sequencer_status(context: &ControlContext) -> ControlResponse {
    let (reply_sender, reply_receiver) = sync_channel(1);
    if let Err(failure) = send_audio(
        context,
        AudioControlCommand::SequencerStatus {
            reply: reply_sender,
        },
    ) {
        return failure.into_response();
    }
    match receive_audio(reply_receiver, "read sequencer status") {
        Ok(sequencer) => ControlResponse::SequencerStatus { sequencer },
        Err(failure) => failure.into_response(),
    }
}

fn sequencer_capture_take(context: &ControlContext, lane: u8) -> ControlResponse {
    let (reply_sender, reply_receiver) = sync_channel(1);
    if let Err(failure) = send_audio(
        context,
        AudioControlCommand::SequencerCaptureTake {
            lane,
            reply: reply_sender,
        },
    ) {
        return failure.into_response();
    }
    match receive_audio(reply_receiver, "drain the capture take") {
        Ok(notes) => ControlResponse::SequencerCapture { notes },
        Err(failure) => failure.into_response(),
    }
}

fn accept_virtual_midi(
    context: &ControlContext,
    client_id: ClientId,
    source_name: Option<String>,
    message: VirtualMidiMessage,
) -> ControlResponse {
    if let Err(message) = message.validate() {
        return error_response(
            ControlErrorCode::InvalidRequest,
            message,
            current_revision(context),
        );
    }
    let packet = match MidiPacket::new(0, &message.bytes()) {
        Ok(packet) => packet,
        Err(error) => {
            return error_response(
                ControlErrorCode::InvalidRequest,
                error.to_string(),
                current_revision(context),
            );
        }
    };
    let source = match source_name.as_deref() {
        Some(name) => match context.midi_sources.resolve_name(name) {
            Some((source, _)) => source,
            None => {
                return error_response(
                    ControlErrorCode::Unavailable,
                    format!("MIDI input {name:?} is disabled or unavailable in RackForge settings"),
                    current_revision(context),
                );
            }
        },
        None => {
            let id = rackforge_midi_api::MidiSourceId::new(crate::live::VIRTUAL_MIDI_SOURCE_ID)
                .expect("the touch MIDI source id is valid");
            context
                .midi_sources
                .resolve_optional(&id)
                .unwrap_or(rackforge_midi_api::MidiSourceKey::new(u32::MAX))
        }
    };
    let ingress = IngressMidiEvent { source, packet };
    if source_name.is_some() {
        observe_forwarded_midi_learn(context, ingress);
    }
    if let Err(failure) = send_audio(
        context,
        AudioControlCommand::InjectVirtualMidi {
            events: vec![ingress],
        },
    ) {
        return failure.into_response();
    }
    let active_notes = match context.virtual_midi.lock() {
        Ok(mut clients) => {
            let state = clients.entry(client_id.clone()).or_default();
            state.source = Some(source);
            let channel = message.channel();
            state.channels.insert(channel);
            if let Some(note) = message.note_on() {
                state.notes.insert((channel, note));
            } else if let Some(note) = message.note_off() {
                state.notes.remove(&(channel, note));
            }
            u16::try_from(state.notes.len()).unwrap_or(u16::MAX)
        }
        Err(_) => return internal_error("virtual MIDI state lock is poisoned", None),
    };
    ControlResponse::VirtualMidiAccepted {
        client_id,
        active_notes,
    }
}

fn release_virtual_midi(context: &ControlContext, client_id: ClientId) -> ControlResponse {
    let state = match context.virtual_midi.lock() {
        Ok(mut clients) => clients.remove(&client_id).unwrap_or_default(),
        Err(_) => return internal_error("virtual MIDI state lock is poisoned", None),
    };
    let source = state.source.unwrap_or_else(|| {
        let id = rackforge_midi_api::MidiSourceId::new(crate::live::VIRTUAL_MIDI_SOURCE_ID)
            .expect("the touch MIDI source id is valid");
        context
            .midi_sources
            .resolve_optional(&id)
            .unwrap_or(rackforge_midi_api::MidiSourceKey::new(u32::MAX))
    });
    let mut events = Vec::with_capacity(state.notes.len() + state.channels.len() * 3);
    for (channel, note) in &state.notes {
        if let Ok(packet) = MidiPacket::new(0, &[0x80 | channel, *note, 0]) {
            events.push(IngressMidiEvent { source, packet });
        }
    }
    let channels = if state.channels.is_empty() {
        vec![0]
    } else {
        state.channels.iter().copied().collect()
    };
    for channel in channels {
        for controller in [64, 123, 120] {
            if let Ok(packet) = MidiPacket::new(0, &[0xb0 | channel, controller, 0]) {
                events.push(IngressMidiEvent { source, packet });
            }
        }
    }
    if !events.is_empty()
        && let Err(failure) = send_audio(context, AudioControlCommand::InjectVirtualMidi { events })
    {
        if let Ok(mut clients) = context.virtual_midi.lock() {
            clients.insert(client_id.clone(), state);
        }
        return failure.into_response();
    }
    ControlResponse::VirtualMidiReleased { client_id }
}

fn load_plugin_resource(
    context: &ControlContext,
    plugin_id: &str,
    instance_id: &InstanceId,
    resource_id: &str,
    path: &Path,
    persist: bool,
    preview: bool,
) -> ControlResponse {
    if persist && preview {
        return error_response(
            ControlErrorCode::InvalidRequest,
            "a resource preview cannot be persisted",
            None,
        );
    }
    let snapshot = match context.store.lock() {
        Ok(store) => store.snapshot(),
        Err(_) => return internal_error("session store lock is poisoned", None),
    };
    let Some(instance_state) = snapshot.instance(instance_id) else {
        return error_response(
            ControlErrorCode::NotFound,
            format!("plugin instance {instance_id} was not found"),
            Some(snapshot.revision),
        );
    };
    if instance_state.plugin_id != plugin_id {
        return error_response(
            ControlErrorCode::InvalidRequest,
            "plugin instance does not belong to the resource owner",
            Some(snapshot.revision),
        );
    }
    let Some(manifest) = context.plugin_manifests.get(&instance_state.plugin_id) else {
        return internal_error(
            "active plugin manifest is unavailable",
            Some(snapshot.revision),
        );
    };
    let Some(requirement) = manifest
        .resources
        .iter()
        .find(|resource| resource.id == resource_id && resource.kind == ResourceKind::File)
    else {
        return error_response(
            ControlErrorCode::InvalidRequest,
            format!("plugin does not declare file resource {resource_id:?}"),
            Some(snapshot.revision),
        );
    };
    let import_targets = requirement.import_targets.clone();
    if !import_targets.is_empty() && !persist {
        return error_response(
            ControlErrorCode::InvalidRequest,
            "resource archives must be installed into private plugin storage",
            Some(snapshot.revision),
        );
    }
    let install_path = if persist && import_targets.is_empty() {
        let Some(relative) = requirement.data_path.as_deref() else {
            return error_response(
                ControlErrorCode::InvalidRequest,
                format!("resource {resource_id:?} has no private data_path"),
                Some(snapshot.revision),
            );
        };
        Some(PathBuf::from(relative))
    } else {
        None
    };
    let Some(runtime) = context
        .portable_plugins
        .get(&instance_state.plugin_id)
        .copied()
    else {
        return error_response(
            ControlErrorCode::Unavailable,
            "dynamic resources currently require a portable wasm-v1 plugin",
            Some(snapshot.revision),
        );
    };
    let path = match fs::canonicalize(path) {
        Ok(path) if path.is_file() => path,
        Ok(_) => {
            return error_response(
                ControlErrorCode::InvalidRequest,
                "selected resource is not a file",
                Some(snapshot.revision),
            );
        }
        Err(error) => {
            return error_response(
                ControlErrorCode::InvalidRequest,
                format!("selected resource is unavailable: {error}"),
                Some(snapshot.revision),
            );
        }
    };
    let mut resources = match context.dynamic_resources.lock() {
        Ok(resources) => resources.get(instance_id).cloned().unwrap_or_default(),
        Err(_) => {
            return internal_error(
                "dynamic resource registry is poisoned",
                Some(snapshot.revision),
            );
        }
    };
    if !import_targets.is_empty() {
        let Some(storage) = context.storage.as_ref() else {
            return error_response(
                ControlErrorCode::Unavailable,
                "private plugin storage is unavailable",
                Some(snapshot.revision),
            );
        };
        let imported = match runtime.0.import_resource_archive(resource_id, &path) {
            Ok(imported) => imported,
            Err(error) => {
                return error_response(
                    ControlErrorCode::Rejected,
                    format!("plugin rejected resource archive {resource_id:?}: {error:#}"),
                    Some(snapshot.revision),
                );
            }
        };
        for (target_id, bytes) in imported {
            let Some(relative) = manifest
                .resources
                .iter()
                .find(|resource| resource.id == target_id)
                .and_then(|resource| resource.data_path.as_deref())
            else {
                return internal_error(
                    format!("imported resource {target_id:?} has no private data_path"),
                    Some(snapshot.revision),
                );
            };
            match storage.write_atomic(plugin_id, Path::new(relative), &bytes) {
                Ok(installed) => {
                    resources.insert(target_id, installed);
                }
                Err(error) => {
                    return internal_error(
                        format!("could not install imported resource {target_id:?}: {error:#}"),
                        Some(snapshot.revision),
                    );
                }
            }
        }
    } else {
        if let Err(error) = runtime.0.validate_resource_file(resource_id, &path) {
            return error_response(
                ControlErrorCode::Rejected,
                format!("plugin rejected resource {resource_id:?}: {error:#}"),
                Some(snapshot.revision),
            );
        }
        resources.insert(resource_id.to_owned(), path.to_path_buf());
        if let Some(relative) = install_path.as_deref() {
            let Some(storage) = context.storage.as_ref() else {
                return error_response(
                    ControlErrorCode::Unavailable,
                    "private plugin storage is unavailable",
                    Some(snapshot.revision),
                );
            };
            match storage.copy_file_atomic(plugin_id, relative, &path) {
                Ok(installed) => {
                    resources.insert(resource_id.to_owned(), installed);
                }
                Err(error) => {
                    return internal_error(
                        format!("could not install plugin resource: {error:#}"),
                        Some(snapshot.revision),
                    );
                }
            }
        }
    }
    if !preview {
        if let Ok(mut registry) = context.dynamic_resources.lock() {
            registry.insert(instance_id.clone(), resources.clone());
        } else {
            return internal_error(
                "dynamic resource registry is poisoned",
                Some(snapshot.revision),
            );
        }
    }
    let mut replacement = match runtime
        .0
        .create_instance_with_resource_overrides(&resources)
    {
        Ok(instance) => instance,
        Err(_error) if persist => {
            return ControlResponse::PluginResourceLoaded {
                instance_id: instance_id.clone(),
                resource_id: resource_id.to_owned(),
            };
        }
        Err(error) => return internal_error(error.to_string(), Some(snapshot.revision)),
    };
    if !preview
        && let Some(sound_id) = instance_state.selected_sound_id.as_deref()
        && let Err(error) = replacement.load_preset(sound_id)
    {
        if persist {
            return ControlResponse::PluginResourceLoaded {
                instance_id: instance_id.clone(),
                resource_id: resource_id.to_owned(),
            };
        }
        return error_response(
            ControlErrorCode::Rejected,
            format!("plugin could not restore sound {sound_id:?}: {error:#}"),
            Some(snapshot.revision),
        );
    }
    let input_channels = context
        .plugin_manifests
        .get(plugin_id)
        .map(|manifest| manifest.resolved_audio_contract().input_channels())
        .unwrap_or(0);
    if let Err(error) = replacement.activate(
        context.plugin_sample_rate,
        context.plugin_maximum_frames,
        input_channels,
        context.plugin_output_channels,
    ) {
        if persist {
            return ControlResponse::PluginResourceLoaded {
                instance_id: instance_id.clone(),
                resource_id: resource_id.to_owned(),
            };
        }
        return error_response(
            ControlErrorCode::Rejected,
            format!("plugin could not activate the resource: {error:#}"),
            Some(snapshot.revision),
        );
    }
    let (reply_sender, reply_receiver) = sync_channel(1);
    if let Err(failure) = send_audio(
        context,
        AudioControlCommand::ReplaceStandaloneVoice {
            instance_id: instance_id.clone(),
            instance: PreparedPluginInstance(replacement),
            reply: reply_sender,
        },
    ) {
        return failure.into_response();
    }
    match receive_audio(reply_receiver, "replace plugin resource") {
        Ok(()) => ControlResponse::PluginResourceLoaded {
            instance_id: instance_id.clone(),
            resource_id: resource_id.to_owned(),
        },
        Err(failure) => failure.into_response(),
    }
}

/// Removes one installed private resource and swaps in an instance built from
/// the remaining set, so the package default is playing again.
fn clear_plugin_resource(
    context: &ControlContext,
    plugin_id: &str,
    instance_id: &InstanceId,
    resource_id: &str,
) -> ControlResponse {
    let snapshot = match context.store.lock() {
        Ok(store) => store.snapshot(),
        Err(_) => return internal_error("session store lock is poisoned", None),
    };
    let Some(instance_state) = snapshot.instance(instance_id) else {
        return error_response(
            ControlErrorCode::NotFound,
            format!("plugin instance {instance_id} was not found"),
            Some(snapshot.revision),
        );
    };
    if instance_state.plugin_id != plugin_id {
        return error_response(
            ControlErrorCode::InvalidRequest,
            "plugin instance does not belong to the resource owner",
            Some(snapshot.revision),
        );
    }
    let Some(manifest) = context.plugin_manifests.get(&instance_state.plugin_id) else {
        return internal_error(
            "active plugin manifest is unavailable",
            Some(snapshot.revision),
        );
    };
    let Some(requirement) = manifest
        .resources
        .iter()
        .find(|resource| resource.id == resource_id && resource.kind == ResourceKind::File)
    else {
        return error_response(
            ControlErrorCode::InvalidRequest,
            format!("plugin does not declare file resource {resource_id:?}"),
            Some(snapshot.revision),
        );
    };
    if requirement.required {
        return error_response(
            ControlErrorCode::InvalidRequest,
            "a required resource cannot be cleared",
            Some(snapshot.revision),
        );
    }
    let Some(relative) = requirement.data_path.as_deref() else {
        return error_response(
            ControlErrorCode::InvalidRequest,
            format!("resource {resource_id:?} has no private data_path"),
            Some(snapshot.revision),
        );
    };
    let Some(runtime) = context
        .portable_plugins
        .get(&instance_state.plugin_id)
        .copied()
    else {
        return error_response(
            ControlErrorCode::Unavailable,
            "dynamic resources currently require a portable wasm-v1 plugin",
            Some(snapshot.revision),
        );
    };
    let Some(storage) = context.storage.as_ref() else {
        return error_response(
            ControlErrorCode::Unavailable,
            "private plugin storage is unavailable",
            Some(snapshot.revision),
        );
    };
    if let Err(error) = storage.remove_file(plugin_id, Path::new(relative)) {
        return internal_error(
            format!("could not clear plugin resource: {error:#}"),
            Some(snapshot.revision),
        );
    }
    let resources = match context.dynamic_resources.lock() {
        Ok(mut registry) => {
            let mut resources = registry.get(instance_id).cloned().unwrap_or_default();
            resources.remove(resource_id);
            registry.insert(instance_id.clone(), resources.clone());
            resources
        }
        Err(_) => {
            return internal_error(
                "dynamic resource registry is poisoned",
                Some(snapshot.revision),
            );
        }
    };
    let mut replacement = match runtime
        .0
        .create_instance_with_resource_overrides(&resources)
    {
        Ok(instance) => instance,
        Err(error) => return internal_error(error.to_string(), Some(snapshot.revision)),
    };
    // The stored selection may not exist in the restored bank. The cleared
    // state must win either way, so a failed restore falls back to defaults.
    if let Some(sound_id) = instance_state.selected_sound_id.as_deref() {
        let _ = replacement.load_preset(sound_id);
    }
    let input_channels = context
        .plugin_manifests
        .get(plugin_id)
        .map(|manifest| manifest.resolved_audio_contract().input_channels())
        .unwrap_or(0);
    if let Err(error) = replacement.activate(
        context.plugin_sample_rate,
        context.plugin_maximum_frames,
        input_channels,
        context.plugin_output_channels,
    ) {
        return error_response(
            ControlErrorCode::Rejected,
            format!("plugin could not activate after clearing the resource: {error:#}"),
            Some(snapshot.revision),
        );
    }
    let (reply_sender, reply_receiver) = sync_channel(1);
    if let Err(failure) = send_audio(
        context,
        AudioControlCommand::ReplaceStandaloneVoice {
            instance_id: instance_id.clone(),
            instance: PreparedPluginInstance(replacement),
            reply: reply_sender,
        },
    ) {
        return failure.into_response();
    }
    match receive_audio(reply_receiver, "replace plugin resource") {
        Ok(()) => ControlResponse::PluginResourceCleared {
            instance_id: instance_id.clone(),
            resource_id: resource_id.to_owned(),
        },
        Err(failure) => failure.into_response(),
    }
}

fn plugin_parameters(context: &ControlContext, instance_id: &InstanceId) -> ControlResponse {
    let snapshot = match context.store.lock() {
        Ok(store) => store.snapshot(),
        Err(_) => return internal_error("session store lock is poisoned", None),
    };
    if let Err(failure) = require_active_instance(&snapshot, instance_id) {
        return failure.into_response();
    }
    let (reply_sender, reply_receiver) = sync_channel(1);
    if let Err(failure) = send_audio(
        context,
        AudioControlCommand::PluginParameters {
            instance_id: instance_id.clone(),
            reply: reply_sender,
        },
    ) {
        return failure.into_response();
    }
    match receive_audio(reply_receiver, "read plugin parameters") {
        Ok((schema, values)) => ControlResponse::PluginParameters {
            instance_id: instance_id.clone(),
            schema: Box::new(schema),
            values,
        },
        Err(failure) => failure.into_response(),
    }
}

fn set_plugin_parameter(
    context: &ControlContext,
    instance_id: &InstanceId,
    parameter_index: u32,
    value: f64,
) -> ControlResponse {
    if !value.is_finite() {
        return error_response(
            ControlErrorCode::InvalidRequest,
            "plugin parameter value must be finite",
            current_revision(context),
        );
    }
    let snapshot = match context.store.lock() {
        Ok(store) => store.snapshot(),
        Err(_) => return internal_error("session store lock is poisoned", None),
    };
    if let Err(failure) = require_active_instance(&snapshot, instance_id) {
        return failure.into_response();
    }
    let (reply_sender, reply_receiver) = sync_channel(1);
    if let Err(failure) = send_audio(
        context,
        AudioControlCommand::SetPluginParameter {
            instance_id: instance_id.clone(),
            parameter_index,
            value,
            reply: reply_sender,
        },
    ) {
        return failure.into_response();
    }
    match receive_audio(reply_receiver, "set plugin parameter") {
        Ok(value) => ControlResponse::PluginParameterSet {
            instance_id: instance_id.clone(),
            parameter_index,
            value,
        },
        Err(failure) => failure.into_response(),
    }
}

fn plugin_state_parameters(
    context: &ControlContext,
    state: &rackforge_plugin_api::PluginStateReference,
) -> ControlResponse {
    let (plugin, bytes, resources, revision) = match isolated_plugin_state(context, state) {
        Ok(parts) => parts,
        Err(failure) => return failure.into_response(),
    };
    let result = (|| -> Result<_> {
        let mut editor = IsolatedPluginStateEditor::open(plugin, &resources, &bytes)?;
        editor.parameters()
    })();
    match result {
        Ok((schema, values)) => ControlResponse::PluginStateParameters {
            state: Box::new(state.clone()),
            schema: Box::new(schema),
            values,
        },
        Err(error) => error_response(
            ControlErrorCode::Rejected,
            format!("reading isolated plugin parameters: {error:#}"),
            Some(revision),
        ),
    }
}

fn set_plugin_state_parameter(
    context: &ControlContext,
    state: &rackforge_plugin_api::PluginStateReference,
    parameter_index: u32,
    value: f64,
) -> ControlResponse {
    if !value.is_finite() {
        return error_response(
            ControlErrorCode::InvalidRequest,
            "plugin parameter value must be finite",
            current_revision(context),
        );
    }
    let (plugin, bytes, resources, revision) = match isolated_plugin_state(context, state) {
        Ok(parts) => parts,
        Err(failure) => return failure.into_response(),
    };
    let edited = (|| -> Result<_> {
        let mut editor = IsolatedPluginStateEditor::open(plugin, &resources, &bytes)?;
        let canonical = editor.set_parameter(parameter_index, value)?;
        let bytes = editor.save_state()?;
        Ok((canonical, bytes))
    })();
    let (canonical, bytes) = match edited {
        Ok(edited) => edited,
        Err(error) => {
            return error_response(
                ControlErrorCode::Rejected,
                format!("editing isolated plugin parameter: {error:#}"),
                Some(revision),
            );
        }
    };
    let manifest = plugin.manifest();
    let next_state = match context.state_store.lock() {
        Ok(mut store) => store.put(
            &manifest.id,
            &manifest.version,
            manifest.state_version,
            state.selected_sound_id.clone(),
            &bytes,
        ),
        Err(_) => {
            return internal_error("plugin state store lock is poisoned", Some(revision));
        }
    };
    match next_state {
        Ok(next_state) => ControlResponse::PluginStateParameterSet {
            state: Box::new(next_state),
            parameter_index,
            value: canonical,
        },
        Err(error) => error_response(
            ControlErrorCode::Rejected,
            format!("storing edited plugin state: {error:#}"),
            Some(revision),
        ),
    }
}

type IsolatedPluginState = (
    &'static LoadedPlugin,
    Vec<u8>,
    BTreeMap<String, PathBuf>,
    Revision,
);

fn isolated_plugin_state(
    context: &ControlContext,
    state: &rackforge_plugin_api::PluginStateReference,
) -> Result<IsolatedPluginState, ControlFailure> {
    let revision = context
        .store
        .lock()
        .map_err(|_| {
            control_failure(
                ControlErrorCode::Internal,
                "session store lock is poisoned",
                None,
            )
        })?
        .state()
        .revision;
    let plugin = context
        .portable_plugins
        .get(&state.plugin_id)
        .map(|runtime| runtime.0)
        .ok_or_else(|| {
            control_failure(
                ControlErrorCode::Unavailable,
                format!(
                    "plugin {} is not available as a portable isolated runtime",
                    state.plugin_id
                ),
                Some(revision),
            )
        })?;
    validate_state_reference(plugin, state).map_err(|error| {
        control_failure(
            ControlErrorCode::Rejected,
            format!("incompatible plugin state: {error:#}"),
            Some(revision),
        )
    })?;
    let bytes = context
        .state_store
        .lock()
        .map_err(|_| {
            control_failure(
                ControlErrorCode::Internal,
                "plugin state store lock is poisoned",
                Some(revision),
            )
        })?
        .read(state)
        .map_err(|error| {
            control_failure(
                ControlErrorCode::NotFound,
                format!("plugin state is unavailable: {error:#}"),
                Some(revision),
            )
        })?;
    let instance_id = context.store.lock().ok().and_then(|store| {
        store
            .state()
            .instances
            .iter()
            .find(|instance| instance.plugin_id == state.plugin_id)
            .map(|instance| instance.instance_id.clone())
    });
    let registry = context.dynamic_resources.lock().map_err(|_| {
        control_failure(
            ControlErrorCode::Internal,
            "dynamic resource registry is poisoned",
            Some(revision),
        )
    })?;
    let resources = instance_id
        .as_ref()
        .and_then(|instance_id| registry.get(instance_id))
        .cloned()
        .unwrap_or_default();
    Ok((plugin, bytes, resources, revision))
}

fn apply_audio_output(
    context: &Arc<ControlContext>,
    profile: AudioOutputProfile,
) -> ControlResponse {
    if let Err(error) = profile.validate() {
        return error_response(
            ControlErrorCode::InvalidRequest,
            error.to_string(),
            current_revision(context),
        );
    }
    let previous = match context.audio_state.lock() {
        Ok(state) => state.active_profile.clone(),
        Err(_) => return internal_error("audio state lock is poisoned", current_revision(context)),
    };
    let (reply_sender, reply_receiver) = sync_channel(1);
    if let Err(failure) = send_audio(
        context,
        AudioControlCommand::ApplyAudioOutput {
            profile: profile.clone(),
            reply: reply_sender,
        },
    ) {
        return failure.into_response();
    }
    let snapshot = match receive_audio_with_timeout(
        reply_receiver,
        "apply audio output",
        AUDIO_RECONFIGURE_TIMEOUT,
    ) {
        Ok(snapshot) => snapshot,
        Err(failure) => return failure.into_response(),
    };
    if let Err(error) = persist_audio_output(&context.audio_state_path, &profile) {
        let (rollback_sender, rollback_receiver) = sync_channel(1);
        let rollback = send_audio(
            context,
            AudioControlCommand::ApplyAudioOutput {
                profile: previous,
                reply: rollback_sender,
            },
        )
        .and_then(|()| {
            receive_audio_with_timeout(
                rollback_receiver,
                "roll back audio output after persistence failure",
                AUDIO_RECONFIGURE_TIMEOUT,
            )
            .map(|_| ())
        });
        let suffix = match rollback {
            Ok(()) => "the previous output was restored".to_owned(),
            Err(failure) => format!("rollback also failed: {}", failure.message),
        };
        return error_response(
            ControlErrorCode::Internal,
            format!("could not persist audio output: {error:#}; {suffix}"),
            current_revision(context),
        );
    }
    ControlResponse::AudioApplied {
        snapshot: Box::new(snapshot),
    }
}

fn edit_performance(
    context: &Arc<ControlContext>,
    expected_revision: LibraryRevision,
    edit: PerformanceEdit,
) -> ControlResponse {
    if let Err(error) = expected_revision.validate() {
        return error_response(
            ControlErrorCode::InvalidRequest,
            error.to_string(),
            current_revision(context),
        );
    }
    let (session_revision, mut live, program_draft, audition) = match context.store.lock() {
        Ok(store) => (
            store.state().revision,
            store.state().live.clone(),
            store.state().program_draft.clone(),
            store.state().audition.clone(),
        ),
        Err(_) => return internal_error("session store lock is poisoned", None),
    };
    let current = match context.performance_repository.lock() {
        Ok(repository) => repository.revision(),
        Err(_) => {
            return internal_error(
                "performance repository lock is poisoned",
                Some(session_revision),
            );
        }
    };
    if current != expected_revision {
        return error_response(
            ControlErrorCode::Conflict,
            format!(
                "performance library changed: expected {}, current {}",
                expected_revision.as_str(),
                current.as_str()
            ),
            Some(session_revision),
        );
    }
    let edit = match materialize_performance_edit(context, edit, session_revision) {
        Ok(edit) => edit,
        Err(failure) => return failure.into_response(),
    };
    let mut repository = match context.performance_repository.lock() {
        Ok(repository) => repository,
        Err(_) => {
            return internal_error(
                "performance repository lock is poisoned",
                Some(session_revision),
            );
        }
    };
    let current = repository.revision();
    if current != expected_revision {
        return error_response(
            ControlErrorCode::Conflict,
            format!(
                "performance library changed: expected {}, current {}",
                expected_revision.as_str(),
                current.as_str()
            ),
            Some(session_revision),
        );
    }
    let previous_live = live.clone();
    if let Err(error) = repository.apply_edit(&expected_revision, edit, &mut live) {
        return error_response(
            ControlErrorCode::Rejected,
            error.to_string(),
            Some(session_revision),
        );
    }
    let revision = repository.revision();
    let library = repository.library().clone();
    drop(repository);

    let active_rack_deleted =
        previous_live.active_rack_id.is_some() && live.active_rack_id.is_none();
    if active_rack_deleted {
        let (reply_sender, reply_receiver) = sync_channel(1);
        let silence_result = send_audio(
            context,
            AudioControlCommand::EmergencyStop {
                reply: reply_sender,
            },
        )
        .and_then(|()| receive_audio(reply_receiver, "stop deleted LIVE Rack"));
        match silence_result {
            Ok(()) => println!("LIVE_RACK_DEACTIVATED reason=deleted mode=Empty"),
            Err(failure) => eprintln!(
                "LIVE_RACK_DEACTIVATION_FAILED reason=deleted error={}",
                failure.message
            ),
        }
        set_lease_deadline(context, None);
    }

    if live != previous_live {
        let checkpoint_state = match context.store.lock() {
            Ok(mut store) => {
                let mut events = vec![SessionEvent::LiveStateReconciled { live: live.clone() }];
                if active_rack_deleted {
                    if let Some(draft) = program_draft.as_ref() {
                        events.push(SessionEvent::ProgramEditCancelled {
                            draft_id: draft.draft_id,
                            instance_id: draft.instance_id.clone(),
                        });
                    }
                    if let Some(audition) = audition.as_ref() {
                        events.push(SessionEvent::AuditionEnded {
                            lease_id: audition.lease_id,
                            instance_id: audition.instance_id.clone(),
                            restored_sound_id: None,
                            reason: AuditionEndReason::Cancelled,
                        });
                    }
                    events.push(SessionEvent::ActiveModeChanged {
                        mode: rackforge_session_api::SurfaceMode::Idle,
                    });
                }
                if let Err(error) = store.record_many(None, events) {
                    return internal_error(error.to_string(), Some(store.state().revision));
                }
                Some(store.snapshot())
            }
            Err(_) => return internal_error("session store lock is poisoned", None),
        };
        if let (Some(checkpoint), Some(state)) = (&context.checkpoint, checkpoint_state.as_ref())
            && let Err(error) = checkpoint.save(state)
        {
            eprintln!("SESSION_CHECKPOINT_ERROR {error:#}");
        }
    }
    let snapshot = PerformanceSnapshot {
        schema_version: PERFORMANCE_SNAPSHOT_SCHEMA_VERSION,
        revision,
        library,
        live,
    };
    ControlResponse::PerformanceEdited {
        snapshot: Box::new(snapshot),
    }
}

fn materialize_performance_edit(
    context: &ControlContext,
    mut edit: PerformanceEdit,
    revision: Revision,
) -> Result<PerformanceEdit, ControlFailure> {
    let PerformanceEdit::PutRack { rack } = &mut edit else {
        return Ok(edit);
    };
    for slot in &mut rack.slots {
        let manifest = context
            .plugin_manifests
            .get(&slot.plugin_id)
            .ok_or_else(|| {
                control_failure(
                    ControlErrorCode::Unavailable,
                    format!(
                        "plugin {} is not loaded by the current runtime",
                        slot.plugin_id
                    ),
                    Some(revision),
                )
            })?;
        if let Some(reference) = &slot.state {
            let store = context.state_store.lock().map_err(|_| {
                control_failure(
                    ControlErrorCode::Internal,
                    "plugin state store lock is poisoned",
                    Some(revision),
                )
            })?;
            store.read(reference).map_err(|error| {
                control_failure(
                    ControlErrorCode::Rejected,
                    format!("Rack Slot {} has unavailable state: {error:#}", slot.id),
                    Some(revision),
                )
            })?;
            continue;
        }
        let program_id = slot.legacy_program_id.clone();
        let (reply_sender, reply_receiver) = sync_channel(1);
        send_audio(
            context,
            AudioControlCommand::MaterializeState {
                instance_id: instance_id_for_plugin(context, &slot.plugin_id, revision)?,
                program_id: program_id.clone(),
                reply: reply_sender,
            },
        )?;
        let bytes = receive_audio(reply_receiver, "materialize Rack Slot state")?;
        let mut store = context.state_store.lock().map_err(|_| {
            control_failure(
                ControlErrorCode::Internal,
                "plugin state store lock is poisoned",
                Some(revision),
            )
        })?;
        slot.state = Some(
            store
                .put(
                    &manifest.id,
                    &manifest.version,
                    manifest.state_version,
                    program_id,
                    &bytes,
                )
                .map_err(|error| {
                    control_failure(
                        ControlErrorCode::Rejected,
                        format!("storing state for Rack Slot {}: {error:#}", slot.id),
                        Some(revision),
                    )
                })?,
        );
        slot.legacy_program_id = None;
    }
    Ok(edit)
}

fn list_plugin_presets(context: &ControlContext, plugin_id: &str) -> ControlResponse {
    match context.state_store.lock() {
        Ok(store) => match store.list_presets(plugin_id) {
            Ok(presets) => ControlResponse::PluginPresets {
                plugin_id: plugin_id.into(),
                presets,
            },
            Err(error) => error_response(
                ControlErrorCode::InvalidRequest,
                error.to_string(),
                current_revision(context),
            ),
        },
        Err(_) => internal_error(
            "plugin state store lock is poisoned",
            current_revision(context),
        ),
    }
}

fn instance_id_for_plugin(
    context: &ControlContext,
    plugin_id: &str,
    revision: Revision,
) -> Result<InstanceId, ControlFailure> {
    let snapshot = context
        .store
        .lock()
        .map_err(|_| {
            control_failure(
                ControlErrorCode::Internal,
                "session store lock is poisoned",
                Some(revision),
            )
        })?
        .snapshot();
    snapshot
        .instances
        .iter()
        .find(|instance| instance.plugin_id == plugin_id)
        .map(|instance| instance.instance_id.clone())
        .ok_or_else(|| {
            control_failure(
                ControlErrorCode::Unavailable,
                format!("plugin {plugin_id} has no runtime instance"),
                Some(revision),
            )
        })
}

fn get_plugin_preset(
    context: &ControlContext,
    plugin_id: &str,
    preset_id: &str,
) -> ControlResponse {
    match context.state_store.lock() {
        Ok(store) => match store.preset(plugin_id, preset_id) {
            Ok(preset) => ControlResponse::PluginPreset {
                preset: Box::new(preset),
            },
            Err(error) => error_response(
                ControlErrorCode::NotFound,
                error.to_string(),
                current_revision(context),
            ),
        },
        Err(_) => internal_error(
            "plugin state store lock is poisoned",
            current_revision(context),
        ),
    }
}

fn export_plugin_preset(
    context: &ControlContext,
    plugin_id: &str,
    preset_id: &str,
) -> ControlResponse {
    let revision = current_revision(context);
    let plugin_name = context
        .plugin_manifests
        .get(plugin_id)
        .map(|manifest| manifest.name.as_str())
        .unwrap_or(plugin_id);
    match context.state_store.lock() {
        Ok(store) => match store.export_preset_file(plugin_id, preset_id, plugin_name) {
            Ok((file_name, file)) => ControlResponse::PluginPresetExported {
                file_name,
                file: Box::new(file),
            },
            Err(error) => error_response(
                ControlErrorCode::Rejected,
                format!("exporting RackForge preset: {error:#}"),
                revision,
            ),
        },
        Err(_) => internal_error("plugin state store lock is poisoned", revision),
    }
}

fn installed_plugin_versions(context: &ControlContext) -> BTreeMap<String, String> {
    context
        .plugin_manifests
        .iter()
        .map(|(plugin_id, manifest)| (plugin_id.clone(), manifest.version.clone()))
        .collect()
}

fn export_live_show(context: &ControlContext, name: &str) -> ControlResponse {
    let revision = current_revision(context);
    let (repository, store) = match (context.performance_repository.lock(), context.state_store.lock()) {
        (Ok(repository), Ok(store)) => (repository, store),
        _ => return internal_error("library or state store lock is poisoned", revision),
    };
    let exported = crate::live_show::now_unix_ms().and_then(|now| {
        crate::live_show::assemble_live_show(name, repository.library(), &store, now)
    });
    match exported {
        Ok(file) => ControlResponse::LiveShowExported {
            file_name: crate::live_show::live_show_file_name(name),
            file: Box::new(file),
        },
        Err(error) => error_response(
            ControlErrorCode::Rejected,
            format!("exporting the show: {error:#}"),
            revision,
        ),
    }
}

fn inspect_live_show_request(
    context: &ControlContext,
    file: &rackforge_control_api::RfLiveFile,
) -> ControlResponse {
    let revision = current_revision(context);
    let repository = match context.performance_repository.lock() {
        Ok(repository) => repository,
        Err(_) => return internal_error("performance repository lock is poisoned", revision),
    };
    match crate::live_show::inspect_live_show(
        file,
        &installed_plugin_versions(context),
        repository.library(),
    ) {
        Ok(preview) => ControlResponse::LiveShowInspected {
            preview: Box::new(preview),
        },
        Err(error) => error_response(
            ControlErrorCode::InvalidRequest,
            format!("validating the .rflive file: {error:#}"),
            revision,
        ),
    }
}

/// Imports a show: authenticate and store the embedded states first so
/// every reference resolves, then upsert each document through the same
/// revisioned edit path a surface would use — LIVE reconciliation,
/// checkpointing and Rack materialization included.
fn import_live_show(
    context: &Arc<ControlContext>,
    file: &rackforge_control_api::RfLiveFile,
) -> ControlResponse {
    let revision = current_revision(context);
    let preview = {
        let repository = match context.performance_repository.lock() {
            Ok(repository) => repository,
            Err(_) => return internal_error("performance repository lock is poisoned", revision),
        };
        match crate::live_show::inspect_live_show(
            file,
            &installed_plugin_versions(context),
            repository.library(),
        ) {
            Ok(preview) => preview,
            Err(error) => {
                return error_response(
                    ControlErrorCode::InvalidRequest,
                    format!("validating the .rflive file: {error:#}"),
                    revision,
                );
            }
        }
    };
    {
        let mut store = match context.state_store.lock() {
            Ok(store) => store,
            Err(_) => return internal_error("state store lock is poisoned", revision),
        };
        if let Err(error) = crate::live_show::store_live_show_states(file, &mut store) {
            return error_response(
                ControlErrorCode::Rejected,
                format!("storing the show's plugin states: {error:#}"),
                revision,
            );
        }
    }
    let mut last_snapshot = None;
    for edit in crate::live_show::live_show_edits(file) {
        let current = match context.performance_repository.lock() {
            Ok(repository) => repository.revision(),
            Err(_) => return internal_error("performance repository lock is poisoned", revision),
        };
        match edit_performance(context, current, edit) {
            ControlResponse::PerformanceEdited { snapshot } => last_snapshot = Some(snapshot),
            other => return other,
        }
    }
    let snapshot = match last_snapshot {
        Some(snapshot) => snapshot,
        None => {
            // An empty show is legal; answer with the library as it stands.
            match (context.performance_repository.lock(), context.store.lock()) {
                (Ok(repository), Ok(store)) => Box::new(PerformanceSnapshot {
                    schema_version: PERFORMANCE_SNAPSHOT_SCHEMA_VERSION,
                    revision: repository.revision(),
                    library: repository.library().clone(),
                    live: store.state().live.clone(),
                }),
                _ => return internal_error("library or session lock is poisoned", revision),
            }
        }
    };
    ControlResponse::LiveShowImported {
        preview: Box::new(preview),
        snapshot,
    }
}

fn inspect_plugin_preset(
    context: &ControlContext,
    target_plugin_id: &str,
    file: &rackforge_control_api::RfPresetFile,
) -> ControlResponse {
    let revision = current_revision(context);
    let Some(manifest) = context.plugin_manifests.get(target_plugin_id) else {
        return error_response(
            ControlErrorCode::Unavailable,
            format!("plugin {target_plugin_id} is not hosted by this runtime"),
            revision,
        );
    };
    match context.state_store.lock() {
        Ok(store) => match store.inspect_preset_file(
            target_plugin_id,
            &manifest.version,
            manifest.state_version,
            file,
        ) {
            Ok(preview) => ControlResponse::PluginPresetInspected {
                preview: Box::new(preview),
            },
            Err(error) => error_response(
                ControlErrorCode::InvalidRequest,
                format!("validating .rfpreset: {error:#}"),
                revision,
            ),
        },
        Err(_) => internal_error("plugin state store lock is poisoned", revision),
    }
}

fn import_plugin_preset(
    context: &ControlContext,
    target_plugin_id: &str,
    file: &rackforge_control_api::RfPresetFile,
    conflict_policy: rackforge_control_api::PresetImportConflictPolicy,
) -> ControlResponse {
    let revision = current_revision(context);
    let Some(manifest) = context.plugin_manifests.get(target_plugin_id) else {
        return error_response(
            ControlErrorCode::Unavailable,
            format!("plugin {target_plugin_id} is not hosted by this runtime"),
            revision,
        );
    };
    let mut store = match context.state_store.lock() {
        Ok(store) => store,
        Err(_) => return internal_error("plugin state store lock is poisoned", revision),
    };
    let preset = match store.import_preset_file(
        target_plugin_id,
        &manifest.version,
        manifest.state_version,
        file,
        conflict_policy,
    ) {
        Ok(preset) => preset,
        Err(error) => {
            return error_response(
                ControlErrorCode::Rejected,
                format!("importing .rfpreset: {error:#}"),
                revision,
            );
        }
    };
    match store.list_presets(target_plugin_id) {
        Ok(presets) => ControlResponse::PluginPresetImported {
            preset: Box::new(preset),
            presets,
        },
        Err(error) => internal_error(error.to_string(), revision),
    }
}

fn materialize_plugin_state(
    context: &ControlContext,
    plugin_id: &str,
    requested_sound_id: Option<String>,
) -> ControlResponse {
    let snapshot = match context.store.lock() {
        Ok(store) => store.snapshot(),
        Err(_) => return internal_error("session store lock is poisoned", None),
    };
    let Some(instance) = snapshot
        .instances
        .iter()
        .find(|instance| instance.plugin_id == plugin_id)
    else {
        return error_response(
            ControlErrorCode::Unavailable,
            format!("plugin {plugin_id} has no runtime instance"),
            Some(snapshot.revision),
        );
    };
    if let Some(sound_id) = requested_sound_id.as_deref()
        && !instance.sounds.iter().any(|sound| sound.id == sound_id)
    {
        return error_response(
            ControlErrorCode::NotFound,
            format!("unknown sound {sound_id:?} for plugin {plugin_id}"),
            Some(snapshot.revision),
        );
    }
    // A Rack Slot without an explicit program must still have a complete,
    // portable initial identity. Plugin Web surfaces commonly require a
    // selected sound in their host context even before their first parameter
    // request. Prefer the current runtime selection, then its first sound.
    let sound_id = requested_sound_id
        .or_else(|| instance.selected_sound_id.clone())
        .or_else(|| instance.sounds.first().map(|sound| sound.id.clone()));
    let Some(manifest) = context.plugin_manifests.get(plugin_id) else {
        return error_response(
            ControlErrorCode::Unavailable,
            format!("plugin {plugin_id} is not hosted by this runtime"),
            Some(snapshot.revision),
        );
    };
    if !manifest.capabilities.contains(&Capability::State) {
        return error_response(
            ControlErrorCode::Unavailable,
            format!("plugin {plugin_id} does not support complete state snapshots"),
            Some(snapshot.revision),
        );
    }
    let (reply_sender, reply_receiver) = sync_channel(1);
    if let Err(failure) = send_audio(
        context,
        AudioControlCommand::MaterializeState {
            instance_id: instance.instance_id.clone(),
            program_id: sound_id.clone(),
            reply: reply_sender,
        },
    ) {
        return failure.into_response();
    }
    let bytes = match receive_audio(reply_receiver, "materialize isolated plugin state") {
        Ok(bytes) => bytes,
        Err(failure) => return failure.into_response(),
    };
    let mut store = match context.state_store.lock() {
        Ok(store) => store,
        Err(_) => {
            return internal_error(
                "plugin state store lock is poisoned",
                Some(snapshot.revision),
            );
        }
    };
    match store.put(
        plugin_id,
        &manifest.version,
        manifest.state_version,
        sound_id,
        &bytes,
    ) {
        Ok(state) => ControlResponse::PluginStateMaterialized {
            state: Box::new(state),
        },
        Err(error) => error_response(
            ControlErrorCode::Rejected,
            format!("storing isolated plugin state: {error:#}"),
            Some(snapshot.revision),
        ),
    }
}

fn save_plugin_preset(
    context: &ControlContext,
    instance_id: &InstanceId,
    name: &str,
) -> ControlResponse {
    let snapshot = match context.store.lock() {
        Ok(store) => store.snapshot(),
        Err(_) => return internal_error("session store lock is poisoned", None),
    };
    let instance = match require_active_instance(&snapshot, instance_id) {
        Ok(instance) => instance,
        Err(failure) => return failure.into_response(),
    };
    let Some(manifest) = context.plugin_manifests.get(&instance.plugin_id) else {
        return error_response(
            ControlErrorCode::Unavailable,
            "the active plugin is not hosted by this runtime",
            Some(snapshot.revision),
        );
    };
    if !manifest.capabilities.contains(&Capability::State) {
        return error_response(
            ControlErrorCode::Unavailable,
            "the plugin does not support complete state snapshots",
            Some(snapshot.revision),
        );
    }
    let (reply_sender, reply_receiver) = sync_channel(1);
    if let Err(failure) = send_audio(
        context,
        AudioControlCommand::CaptureState {
            instance_id: instance_id.clone(),
            reply: reply_sender,
        },
    ) {
        return failure.into_response();
    }
    let bytes = match receive_audio(reply_receiver, "capture plugin state") {
        Ok(bytes) => bytes,
        Err(failure) => return failure.into_response(),
    };
    let mut store = match context.state_store.lock() {
        Ok(store) => store,
        Err(_) => {
            return internal_error(
                "plugin state store lock is poisoned",
                Some(snapshot.revision),
            );
        }
    };
    let state = match store.put(
        &manifest.id,
        &manifest.version,
        manifest.state_version,
        instance.selected_sound_id.clone(),
        &bytes,
    ) {
        Ok(state) => state,
        Err(error) => {
            return error_response(
                ControlErrorCode::Rejected,
                format!("storing plugin state: {error:#}"),
                Some(snapshot.revision),
            );
        }
    };
    let preset = match store.save_preset(name, state) {
        Ok(preset) => preset,
        Err(error) => {
            return error_response(
                ControlErrorCode::InvalidRequest,
                format!("saving RackForge preset: {error:#}"),
                Some(snapshot.revision),
            );
        }
    };
    let presets = match store.list_presets(&instance.plugin_id) {
        Ok(presets) => presets,
        Err(error) => {
            return internal_error(error.to_string(), Some(snapshot.revision));
        }
    };
    ControlResponse::PluginPresetSaved {
        preset: Box::new(preset),
        presets,
    }
}

fn rename_plugin_preset(
    context: &ControlContext,
    plugin_id: &str,
    preset_id: &str,
    name: &str,
) -> ControlResponse {
    let revision = current_revision(context);
    let mut store = match context.state_store.lock() {
        Ok(store) => store,
        Err(_) => return internal_error("plugin state store lock is poisoned", revision),
    };
    let preset = match store.rename_preset(plugin_id, preset_id, name) {
        Ok(preset) => preset,
        Err(error) => {
            return error_response(
                ControlErrorCode::InvalidRequest,
                format!("renaming RackForge preset: {error:#}"),
                revision,
            );
        }
    };
    let presets = match store.list_presets(plugin_id) {
        Ok(presets) => presets,
        Err(error) => return internal_error(error.to_string(), revision),
    };
    ControlResponse::PluginPresetRenamed {
        preset: Box::new(preset),
        presets,
    }
}

fn delete_plugin_preset(
    context: &ControlContext,
    plugin_id: &str,
    preset_id: &str,
) -> ControlResponse {
    let revision = current_revision(context);
    let mut store = match context.state_store.lock() {
        Ok(store) => store,
        Err(_) => return internal_error("plugin state store lock is poisoned", revision),
    };
    if let Err(error) = store.delete_preset(plugin_id, preset_id) {
        return error_response(
            ControlErrorCode::NotFound,
            format!("deleting RackForge preset: {error:#}"),
            revision,
        );
    }
    let presets = match store.list_presets(plugin_id) {
        Ok(presets) => presets,
        Err(error) => return internal_error(error.to_string(), revision),
    };
    ControlResponse::PluginPresetDeleted {
        plugin_id: plugin_id.into(),
        preset_id: preset_id.into(),
        presets,
    }
}

fn load_plugin_preset(
    context: &ControlContext,
    instance_id: &InstanceId,
    preset_id: &str,
) -> ControlResponse {
    let snapshot = match context.store.lock() {
        Ok(store) => store.snapshot(),
        Err(_) => return internal_error("session store lock is poisoned", None),
    };
    let instance = match require_active_instance(&snapshot, instance_id) {
        Ok(instance) => instance,
        Err(failure) => return failure.into_response(),
    };
    let (preset, bytes) = {
        let store = match context.state_store.lock() {
            Ok(store) => store,
            Err(_) => {
                return internal_error(
                    "plugin state store lock is poisoned",
                    Some(snapshot.revision),
                );
            }
        };
        let preset = match store.preset(&instance.plugin_id, preset_id) {
            Ok(preset) => preset,
            Err(error) => {
                return error_response(
                    ControlErrorCode::NotFound,
                    error.to_string(),
                    Some(snapshot.revision),
                );
            }
        };
        let Some(manifest) = context.plugin_manifests.get(&instance.plugin_id) else {
            return error_response(
                ControlErrorCode::Unavailable,
                "the active plugin is not hosted by this runtime",
                Some(snapshot.revision),
            );
        };
        if preset.state.state_version != manifest.state_version {
            return error_response(
                ControlErrorCode::Rejected,
                format!(
                    "preset state v{} is incompatible with installed state v{}",
                    preset.state.state_version, manifest.state_version
                ),
                Some(snapshot.revision),
            );
        }
        let bytes = match store.read(&preset.state) {
            Ok(bytes) => bytes,
            Err(error) => {
                return error_response(
                    ControlErrorCode::Rejected,
                    format!("reading RackForge preset state: {error:#}"),
                    Some(snapshot.revision),
                );
            }
        };
        (preset, bytes)
    };
    let (reply_sender, reply_receiver) = sync_channel(1);
    if let Err(failure) = send_audio(
        context,
        AudioControlCommand::RestoreState {
            instance_id: instance_id.clone(),
            bytes,
            reply: reply_sender,
        },
    ) {
        return failure.into_response();
    }
    if let Err(failure) = receive_audio(reply_receiver, "restore plugin state") {
        return failure.into_response();
    }
    let (revision, checkpoint_state) = match context.store.lock() {
        Ok(mut store) => match store.record(
            None,
            SessionEvent::PluginStateRestored {
                instance_id: instance_id.clone(),
                selected_sound_id: preset.state.selected_sound_id.clone(),
            },
        ) {
            Ok(event) => (event.revision, store.snapshot()),
            Err(error) => {
                return internal_error(error.to_string(), Some(store.state().revision));
            }
        },
        Err(_) => return internal_error("session store lock is poisoned", None),
    };
    if let Some(checkpoint) = &context.checkpoint
        && let Err(error) = checkpoint.save(&checkpoint_state)
    {
        eprintln!("SESSION_CHECKPOINT_ERROR {error:#}");
    }
    ControlResponse::PluginPresetLoaded {
        preset: Box::new(preset),
        revision,
    }
}

fn persist_audio_output(path: &Path, profile: &AudioOutputProfile) -> Result<()> {
    let parent = path
        .parent()
        .context("audio output state path has no parent")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("creating audio state directory {}", parent.display()))?;
    let document = AudioOutputDocument::new(profile.clone());
    document
        .validate()
        .context("validating audio output state")?;
    let bytes = toml::to_string_pretty(&document)?.into_bytes();
    let temporary = path.with_extension(format!("toml.tmp.{}", std::process::id()));
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .with_context(|| format!("creating {}", temporary.display()))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| format!("replacing {}", path.display()));
    }
    Ok(())
}

fn begin_midi_learn(
    context: &ControlContext,
    instance_id: &str,
    parameter_index: u32,
) -> ControlResponse {
    let snapshot = match context.store.lock() {
        Ok(store) => store.state().clone(),
        Err(_) => return internal_error("session store lock is poisoned", None),
    };
    let repository = match context.performance_repository.lock() {
        Ok(repository) => repository,
        Err(_) => {
            return internal_error(
                "performance repository lock is poisoned",
                Some(snapshot.revision),
            );
        }
    };
    let plugin_id = snapshot
        .instances
        .iter()
        .find(|instance| instance.instance_id.as_str() == instance_id)
        .map(|instance| instance.plugin_id.as_str())
        .or_else(|| {
            repository
                .library()
                .racks
                .iter()
                .flat_map(|rack| rack.slots.iter())
                .find(|slot| slot.id.as_str() == instance_id)
                .map(|slot| slot.plugin_id.as_str())
        });
    let Some(plugin_id) = plugin_id else {
        return error_response(
            ControlErrorCode::NotFound,
            format!("MIDI Learn targets unknown instance {instance_id}"),
            Some(snapshot.revision),
        );
    };
    let Some(plugin) = context.portable_plugins.get(plugin_id) else {
        return error_response(
            ControlErrorCode::Unavailable,
            format!("plugin {plugin_id} does not expose a portable parameter runtime"),
            Some(snapshot.revision),
        );
    };
    let Some(parameter) = plugin
        .0
        .parameters()
        .parameters
        .iter()
        .find(|parameter| parameter.index == parameter_index)
    else {
        return error_response(
            ControlErrorCode::NotFound,
            format!("unknown plugin parameter {parameter_index}"),
            Some(snapshot.revision),
        );
    };
    if parameter.flags.read_only
        || matches!(
            parameter.kind,
            rackforge_plugin_api::ParameterKind::Meter { .. }
        )
    {
        return error_response(
            ControlErrorCode::Rejected,
            format!("plugin parameter {} is read-only", parameter.id),
            Some(snapshot.revision),
        );
    }
    drop(repository);
    if let Ok(observer) = context.midi_observer.lock() {
        while observer.try_recv().is_ok() {}
    }
    let learn_id = context
        .next_midi_learn_id
        .fetch_add(1, Ordering::Relaxed)
        .max(1);
    match context.midi_learn.lock() {
        Ok(mut learn) => {
            *learn = Some(MidiLearnState {
                id: learn_id,
                candidate: None,
            });
            ControlResponse::MidiLearnStarted { learn_id }
        }
        Err(_) => internal_error("MIDI Learn state lock is poisoned", Some(snapshot.revision)),
    }
}

fn midi_learn_candidate(
    sources: &MidiSourceRegistry,
    ingress: IngressMidiEvent,
) -> Option<MidiLearnCandidate> {
    let message = match ingress.packet.kind() {
        MidiMessageKind::ControlChange => Some(ParameterLinkMessage::ControlChange {
            controller: ingress.packet.data[1],
        }),
        MidiMessageKind::PitchBend => Some(ParameterLinkMessage::PitchBend),
        MidiMessageKind::Note => Some(ParameterLinkMessage::Note {
            note: ingress.packet.data[1],
        }),
        MidiMessageKind::ChannelPressure => Some(ParameterLinkMessage::ChannelPressure),
        MidiMessageKind::PolyPressure => Some(ParameterLinkMessage::PolyPressure {
            note: ingress.packet.data[1],
        }),
        MidiMessageKind::ProgramChange => None,
    }?;
    Some(MidiLearnCandidate {
        source: sources.descriptor(ingress.source)?.clone(),
        channel: ingress.packet.channel(),
        message,
    })
}

fn observe_forwarded_midi_learn(context: &ControlContext, ingress: IngressMidiEvent) {
    let Some(candidate) = midi_learn_candidate(&context.midi_sources, ingress) else {
        return;
    };
    if let Ok(mut learn) = context.midi_learn.lock()
        && let Some(active) = learn.as_mut()
        && active.candidate.is_none()
    {
        active.candidate = Some(candidate);
    }
}

fn midi_learn_status(context: &ControlContext, learn_id: u64) -> ControlResponse {
    let mut learn = match context.midi_learn.lock() {
        Ok(learn) => learn,
        Err(_) => {
            return internal_error(
                "MIDI Learn state lock is poisoned",
                current_revision(context),
            );
        }
    };
    let Some(active) = learn.as_mut() else {
        return error_response(
            ControlErrorCode::NotFound,
            format!("MIDI Learn session {learn_id} does not exist"),
            current_revision(context),
        );
    };
    if active.id != learn_id {
        return error_response(
            ControlErrorCode::NotFound,
            format!("MIDI Learn session {learn_id} does not exist"),
            current_revision(context),
        );
    }
    if active.candidate.is_none()
        && let Ok(observer) = context.midi_observer.lock()
    {
        while let Ok(ingress) = observer.try_recv() {
            if let Some(candidate) = midi_learn_candidate(&context.midi_sources, ingress) {
                active.candidate = Some(candidate);
                break;
            }
        }
    }
    ControlResponse::MidiLearnStatus {
        learn_id,
        candidate: active.candidate.clone(),
    }
}

fn cancel_midi_learn(context: &ControlContext, learn_id: u64) -> ControlResponse {
    match context.midi_learn.lock() {
        Ok(mut learn) if learn.as_ref().is_some_and(|active| active.id == learn_id) => {
            *learn = None;
            ControlResponse::MidiLearnCancelled { learn_id }
        }
        Ok(_) => error_response(
            ControlErrorCode::NotFound,
            format!("MIDI Learn session {learn_id} does not exist"),
            current_revision(context),
        ),
        Err(_) => internal_error(
            "MIDI Learn state lock is poisoned",
            current_revision(context),
        ),
    }
}

fn compile_parameter_links(
    context: &ControlContext,
    snapshot: &rackforge_session_api::SessionState,
    links: &[ParameterLink],
) -> Result<Vec<CompiledParameterLink>, ControlFailure> {
    let repository = context.performance_repository.lock().map_err(|_| {
        control_failure(
            ControlErrorCode::Internal,
            "performance repository lock is poisoned",
            Some(snapshot.revision),
        )
    })?;
    let mut compiled = Vec::with_capacity(links.len());
    for link in links {
        let plugin_id = snapshot
            .instances
            .iter()
            .find(|instance| instance.instance_id.as_str() == link.instance_id)
            .map(|instance| instance.plugin_id.as_str())
            .or_else(|| {
                repository
                    .library()
                    .racks
                    .iter()
                    .flat_map(|rack| rack.slots.iter())
                    .find(|slot| slot.id.as_str() == link.instance_id)
                    .map(|slot| slot.plugin_id.as_str())
            })
            .ok_or_else(|| {
                control_failure(
                    ControlErrorCode::NotFound,
                    format!(
                        "parameter link {} targets unknown instance {}",
                        link.id, link.instance_id
                    ),
                    Some(snapshot.revision),
                )
            })?;
        let plugin = context.portable_plugins.get(plugin_id).ok_or_else(|| {
            control_failure(
                ControlErrorCode::Unavailable,
                format!("plugin {plugin_id} does not expose a portable parameter runtime"),
                Some(snapshot.revision),
            )
        })?;
        // A missing source is a valid pending link. It will compile again when
        // the stable endpoint identity reconnects.
        let source_key = context
            .midi_sources
            .resolve_optional(&link.source.source_id);
        let runtime = CompiledParameterLink::new(
            link.clone(),
            source_key.unwrap_or_else(|| rackforge_midi_api::MidiSourceKey::new(u32::MAX)),
            plugin.0.parameters(),
        )
        .map_err(|error| {
            control_failure(
                ControlErrorCode::InvalidRequest,
                format!("invalid parameter link {}: {error:#}", link.id),
                Some(snapshot.revision),
            )
        })?;
        if source_key.is_some() {
            compiled.push(runtime);
        }
    }

    let semantic_profiles = context.semantic_profiles.lock().map_err(|_| {
        control_failure(
            ControlErrorCode::Internal,
            "semantic controller profile lock is poisoned",
            Some(snapshot.revision),
        )
    })?;
    for (controller_id, registered) in semantic_profiles.iter() {
        let Some(source_id) = &registered.runtime_source_id else {
            continue;
        };
        let profile = &registered.profile;
        let Some(source_key) = context.midi_sources.resolve_optional(source_id) else {
            // The package remains registered while its device is absent.
            continue;
        };
        for instance in &snapshot.instances {
            let Some(plugin) = context.portable_plugins.get(&instance.plugin_id) else {
                continue;
            };
            compiled.extend(
                compile_semantic_parameter_links(SemanticParameterLinkContext {
                    controller_id,
                    controller_name: registered
                        .runtime_source_name
                        .as_deref()
                        .unwrap_or(controller_id),
                    profile,
                    runtime_source_id: source_id,
                    source_key,
                    instance_id: instance.instance_id.as_str(),
                    schema: plugin.0.parameters(),
                    explicit_links: links,
                })
                .map_err(|error| {
                    control_failure(
                        ControlErrorCode::InvalidRequest,
                        format!(
                            "invalid semantic mapping for controller {controller_id} and plugin {}: {error:#}",
                            instance.plugin_id
                        ),
                        Some(snapshot.revision),
                    )
                })?,
            );
        }
    }
    Ok(compiled)
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
        SessionCommand::UpsertParameterLink { link } => {
            if let Err(error) = link.validate() {
                return error_response(
                    ControlErrorCode::InvalidRequest,
                    error.to_string(),
                    Some(snapshot.revision),
                );
            }
            let mut links = snapshot.parameter_links.clone();
            if let Some(existing) = links.iter_mut().find(|existing| existing.id == link.id) {
                *existing = link.clone();
            } else {
                links.push(link.clone());
            }
            let compiled = match compile_parameter_links(context, &snapshot, &links) {
                Ok(compiled) => compiled,
                Err(failure) => return failure.into_response(),
            };
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::ReplaceParameterLinks {
                    links: compiled,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "replace parameter links") {
                Ok(()) => record_command_event(
                    context,
                    command_ref,
                    SessionEvent::ParameterLinkUpserted { link },
                ),
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::RemoveParameterLink { link_id } => {
            if !snapshot
                .parameter_links
                .iter()
                .any(|link| link.id == link_id)
            {
                return error_response(
                    ControlErrorCode::NotFound,
                    format!("unknown parameter link {link_id}"),
                    Some(snapshot.revision),
                );
            }
            let links = snapshot
                .parameter_links
                .iter()
                .filter(|link| link.id != link_id)
                .cloned()
                .collect::<Vec<_>>();
            let compiled = match compile_parameter_links(context, &snapshot, &links) {
                Ok(compiled) => compiled,
                Err(failure) => return failure.into_response(),
            };
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::ReplaceParameterLinks {
                    links: compiled,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "replace parameter links") {
                Ok(()) => record_command_event(
                    context,
                    command_ref,
                    SessionEvent::ParameterLinkRemoved { link_id },
                ),
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::RegisterHostControls {
            controller_id,
            bindings,
        } => {
            if ClientId::new(&controller_id).is_err()
                || bindings
                    .iter()
                    .any(|binding| binding.midi_cc.validate().is_err())
            {
                return error_response(
                    ControlErrorCode::InvalidRequest,
                    "invalid reserved host-control registration",
                    Some(snapshot.revision),
                );
            }
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::RegisterHostControls {
                    controller_id,
                    bindings,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "register reserved host controls") {
                Ok(()) => command_applied_without_events(context, command_ref),
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::RegisterHostBindings {
            controller_id,
            controls,
            actions,
            midi_source_name,
            semantic_profile,
        } => {
            let invalid = ClientId::new(&controller_id).is_err()
                || controls
                    .iter()
                    .any(|binding| binding.midi_cc.validate().is_err())
                || actions
                    .iter()
                    .any(|binding| binding.midi_cc.validate().is_err())
                || semantic_profile.as_ref().is_some_and(|profile| {
                    profile
                        .validate_against_reserved(&controls, &actions)
                        .is_err()
                });
            if invalid {
                return error_response(
                    ControlErrorCode::InvalidRequest,
                    "invalid reserved host binding registration",
                    Some(snapshot.revision),
                );
            }
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::RegisterHostBindings {
                    controller_id: controller_id.clone(),
                    controls,
                    actions,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "register reserved host bindings") {
                Ok(()) => {
                    let previous = match context.semantic_profiles.lock() {
                        Ok(mut profiles) => {
                            let previous = profiles.get(&controller_id).cloned();
                            match semantic_profile {
                                Some(profile) => {
                                    let resolved = midi_source_name
                                        .as_deref()
                                        .and_then(|name| context.midi_sources.resolve_name(name))
                                        .map(|(_, source)| {
                                            (Some(source.id.clone()), Some(source.name.clone()))
                                        })
                                        .unwrap_or_else(|| {
                                            if midi_source_name.is_none() {
                                                previous.as_ref().map_or(
                                                    (None, None),
                                                    |registered| {
                                                        (
                                                            registered.runtime_source_id.clone(),
                                                            registered.runtime_source_name.clone(),
                                                        )
                                                    },
                                                )
                                            } else {
                                                (None, None)
                                            }
                                        });
                                    profiles.insert(
                                        controller_id.clone(),
                                        RegisteredSemanticProfile {
                                            profile,
                                            runtime_source_id: resolved.0,
                                            runtime_source_name: resolved.1,
                                        },
                                    );
                                }
                                None => {
                                    profiles.remove(&controller_id);
                                }
                            }
                            previous
                        }
                        Err(_) => {
                            return internal_error(
                                "semantic controller profile lock is poisoned",
                                Some(snapshot.revision),
                            );
                        }
                    };
                    let compiled = match compile_parameter_links(
                        context,
                        &snapshot,
                        &snapshot.parameter_links,
                    ) {
                        Ok(compiled) => compiled,
                        Err(failure) => {
                            if let Ok(mut profiles) = context.semantic_profiles.lock() {
                                match previous {
                                    Some(profile) => {
                                        profiles.insert(controller_id, profile);
                                    }
                                    None => {
                                        profiles.remove(&controller_id);
                                    }
                                }
                            }
                            return failure.into_response();
                        }
                    };
                    let (links_sender, links_receiver) = sync_channel(1);
                    if let Err(failure) = send_audio(
                        context,
                        AudioControlCommand::ReplaceParameterLinks {
                            links: compiled,
                            reply: links_sender,
                        },
                    ) {
                        return failure.into_response();
                    }
                    match receive_audio(links_receiver, "apply semantic parameter links") {
                        Ok(()) => command_applied_without_events(context, command_ref),
                        Err(failure) => failure.into_response(),
                    }
                }
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::SetMasterLevel { level } => {
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::SetMasterLevel {
                    level,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "apply master level") {
                Ok(()) => record_command_event(
                    context,
                    command_ref,
                    SessionEvent::MasterLevelChanged { level },
                ),
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::SetMasterPan { pan } => {
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::SetMasterPan {
                    pan,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "apply master pan") {
                Ok(()) => record_command_event(
                    context,
                    command_ref,
                    SessionEvent::MasterPanChanged { pan },
                ),
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::SetActiveMode { mode } => {
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::SetRenderMode {
                    mode,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "change render mode") {
                Ok(()) => {
                    let mut events = vec![SessionEvent::ActiveModeChanged { mode }];
                    if mode == rackforge_session_api::SurfaceMode::Play
                        && snapshot.live.active.is_some()
                    {
                        let mut live = snapshot.live.clone();
                        live.deactivate();
                        events.push(SessionEvent::LiveStateReconciled { live });
                    }
                    record_command_events(context, command_ref, events)
                }
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::EmergencyStop => {
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::EmergencyStop {
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            if let Err(failure) = receive_audio(reply_receiver, "perform emergency stop") {
                return failure.into_response();
            }

            set_lease_deadline(context, None);
            let mut events = Vec::with_capacity(3);
            if let Some(draft) = snapshot.program_draft.as_ref() {
                events.push(SessionEvent::ProgramEditCancelled {
                    draft_id: draft.draft_id,
                    instance_id: draft.instance_id.clone(),
                });
            }
            if let Some(audition) = snapshot.audition.as_ref() {
                events.push(SessionEvent::AuditionEnded {
                    lease_id: audition.lease_id,
                    instance_id: audition.instance_id.clone(),
                    restored_sound_id: None,
                    reason: AuditionEndReason::Cancelled,
                });
            }
            events.push(SessionEvent::ActiveModeChanged {
                mode: rackforge_session_api::SurfaceMode::Idle,
            });
            record_command_events(context, command_ref, events)
        }
        SessionCommand::SelectPlugin { instance_id } => {
            if snapshot.instance(&instance_id).is_none() {
                return error_response(
                    ControlErrorCode::NotFound,
                    format!("unknown instance {instance_id}"),
                    Some(snapshot.revision),
                );
            }
            if snapshot.audition.is_some() || snapshot.program_draft.is_some() {
                return error_response(
                    ControlErrorCode::Conflict,
                    "finish or cancel the active plugin edit before changing plugins",
                    Some(snapshot.revision),
                );
            }
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::SelectPlugin {
                    instance_id: instance_id.clone(),
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "select plugin") {
                Ok(()) => record_command_event(
                    context,
                    command_ref,
                    SessionEvent::ActiveInstanceChanged { instance_id },
                ),
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::SetLiveBrowseMode { mode } => record_command_event(
            context,
            command_ref,
            SessionEvent::LiveBrowseModeChanged { mode },
        ),
        SessionCommand::ActivateLiveTarget { location } => {
            if snapshot.active_mode != rackforge_session_api::SurfaceMode::Live {
                return error_response(
                    ControlErrorCode::Rejected,
                    "LIVE targets can only be activated while LIVE is active",
                    Some(snapshot.revision),
                );
            }
            let (library, rack) = match context.performance_repository.lock() {
                Ok(repository) => match repository.library().resolve_playable(&location) {
                    Ok(rack) if rack.enabled => (repository.library().clone(), rack),
                    Ok(_) => {
                        return error_response(
                            ControlErrorCode::Rejected,
                            "the selected Rack is disabled",
                            Some(snapshot.revision),
                        );
                    }
                    Err(error) => {
                        return error_response(
                            ControlErrorCode::NotFound,
                            error.to_string(),
                            Some(snapshot.revision),
                        );
                    }
                },
                Err(_) => {
                    return internal_error(
                        "performance repository lock is poisoned",
                        Some(snapshot.revision),
                    );
                }
            };
            let rack_id = rack.id.clone();
            let (instance_id, slots) = match prepare_rack_definition_runtime(
                &snapshot,
                &library,
                &rack,
                &context.state_store,
            ) {
                Ok(runtime) => runtime,
                Err(failure) => return failure.into_response(),
            };
            let prepared_slots =
                match prepare_portable_rack_slots(context, snapshot.revision, &slots) {
                    Ok(prepared) => prepared,
                    Err(failure) => return failure.into_response(),
                };
            let slots = if prepared_slots.is_some() {
                Vec::new()
            } else {
                slots
            };
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::ActivateRack {
                    rack_id: rack_id.as_str().to_owned(),
                    instance_id: instance_id.clone(),
                    slots,
                    prepared_slots,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "activate LIVE Rack") {
                Ok(()) => {
                    // The Part carries its groove on stage with it: queue its
                    // bound patterns, each on its lane, all on the next bar.
                    // A lane the Part does not bind is left alone, and a
                    // binding that fails must never fail the activation —
                    // the Rack is already sounding.
                    if let Some(part) = library.resolve_part(&location) {
                        for command in
                            crate::sequencer::part_launch_commands(part, &library.patterns)
                        {
                            let (reply_sender, reply_receiver) = sync_channel(1);
                            if send_audio(
                                context,
                                AudioControlCommand::Sequencer {
                                    command,
                                    reply: reply_sender,
                                },
                            )
                            .is_err()
                            {
                                eprintln!("SEQUENCER_PART_QUEUE_UNREACHABLE");
                                break;
                            }
                            if receive_audio(reply_receiver, "queue Part pattern").is_err() {
                                eprintln!("SEQUENCER_PART_QUEUE_REJECTED");
                            }
                        }
                    }
                    record_command_event(
                        context,
                        command_ref,
                        SessionEvent::LiveTargetActivated { location, rack_id },
                    )
                }
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::PreviewRack { rack } => {
            let graph = rack.resolved_graph();
            let graph_plugins = graph
                .nodes
                .iter()
                .filter(|node| {
                    matches!(
                        node.kind,
                        rackforge_performance_api::RackGraphNodeKind::Plugin { .. }
                    )
                })
                .count();
            let graph_children = graph
                .nodes
                .iter()
                .filter(|node| {
                    matches!(
                        node.kind,
                        rackforge_performance_api::RackGraphNodeKind::Rack { .. }
                    )
                })
                .count();
            println!(
                "LIVE_RACK_PREVIEW_REQUESTED rack={} slots={} graph_plugins={} graph_children={}",
                rack.id,
                rack.slots.len(),
                graph_plugins,
                graph_children
            );
            if rack.slots.is_empty() && graph_children == 0 {
                eprintln!(
                    "LIVE_RACK_PREVIEW_REJECTED rack={} error=no-playable-voices",
                    rack.id
                );
                return error_response(
                    ControlErrorCode::Rejected,
                    "a Rack preview needs at least one playable instrument or child Rack",
                    Some(snapshot.revision),
                );
            }
            let mut library = match context.performance_repository.lock() {
                Ok(repository) => repository.library().clone(),
                Err(_) => {
                    return internal_error(
                        "performance repository lock is poisoned",
                        Some(snapshot.revision),
                    );
                }
            };
            if let Some(index) = library
                .racks
                .iter()
                .position(|candidate| candidate.id == rack.id)
            {
                library.racks[index] = rack.clone();
            } else {
                library.racks.push(rack.clone());
            }
            let runtime = prepare_rack_runtime(&snapshot, &library, &rack.id, &context.state_store);
            let (instance_id, slots) = match runtime {
                Ok(runtime) => runtime,
                Err(failure) => {
                    eprintln!(
                        "LIVE_RACK_PREVIEW_REJECTED rack={} error={}",
                        rack.id, failure.message
                    );
                    return failure.into_response();
                }
            };
            let rack_id = rack.id.as_str().to_owned();
            let prepared_slots =
                match prepare_portable_rack_slots(context, snapshot.revision, &slots) {
                    Ok(prepared) => prepared,
                    Err(failure) => return failure.into_response(),
                };
            let slots = if prepared_slots.is_some() {
                Vec::new()
            } else {
                slots
            };
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::ActivateRack {
                    rack_id,
                    instance_id,
                    slots,
                    prepared_slots,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "preview Rack") {
                Ok(()) => command_applied_without_events(context, command_ref),
                Err(failure) => {
                    eprintln!(
                        "LIVE_RACK_PREVIEW_FAILED rack={} error={}",
                        rack.id, failure.message
                    );
                    failure.into_response()
                }
            }
        }
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
            if snapshot.program_draft.as_ref().is_some_and(|draft| {
                snapshot.audition.as_ref().is_some_and(|audition| {
                    audition.lease_id == lease_id && audition.instance_id == draft.instance_id
                })
            }) {
                return error_response(
                    ControlErrorCode::Conflict,
                    "program editing must be saved or cancelled before releasing audition focus",
                    Some(snapshot.revision),
                );
            }
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
        SessionCommand::BeginProgramEdit {
            instance_id,
            program_id,
        } => {
            let instance = match require_active_instance(&snapshot, &instance_id) {
                Ok(instance) => instance,
                Err(failure) => return failure.into_response(),
            };
            if snapshot.audition.is_some() || snapshot.program_draft.is_some() {
                return error_response(
                    ControlErrorCode::Conflict,
                    "another audition or program edit is already active",
                    Some(snapshot.revision),
                );
            }
            let previous_sound_id = instance.selected_sound_id.clone();
            let request = ProgramEditRequest::new(program_id.clone());
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::BeginProgramEdit {
                    instance_id: instance_id.clone(),
                    request,
                    previous_sound_id: previous_sound_id.clone(),
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "begin program editing") {
                Ok((lease_id, prepared, editor)) => {
                    if prepared.document.plugin_id != instance.plugin_id {
                        return internal_error(
                            "plugin prepared a program for a different plugin",
                            Some(snapshot.revision),
                        );
                    }
                    let draft_id = context.next_draft_id.fetch_add(1, Ordering::Relaxed).max(1);
                    let draft = match program_draft_state(
                        draft_id,
                        instance_id.clone(),
                        program_id,
                        &prepared,
                        editor,
                        false,
                    ) {
                        Ok(draft) => draft,
                        Err(response) => return response,
                    };
                    let response = record_command_events(
                        context,
                        command_ref,
                        vec![
                            SessionEvent::AuditionStarted {
                                lease_id,
                                instance_id,
                                previous_sound_id,
                            },
                            SessionEvent::ProgramEditStarted { draft },
                        ],
                    );
                    if matches!(response, ControlResponse::CommandApplied { .. }) {
                        set_lease_deadline(context, Some(lease_id));
                    }
                    response
                }
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::ReplaceProgramDraft {
            draft_id,
            document_json,
        } => {
            let draft = match snapshot
                .program_draft
                .as_ref()
                .filter(|draft| draft.draft_id == draft_id)
            {
                Some(draft) => draft,
                None => {
                    return error_response(
                        ControlErrorCode::NotFound,
                        "program draft is missing or no longer valid",
                        Some(snapshot.revision),
                    );
                }
            };
            let audition = match snapshot
                .audition
                .as_ref()
                .filter(|audition| audition.instance_id == draft.instance_id)
            {
                Some(audition) => audition,
                None => {
                    return internal_error(
                        "program draft lost its audition lease",
                        Some(snapshot.revision),
                    );
                }
            };
            let document: ProgramDocument = match serde_json::from_str(&document_json) {
                Ok(document) => document,
                Err(error) => {
                    return error_response(
                        ControlErrorCode::InvalidRequest,
                        format!("invalid program document JSON: {error}"),
                        Some(snapshot.revision),
                    );
                }
            };
            let previous_document: ProgramDocument =
                match serde_json::from_str(&draft.document_json) {
                    Ok(document) => document,
                    Err(error) => {
                        return internal_error(
                            format!("stored program draft is invalid: {error}"),
                            Some(snapshot.revision),
                        );
                    }
                };
            if document.id != previous_document.id
                || document.plugin_id != previous_document.plugin_id
            {
                return error_response(
                    ControlErrorCode::Rejected,
                    "program identity cannot change while editing",
                    Some(snapshot.revision),
                );
            }
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::ReplaceProgramDraft {
                    instance_id: draft.instance_id.clone(),
                    document,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "validate program draft") {
                Ok((prepared, editor)) => {
                    let updated = match program_draft_state(
                        draft_id,
                        draft.instance_id.clone(),
                        draft.original_program_id.clone(),
                        &prepared,
                        editor,
                        true,
                    ) {
                        Ok(draft) => draft,
                        Err(response) => return response,
                    };
                    set_lease_deadline(context, Some(audition.lease_id));
                    record_command_events(
                        context,
                        command_ref,
                        vec![
                            SessionEvent::ProgramDraftUpdated {
                                draft: updated.clone(),
                            },
                            SessionEvent::SoundSelected {
                                instance_id: draft.instance_id.clone(),
                                sound_id: updated.preview_sound_id,
                            },
                        ],
                    )
                }
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::PreviewProgramDraft {
            draft_id,
            document_json,
        } => {
            let draft = match snapshot
                .program_draft
                .as_ref()
                .filter(|draft| draft.draft_id == draft_id)
            {
                Some(draft) => draft,
                None => {
                    return error_response(
                        ControlErrorCode::NotFound,
                        "program draft is missing or no longer valid",
                        Some(snapshot.revision),
                    );
                }
            };
            let audition = match snapshot
                .audition
                .as_ref()
                .filter(|audition| audition.instance_id == draft.instance_id)
            {
                Some(audition) => audition,
                None => {
                    return internal_error(
                        "program draft lost its audition lease",
                        Some(snapshot.revision),
                    );
                }
            };
            let document: ProgramDocument = match serde_json::from_str(&document_json) {
                Ok(document) => document,
                Err(error) => {
                    return error_response(
                        ControlErrorCode::InvalidRequest,
                        format!("invalid preview program document JSON: {error}"),
                        Some(snapshot.revision),
                    );
                }
            };
            let confirmed: ProgramDocument = match serde_json::from_str(&draft.document_json) {
                Ok(document) => document,
                Err(error) => {
                    return internal_error(
                        format!("stored program draft is invalid: {error}"),
                        Some(snapshot.revision),
                    );
                }
            };
            if document.id != confirmed.id || document.plugin_id != confirmed.plugin_id {
                return error_response(
                    ControlErrorCode::Rejected,
                    "program identity cannot change during preview",
                    Some(snapshot.revision),
                );
            }
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::ReplaceProgramDraft {
                    instance_id: draft.instance_id.clone(),
                    document,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "preview program draft") {
                Ok(_) => {
                    set_lease_deadline(context, Some(audition.lease_id));
                    command_applied_without_events(context, command_ref)
                }
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::EditProgramDraftField {
            draft_id,
            field_id,
            value,
            preview,
        } => {
            let draft = match snapshot
                .program_draft
                .as_ref()
                .filter(|draft| draft.draft_id == draft_id)
            {
                Some(draft) => draft,
                None => {
                    return error_response(
                        ControlErrorCode::NotFound,
                        "program draft is missing or no longer valid",
                        Some(snapshot.revision),
                    );
                }
            };
            let audition = match snapshot
                .audition
                .as_ref()
                .filter(|audition| audition.instance_id == draft.instance_id)
            {
                Some(audition) => audition,
                None => {
                    return internal_error(
                        "program draft lost its audition lease",
                        Some(snapshot.revision),
                    );
                }
            };
            let document: ProgramDocument = match serde_json::from_str(&draft.document_json) {
                Ok(document) => document,
                Err(error) => {
                    return internal_error(
                        format!("stored program draft is invalid: {error}"),
                        Some(snapshot.revision),
                    );
                }
            };
            let request = ProgramFieldEditRequest {
                schema_version: rackforge_plugin_api::PROGRAM_EDITOR_SCHEMA_VERSION,
                document,
                field_id,
                value,
            };
            if let Err(error) = request.validate() {
                return error_response(
                    ControlErrorCode::InvalidRequest,
                    error.to_string(),
                    Some(snapshot.revision),
                );
            }
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::EditProgramDraftField {
                    instance_id: draft.instance_id.clone(),
                    request,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "apply program field edit") {
                Ok((_prepared, _editor)) if preview => {
                    set_lease_deadline(context, Some(audition.lease_id));
                    command_applied_without_events(context, command_ref)
                }
                Ok((prepared, editor)) => {
                    let updated = match program_draft_state(
                        draft_id,
                        draft.instance_id.clone(),
                        draft.original_program_id.clone(),
                        &prepared,
                        editor,
                        true,
                    ) {
                        Ok(draft) => draft,
                        Err(response) => return response,
                    };
                    set_lease_deadline(context, Some(audition.lease_id));
                    record_command_events(
                        context,
                        command_ref,
                        vec![
                            SessionEvent::ProgramDraftUpdated {
                                draft: updated.clone(),
                            },
                            SessionEvent::SoundSelected {
                                instance_id: draft.instance_id.clone(),
                                sound_id: updated.preview_sound_id,
                            },
                        ],
                    )
                }
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::RestoreProgramDraftPreview { draft_id } => {
            let draft = match snapshot
                .program_draft
                .as_ref()
                .filter(|draft| draft.draft_id == draft_id)
            {
                Some(draft) => draft,
                None => {
                    return error_response(
                        ControlErrorCode::NotFound,
                        "program draft is missing or no longer valid",
                        Some(snapshot.revision),
                    );
                }
            };
            let audition = match snapshot
                .audition
                .as_ref()
                .filter(|audition| audition.instance_id == draft.instance_id)
            {
                Some(audition) => audition,
                None => {
                    return internal_error(
                        "program draft lost its audition lease",
                        Some(snapshot.revision),
                    );
                }
            };
            let document: ProgramDocument = match serde_json::from_str(&draft.document_json) {
                Ok(document) => document,
                Err(error) => {
                    return internal_error(
                        format!("stored program draft is invalid: {error}"),
                        Some(snapshot.revision),
                    );
                }
            };
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::ReplaceProgramDraft {
                    instance_id: draft.instance_id.clone(),
                    document,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "restore confirmed program draft preview") {
                Ok(_) => {
                    set_lease_deadline(context, Some(audition.lease_id));
                    command_applied_without_events(context, command_ref)
                }
                Err(failure) => failure.into_response(),
            }
        }
        SessionCommand::SaveProgramDraft { draft_id } => {
            let draft = match snapshot
                .program_draft
                .as_ref()
                .filter(|draft| draft.draft_id == draft_id)
            {
                Some(draft) => draft,
                None => {
                    return error_response(
                        ControlErrorCode::NotFound,
                        "program draft is missing or no longer valid",
                        Some(snapshot.revision),
                    );
                }
            };
            let audition = match snapshot
                .audition
                .as_ref()
                .filter(|audition| audition.instance_id == draft.instance_id)
            {
                Some(audition) => audition,
                None => {
                    return internal_error(
                        "program draft lost its audition lease",
                        Some(snapshot.revision),
                    );
                }
            };
            let document: ProgramDocument = match serde_json::from_str(&draft.document_json) {
                Ok(document) => document,
                Err(error) => {
                    return internal_error(
                        format!("stored program draft is invalid: {error}"),
                        Some(snapshot.revision),
                    );
                }
            };
            let prepared = PreparedProgram {
                schema_version: rackforge_plugin_api::PROGRAM_EDIT_SCHEMA_VERSION,
                storage_path: draft.storage_path.clone(),
                preview_sound_id: draft.preview_sound_id.clone(),
                document,
                artifacts: draft.artifacts.clone(),
            };
            if let Err(error) = prepared.validate() {
                return internal_error(
                    format!("stored prepared program is invalid: {error}"),
                    Some(snapshot.revision),
                );
            }
            let Some(storage) = &context.storage else {
                return error_response(
                    ControlErrorCode::Unavailable,
                    "RackForge data storage is not configured",
                    Some(snapshot.revision),
                );
            };
            if let Err(error) = storage.save_prepared_program(&prepared) {
                return error_response(
                    ControlErrorCode::Rejected,
                    format!("saving program: {error:#}"),
                    Some(snapshot.revision),
                );
            }
            let (install_sender, install_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::InstallProgram {
                    instance_id: draft.instance_id.clone(),
                    prepared: prepared.clone(),
                    reply: install_sender,
                },
            ) {
                return failure.into_response();
            }
            let catalog = match receive_audio(install_receiver, "install saved program") {
                Ok(catalog) => catalog,
                Err(failure) => return failure.into_response(),
            };
            let preset_id = format!("custom.{}", prepared.document.id);
            let Some(preset) = catalog.presets.iter().find(|preset| preset.id == preset_id) else {
                return internal_error(
                    "installed program is missing from the plugin catalog",
                    Some(snapshot.revision),
                );
            };
            let (end_sender, end_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::EndAudition {
                    lease_id: audition.lease_id,
                    reply: end_sender,
                },
            ) {
                return failure.into_response();
            }
            if let Err(failure) = receive_audio(end_receiver, "release program edit focus") {
                return failure.into_response();
            }
            set_lease_deadline(context, None);
            record_command_events(
                context,
                command_ref,
                vec![
                    SessionEvent::ProgramSaved {
                        draft_id,
                        instance_id: draft.instance_id.clone(),
                        sound: SoundSummary {
                            id: preset.id.clone(),
                            name: preset.name.clone(),
                            bank: preset.bank.clone(),
                            detail: preset
                                .description
                                .clone()
                                .or_else(|| preset.category.clone()),
                            category: None,
                            tags: Vec::new(),
                            editable: preset.editable,
                        },
                    },
                    SessionEvent::AuditionEnded {
                        lease_id: audition.lease_id,
                        instance_id: audition.instance_id.clone(),
                        restored_sound_id: audition.previous_sound_id.clone(),
                        reason: AuditionEndReason::Released,
                    },
                ],
            )
        }
        SessionCommand::CancelProgramEdit { draft_id } => {
            let draft = match snapshot
                .program_draft
                .as_ref()
                .filter(|draft| draft.draft_id == draft_id)
            {
                Some(draft) => draft,
                None => {
                    return error_response(
                        ControlErrorCode::NotFound,
                        "program draft is missing or no longer valid",
                        Some(snapshot.revision),
                    );
                }
            };
            let audition = match snapshot
                .audition
                .as_ref()
                .filter(|audition| audition.instance_id == draft.instance_id)
            {
                Some(audition) => audition,
                None => {
                    return internal_error(
                        "program draft lost its audition lease",
                        Some(snapshot.revision),
                    );
                }
            };
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::EndAudition {
                    lease_id: audition.lease_id,
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            if let Err(failure) = receive_audio(reply_receiver, "cancel program editing") {
                return failure.into_response();
            }
            set_lease_deadline(context, None);
            record_command_events(
                context,
                command_ref,
                vec![
                    SessionEvent::ProgramEditCancelled {
                        draft_id,
                        instance_id: draft.instance_id.clone(),
                    },
                    SessionEvent::AuditionEnded {
                        lease_id: audition.lease_id,
                        instance_id: audition.instance_id.clone(),
                        restored_sound_id: audition.previous_sound_id.clone(),
                        reason: AuditionEndReason::Cancelled,
                    },
                ],
            )
        }
        SessionCommand::ActivateSurface {
            instance_id,
            request,
        } => {
            let instance = match require_active_instance(&snapshot, &instance_id) {
                Ok(instance) => instance,
                Err(failure) => return failure.into_response(),
            };
            if let Err(error) = request.validate() {
                return error_response(
                    ControlErrorCode::InvalidRequest,
                    error.to_string(),
                    Some(snapshot.revision),
                );
            }
            if !instance
                .ui_layouts
                .iter()
                .any(|layout| layout == &request.layout_id)
            {
                return error_response(
                    ControlErrorCode::Rejected,
                    format!(
                        "plugin {} does not expose layout {}",
                        instance.plugin_id, request.layout_id
                    ),
                    Some(snapshot.revision),
                );
            }
            let (reply_sender, reply_receiver) = sync_channel(1);
            if let Err(failure) = send_audio(
                context,
                AudioControlCommand::ActivateSurface {
                    instance_id: instance_id.clone(),
                    request: request.clone(),
                    reply: reply_sender,
                },
            ) {
                return failure.into_response();
            }
            match receive_audio(reply_receiver, "activate plugin surface") {
                Ok(response) => record_command_event(
                    context,
                    command_ref,
                    SessionEvent::SurfaceActivated {
                        instance_id,
                        request,
                        response,
                    },
                ),
                Err(failure) => failure.into_response(),
            }
        }
    }
}

fn require_active_instance<'a>(
    snapshot: &'a rackforge_session_api::SessionState,
    instance_id: &InstanceId,
) -> Result<&'a rackforge_session_api::PluginInstanceState, ControlFailure> {
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

fn prepare_rack_runtime(
    snapshot: &rackforge_session_api::SessionState,
    library: &rackforge_performance_api::PerformanceLibrary,
    rack_id: &RackId,
    state_store: &Arc<Mutex<PluginStateStore>>,
) -> Result<(InstanceId, Vec<RackSlotRuntimeSpec>), ControlFailure> {
    let compiled_slots = compile_instrument_rack(library, rack_id).map_err(|error| {
        control_failure(
            ControlErrorCode::Rejected,
            format!("Rack {rack_id} cannot run: {error}"),
            Some(snapshot.revision),
        )
    })?;
    prepare_compiled_rack_runtime(snapshot, rack_id.as_str(), compiled_slots, state_store)
}

fn prepare_portable_rack_slots(
    context: &ControlContext,
    revision: Revision,
    specs: &[RackSlotRuntimeSpec],
) -> Result<Option<Vec<PreparedRackSlot>>, ControlFailure> {
    if specs
        .iter()
        .any(|spec| !context.portable_plugins.contains_key(&spec.plugin_id))
    {
        // Native plugin instances may have thread-affinity requirements. Keep
        // their existing audio-thread construction path until the native ABI
        // explicitly guarantees transferable instances.
        return Ok(None);
    }

    let mut prepared = Vec::with_capacity(specs.len());
    let mut warmup_output = vec![
        0.0_f32;
        context.plugin_maximum_frames as usize
            * context.plugin_output_channels as usize
    ];
    for spec in specs {
        let plugin = context.portable_plugins[&spec.plugin_id].0;
        let audio = plugin.manifest().resolved_audio_contract();
        let input_channels = audio.input_channels() as usize;
        if input_channels > rackforge_audio_api::MAX_ACTIVE_INPUT_CHANNELS
            || audio.output_channels() != context.plugin_output_channels
        {
            return Err(control_failure(
                ControlErrorCode::Rejected,
                format!(
                    "Rack Slot {} has unsupported audio layout: {} input / {} output channels",
                    spec.slot_id,
                    input_channels,
                    audio.output_channels()
                ),
                Some(revision),
            ));
        }
        let mut instance = plugin.create_instance().map_err(|error| {
            control_failure(
                ControlErrorCode::Internal,
                format!("preparing Rack Slot {}: {error:#}", spec.slot_id),
                Some(revision),
            )
        })?;
        match &spec.state {
            RackSlotStateLoad::Default => {}
            RackSlotStateLoad::Opaque(bytes) => instance.load_state(bytes).map_err(|error| {
                control_failure(
                    ControlErrorCode::Rejected,
                    format!("restoring Rack Slot {} state: {error:#}", spec.slot_id),
                    Some(revision),
                )
            })?,
            RackSlotStateLoad::LegacyPreset(preset_id) => {
                instance.load_preset(preset_id).map_err(|error| {
                    control_failure(
                        ControlErrorCode::Rejected,
                        format!(
                            "loading legacy program {preset_id:?} for Rack Slot {}: {error:#}",
                            spec.slot_id
                        ),
                        Some(revision),
                    )
                })?;
            }
        }
        instance
            .activate(
                context.plugin_sample_rate,
                context.plugin_maximum_frames,
                input_channels as u32,
                context.plugin_output_channels,
            )
            .map_err(|error| {
                control_failure(
                    ControlErrorCode::Rejected,
                    format!("activating Rack Slot {}: {error:#}", spec.slot_id),
                    Some(revision),
                )
            })?;

        // Touch code/data pages and plugin-owned buffers before ownership is
        // transferred to the deadline-bound audio loop. The silent block is
        // intentionally discarded; it prevents first-use work from becoming
        // an audible graph-transition XRUN.
        warmup_output.fill(0.0);
        let warmup_input = vec![0.0_f32; context.plugin_maximum_frames as usize * input_channels];
        instance
            .process_interleaved(
                &warmup_input,
                &mut warmup_output,
                context.plugin_maximum_frames,
                input_channels as u32,
                context.plugin_output_channels,
                &[],
                &[],
            )
            .map_err(|error| {
                control_failure(
                    ControlErrorCode::Rejected,
                    format!("warming Rack Slot {}: {error:#}", spec.slot_id),
                    Some(revision),
                )
            })?;
        let parallel =
            prepare_rack_slot_parallel_units(context, revision, spec, plugin, &mut instance)?;
        prepared.push(PreparedRackSlot {
            slot_id: spec.slot_id.clone(),
            midi_stages: spec.midi_stages.clone(),
            audio_sources: spec.audio_sources.clone(),
            sends_to_main: spec.sends_to_main,
            level_per_mille: spec.level_per_mille,
            pan_per_mille: spec.pan_per_mille,
            plugin,
            instance: PreparedPluginInstance(instance),
            parallel,
            input_channels,
            input: vec![0.0; context.plugin_maximum_frames as usize * input_channels],
            output: vec![
                0.0;
                context.plugin_maximum_frames as usize
                    * context.plugin_output_channels as usize
            ],
            events: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
            parameter_events: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
        });
    }
    Ok(Some(prepared))
}

/// Builds and warms the host-owned unit instances for one prepared Slot of a
/// `parallel_render_v1` plugin, mirroring the Slot's state load so control
/// state agrees across the coordinator and every unit.
fn prepare_rack_slot_parallel_units(
    context: &ControlContext,
    revision: Revision,
    spec: &RackSlotRuntimeSpec,
    plugin: &'static LoadedPlugin,
    coordinator: &mut PluginInstance<'static>,
) -> Result<Option<PreparedParallelUnits>, ControlFailure> {
    if !crate::parallel_render::parallel_units_enabled() || plugin.parallel_layout().is_none() {
        return Ok(None);
    }
    let failure = |error: anyhow::Error| {
        control_failure(
            ControlErrorCode::Internal,
            format!(
                "preparing parallel units for Rack Slot {}: {error:#}",
                spec.slot_id
            ),
            Some(revision),
        )
    };
    let audio = plugin.manifest().resolved_audio_contract();
    let mut units = crate::parallel_render::ParallelUnits::create(
        plugin,
        context.plugin_sample_rate,
        context.plugin_maximum_frames,
        audio.input_channels(),
        context.plugin_output_channels,
    )
    .map_err(failure)?
    .expect("parallel layout was just observed");
    match &spec.state {
        RackSlotStateLoad::Default => {}
        RackSlotStateLoad::Opaque(bytes) => units
            .mirror(|instance| instance.load_state(bytes))
            .map_err(failure)?,
        RackSlotStateLoad::LegacyPreset(preset_id) => units
            .mirror(|instance| instance.load_preset(preset_id))
            .map_err(failure)?,
    }
    units.warmup(coordinator).map_err(failure)?;
    Ok(Some(PreparedParallelUnits(units)))
}

fn prepare_compiled_rack_runtime(
    snapshot: &rackforge_session_api::SessionState,
    rack_id: &str,
    compiled_slots: Vec<CompiledRackSlot>,
    state_store: &Arc<Mutex<PluginStateStore>>,
) -> Result<(InstanceId, Vec<RackSlotRuntimeSpec>), ControlFailure> {
    if compiled_slots.len() > MAX_ACTIVE_RACK_SLOTS {
        return Err(control_failure(
            ControlErrorCode::Rejected,
            format!(
                "Rack {rack_id} compiles to {} Slots; this engine supports at most {MAX_ACTIVE_RACK_SLOTS}",
                compiled_slots.len()
            ),
            Some(snapshot.revision),
        ));
    }
    let active_instance = snapshot.active_instance().ok_or_else(|| {
        control_failure(
            ControlErrorCode::Unavailable,
            "the LIVE session has no active plugin instance",
            Some(snapshot.revision),
        )
    })?;
    let mut specs = Vec::with_capacity(compiled_slots.len());
    for compiled in compiled_slots {
        let slot = &compiled.slot;
        let instance = snapshot
            .instances
            .iter()
            .find(|instance| instance.plugin_id == slot.plugin_id)
            .ok_or_else(|| {
                control_failure(
                    ControlErrorCode::Unavailable,
                    format!(
                        "Rack Slot {} requires unavailable plugin {}",
                        slot.id, slot.plugin_id
                    ),
                    Some(snapshot.revision),
                )
            })?;
        let state = if let Some(reference) = &slot.state {
            let store = state_store.lock().map_err(|_| {
                control_failure(
                    ControlErrorCode::Internal,
                    "plugin state store lock is poisoned",
                    Some(snapshot.revision),
                )
            })?;
            RackSlotStateLoad::Opaque(store.read(reference).map_err(|error| {
                control_failure(
                    ControlErrorCode::NotFound,
                    format!("loading state for Rack Slot {}: {error:#}", slot.id),
                    Some(snapshot.revision),
                )
            })?)
        } else if let Some(program_id) = &slot.legacy_program_id {
            if !instance.sounds.iter().any(|sound| sound.id == *program_id) {
                return Err(control_failure(
                    ControlErrorCode::NotFound,
                    format!(
                        "Rack Slot {} references missing legacy program {:?}",
                        slot.id, program_id
                    ),
                    Some(snapshot.revision),
                ));
            }
            RackSlotStateLoad::LegacyPreset(program_id.clone())
        } else {
            RackSlotStateLoad::Default
        };
        specs.push(RackSlotRuntimeSpec {
            slot_id: compiled.runtime_slot_id,
            plugin_id: slot.plugin_id.clone(),
            state,
            midi_stages: compiled
                .midi_stages
                .iter()
                .map(|stage| RackMidiStageRuntimeSpec {
                    transform: stage.transform.clone(),
                    keyboard_parts: stage.keyboard_parts,
                })
                .collect(),
            audio_sources: compiled.audio_sources.clone(),
            sends_to_main: compiled.sends_to_main,
            level_per_mille: slot.level_per_mille,
            pan_per_mille: slot.pan_per_mille,
        });
    }
    Ok((active_instance.instance_id.clone(), specs))
}

fn prepare_rack_definition_runtime(
    snapshot: &rackforge_session_api::SessionState,
    library: &rackforge_performance_api::PerformanceLibrary,
    rack: &RackDefinition,
    state_store: &Arc<Mutex<PluginStateStore>>,
) -> Result<(InstanceId, Vec<RackSlotRuntimeSpec>), ControlFailure> {
    let compiled_slots = compile_instrument_definition(library, rack).map_err(|error| {
        control_failure(
            ControlErrorCode::Rejected,
            format!("Rack {} cannot run: {error}", rack.id),
            Some(snapshot.revision),
        )
    })?;
    prepare_compiled_rack_runtime(snapshot, rack.id.as_str(), compiled_slots, state_store)
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
    receive_audio_with_timeout(receiver, action, AUDIO_COMMAND_TIMEOUT)
}

fn receive_audio_with_timeout<T>(
    receiver: Receiver<Result<T, String>>,
    action: &str,
    timeout: Duration,
) -> Result<T, ControlFailure> {
    match receiver.recv_timeout(timeout) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(message)) => Err(control_failure(ControlErrorCode::Rejected, message, None)),
        Err(_) => Err(control_failure(
            ControlErrorCode::Timeout,
            format!("LIVE did not {action} in time"),
            None,
        )),
    }
}

fn program_draft_state(
    draft_id: u64,
    instance_id: InstanceId,
    original_program_id: Option<String>,
    prepared: &PreparedProgram,
    editor: ProgramEditorView,
    dirty: bool,
) -> Result<ProgramDraftState, ControlResponse> {
    if let Err(error) = prepared.validate() {
        return Err(internal_error(
            format!("plugin returned an invalid prepared program: {error}"),
            None,
        ));
    }
    if let Err(error) = editor.validate() {
        return Err(internal_error(
            format!("plugin returned an invalid program editor: {error}"),
            None,
        ));
    }
    let document_json = serde_json::to_string(&prepared.document).map_err(|error| {
        internal_error(
            format!("serializing prepared program document: {error}"),
            None,
        )
    })?;
    Ok(ProgramDraftState {
        draft_id,
        instance_id,
        original_program_id,
        name: prepared.document.name.clone(),
        preview_sound_id: prepared.preview_sound_id.clone(),
        storage_path: prepared.storage_path.clone(),
        artifacts: prepared.artifacts.clone(),
        document_json,
        editor,
        dirty,
    })
}

fn record_command_event(
    context: &ControlContext,
    command: CommandRef,
    event: SessionEvent,
) -> ControlResponse {
    record_command_events(context, command, vec![event])
}

fn command_applied_without_events(
    context: &ControlContext,
    command: CommandRef,
) -> ControlResponse {
    match context.store.lock() {
        Ok(store) => ControlResponse::CommandApplied {
            client_id: command.client_id,
            command_id: command.command_id,
            revision: store.state().revision,
            events: Vec::new(),
        },
        Err(_) => internal_error("session store lock is poisoned", None),
    }
}

fn record_command_events(
    context: &ControlContext,
    command: CommandRef,
    events: Vec<SessionEvent>,
) -> ControlResponse {
    let should_checkpoint = events.iter().any(|event| {
        matches!(
            event,
            SessionEvent::MasterLevelChanged { .. }
                | SessionEvent::MasterPanChanged { .. }
                | SessionEvent::ActiveModeChanged { .. }
                | SessionEvent::ActiveInstanceChanged { .. }
                | SessionEvent::LiveBrowseModeChanged { .. }
                | SessionEvent::LiveTargetActivated { .. }
                | SessionEvent::LiveStateReconciled { .. }
                | SessionEvent::SoundSelected { .. }
                | SessionEvent::ProgramSaved { .. }
                | SessionEvent::ParameterLinkUpserted { .. }
                | SessionEvent::ParameterLinkRemoved { .. }
        )
    });
    let (events, revision, checkpoint_state) = match context.store.lock() {
        Ok(mut store) => match store.record_many(Some(command.clone()), events) {
            Ok(events) => {
                let revision = events
                    .last()
                    .map(|event| event.revision)
                    .unwrap_or(store.state().revision);
                let checkpoint_state = should_checkpoint.then(|| store.snapshot());
                (events, revision, checkpoint_state)
            }
            Err(error) => {
                return internal_error(error.to_string(), Some(store.state().revision));
            }
        },
        Err(_) => return internal_error("session store lock is poisoned", None),
    };
    if let (Some(checkpoint), Some(state)) = (&context.checkpoint, checkpoint_state.as_ref())
        && let Err(error) = checkpoint.save(state)
    {
        eprintln!("SESSION_CHECKPOINT_ERROR {error:#}");
    }
    ControlResponse::CommandApplied {
        client_id: command.client_id,
        command_id: command.command_id,
        revision,
        events,
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
            Ok(mut store) => {
                let mut events = Vec::new();
                if let Some(draft) = snapshot
                    .program_draft
                    .as_ref()
                    .filter(|draft| draft.instance_id == audition.instance_id)
                {
                    events.push(SessionEvent::ProgramEditCancelled {
                        draft_id: draft.draft_id,
                        instance_id: draft.instance_id.clone(),
                    });
                }
                events.push(SessionEvent::AuditionEnded {
                    lease_id,
                    instance_id: audition.instance_id.clone(),
                    restored_sound_id: audition.previous_sound_id.clone(),
                    reason: AuditionEndReason::Expired,
                });
                match store.record_many(None, events) {
                    Ok(events) => println!(
                        "AUDITION_EXPIRED lease={lease_id} revision={}",
                        events.last().map_or(0, |event| event.revision.get())
                    ),
                    Err(error) => eprintln!("AUDITION_WATCHDOG_ERROR {error:#}"),
                }
            }
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

fn output_meter_response(context: &ControlContext) -> ControlResponse {
    ControlResponse::OutputMeter {
        meter: context.output_meter.take(),
    }
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
    use rackforge_audio_api::{
        AUDIO_DEVICE_SCHEMA_VERSION, AUDIO_OUTPUT_STATE_SCHEMA_VERSION, AudioBackend,
        AudioDeviceDescriptor, AudioDeviceId, AudioDeviceSelector, AudioFallbackPolicy,
        AudioSampleFormat, AudioStreamCapabilities, AudioTransport, AudioValueRange,
    };
    use rackforge_performance_api::{
        LiveLocation, MidiOutputRoute, PERFORMANCE_SCHEMA_VERSION, RackDefinition, RackId,
        RackSlot, RackSlotId,
    };
    use rackforge_session_api::{
        ClientId, DEFAULT_LIVE_INSTANCE_ID, DEFAULT_LIVE_SESSION_ID, EventEnvelope,
        PluginInstanceState, SessionId, SessionState, SoundSummary,
    };
    use std::sync::mpsc::sync_channel;

    fn context() -> (Arc<ControlContext>, Receiver<AudioControlCommand>) {
        let instance_id = InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).unwrap();
        let state = SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            session_id: SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            revision: Revision::ZERO,
            active_mode: rackforge_session_api::SurfaceMode::Live,
            master_level: rackforge_session_api::MasterLevel::UNITY,
            master_pan: rackforge_session_api::MasterPan::CENTER,
            live: rackforge_session_api::LivePerformanceState::default(),
            active_instance_id: Some(instance_id.clone()),
            instances: vec![PluginInstanceState {
                instance_id,
                plugin_id: "org.rackforge.rf-dls".into(),
                plugin_name: "RF-DLS".into(),
                plugin_short_name: "RF-DLS".into(),
                ui_layouts: vec!["little@1".into()],
                config_available: true,
                banks: Vec::new(),
                sounds: vec![SoundSummary {
                    id: "piano".into(),
                    name: "Piano".into(),
                    bank: None,
                    detail: None,
                    category: None,
                    tags: Vec::new(),
                    editable: false,
                }],
                selected_sound_id: Some("piano".into()),
            }],
            audition: None,
            program_draft: None,
            parameter_links: Vec::new(),
        };
        let (sender, receiver) = sync_channel(4);
        let (_midi_sender, midi_receiver) = sync_channel(4);
        let device = AudioDeviceDescriptor {
            schema_version: AUDIO_DEVICE_SCHEMA_VERSION,
            id: AudioDeviceId::new("alsa.card-test.pcm-0").unwrap(),
            name: "Test Output".into(),
            backend: AudioBackend::Alsa,
            backend_address: "hw:0,0".into(),
            transport: AudioTransport::BuiltIn,
            usb: None,
            playback: Some(AudioStreamCapabilities {
                sample_formats: vec![AudioSampleFormat::S32Le],
                sample_rates_hz: vec![48_000],
                channels: AudioValueRange::new(2, 2).unwrap(),
                period_frames: AudioValueRange::new(32, 1024).unwrap(),
                buffer_frames: AudioValueRange::new(64, 4096).unwrap(),
            }),
            capture: None,
        };
        let profile = AudioOutputProfile {
            device: AudioDeviceSelector::Id {
                id: device.id.clone(),
            },
            fallback: AudioFallbackPolicy::None,
            sample_format: AudioSampleFormat::S32Le,
            sample_rate_hz: 48_000,
            channels: 2,
            period_frames: 128,
            buffer_frames: 384,
        };
        let mut midi_sources = MidiSourceRegistry::default();
        midi_sources
            .register(
                rackforge_midi_api::MidiSourceKey::new(0),
                rackforge_midi_api::MidiSourceDescriptor {
                    id: rackforge_midi_api::MidiSourceId::new("test.enabled-midi").unwrap(),
                    name: "Enabled Controller".into(),
                    primary: true,
                },
            )
            .unwrap();
        midi_sources
            .register(
                rackforge_midi_api::MidiSourceKey::new(1),
                rackforge_midi_api::MidiSourceDescriptor {
                    id: rackforge_midi_api::MidiSourceId::new(crate::live::VIRTUAL_MIDI_SOURCE_ID)
                        .unwrap(),
                    name: "RackForge Touch Controller".into(),
                    primary: false,
                },
            )
            .unwrap();
        (
            Arc::new(ControlContext {
                store: SessionStore::shared(state).unwrap(),
                audio_sender: sender,
                audio_state: Arc::new(Mutex::new(AudioOutputState {
                    schema_version: AUDIO_OUTPUT_STATE_SCHEMA_VERSION,
                    active_device: device.clone(),
                    active_profile: profile,
                    devices: vec![device],
                })),
                output_meter: Arc::new(OutputMeter::default()),
                audio_state_path: std::env::temp_dir().join("rackforge-control-audio.toml"),
                performance_repository: Arc::new(Mutex::new(
                    PerformanceRepository::in_memory(PerformanceLibrary {
                        schema_version: PERFORMANCE_SCHEMA_VERSION,
                        racks: vec![RackDefinition {
                            schema_version: PERFORMANCE_SCHEMA_VERSION,
                            id: RackId::new("rack.test").unwrap(),
                            name: "Test Rack".into(),
                            enabled: true,
                            keyboard_parts: None,
                            graph: None,
                            slots: vec![RackSlot {
                                id: RackSlotId::new("instrument.main").unwrap(),
                                name: "Main Instrument".into(),
                                plugin_id: "org.rackforge.rf-dls".into(),
                                state: None,
                                legacy_program_id: Some("piano".into()),
                                enabled: true,
                                midi_input_channel: None,
                                midi_note_low: 0,
                                midi_note_high: 127,
                                midi_transpose: 0,
                                midi_output: MidiOutputRoute::None,
                                audio_output_bus: "main".into(),
                                level_per_mille: 1_000,
                                pan_per_mille: 0,
                            }],
                        }],
                        songs: Vec::new(),
                        setlists: Vec::new(),
                        patterns: Vec::new(),
                    })
                    .unwrap(),
                )),
                storage: None,
                state_store: Arc::new(Mutex::new(PluginStateStore::new(None).unwrap())),
                plugin_manifests: BTreeMap::from([(
                    "org.rackforge.rf-dls".into(),
                    serde_json::from_value(serde_json::json!({
                        "schema_version": 1,
                        "id": "org.rackforge.rf-dls",
                        "name": "RF-DLS",
                        "vendor": "RackForge",
                        "version": "0.1.0",
                        "api": { "major": 1, "minor": 0 },
                        "kind": "instrument",
                        "state_version": 1,
                        "binaries": { "linux-aarch64": "lib/plugin.so" }
                    }))
                    .unwrap(),
                )]),
                portable_plugins: BTreeMap::new(),
                semantic_profiles: Mutex::new(BTreeMap::new()),
                midi_sources,
                connected_midi_sources: Arc::new(Mutex::new(BTreeSet::from([0, 1]))),
                midi_observer: Mutex::new(midi_receiver),
                midi_learn: Mutex::new(None),
                dynamic_resources: Mutex::new(BTreeMap::new()),
                virtual_midi: Mutex::new(BTreeMap::new()),
                plugin_sample_rate: 48_000.0,
                plugin_maximum_frames: 128,
                plugin_output_channels: 2,
                checkpoint: None,
                dispatch_lock: Mutex::new(()),
                lease_deadline: Mutex::new(None),
                next_draft_id: AtomicU64::new(1),
                next_midi_learn_id: AtomicU64::new(1),
            }),
            receiver,
        )
    }

    #[test]
    fn output_meter_request_drains_the_shared_post_master_peaks() {
        let (context, _receiver) = context();
        context.output_meter.observe_stereo(0.25, 1.25);

        assert!(matches!(
            output_meter_response(&context),
            ControlResponse::OutputMeter { meter }
                if meter.left_peak == 0.25 && meter.right_peak == 1.25
        ));
        assert!(matches!(
            output_meter_response(&context),
            ControlResponse::OutputMeter { meter }
                if meter.left_peak == 0.0 && meter.right_peak == 0.0
        ));
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
    fn rejects_empty_rack_previews_without_silencing_audio() {
        let (context, receiver) = context();
        let response = dispatch_command(
            &context,
            CommandEnvelope::new(
                ClientId::new("test.empty-preview").unwrap(),
                2,
                SessionCommand::PreviewRack {
                    rack: RackDefinition {
                        schema_version: PERFORMANCE_SCHEMA_VERSION,
                        id: RackId::new("rack.empty-preview").unwrap(),
                        name: "Empty Preview".into(),
                        enabled: true,
                        keyboard_parts: None,
                        graph: None,
                        slots: Vec::new(),
                    },
                },
            ),
        );
        assert!(matches!(
            response,
            ControlResponse::Error {
                code: ControlErrorCode::Rejected,
                ..
            }
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn confirms_rack_preview_only_after_one_voice_reaches_audio() {
        let (context, receiver) = context();
        let rack = context
            .performance_repository
            .lock()
            .unwrap()
            .library()
            .racks[0]
            .clone();
        let audio = thread::spawn(move || match receiver.recv().unwrap() {
            AudioControlCommand::ActivateRack { slots, reply, .. } => {
                assert_eq!(slots.len(), 1);
                assert_eq!(slots[0].plugin_id, "org.rackforge.rf-dls");
                reply.send(Ok(())).unwrap();
            }
            _ => panic!("unexpected audio command"),
        });
        let response = dispatch_command(
            &context,
            CommandEnvelope::new(
                ClientId::new("test.one-voice-preview").unwrap(),
                3,
                SessionCommand::PreviewRack { rack },
            ),
        );
        assert!(matches!(
            response,
            ControlResponse::CommandApplied {
                command_id: 3,
                ref events,
                ..
            } if events.is_empty()
        ));
        audio.join().unwrap();
    }

    #[test]
    fn deleting_the_active_rack_clears_live_and_silences_audio() {
        let (context, receiver) = context();
        let rack_id = RackId::new("rack.test").unwrap();
        let mut live = rackforge_session_api::LivePerformanceState::default();
        live.activate(
            LiveLocation::Rack {
                rack_id: rack_id.clone(),
            },
            rack_id.clone(),
        );
        context
            .store
            .lock()
            .unwrap()
            .record(None, SessionEvent::LiveStateReconciled { live })
            .unwrap();
        let expected_revision = context.performance_repository.lock().unwrap().revision();
        let audio = thread::spawn(move || match receiver.recv().unwrap() {
            AudioControlCommand::EmergencyStop { reply } => {
                reply.send(Ok(())).unwrap();
            }
            _ => panic!("expected deleted Rack to silence LIVE audio"),
        });

        let response = edit_performance(
            &context,
            expected_revision,
            PerformanceEdit::DeleteRack {
                rack_id: rack_id.clone(),
            },
        );
        audio.join().unwrap();

        let ControlResponse::PerformanceEdited { snapshot } = response else {
            panic!("expected the active Rack deletion to succeed");
        };
        assert!(snapshot.library.racks.is_empty());
        assert_eq!(snapshot.live.active, None);
        assert_eq!(snapshot.live.active_rack_id, None);
        assert_eq!(context.store.lock().unwrap().state().live.active, None);
        assert_eq!(
            context.store.lock().unwrap().state().active_mode,
            rackforge_session_api::SurfaceMode::Idle
        );
    }

    #[test]
    fn virtual_midi_release_is_scoped_to_its_client() {
        let (context, receiver) = context();
        let first = ClientId::new("web.touch.first").unwrap();
        let second = ClientId::new("web.touch.second").unwrap();
        for (client_id, note) in [(first.clone(), 60), (second.clone(), 64)] {
            let response = accept_virtual_midi(
                &context,
                client_id,
                None,
                VirtualMidiMessage {
                    status: 0x90,
                    data1: note,
                    data2: 100,
                },
            );
            assert!(matches!(
                response,
                ControlResponse::VirtualMidiAccepted {
                    active_notes: 1,
                    ..
                }
            ));
            assert!(matches!(
                receiver.recv().unwrap(),
                AudioControlCommand::InjectVirtualMidi { .. }
            ));
        }

        let response = release_virtual_midi(&context, first.clone());
        assert_eq!(
            response,
            ControlResponse::VirtualMidiReleased {
                client_id: first.clone()
            }
        );
        let AudioControlCommand::InjectVirtualMidi { events } = receiver.recv().unwrap() else {
            panic!("expected virtual MIDI release");
        };
        assert!(
            events
                .iter()
                .any(|event| event.packet.data == [0x80, 60, 0])
        );
        assert!(
            !events
                .iter()
                .any(|event| event.packet.data == [0x80, 64, 0])
        );
        assert!(context.virtual_midi.lock().unwrap().contains_key(&second));
        assert!(!context.virtual_midi.lock().unwrap().contains_key(&first));
    }

    #[test]
    fn forwarded_controller_midi_uses_the_approved_source_for_learn() {
        let (context, receiver) = context();
        *context.midi_learn.lock().unwrap() = Some(MidiLearnState {
            id: 77,
            candidate: None,
        });
        let response = accept_virtual_midi(
            &context,
            ClientId::new("controller.test.forwarder").unwrap(),
            Some("Enabled Controller".into()),
            VirtualMidiMessage {
                status: 0xb2,
                data1: 74,
                data2: 96,
            },
        );
        assert!(matches!(
            response,
            ControlResponse::VirtualMidiAccepted { .. }
        ));
        let AudioControlCommand::InjectVirtualMidi { events } = receiver.recv().unwrap() else {
            panic!("expected forwarded MIDI");
        };
        assert_eq!(events[0].source, rackforge_midi_api::MidiSourceKey::new(0));
        assert!(matches!(
            midi_learn_status(&context, 77),
            ControlResponse::MidiLearnStatus {
                candidate: Some(MidiLearnCandidate {
                    source: MidiSourceDescriptor { ref name, .. },
                    message: ParameterLinkMessage::ControlChange { controller: 74 },
                    ..
                }),
                ..
            } if name == "Enabled Controller"
        ));

        let rejected = accept_virtual_midi(
            &context,
            ClientId::new("controller.test.hidden").unwrap(),
            Some("Hidden Controller".into()),
            VirtualMidiMessage {
                status: 0xb0,
                data1: 1,
                data2: 1,
            },
        );
        assert!(matches!(
            rejected,
            ControlResponse::Error {
                code: ControlErrorCode::Unavailable,
                ..
            }
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn activates_a_live_rack_only_after_audio_accepts_it() {
        let (context, receiver) = context();
        let worker = thread::spawn(move || match receiver.recv().unwrap() {
            AudioControlCommand::ActivateRack {
                rack_id,
                instance_id,
                slots,
                prepared_slots: _,
                reply,
            } => {
                assert_eq!(rack_id, "rack.test");
                assert_eq!(instance_id.as_str(), DEFAULT_LIVE_INSTANCE_ID);
                assert_eq!(slots.len(), 1);
                assert_eq!(
                    slots[0].state,
                    RackSlotStateLoad::LegacyPreset("piano".into())
                );
                assert_eq!(slots[0].midi_stages.len(), 1);
                assert!(slots[0].midi_stages[0].transform.source_channels.is_empty());
                assert_eq!(slots[0].level_per_mille, 1_000);
                assert_eq!(slots[0].pan_per_mille, 0);
                reply.send(Ok(())).unwrap();
            }
            _ => panic!("expected LIVE Rack activation"),
        });
        let location = LiveLocation::Rack {
            rack_id: RackId::new("rack.test").unwrap(),
        };
        let response = dispatch_command(
            &context,
            CommandEnvelope::new(
                ClientId::new("test.live").unwrap(),
                88,
                SessionCommand::ActivateLiveTarget {
                    location: location.clone(),
                },
            ),
        );
        worker.join().unwrap();
        let ControlResponse::CommandApplied { events, .. } = response else {
            panic!("expected applied LIVE Rack command");
        };
        assert_eq!(
            events[0].event,
            SessionEvent::LiveTargetActivated {
                location,
                rack_id: RackId::new("rack.test").unwrap(),
            }
        );
    }

    #[test]
    fn changes_active_mode_and_the_audio_render_path_together() {
        let (context, receiver) = context();
        let worker = thread::spawn(move || match receiver.recv().unwrap() {
            AudioControlCommand::SetRenderMode { mode, reply } => {
                assert_eq!(mode, rackforge_session_api::SurfaceMode::Play);
                reply.send(Ok(())).unwrap();
            }
            _ => panic!("expected render-mode change"),
        });
        let response = dispatch_command(
            &context,
            CommandEnvelope::new(
                ClientId::new("test.surface").unwrap(),
                2,
                SessionCommand::SetActiveMode {
                    mode: rackforge_session_api::SurfaceMode::Play,
                },
            ),
        );
        worker.join().unwrap();

        assert!(matches!(
            response,
            ControlResponse::CommandApplied { ref events, .. }
                if matches!(
                    events.as_slice(),
                    [EventEnvelope {
                        event: SessionEvent::ActiveModeChanged {
                            mode: rackforge_session_api::SurfaceMode::Play
                        },
                        ..
                    }]
                )
        ));
        assert_eq!(
            context.store.lock().unwrap().state().active_mode,
            rackforge_session_api::SurfaceMode::Play
        );
    }

    #[test]
    fn switching_to_play_deactivates_the_live_target_but_keeps_its_browse_position() {
        let (context, receiver) = context();
        let rack_id = RackId::new("rack.test").unwrap();
        let location = LiveLocation::Rack {
            rack_id: rack_id.clone(),
        };
        let mut live = rackforge_session_api::LivePerformanceState::default();
        live.activate(location.clone(), rack_id);
        context
            .store
            .lock()
            .unwrap()
            .record(None, SessionEvent::LiveStateReconciled { live })
            .unwrap();
        let worker = thread::spawn(move || match receiver.recv().unwrap() {
            AudioControlCommand::SetRenderMode { mode, reply } => {
                assert_eq!(mode, rackforge_session_api::SurfaceMode::Play);
                reply.send(Ok(())).unwrap();
            }
            _ => panic!("expected render-mode change"),
        });

        let response = dispatch_command(
            &context,
            CommandEnvelope::new(
                ClientId::new("test.play-clears-live").unwrap(),
                3,
                SessionCommand::SetActiveMode {
                    mode: rackforge_session_api::SurfaceMode::Play,
                },
            ),
        );
        worker.join().unwrap();

        let ControlResponse::CommandApplied { events, .. } = response else {
            panic!("expected PLAY transition to be applied");
        };
        assert!(matches!(
            events.as_slice(),
            [
                EventEnvelope {
                    event: SessionEvent::ActiveModeChanged {
                        mode: rackforge_session_api::SurfaceMode::Play
                    },
                    ..
                },
                EventEnvelope {
                    event: SessionEvent::LiveStateReconciled { .. },
                    ..
                }
            ]
        ));
        let state = context.store.lock().unwrap();
        assert_eq!(state.state().live.active, None);
        assert_eq!(state.state().live.active_rack_id, None);
        assert_eq!(state.state().live.rack, Some(location));
    }

    #[test]
    fn emergency_stop_terminates_runtimes_before_publishing_idle_mode() {
        let (context, receiver) = context();
        let worker = thread::spawn(move || match receiver.recv().unwrap() {
            AudioControlCommand::EmergencyStop { reply } => {
                reply.send(Ok(())).unwrap();
            }
            _ => panic!("expected emergency runtime stop"),
        });

        let response = dispatch_command(
            &context,
            CommandEnvelope::new(
                ClientId::new("test.emergency").unwrap(),
                3,
                SessionCommand::EmergencyStop,
            ),
        );
        worker.join().unwrap();

        assert!(matches!(
            response,
            ControlResponse::CommandApplied { ref events, .. }
                if matches!(
                    events.as_slice(),
                    [EventEnvelope {
                        event: SessionEvent::ActiveModeChanged {
                            mode: rackforge_session_api::SurfaceMode::Idle
                        },
                        ..
                    }]
                )
        ));
        assert_eq!(
            context.store.lock().unwrap().state().active_mode,
            rackforge_session_api::SurfaceMode::Idle
        );
    }

    #[test]
    fn registers_reserved_controls_without_advancing_session_state() {
        let (context, receiver) = context();
        let binding = HostControlBinding {
            target: rackforge_session_api::HostControlTarget::MasterLevel,
            midi_cc: rackforge_session_api::MidiControlChangeBinding {
                channel: 0,
                controller: 82,
            },
        };
        let audio = thread::spawn(move || match receiver.recv().unwrap() {
            AudioControlCommand::RegisterHostControls {
                controller_id,
                bindings,
                reply,
            } => {
                assert_eq!(controller_id, "org.rackforge.test-controller");
                assert_eq!(bindings, vec![binding]);
                reply.send(Ok(())).unwrap();
            }
            _ => panic!("unexpected audio command"),
        });
        let response = dispatch_command(
            &context,
            CommandEnvelope::new(
                ClientId::new("test.controller").unwrap(),
                4,
                SessionCommand::RegisterHostControls {
                    controller_id: "org.rackforge.test-controller".into(),
                    bindings: vec![binding],
                },
            ),
        );

        assert!(matches!(
            response,
            ControlResponse::CommandApplied {
                revision: Revision::ZERO,
                ref events,
                ..
            } if events.is_empty()
        ));
        audio.join().unwrap();
    }

    #[test]
    fn applies_master_level_to_audio_before_publishing_state() {
        let (context, receiver) = context();
        let level = MasterLevel::new(640).unwrap();
        let audio = thread::spawn(move || match receiver.recv().unwrap() {
            AudioControlCommand::SetMasterLevel {
                level: received,
                reply,
            } => {
                assert_eq!(received, level);
                reply.send(Ok(())).unwrap();
            }
            _ => panic!("unexpected audio command"),
        });
        let response = dispatch_command(
            &context,
            CommandEnvelope::new(
                ClientId::new("test.master").unwrap(),
                3,
                SessionCommand::SetMasterLevel { level },
            ),
        );

        assert!(matches!(
            response,
            ControlResponse::CommandApplied { ref events, .. }
                if matches!(
                    events.as_slice(),
                    [EventEnvelope {
                        event: SessionEvent::MasterLevelChanged { level: changed },
                        ..
                    }] if *changed == level
                )
        ));
        assert_eq!(context.store.lock().unwrap().state().master_level, level);
        audio.join().unwrap();
    }

    #[test]
    fn applies_master_pan_to_audio_before_publishing_state() {
        let (context, receiver) = context();
        let pan = MasterPan::new(-375).unwrap();
        let audio = thread::spawn(move || match receiver.recv().unwrap() {
            AudioControlCommand::SetMasterPan {
                pan: received,
                reply,
            } => {
                assert_eq!(received, pan);
                reply.send(Ok(())).unwrap();
            }
            _ => panic!("unexpected audio command"),
        });
        let response = dispatch_command(
            &context,
            CommandEnvelope::new(
                ClientId::new("test.master-pan").unwrap(),
                4,
                SessionCommand::SetMasterPan { pan },
            ),
        );

        assert!(matches!(
            response,
            ControlResponse::CommandApplied { ref events, .. }
                if matches!(
                    events.as_slice(),
                    [EventEnvelope {
                        event: SessionEvent::MasterPanChanged { pan: changed },
                        ..
                    }] if *changed == pan
                )
        ));
        assert_eq!(context.store.lock().unwrap().state().master_pan, pan);
        audio.join().unwrap();
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

    #[test]
    fn transient_command_response_does_not_advance_session_revision() {
        let (context, _) = context();
        let response = command_applied_without_events(
            &context,
            CommandRef {
                client_id: ClientId::new("test.preview").unwrap(),
                command_id: 8,
            },
        );
        let ControlResponse::CommandApplied {
            revision, events, ..
        } = response
        else {
            panic!("transient command was not applied");
        };
        assert_eq!(revision, Revision::ZERO);
        assert!(events.is_empty());
        assert_eq!(
            context.store.lock().unwrap().state().revision,
            Revision::ZERO
        );
    }
}
