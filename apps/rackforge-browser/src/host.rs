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
    ControlErrorCode, ControlRequest, ControlResponse, MidiSourceStatus, PluginParameterValue,
    VirtualMidiMessage,
};
use rackforge_controller_package::ControllerPackageManifest;
use rackforge_core::performance::PerformanceRepository;
use rackforge_core::session::SessionStore;
use rackforge_core::session_checkpoint::SessionCheckpointStore;
use rackforge_core::{
    CompiledParameterLink, LoadedPlugin, PluginInstance, PluginPackage, PluginStateStore,
    PluginStorage, compile_semantic_parameter_links,
};
use rackforge_midi_api::{
    IngressMidiEvent, MidiPacket, MidiSourceDescriptor, MidiSourceId, MidiSourceKey,
    ParameterLinkPassThrough,
};
use rackforge_performance_api::{
    LibraryRevision, PERFORMANCE_SNAPSHOT_SCHEMA_VERSION, PerformanceEdit, PerformanceSnapshot,
};
use rackforge_plugin_api::abi::MidiEventV1;
use rackforge_plugin_api::{
    Capability, HostPresetSummary, PluginKind, PluginManifest, PresetCatalog, ResourceKind,
};
use rackforge_repository::{
    PluginUserDataRemovalOptions, inspect_local_archive, install_local_archive, plugin_is_enabled,
    remove_plugin_user_data, set_plugin_enabled, uninstall_plugin,
};
use rackforge_session_api::{
    BankSummary, ClientId, CommandEnvelope, CommandRef, EventEnvelope, InstanceId, MasterLevel,
    MasterPan, PluginInstanceState, Revision, SESSION_SCHEMA_VERSION, SessionCommand, SessionEvent,
    SessionId, SessionState, SoundSummary, SurfaceMode,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::audio::{AudioEngine, RenderRequest};
use crate::controller::{
    BrowserControllerAction, BrowserControllerOutput, BrowserKeyLabController,
};

/// Where the embedder mounts RackForge's private storage.
pub const DATA_ROOT: &str = "/rackforge";
/// Packages RackForge ships with the page, relative to [`DATA_ROOT`].
const PLUGIN_DIRECTORY: &str = "plugins";
/// Packages installed from a `.rfplugin`, relative to [`DATA_ROOT`]. This is a
/// plugin store in the same layout every other host keeps one in.
const STORE_DIRECTORY: &str = "store";
/// The browser cannot enumerate audio hardware, so it reports the single
/// output the page gave it.
const BROWSER_DEVICE_ID: &str = "browser.audio-output";
const BROWSER_KEYLAB_SOURCE_KEY: MidiSourceKey = MidiSourceKey::new(1);

/// The audio format the page renders in.
#[derive(Clone, Copy)]
struct StreamFormat {
    sample_rate_hz: f64,
    maximum_frames: u32,
    channels: u32,
}

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
    /// True when the package was installed into the page's plugin store, as
    /// opposed to shipped with the site.
    managed: bool,
    /// Where the package sits in the host's storage, so the page can publish
    /// the files a plugin's own interface is made of.
    package_root: String,
}

pub struct BrowserHost {
    /// How the page opened its output, kept so plugins loaded later can be
    /// activated the same way the first ones were.
    stream: StreamFormat,
    store: SessionStore,
    /// Where the session is written after every change, so the next visit
    /// opens on the instrument, program and mix that were left playing.
    checkpoint: SessionCheckpointStore,
    performance: PerformanceRepository,
    state_store: PluginStateStore,
    plugins: Vec<HostedPlugin>,
    audio: AudioEngine,
    audio_state: AudioOutputState,
    /// Notes a browser client is holding down, so a disconnect can release
    /// exactly the notes that connection owns.
    virtual_notes: BTreeMap<ClientId, BTreeSet<(u8, u8)>>,
    /// Portable LITTLE state for the bundled Arturia `.rfcontroller`. Web
    /// MIDI itself stays in the page; this owns no browser handle.
    controller: BrowserKeyLabController,
    controller_command_id: u64,
    parameter_links: Vec<CompiledParameterLink>,
    warnings: Vec<String>,
}

impl BrowserHost {
    /// Boots from whatever the embedder mounted: scan the installed packages,
    /// load the portable instruments, restore the performance library and
    /// publish an initial session.
    pub fn open(sample_rate_hz: f64, maximum_frames: u32, output_channels: u32) -> Result<Self> {
        let data_root = PathBuf::from(DATA_ROOT);
        let stream = StreamFormat {
            sample_rate_hz,
            maximum_frames,
            channels: output_channels,
        };
        let mut warnings = Vec::new();
        let mut plugins = Vec::new();

        for root in package_roots(&data_root)? {
            match load_plugin(&root, &data_root, stream) {
                Ok(plugin) => plugins.push(plugin),
                Err(error) => warnings.push(format!("{}: {error:#}", root.display())),
            }
        }
        prefer_installed(&mut plugins);
        if plugins.is_empty() {
            // The reasons matter more than the count: a package that failed to
            // load is the usual cause, and its message is the only clue.
            bail!(
                "no playable instrument was found in {}{}",
                data_root.join(PLUGIN_DIRECTORY).display(),
                if warnings.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", warnings.join("; "))
                }
            );
        }

        let state_store = PluginStateStore::new(Some(&data_root))?;
        let performance = PerformanceRepository::load_or_empty(Some(&data_root))?;

        let session_id = SessionId::new("browser").map_err(|message| anyhow!(message))?;
        let checkpoint = SessionCheckpointStore::live(&data_root);

        // What the previous visit left playing. Anything unreadable or no
        // longer installed is ignored rather than refused: a performer opening
        // RackForge should get an instrument, not an explanation.
        for plugin in &mut plugins {
            let Ok(Some(sound)) =
                checkpoint.selected_sound(&session_id, plugin.instance_id.as_str())
            else {
                continue;
            };
            if plugin
                .presets
                .presets
                .iter()
                .any(|preset| preset.id == sound)
                && plugin.instance.load_preset(&sound).is_ok()
            {
                plugin.selected_sound_id = Some(sound);
            }
        }
        let instances: Vec<PluginInstanceState> =
            plugins.iter().map(session_instance_state).collect();
        let restored_instance = checkpoint
            .active_instance_id(&session_id)
            .ok()
            .flatten()
            .and_then(|id| InstanceId::new(id).ok())
            .filter(|id| instances.iter().any(|instance| instance.instance_id == *id));
        let active_instance_id = restored_instance.or_else(|| {
            instances
                .first()
                .map(|instance| instance.instance_id.clone())
        });
        let session = SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            revision: Revision::ZERO,
            active_mode: checkpoint
                .active_mode(&session_id)
                .ok()
                .flatten()
                .unwrap_or(SurfaceMode::Play),
            master_level: checkpoint
                .master_level(&session_id)
                .ok()
                .flatten()
                .unwrap_or(MasterLevel::UNITY),
            master_pan: checkpoint
                .master_pan(&session_id)
                .ok()
                .flatten()
                .unwrap_or(MasterPan::CENTER),
            live: performance.initial_live_state(),
            active_instance_id,
            instances,
            audition: None,
            program_draft: None,
            parameter_links: Vec::new(),
            session_id,
        };

        let audio = AudioEngine::new(
            sample_rate_hz,
            maximum_frames,
            output_channels,
            session.master_level,
            session.master_pan,
        );
        let audio_state = browser_audio_state(sample_rate_hz, maximum_frames, output_channels);

        let mut host = Self {
            stream,
            checkpoint,
            store: SessionStore::new(session)?,
            performance,
            state_store,
            plugins,
            audio,
            audio_state,
            virtual_notes: BTreeMap::new(),
            controller: BrowserKeyLabController::default(),
            controller_command_id: 1,
            parameter_links: Vec::new(),
            warnings,
        };
        host.rebuild_parameter_links()?;
        host.sync_controller();
        host.save_checkpoint();
        Ok(host)
    }

    /// Writes the session where the next visit will look for it.
    ///
    /// A failure here costs the performer the next start's convenience and
    /// nothing else, so it is recorded rather than raised.
    fn save_checkpoint(&self) {
        if let Err(error) = self.checkpoint.save(self.store.state()) {
            eprintln!("SESSION_CHECKPOINT_ERROR {error:#}");
        }
    }

    /// Problems found while booting that did not stop the host from running,
    /// such as one package out of several failing to load.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn handle(&mut self, request: ControlRequest) -> ControlResponse {
        let response = match self.dispatch(request) {
            Ok(response) => response,
            Err(failure) => failure.into_response(),
        };
        self.sync_controller();
        response
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
            ControlRequest::ExportPluginPreset {
                plugin_id,
                preset_id,
            } => {
                let plugin_name = self
                    .plugins
                    .iter()
                    .find(|plugin| plugin.plugin_id == plugin_id)
                    .map(|plugin| plugin.runtime.manifest().name.as_str())
                    .unwrap_or(&plugin_id);
                self.state_store
                    .export_preset_file(&plugin_id, &preset_id, plugin_name)
                    .map(|(file_name, file)| ControlResponse::PluginPresetExported {
                        file_name,
                        file: Box::new(file),
                    })
                    .map_err(|error| Failure::new(ControlErrorCode::Rejected, format!("{error:#}")))
            }
            ControlRequest::InspectPluginPreset {
                target_plugin_id,
                file,
            } => {
                let plugin = self
                    .plugins
                    .iter()
                    .find(|plugin| plugin.plugin_id == target_plugin_id)
                    .ok_or_else(|| {
                        Failure::new(
                            ControlErrorCode::Unavailable,
                            format!("plugin {target_plugin_id} is not active in the browser"),
                        )
                    })?;
                self.state_store
                    .inspect_preset_file(
                        &target_plugin_id,
                        &plugin.runtime.manifest().version,
                        plugin.runtime.manifest().state_version,
                        &file,
                    )
                    .map(|preview| ControlResponse::PluginPresetInspected {
                        preview: Box::new(preview),
                    })
                    .map_err(|error| {
                        Failure::new(ControlErrorCode::InvalidRequest, format!("{error:#}"))
                    })
            }
            ControlRequest::ImportPluginPreset {
                target_plugin_id,
                file,
                conflict_policy,
            } => {
                let (version, state_version) = self
                    .plugins
                    .iter()
                    .find(|plugin| plugin.plugin_id == target_plugin_id)
                    .map(|plugin| {
                        (
                            plugin.runtime.manifest().version.clone(),
                            plugin.runtime.manifest().state_version,
                        )
                    })
                    .ok_or_else(|| {
                        Failure::new(
                            ControlErrorCode::Unavailable,
                            format!("plugin {target_plugin_id} is not active in the browser"),
                        )
                    })?;
                let preset = self
                    .state_store
                    .import_preset_file(
                        &target_plugin_id,
                        &version,
                        state_version,
                        &file,
                        conflict_policy,
                    )
                    .map_err(|error| {
                        Failure::new(ControlErrorCode::Rejected, format!("{error:#}"))
                    })?;
                Ok(ControlResponse::PluginPresetImported {
                    preset: Box::new(preset),
                    presets: self.host_presets(&target_plugin_id)?,
                })
            }
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
            ControlRequest::MidiSources => {
                let mut sources = Vec::new();
                for link in &self.store.state().parameter_links {
                    if sources
                        .iter()
                        .any(|status: &MidiSourceStatus| status.source.id == link.source.source_id)
                    {
                        continue;
                    }
                    sources.push(MidiSourceStatus {
                        source: MidiSourceDescriptor {
                            id: MidiSourceId::new(link.source.source_id.to_string())
                                .expect("persisted source ids are validated"),
                            name: link.source.display_name.clone(),
                            primary: false,
                        },
                        connected: false,
                    });
                }
                Ok(ControlResponse::MidiSources { sources })
            }
            ControlRequest::BeginMidiLearn { .. }
            | ControlRequest::MidiLearnStatus { .. }
            | ControlRequest::CancelMidiLearn { .. } => Err(Failure::new(
                ControlErrorCode::Unavailable,
                "MIDI Learn is unavailable until this browser grants Web MIDI access",
            )),
            ControlRequest::AudioSnapshot => Ok(ControlResponse::AudioSnapshot {
                snapshot: Box::new(self.audio_state.clone()),
            }),
            ControlRequest::ApplyAudioOutput { .. } => Err(Failure::new(
                ControlErrorCode::Unavailable,
                "the browser host plays through the page's audio output, which it cannot reconfigure",
            )),
            ControlRequest::VirtualMidi {
                client_id, message, ..
            } => self.virtual_midi(client_id, message),
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
                format!(
                    "the browser host does not implement {}",
                    request_name(&other)
                ),
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
                .map_err(|error| Failure::new(ControlErrorCode::Internal, format!("{error:#}")))?;
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
        let installed_state_version = self
            .plugin_mut(instance_id)?
            .runtime
            .manifest()
            .state_version;
        if preset.state.state_version != installed_state_version {
            return Err(Failure::new(
                ControlErrorCode::Rejected,
                format!(
                    "preset state v{} is incompatible with installed state v{}",
                    preset.state.state_version, installed_state_version
                ),
            ));
        }
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
            SessionCommand::UpsertParameterLink { link } => {
                let plugin = self
                    .plugins
                    .iter()
                    .find(|plugin| plugin.instance_id.as_str() == link.instance_id)
                    .ok_or_else(|| {
                        Failure::new(
                            ControlErrorCode::NotFound,
                            format!("no plugin instance {}", link.instance_id),
                        )
                    })?;
                CompiledParameterLink::new(
                    link.clone(),
                    MidiSourceKey::new(1),
                    plugin.runtime.parameters(),
                )
                .map_err(|error| {
                    Failure::new(
                        ControlErrorCode::Rejected,
                        format!("invalid MIDI parameter link: {error:#}"),
                    )
                })?;
                SessionEvent::ParameterLinkUpserted { link }
            }
            SessionCommand::RemoveParameterLink { link_id } => {
                if !self
                    .store
                    .state()
                    .parameter_links
                    .iter()
                    .any(|link| link.id == link_id)
                {
                    return Err(Failure::new(
                        ControlErrorCode::NotFound,
                        format!("no MIDI parameter link {link_id}"),
                    ));
                }
                SessionEvent::ParameterLinkRemoved { link_id }
            }
            other => {
                return Err(Failure::new(
                    ControlErrorCode::Unavailable,
                    format!("the browser host does not implement {other:?} yet"),
                ));
            }
        };
        let recorded = self
            .store
            .record(Some(command_ref.clone()), event)
            .map(|envelope| vec![envelope])
            .map_err(|error| Failure::new(ControlErrorCode::Internal, format!("{error:#}")))?;
        if recorded.iter().any(|envelope| {
            matches!(
                envelope.event,
                SessionEvent::ParameterLinkUpserted { .. }
                    | SessionEvent::ParameterLinkRemoved { .. }
            )
        }) {
            self.rebuild_parameter_links().map_err(|error| {
                Failure::new(
                    ControlErrorCode::Internal,
                    format!("rebuilding MIDI parameter links: {error:#}"),
                )
            })?;
        }
        self.save_checkpoint();
        Ok(recorded)
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

    /// Describes every plugin the host has loaded, in the shape the interface
    /// uses to populate PLAY and the Plugin Manager.
    ///
    /// A networked RackForge builds this by indexing its plugin directory. The
    /// browser host answers from what it actually loaded, so a package that
    /// failed to load is absent rather than offered and then unplayable.
    pub fn plugin_catalog(&self) -> serde_json::Value {
        let data_root = PathBuf::from(DATA_ROOT);
        let mut catalog = BTreeMap::<String, (semver::Version, bool, serde_json::Value)>::new();
        for root in all_package_roots(&data_root).unwrap_or_default() {
            let Ok(package) = PluginPackage::open(&root) else {
                continue;
            };
            let manifest = package.manifest();
            let Ok(version) = semver::Version::parse(&manifest.version) else {
                continue;
            };
            let managed = root.starts_with(data_root.join(STORE_DIRECTORY));
            let is_active =
                !managed || plugin_is_enabled(store_root(), &manifest.id).unwrap_or(true);
            let candidate = plugin_catalog_entry(manifest, &root, is_active, managed);
            let replace = catalog
                .get(&manifest.id)
                .is_none_or(|(current, current_managed, _)| {
                    (managed && !*current_managed)
                        || (managed == *current_managed && version >= *current)
                });
            if replace {
                catalog.insert(manifest.id.clone(), (version, managed, candidate));
            }
        }
        serde_json::Value::Array(
            catalog
                .into_values()
                .map(|(_, _, descriptor)| descriptor)
                .collect(),
        )
    }

    /// Installs a file a plugin declares — a sound library, a ROM — into that
    /// plugin's private storage, then reloads it so the resource is in place.
    ///
    /// A page has no filesystem to point a plugin at, so the file always
    /// becomes part of the plugin's private data rather than a reference to
    /// somewhere on a device. That is the same thing the appliance host does
    /// when a resource is installed rather than merely loaded.
    pub fn import_resource(
        &mut self,
        plugin_id: &str,
        resource_id: &str,
        bytes: &[u8],
    ) -> Result<serde_json::Value> {
        let plugin = self
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == plugin_id)
            .with_context(|| format!("no plugin {plugin_id} is loaded"))?;
        let requirement = plugin
            .runtime
            .manifest()
            .resources
            .iter()
            .find(|resource| resource.id == resource_id)
            .with_context(|| format!("plugin does not declare resource {resource_id:?}"))?;
        if requirement.kind != ResourceKind::File {
            bail!("resource {resource_id:?} is a directory, which a page cannot deliver");
        }
        let relative = requirement.data_path.clone().with_context(|| {
            format!("resource {resource_id:?} has no private data path to install into")
        })?;

        let storage = PluginStorage::new(PathBuf::from(DATA_ROOT));
        let path = PathBuf::from(&relative);
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            storage.ensure_directory(plugin_id, parent)?;
        }
        storage.write_atomic(plugin_id, &path, bytes)?;
        let warnings = self.reload_plugins()?;
        Ok(serde_json::json!({
            "plugin_id": plugin_id,
            "resource_id": resource_id,
            "byte_length": bytes.len(),
            "persisted": true,
            "warnings": warnings,
        }))
    }

    /// Reports which declared resources a plugin currently has installed, so
    /// its own interface can show what is missing.
    pub fn resource_status(&self, plugin_id: &str) -> Result<serde_json::Value> {
        let plugin = self
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == plugin_id)
            .with_context(|| format!("no plugin {plugin_id} is loaded"))?;
        let storage = PluginStorage::new(PathBuf::from(DATA_ROOT));
        let root = storage.ensure_plugin(plugin_id)?.root;
        let resources: Vec<serde_json::Value> = plugin
            .runtime
            .manifest()
            .resources
            .iter()
            .map(|requirement| {
                let installed = requirement
                    .data_path
                    .as_ref()
                    .map(|relative| root.join(relative).is_file())
                    .unwrap_or(false);
                serde_json::json!({
                    "resource_id": requirement.id,
                    "name": requirement.name,
                    "required": requirement.required,
                    "installed": installed,
                })
            })
            .collect();
        Ok(serde_json::Value::Array(resources))
    }

    /// Removes an installed plugin and reloads the session without it.
    ///
    /// The packages RackForge ships with the page are not installed and cannot
    /// be removed: they come back with the site on the next visit, so deleting
    /// them would be a promise the host could not keep.
    pub fn uninstall_package(
        &mut self,
        plugin_id: &str,
        options: PluginUserDataRemovalOptions,
    ) -> Result<serde_json::Value> {
        let bundled = self
            .plugins
            .iter()
            .any(|plugin| plugin.plugin_id == plugin_id && !plugin.managed);
        if bundled {
            bail!("{plugin_id} is part of this RackForge and cannot be removed");
        }
        let removed = uninstall_plugin(store_root(), plugin_id)?;
        // Presets the host itself saved live in its state store rather than in
        // the package, so they are removed here rather than by the repository.
        let mut presets_removed = 0;
        if options.presets {
            for preset in self.state_store.list_presets(plugin_id).unwrap_or_default() {
                if self
                    .state_store
                    .delete_preset(plugin_id, &preset.id)
                    .is_ok()
                {
                    presets_removed += 1;
                }
            }
        }
        let user_data = remove_plugin_user_data(PathBuf::from(DATA_ROOT), plugin_id, options)?;
        let warnings = self.reload_plugins()?;
        Ok(serde_json::json!({
            "plugin_id": removed.plugin_id,
            "removed_versions": removed.removed_versions,
            "cleanup_pending": removed.cleanup_pending,
            "presets_deleted": options.presets
                && (presets_removed > 0 || user_data.preset_files_removed > 0),
            "plugin_data_deleted": options.plugin_data && user_data.data_namespaces_removed > 0,
            "warnings": warnings,
        }))
    }

    /// Validates a `.rfplugin` without installing it, so the interface can
    /// show what is inside before anyone commits to it.
    pub fn inspect_package(&self, archive: &[u8]) -> Result<serde_json::Value> {
        let inspection = inspect_local_archive(store_root(), archive)?;
        Ok(serde_json::json!({
            "plugin_id": inspection.plugin_id,
            "plugin_name": inspection.plugin_name,
            "vendor": inspection.vendor,
            "version": inspection.version,
            "description": inspection.description,
            "kind": plugin_kind_name(inspection.kind),
            "platform": inspection.platform,
            "portable": inspection.portable,
            "archive_bytes": inspection.archive_bytes,
            "installable": inspection.portable,
        }))
    }

    /// Installs a `.rfplugin` into the page's plugin store without enabling it.
    ///
    /// Only portable packages are accepted: a native one would install
    /// perfectly well and then refuse to run, which is a worse answer than
    /// refusing it here.
    pub fn install_package(&mut self, archive: &[u8]) -> Result<serde_json::Value> {
        let inspection = inspect_local_archive(store_root(), archive)?;
        if !inspection.portable {
            bail!(
                "{} is a native package; a browser can only run portable wasm-v1 packages",
                inspection.plugin_name
            );
        }
        let installed = install_local_archive(store_root(), archive)?;
        let warnings = self.reload_plugins()?;
        Ok(serde_json::json!({
            "plugin_id": installed.record.plugin_id,
            "version": installed.record.version,
            "already_installed": installed.already_installed,
            "activation_required": true,
            "warnings": warnings,
        }))
    }

    /// Changes package enablement and rebuilds the browser audio session over
    /// the enabled set. Installation remains immutable and untouched.
    pub fn set_package_active(
        &mut self,
        plugin_id: &str,
        active: bool,
    ) -> Result<serde_json::Value> {
        let installed = all_package_roots(&PathBuf::from(DATA_ROOT))?
            .into_iter()
            .filter(|root| root.starts_with(PathBuf::from(DATA_ROOT).join(STORE_DIRECTORY)))
            .filter_map(|root| PluginPackage::open(root).ok())
            .any(|package| package.manifest().id == plugin_id);
        if !installed {
            bail!("{plugin_id} is not an installed plugin");
        }
        let previous = plugin_is_enabled(store_root(), plugin_id)?;
        set_plugin_enabled(store_root(), plugin_id, active)?;
        match self.reload_plugins() {
            Ok(warnings) => Ok(serde_json::json!({
                "plugin_id": plugin_id,
                "active": active,
                "warnings": warnings,
            })),
            Err(error) => {
                let _ = set_plugin_enabled(store_root(), plugin_id, previous);
                Err(error)
            }
        }
    }

    /// Rebuilds the session over the packages now on disk, keeping what the
    /// performer had chosen: the active instrument and each instrument's
    /// selected program.
    fn reload_plugins(&mut self) -> Result<Vec<String>> {
        let data_root = PathBuf::from(DATA_ROOT);
        let previous = self.store.state().clone();
        let selected: BTreeMap<InstanceId, String> = previous
            .instances
            .iter()
            .filter_map(|instance| {
                instance
                    .selected_sound_id
                    .clone()
                    .map(|sound| (instance.instance_id.clone(), sound))
            })
            .collect();

        let mut warnings = Vec::new();
        let mut plugins = Vec::new();
        for root in package_roots(&data_root)? {
            match load_plugin(&root, &data_root, self.stream) {
                Ok(mut plugin) => {
                    if let Some(sound) = selected.get(&plugin.instance_id)
                        && plugin
                            .presets
                            .presets
                            .iter()
                            .any(|preset| preset.id == *sound)
                        && plugin.instance.load_preset(sound).is_ok()
                    {
                        plugin.selected_sound_id = Some(sound.clone());
                    }
                    plugins.push(plugin);
                }
                Err(error) => warnings.push(format!("{}: {error:#}", root.display())),
            }
        }
        prefer_installed(&mut plugins);
        let instances: Vec<PluginInstanceState> =
            plugins.iter().map(session_instance_state).collect();
        let previous_active_instance_id = previous.active_instance_id.clone();
        let active_instance_id = previous_active_instance_id
            .clone()
            .filter(|id| instances.iter().any(|instance| instance.instance_id == *id))
            .or_else(|| {
                instances
                    .first()
                    .map(|instance| instance.instance_id.clone())
            });
        let active_mode = if previous_active_instance_id != active_instance_id
            && previous_active_instance_id.is_some()
        {
            SurfaceMode::Idle
        } else {
            previous.active_mode
        };
        // Loading plugins is a discontinuity rather than an edit, so the
        // session starts again from the revision it had reached instead of
        // pretending a sequence of events produced this.
        let session = SessionState {
            revision: previous
                .revision
                .next()
                .map_err(|message| anyhow!(message))?,
            active_mode,
            active_instance_id,
            instances,
            audition: None,
            program_draft: None,
            ..previous
        };
        self.audio.silence();
        self.plugins = plugins;
        self.store = SessionStore::new(session)?;
        self.rebuild_parameter_links()?;
        self.sync_controller();
        self.save_checkpoint();
        Ok(warnings)
    }

    /// Queues one live MIDI message for the next audio block.
    pub fn push_midi(&mut self, frame: u32, data: [u8; 3], length: u8) {
        self.audio.push_midi(frame, data, length);
    }

    pub fn controller_connect(&mut self) {
        self.sync_controller();
        self.controller.connect();
    }

    pub fn controller_disconnect(&mut self) {
        self.controller.disconnect();
    }

    pub fn push_controller_midi(&mut self, data: [u8; 3], length: u8) {
        let outcome = self.controller.handle_midi(&data[..usize::from(length)]);
        let mut consumed = outcome.consumed;
        if let Ok(packet) = MidiPacket::new(0, &data[..usize::from(length)]) {
            let ingress = IngressMidiEvent {
                source: BROWSER_KEYLAB_SOURCE_KEY,
                packet,
            };
            let active_instance_id = self.store.state().active_instance_id.as_ref();
            for link in self.parameter_links.iter().filter(|link| {
                active_instance_id
                    .is_some_and(|instance_id| link.link.instance_id == instance_id.as_str())
            }) {
                let Some(mapped) = link.apply(ingress) else {
                    continue;
                };
                consumed |= mapped.pass_through == ParameterLinkPassThrough::Consume;
                self.audio.push_parameter(mapped.event);
            }
        }
        if !consumed {
            self.audio.push_midi(0, data, length);
        }
        self.apply_controller_actions(outcome.actions);
    }

    pub fn controller_output_pending(&self) -> bool {
        self.controller.has_output()
    }

    pub fn drain_controller_output(&mut self) -> Vec<BrowserControllerOutput> {
        self.controller.drain_output()
    }

    pub fn set_controller_color(&mut self, rgb: [u8; 3]) {
        self.controller.set_ambient_color(rgb);
    }

    pub fn controller_catalog(&self) -> serde_json::Value {
        let manifest: ControllerPackageManifest =
            toml::from_str(keylab_essential_mk3::controller::PACKAGE_MANIFEST)
                .expect("the bundled Arturia controller manifest is validated at build time");
        serde_json::json!({
            "controllers": [{
                "id": manifest.id,
                "name": manifest.name,
                "version": manifest.version,
                "enabled": true,
                "trust": "official",
                "runtime": "Browser",
                "devices": manifest.devices.len(),
                "settings": manifest.settings.into_iter().map(|setting| serde_json::json!({
                    "id": setting.id,
                    "name": setting.name,
                    "kind": match setting.kind {
                        rackforge_controller_package::ControllerSettingKind::Color => "color",
                    },
                    "default": setting.default,
                    "page": setting.page,
                })).collect::<Vec<_>>(),
            }]
        })
    }

    fn sync_controller(&mut self) {
        self.controller.sync(self.store.state());
    }

    fn rebuild_parameter_links(&mut self) -> Result<()> {
        let controller = keylab_essential_mk3::controller::package_profile();
        let Some(profile) = controller.semantic_profile.as_ref() else {
            self.parameter_links.clear();
            return Ok(());
        };
        let source_id = MidiSourceId::new(profile.source_id.clone()).map_err(anyhow::Error::msg)?;
        let explicit = &self.store.state().parameter_links;
        let mut compiled = Vec::new();
        for plugin in &self.plugins {
            compiled.extend(compile_semantic_parameter_links(
                controller.driver_id.as_str(),
                controller.name.as_str(),
                profile,
                &source_id,
                BROWSER_KEYLAB_SOURCE_KEY,
                plugin.instance_id.as_str(),
                plugin.runtime.parameters(),
                explicit,
            )?);
            for link in explicit.iter().filter(|link| {
                link.instance_id == plugin.instance_id.as_str()
                    && link.source.source_id == source_id
            }) {
                compiled.push(CompiledParameterLink::new(
                    link.clone(),
                    BROWSER_KEYLAB_SOURCE_KEY,
                    plugin.runtime.parameters(),
                )?);
            }
        }
        self.parameter_links = compiled;
        Ok(())
    }

    fn next_controller_command_ref(&mut self) -> CommandRef {
        let command_ref = CommandRef {
            client_id: ClientId::new("controller.browser.arturia-keylab")
                .expect("built-in controller client id is valid"),
            command_id: self.controller_command_id,
        };
        self.controller_command_id = self.controller_command_id.saturating_add(1);
        command_ref
    }

    fn apply_controller_session_command(&mut self, command: SessionCommand) {
        let command_ref = self.next_controller_command_ref();
        let _ = self.apply(command, &command_ref);
    }

    fn apply_controller_actions(&mut self, actions: Vec<BrowserControllerAction>) {
        for action in actions {
            match action {
                BrowserControllerAction::RackForgeParameter(parameter) => match parameter {
                    rackforge_session_api::RackForgeParameterValue::MasterLevel(level) => self
                        .apply_controller_session_command(SessionCommand::SetMasterLevel { level }),
                    rackforge_session_api::RackForgeParameterValue::MasterPan(pan) => {
                        self.apply_controller_session_command(SessionCommand::SetMasterPan { pan })
                    }
                },
                BrowserControllerAction::Menu(command) => {
                    self.apply_controller_menu_command(command);
                }
            }
        }
        self.sync_controller();
    }

    fn apply_controller_menu_command(&mut self, command: rackforge_surface_runtime::MenuCommand) {
        use rackforge_surface_runtime::{ActiveMode, MenuCommand};
        let mode = |mode| match mode {
            ActiveMode::Idle => SurfaceMode::Idle,
            ActiveMode::Live => SurfaceMode::Live,
            ActiveMode::Play => SurfaceMode::Play,
        };
        match command {
            MenuCommand::SetActiveMode { mode: next } => {
                self.apply_controller_session_command(SessionCommand::SetActiveMode {
                    mode: mode(next),
                });
            }
            MenuCommand::SelectPlugin { instance_id } => {
                if let Ok(instance_id) = InstanceId::new(instance_id) {
                    self.apply_controller_session_command(SessionCommand::SelectPlugin {
                        instance_id,
                    });
                }
            }
            MenuCommand::SelectSound { id } => {
                if let Some(instance_id) = self.store.state().active_instance_id.clone() {
                    self.apply_controller_session_command(SessionCommand::SelectSound {
                        instance_id,
                        sound_id: id,
                    });
                }
            }
            MenuCommand::ReturnToActiveMode {
                mode: next,
                cancel_draft_id,
                selected_sound_id,
            } => {
                if let Some(draft_id) = cancel_draft_id {
                    self.apply_controller_session_command(SessionCommand::CancelProgramEdit {
                        draft_id,
                    });
                }
                if let (Some(instance_id), Some(sound_id)) = (
                    self.store.state().active_instance_id.clone(),
                    selected_sound_id,
                ) {
                    self.apply_controller_session_command(SessionCommand::SelectSound {
                        instance_id,
                        sound_id,
                    });
                }
                self.apply_controller_session_command(SessionCommand::SetActiveMode {
                    mode: mode(next),
                });
                let focus_sound_id = self
                    .store
                    .state()
                    .active_instance()
                    .and_then(|instance| instance.selected_sound_id.as_deref());
                self.controller
                    .complete_return_to_active_mode(next, focus_sound_id);
            }
            MenuCommand::ForceHome => {
                self.audio.silence();
                self.apply_controller_session_command(SessionCommand::SetActiveMode {
                    mode: SurfaceMode::Idle,
                });
            }
            // The autonomous browser host does not render racks yet. Keep the
            // menus visible, but never pretend an unsupported LIVE/config
            // operation was applied.
            _ => {}
        }
    }

    pub fn revision(&self) -> u32 {
        self.store.state().revision.get().min(u64::from(u32::MAX)) as u32
    }

    /// Renders one interleaved block for the page's audio callback.
    pub fn render(&mut self, frames: u32) -> &[f32] {
        let actions = self.controller.poll();
        self.apply_controller_actions(actions);
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

/// Keeps one plugin per identity when a package is both shipped with the site
/// and installed into the store: the installed copy wins, because installing
/// is the only way a person can upgrade a bundled instrument, and two
/// instances of one plugin id would collide in the session.
fn prefer_installed(plugins: &mut Vec<HostedPlugin>) {
    let mut index = 0;
    while index < plugins.len() {
        let duplicate = plugins.iter().enumerate().any(|(other, plugin)| {
            other != index
                && plugin.plugin_id == plugins[index].plugin_id
                && (plugin.managed && !plugins[index].managed)
        });
        if duplicate {
            plugins.remove(index);
        } else {
            index += 1;
        }
    }
}

fn store_root() -> PathBuf {
    PathBuf::from(DATA_ROOT).join(STORE_DIRECTORY)
}

fn plugin_kind_name(kind: PluginKind) -> &'static str {
    match kind {
        PluginKind::Instrument => "instrument",
        PluginKind::Effect => "effect",
        _ => "other",
    }
}

/// Lists every package the host should load: those RackForge ships with the
/// page, and those installed into its plugin store.
fn package_roots(data_root: &Path) -> Result<Vec<PathBuf>> {
    let mut roots = manifest_roots(&data_root.join(PLUGIN_DIRECTORY))?;
    let packages = data_root.join(STORE_DIRECTORY).join("packages");
    if packages.is_dir() {
        for plugin in fs::read_dir(&packages)
            .with_context(|| format!("reading {}", packages.display()))?
            .flatten()
        {
            let plugin_id = plugin.file_name().to_string_lossy().into_owned();
            if plugin_is_enabled(store_root(), &plugin_id)? {
                roots.extend(manifest_roots(&plugin.path())?);
            }
        }
    }
    roots.sort();
    Ok(roots)
}

fn all_package_roots(data_root: &Path) -> Result<Vec<PathBuf>> {
    let mut roots = manifest_roots(&data_root.join(PLUGIN_DIRECTORY))?;
    let packages = data_root.join(STORE_DIRECTORY).join("packages");
    if packages.is_dir() {
        for plugin in fs::read_dir(&packages)
            .with_context(|| format!("reading {}", packages.display()))?
            .flatten()
        {
            // One directory per plugin, one directory per version inside it.
            roots.extend(manifest_roots(&plugin.path())?);
        }
    }
    roots.sort();
    Ok(roots)
}

fn plugin_catalog_entry(
    manifest: &PluginManifest,
    package_root: &Path,
    active: bool,
    managed: bool,
) -> serde_json::Value {
    serde_json::json!({
        "plugin_id": manifest.id,
        "plugin_name": manifest.name,
        "version": manifest.version,
        "kind": plugin_kind_name(manifest.kind),
        "active": active,
        "managed": managed,
        "api_version": manifest.api.major,
        "branding": manifest.branding.as_ref().map(|branding| {
            serde_json::json!({
                "icon": branding.icon,
                "banner": branding.banner,
                "splash": branding.splash,
                "background_color": branding.background_color,
                "accent_color": branding.accent_color,
            })
        }),
        "package_root": package_root.strip_prefix(DATA_ROOT).unwrap_or(package_root),
        "surfaces": manifest.web_ui.as_ref().map(|web_ui| {
            web_ui.surfaces.iter().map(|surface| {
                serde_json::json!({
                    "kind": match surface.kind {
                        rackforge_plugin_api::WebSurfaceKind::Config => "config",
                        _ => "play",
                    },
                    "entry": surface.entry,
                })
            }).collect::<Vec<_>>()
        }).unwrap_or_default(),
        "resources": manifest.resources.iter().map(|resource| {
            serde_json::json!({
                "id": resource.id,
                "name": resource.name,
                "kind": match resource.kind {
                    rackforge_plugin_api::ResourceKind::Directory => "directory",
                    _ => "file",
                },
                "required": resource.required,
                "data_path": resource.data_path,
                "package_path": resource.package_path,
            })
        }).collect::<Vec<_>>(),
    })
}

/// Lists the directories directly below `directory` that hold a plugin
/// manifest.
fn manifest_roots(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    for entry in
        fs::read_dir(directory).with_context(|| format!("reading {}", directory.display()))?
    {
        let path = entry
            .with_context(|| format!("listing {}", directory.display()))?
            .path();
        let manifest = path.join(rackforge_core::package::MANIFEST_FILE);
        match fs::metadata(&manifest) {
            Ok(metadata) if metadata.is_file() => roots.push(path),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => bail!("reading {}: {error}", manifest.display()),
        }
    }
    roots.sort();
    Ok(roots)
}

fn load_plugin(root: &Path, data_root: &Path, stream: StreamFormat) -> Result<HostedPlugin> {
    let managed = root.starts_with(data_root.join(STORE_DIRECTORY));
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
    instance.activate(
        stream.sample_rate_hz,
        stream.maximum_frames,
        0,
        stream.channels,
    )?;

    Ok(HostedPlugin {
        package_root: root.to_string_lossy().into_owned(),
        instance_id,
        plugin_id: package.manifest().id.clone(),
        runtime: loaded,
        instance,
        presets,
        selected_sound_id,
        managed,
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
