//! The RackForge host as a web page runs it.
//!
//! Windows, Android and Raspberry Pi each drive the shared RackForge crates
//! from their own shell. This is the fourth: the same session store, the same
//! performance library, the same portable plugin runtime, hosted by a browser
//! instead of an operating system. Nothing about the domain is re-implemented
//! here — the shell only supplies what a page has instead of hardware:
//!
//! * storage is the WASI filesystem the embedder mounts, so `PluginStorage`,
//!   `PluginPackage` and saved programs work unchanged;
//! * audio is pulled by an `AudioWorklet` rather than pushed to ALSA, WASAPI
//!   or AAudio, so [`BrowserHost::render`] renders on demand;
//! * MIDI arrives from Web MIDI or the on-screen keyboard as ordinary
//!   channel-voice messages.
//!
//! Requests and responses are the very `ControlRequest`/`ControlResponse`
//! values the native gateway speaks, so the browser UI talks to this host with
//! the client code it already uses for a networked one.

use anyhow::{Context, Result, anyhow, bail};
use rackforge_audio_api::{
    AUDIO_DEVICE_SCHEMA_VERSION, AudioBackend, AudioDeviceDescriptor, AudioDeviceId,
    AudioDeviceSelector, AudioFallbackPolicy, AudioOutputProfile, AudioOutputState,
    AudioSampleFormat, AudioStreamCapabilities, AudioTransport, AudioValueRange,
};
use rackforge_control_api::{
    ControlErrorCode, ControlRequest, ControlResponse, PluginParameterValue, VirtualMidiMessage,
};
use rackforge_core::performance::{PerformanceBootstrap, PerformanceRepository};
use rackforge_core::session::SessionStore;
use rackforge_core::{LoadedPlugin, PluginInstance, PluginPackage, PluginStateStore};
use rackforge_performance_api::{
    LibraryRevision, PERFORMANCE_SNAPSHOT_SCHEMA_VERSION, PerformanceEdit, PerformanceSnapshot,
};
use rackforge_plugin_api::abi::MidiEventV1;
use rackforge_plugin_api::{Capability, HostPresetSummary, PluginKind, PresetCatalog};
use rackforge_session_api::{
    BankSummary, ClientId, CommandEnvelope, CommandRef, EventEnvelope, InstanceId, MasterLevel,
    MasterPan, PluginInstanceState, Revision, SESSION_SCHEMA_VERSION, SessionCommand, SessionEvent,
    SessionId, SessionState, SoundSummary, SurfaceMode,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::audio::{AudioEngine, RenderRequest};

/// Where the embedder mounts RackForge's private storage.
pub const DATA_ROOT: &str = "/rackforge";
/// Package roots the boot scan reads, relative to [`DATA_ROOT`].
const PLUGIN_DIRECTORY: &str = "plugins";
/// The browser cannot enumerate audio hardware, so it reports the single
/// output the page gave it.
const BROWSER_DEVICE_ID: &str = "browser.audio-output";

/// One loaded instrument and the presentation-level facts the session needs
/// about it.
struct HostedPlugin {
    instance_id: InstanceId,
    plugin_id: String,
    /// Leaked deliberately, exactly as the desktop host does: an instance may
    /// hold pointers into the loaded plugin for its whole lifetime.
    runtime: &'static LoadedPlugin,
    instance: PluginInstance<'static>,
    presets: PresetCatalog,
    selected_sound_id: Option<String>,
}

pub struct BrowserHost {
    store: SessionStore,
    performance: PerformanceRepository,
    state_store: PluginStateStore,
    plugins: Vec<HostedPlugin>,
    audio: AudioEngine,
    audio_state: AudioOutputState,
    /// Notes a browser client is holding down, so a disconnect can release
    /// exactly the notes that connection owns.
    virtual_notes: BTreeMap<ClientId, BTreeSet<(u8, u8)>>,
    warnings: Vec<String>,
}

impl BrowserHost {
    /// Boots from whatever the embedder mounted: scan the installed packages,
    /// load the portable instruments, restore the performance library and
    /// publish an initial session.
    pub fn open(sample_rate_hz: f64, maximum_frames: u32, output_channels: u32) -> Result<Self> {
        let data_root = PathBuf::from(DATA_ROOT);
        let mut warnings = Vec::new();
        let mut plugins = Vec::new();

        for root in package_roots(&data_root.join(PLUGIN_DIRECTORY))? {
            match load_plugin(&root, &data_root, sample_rate_hz, maximum_frames, output_channels) {
                Ok(plugin) => plugins.push(plugin),
                Err(error) => warnings.push(format!("{}: {error:#}", root.display())),
            }
        }
        if plugins.is_empty() {
            bail!(
                "no playable instrument was found in {}",
                data_root.join(PLUGIN_DIRECTORY).display()
            );
        }

        let mut state_store = PluginStateStore::new(Some(&data_root))?;
        let primary = plugins.first_mut().expect("at least one plugin");
        let bootstrap_state = state_store.put(
            &primary.runtime.manifest().id,
            &primary.runtime.manifest().version,
            primary.runtime.manifest().state_version,
            primary.selected_sound_id.clone(),
            &primary
                .instance
                .save_state()
                .context("capturing the initial plugin state")?,
        )?;
        let bootstrap = PerformanceBootstrap {
            plugin_id: primary.plugin_id.clone(),
            state: bootstrap_state,
            name: primary.runtime.manifest().name.clone(),
        };
        let performance = PerformanceRepository::load_or_bootstrap(Some(&data_root), bootstrap)?;

        let instances: Vec<PluginInstanceState> =
            plugins.iter().map(session_instance_state).collect();
        let active_instance_id = instances
            .first()
            .map(|instance| instance.instance_id.clone());
        let session = SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            session_id: SessionId::new("browser").map_err(|message| anyhow!(message))?,
            revision: Revision::ZERO,
            active_mode: SurfaceMode::Play,
            master_level: MasterLevel::UNITY,
            master_pan: MasterPan::CENTER,
            live: performance.initial_live_state(),
            active_instance_id,
            instances,
            audition: None,
            program_draft: None,
        };

        let audio = AudioEngine::new(
            sample_rate_hz,
            maximum_frames,
            output_channels,
            session.master_level,
            session.master_pan,
        );
        let audio_state = browser_audio_state(sample_rate_hz, maximum_frames, output_channels);

        Ok(Self {
            store: SessionStore::new(session)?,
            performance,
            state_store,
            plugins,
            audio,
            audio_state,
            virtual_notes: BTreeMap::new(),
            warnings,
        })
    }

    /// Problems found while booting that did not stop the host from running,
    /// such as one package out of several failing to load.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn handle(&mut self, request: ControlRequest) -> ControlResponse {
        match self.dispatch(request) {
            Ok(response) => response,
            Err(failure) => failure.into_response(),
        }
    }

    fn dispatch(&mut self, request: ControlRequest) -> Result<ControlResponse, Failure> {
        match request {
            ControlRequest::Snapshot => Ok(ControlResponse::Snapshot {
                snapshot: Box::new(self.store.snapshot()),
            }),
            ControlRequest::PerformanceSnapshot => {
                let snapshot = self.performance_snapshot();
                Ok(ControlResponse::PerformanceSnapshot {
                    snapshot: Box::new(snapshot),
                })
            }
            ControlRequest::EditPerformance {
                expected_revision,
                edit,
            } => self.edit_performance(expected_revision, edit),
            ControlRequest::PluginPresets { plugin_id } => {
                let presets = self.host_presets(&plugin_id)?;
                Ok(ControlResponse::PluginPresets { plugin_id, presets })
            }
            ControlRequest::PluginPreset {
                plugin_id,
                preset_id,
            } => self
                .state_store
                .preset(&plugin_id, &preset_id)
                .map(|preset| ControlResponse::PluginPreset {
                    preset: Box::new(preset),
                })
                .map_err(|error| Failure::new(ControlErrorCode::NotFound, format!("{error:#}"))),
            ControlRequest::SavePluginPreset { instance_id, name } => {
                self.save_plugin_preset(&instance_id, &name)
            }
            ControlRequest::LoadPluginPreset {
                instance_id,
                preset_id,
            } => self.load_plugin_preset(&instance_id, &preset_id),
            ControlRequest::RenamePluginPreset {
                plugin_id,
                preset_id,
                name,
            } => {
                let preset = self
                    .state_store
                    .rename_preset(&plugin_id, &preset_id, &name)
                    .map_err(|error| {
                        Failure::new(ControlErrorCode::Rejected, format!("{error:#}"))
                    })?;
                Ok(ControlResponse::PluginPresetRenamed {
                    preset: Box::new(preset),
                    presets: self.host_presets(&plugin_id)?,
                })
            }
            ControlRequest::DeletePluginPreset {
                plugin_id,
                preset_id,
            } => {
                self.state_store
                    .delete_preset(&plugin_id, &preset_id)
                    .map_err(|error| {
                        Failure::new(ControlErrorCode::Rejected, format!("{error:#}"))
                    })?;
                Ok(ControlResponse::PluginPresetDeleted {
                    plugin_id: plugin_id.clone(),
                    preset_id,
                    presets: self.host_presets(&plugin_id)?,
                })
            }
            ControlRequest::PluginParameters { instance_id } => {
                self.plugin_parameters(&instance_id)
            }
            ControlRequest::SetPluginParameter {
                instance_id,
                parameter_index,
                value,
            } => self.set_plugin_parameter(&instance_id, parameter_index, value),
            ControlRequest::AudioSnapshot => Ok(ControlResponse::AudioSnapshot {
                snapshot: Box::new(self.audio_state.clone()),
            }),
            ControlRequest::ApplyAudioOutput { .. } => Err(Failure::new(
                ControlErrorCode::Unavailable,
                "the browser host plays through the page's audio output, which it cannot reconfigure",
            )),
            ControlRequest::VirtualMidi { client_id, message } => {
                self.virtual_midi(client_id, message)
            }
            ControlRequest::ReleaseVirtualMidi { client_id } => {
                self.release_virtual_midi(&client_id);
                Ok(ControlResponse::VirtualMidiReleased { client_id })
            }
            ControlRequest::Events { after_revision } => {
                let events = self.store.events_after(after_revision).map_err(|error| {
                    Failure::new(ControlErrorCode::Internal, format!("{error:#}"))
                })?;
                Ok(ControlResponse::Events {
                    current_revision: self.store.state().revision,
                    events,
                })
            }
            ControlRequest::Dispatch { envelope } => self.dispatch_command(envelope),
            other => Err(Failure::new(
                ControlErrorCode::Unavailable,
                format!("the browser host does not implement {}", request_name(&other)),
            )),
        }
    }

    /// Applies one library edit and reconciles the LIVE state with it, the way
    /// the appliance host does: a deleted Rack must stop sounding and must
    /// leave the session pointing at nothing rather than at a Rack that is
    /// gone.
    fn edit_performance(
        &mut self,
        expected_revision: LibraryRevision,
        edit: PerformanceEdit,
    ) -> Result<ControlResponse, Failure> {
        let current = self.performance.revision();
        if current != expected_revision {
            return Err(Failure::new(
                ControlErrorCode::Conflict,
                format!(
                    "performance library changed: expected {}, current {}",
                    expected_revision.as_str(),
                    current.as_str()
                ),
            ));
        }
        let previous_live = self.store.state().live.clone();
        let mut live = previous_live.clone();
        self.performance
            .apply_edit(&expected_revision, edit, &mut live)
            .map_err(|error| Failure::new(ControlErrorCode::Rejected, format!("{error:#}")))?;

        if previous_live.active_rack_id.is_some() && live.active_rack_id.is_none() {
            self.audio.silence();
        }
        if live != previous_live {
            self.store
                .record(None, SessionEvent::LiveStateReconciled { live })
                .map_err(|error| {
                    Failure::new(ControlErrorCode::Internal, format!("{error:#}"))
                })?;
        }
        let snapshot = self.performance_snapshot();
        Ok(ControlResponse::PerformanceEdited {
            snapshot: Box::new(snapshot),
        })
    }

    fn performance_snapshot(&self) -> PerformanceSnapshot {
        PerformanceSnapshot {
            schema_version: PERFORMANCE_SNAPSHOT_SCHEMA_VERSION,
            revision: self.performance.revision(),
            library: self.performance.library().clone(),
            live: self.store.state().live.clone(),
        }
    }

    fn host_presets(&self, plugin_id: &str) -> Result<Vec<HostPresetSummary>, Failure> {
        self.state_store
            .list_presets(plugin_id)
            .map_err(|error| Failure::new(ControlErrorCode::Internal, format!("{error:#}")))
    }

    /// Captures what the instance is playing right now and files it as a host
    /// preset of that plugin.
    fn save_plugin_preset(
        &mut self,
        instance_id: &InstanceId,
        name: &str,
    ) -> Result<ControlResponse, Failure> {
        let plugin = self.plugin_mut(instance_id)?;
        let manifest = plugin.runtime.manifest();
        if !manifest.capabilities.contains(&Capability::State) {
            return Err(Failure::new(
                ControlErrorCode::Unavailable,
                "the plugin does not support complete state snapshots",
            ));
        }
        let plugin_id = plugin.plugin_id.clone();
        let version = manifest.version.clone();
        let state_version = manifest.state_version;
        let sound_id = plugin.selected_sound_id.clone();
        let bytes = plugin
            .instance
            .save_state()
            .map_err(|error| Failure::new(ControlErrorCode::Internal, format!("{error:#}")))?;

        let reference = self
            .state_store
            .put(&plugin_id, &version, state_version, sound_id, &bytes)
            .map_err(|error| Failure::new(ControlErrorCode::Internal, format!("{error:#}")))?;
        let preset = self
            .state_store
            .save_preset(name, reference)
            .map_err(|error| Failure::new(ControlErrorCode::Rejected, format!("{error:#}")))?;
        Ok(ControlResponse::PluginPresetSaved {
            preset: Box::new(preset),
            presets: self.host_presets(&plugin_id)?,
        })
    }

    /// Restores a stored state into the live instance and tells the session
    /// which sound the instance is on now, so surfaces stop advertising the
    /// program that was playing before.
    fn load_plugin_preset(
        &mut self,
        instance_id: &InstanceId,
        preset_id: &str,
    ) -> Result<ControlResponse, Failure> {
        let plugin_id = self.plugin_mut(instance_id)?.plugin_id.clone();
        let preset = self
            .state_store
            .preset(&plugin_id, preset_id)
            .map_err(|error| Failure::new(ControlErrorCode::NotFound, format!("{error:#}")))?;
        let bytes = self
            .state_store
            .read(&preset.state)
            .map_err(|error| Failure::new(ControlErrorCode::Internal, format!("{error:#}")))?;

        let selected_sound_id = preset.state.selected_sound_id.clone();
        let plugin = self.plugin_mut(instance_id)?;
        plugin
            .instance
            .load_state(&bytes)
            .map_err(|error| Failure::new(ControlErrorCode::Rejected, format!("{error:#}")))?;
        plugin.selected_sound_id = selected_sound_id.clone();

        self.store
            .record(
                None,
                SessionEvent::PluginStateRestored {
                    instance_id: instance_id.clone(),
                    selected_sound_id,
                },
            )
            .map_err(|error| Failure::new(ControlErrorCode::Internal, format!("{error:#}")))?;
        Ok(ControlResponse::PluginPresetLoaded {
            preset: Box::new(preset),
            revision: self.store.state().revision,
        })
    }

    fn plugin_parameters(&mut self, instance_id: &InstanceId) -> Result<ControlResponse, Failure> {
        let plugin = self.plugin_mut(instance_id)?;
        let schema = plugin.runtime.parameters().clone();
        let mut values = Vec::with_capacity(schema.parameters.len());
        for (index, _) in schema.parameters.iter().enumerate() {
            let index = index as u32;
            let value = plugin
                .instance
                .get_parameter(index)
                .map_err(|error| Failure::new(ControlErrorCode::Internal, format!("{error:#}")))?;
            values.push(PluginParameterValue { index, value });
        }
        Ok(ControlResponse::PluginParameters {
            instance_id: instance_id.clone(),
            schema: Box::new(schema),
            values,
        })
    }

    fn set_plugin_parameter(
        &mut self,
        instance_id: &InstanceId,
        parameter_index: u32,
        value: f64,
    ) -> Result<ControlResponse, Failure> {
        let plugin = self.plugin_mut(instance_id)?;
        plugin
            .instance
            .set_parameter(parameter_index, value)
            .map_err(|error| Failure::new(ControlErrorCode::Rejected, format!("{error:#}")))?;
        Ok(ControlResponse::PluginParameterSet {
            instance_id: instance_id.clone(),
            parameter_index,
            value,
        })
    }

    fn virtual_midi(
        &mut self,
        client_id: ClientId,
        message: VirtualMidiMessage,
    ) -> Result<ControlResponse, Failure> {
        message
            .validate()
            .map_err(|message| Failure::new(ControlErrorCode::InvalidRequest, message))?;
        let notes = self.virtual_notes.entry(client_id.clone()).or_default();
        let channel = message.status & 0x0f;
        match message.status & 0xf0 {
            0x90 if message.data2 > 0 => {
                notes.insert((channel, message.data1));
            }
            0x80 | 0x90 => {
                notes.remove(&(channel, message.data1));
            }
            _ => {}
        }
        let active_notes = notes.len() as u16;
        self.audio
            .push_midi(0, [message.status, message.data1, message.data2], 3);
        Ok(ControlResponse::VirtualMidiAccepted {
            client_id,
            active_notes,
        })
    }

    /// Releases every note a client is still holding. A page that navigates
    /// away must not leave the instrument sounding.
    fn release_virtual_midi(&mut self, client_id: &ClientId) {
        let Some(notes) = self.virtual_notes.remove(client_id) else {
            return;
        };
        for (channel, note) in notes {
            self.audio.push_midi(0, [0x80 | channel, note, 0], 3);
        }
    }

    fn dispatch_command(&mut self, envelope: CommandEnvelope) -> Result<ControlResponse, Failure> {
        if let Some(expected) = envelope.expected_revision
            && expected != self.store.state().revision
        {
            return Err(Failure::conflict(self.store.state().revision));
        }
        let command_ref = CommandRef {
            client_id: envelope.client_id.clone(),
            command_id: envelope.command_id,
        };
        let events = self.apply(envelope.command, &command_ref)?;
        Ok(ControlResponse::CommandApplied {
            client_id: envelope.client_id,
            command_id: envelope.command_id,
            revision: self.store.state().revision,
            events,
        })
    }

    fn apply(
        &mut self,
        command: SessionCommand,
        command_ref: &CommandRef,
    ) -> Result<Vec<EventEnvelope>, Failure> {
        let event = match command {
            SessionCommand::SetMasterLevel { level } => {
                self.audio.set_master_level(level);
                SessionEvent::MasterLevelChanged { level }
            }
            SessionCommand::SetMasterPan { pan } => {
                self.audio.set_master_pan(pan);
                SessionEvent::MasterPanChanged { pan }
            }
            SessionCommand::SetActiveMode { mode } => SessionEvent::ActiveModeChanged { mode },
            SessionCommand::SelectPlugin { instance_id } => {
                self.plugin_mut(&instance_id)?;
                self.audio.silence();
                SessionEvent::ActiveInstanceChanged { instance_id }
            }
            SessionCommand::SelectSound {
                instance_id,
                sound_id,
            } => {
                let plugin = self.plugin_mut(&instance_id)?;
                plugin.instance.load_preset(&sound_id).map_err(|error| {
                    Failure::new(ControlErrorCode::Rejected, format!("{error:#}"))
                })?;
                plugin.selected_sound_id = Some(sound_id.clone());
                SessionEvent::SoundSelected {
                    instance_id,
                    sound_id,
                }
            }
            other => {
                return Err(Failure::new(
                    ControlErrorCode::Unavailable,
                    format!("the browser host does not implement {other:?} yet"),
                ));
            }
        };
        self.store
            .record(Some(command_ref.clone()), event)
            .map(|envelope| vec![envelope])
            .map_err(|error| Failure::new(ControlErrorCode::Internal, format!("{error:#}")))
    }

    fn plugin_mut(&mut self, instance_id: &InstanceId) -> Result<&mut HostedPlugin, Failure> {
        self.plugins
            .iter_mut()
            .find(|plugin| plugin.instance_id == *instance_id)
            .ok_or_else(|| {
                Failure::new(
                    ControlErrorCode::NotFound,
                    format!("no plugin instance {instance_id}"),
                )
            })
    }

    /// Queues one live MIDI message for the next audio block.
    pub fn push_midi(&mut self, frame: u32, data: [u8; 3], length: u8) {
        self.audio.push_midi(frame, data, length);
    }

    /// Renders one interleaved block for the page's audio callback.
    pub fn render(&mut self, frames: u32) -> &[f32] {
        let active = self.store.state().active_instance_id.clone();
        let request = RenderRequest { frames };
        let Some(index) = active.and_then(|id| {
            self.plugins
                .iter()
                .position(|plugin| plugin.instance_id == id)
        }) else {
            return self.audio.render_silence(request);
        };
        let plugin = &mut self.plugins[index];
        self.audio.render(request, &mut plugin.instance)
    }
}

/// A refused request, in the shape the control protocol reports it.
struct Failure {
    code: ControlErrorCode,
    message: String,
    current_revision: Option<Revision>,
}

impl Failure {
    fn new(code: ControlErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            current_revision: None,
        }
    }

    fn conflict(current_revision: Revision) -> Self {
        Self {
            code: ControlErrorCode::Conflict,
            message: "the session changed since the command was prepared".into(),
            current_revision: Some(current_revision),
        }
    }

    fn into_response(self) -> ControlResponse {
        ControlResponse::Error {
            code: self.code,
            message: self.message,
            current_revision: self.current_revision,
        }
    }
}

fn request_name(request: &ControlRequest) -> &'static str {
    match request {
        ControlRequest::MaterializePluginState { .. } => "materializing plugin state",
        ControlRequest::PluginStateParameters { .. } => "isolated plugin parameters",
        ControlRequest::SetPluginStateParameter { .. } => "isolated parameter edits",
        ControlRequest::LoadPluginResource { .. } => "loading plugin resources",
        _ => "this request",
    }
}

/// Lists package roots below the mounted plugin directory. A root is any
/// directory holding a plugin manifest.
fn package_roots(directory: &Path) -> Result<Vec<PathBuf>> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut roots = Vec::new();
    for entry in fs::read_dir(directory)
        .with_context(|| format!("reading {}", directory.display()))?
        .flatten()
    {
        let path = entry.path();
        if path.join(rackforge_core::package::MANIFEST_FILE).is_file() {
            roots.push(path);
        }
    }
    roots.sort();
    Ok(roots)
}

fn load_plugin(
    root: &Path,
    data_root: &Path,
    sample_rate_hz: f64,
    maximum_frames: u32,
    output_channels: u32,
) -> Result<HostedPlugin> {
    let package = PluginPackage::open(root)?;
    if package.manifest().kind != PluginKind::Instrument {
        bail!(
            "the browser host currently plays instruments, found {:?}",
            package.manifest().kind
        );
    }
    if package.manifest().portable_component().is_none() {
        bail!("only portable wasm-v1 packages run in a browser");
    }
    let instance_id = InstanceId::new(format!("browser.{}", package.manifest().id))
        .map_err(|message| anyhow!(message))?;

    // SAFETY: the browser loader refuses native packages outright, and the
    // check above already rejected anything without a portable component, so
    // no foreign code enters the page.
    let loaded = unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), Some(data_root)) }?;
    // Plugin lifetimes outlive their instances by construction; the desktop
    // host leaks the same box for the same reason.
    let loaded: &'static LoadedPlugin = Box::leak(Box::new(loaded));
    let mut instance = loaded.create_instance()?;
    let presets = instance.preset_catalog()?;
    let selected_sound_id = presets.presets.first().map(|preset| preset.id.clone());
    if let Some(id) = selected_sound_id.as_deref() {
        instance
            .load_preset(id)
            .with_context(|| format!("loading initial program {id:?}"))?;
    }
    instance.activate(sample_rate_hz, maximum_frames, 0, output_channels)?;

    Ok(HostedPlugin {
        instance_id,
        plugin_id: package.manifest().id.clone(),
        runtime: loaded,
        instance,
        presets,
        selected_sound_id,
    })
}

fn session_instance_state(plugin: &HostedPlugin) -> PluginInstanceState {
    let manifest = plugin.runtime.manifest();
    PluginInstanceState {
        instance_id: plugin.instance_id.clone(),
        plugin_id: plugin.plugin_id.clone(),
        plugin_name: manifest.name.clone(),
        ui_layouts: manifest.ui_layouts.clone(),
        config_available: manifest.config_mode,
        banks: plugin
            .presets
            .banks
            .iter()
            .map(|bank| BankSummary {
                id: bank.id.clone(),
                name: bank.name.clone(),
                order: bank.order,
            })
            .collect(),
        sounds: plugin
            .presets
            .presets
            .iter()
            .map(|preset| SoundSummary {
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
            })
            .collect(),
        selected_sound_id: plugin.selected_sound_id.clone(),
    }
}

/// Describes the page's output the way the UI expects a device to be
/// described. There is exactly one, and the browser owns its settings.
fn browser_audio_state(
    sample_rate_hz: f64,
    maximum_frames: u32,
    channels: u32,
) -> AudioOutputState {
    let sample_rate_hz = sample_rate_hz.round().max(1.0) as u32;
    let device = AudioDeviceDescriptor {
        schema_version: AUDIO_DEVICE_SCHEMA_VERSION,
        id: AudioDeviceId::new(BROWSER_DEVICE_ID).expect("static device id"),
        name: "Browser audio output".into(),
        backend: AudioBackend::WebAudio,
        backend_address: "webaudio".into(),
        transport: AudioTransport::BuiltIn,
        usb: None,
        playback: Some(AudioStreamCapabilities {
            sample_formats: vec![AudioSampleFormat::F32Le],
            sample_rates_hz: vec![sample_rate_hz],
            channels: fixed_range(channels),
            period_frames: fixed_range(maximum_frames),
            buffer_frames: fixed_range(maximum_frames),
        }),
        capture: None,
    };
    let profile = AudioOutputProfile {
        device: AudioDeviceSelector::Id {
            id: device.id.clone(),
        },
        fallback: AudioFallbackPolicy::None,
        sample_format: AudioSampleFormat::F32Le,
        sample_rate_hz,
        channels,
        period_frames: maximum_frames,
        buffer_frames: maximum_frames,
    };
    AudioOutputState {
        schema_version: rackforge_audio_api::AUDIO_OUTPUT_STATE_SCHEMA_VERSION,
        active_device: device.clone(),
        active_profile: profile,
        devices: vec![device],
    }
}

/// The page offers exactly one setting per stream property, so every
/// capability range is that one value.
fn fixed_range(value: u32) -> AudioValueRange {
    AudioValueRange::new(value.max(1), value.max(1)).expect("a non-zero fixed range is valid")
}

/// Convenience for the ABI layer: turn a queued MIDI byte triple into the
/// event shape plugins consume.
pub fn midi_event(frame: u32, data: [u8; 3], length: u8) -> MidiEventV1 {
    MidiEventV1 {
        frame,
        length,
        data,
    }
}
