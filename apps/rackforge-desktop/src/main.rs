#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod desktop_audio;
#[cfg(windows)]
mod desktop_webview;
#[cfg(windows)]
#[rustfmt::skip]
mod paths;
mod setup;
mod shutdown;
#[cfg(windows)]
mod single_instance;
mod startup;
#[cfg(windows)]
use rackforge_ump as ump_input;
mod web;

use anyhow::{Context, Result, bail};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Key, Layout, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, Vec2,
};
use rackforge_control_api::{
    ClientId, ControlErrorCode, ControlRequest, ControlResponse, MidiLearnCandidate,
    MidiSourceStatus, ParameterLinkMessage, VirtualMidiMessage,
};
#[cfg(windows)]
use rackforge_controller_api::{
    ButtonPhase, DeclarativeControllerInput, HostActionTarget, HostControlTarget,
    rackforge_parameter_input, semantic_control_input,
};
use rackforge_controller_api::{HostActionBinding, HostControlBinding};
use rackforge_core::performance::PerformanceRepository;
use rackforge_core::session_checkpoint::SessionCheckpointStore;
use rackforge_core::{
    CompiledParameterLink, IsolatedPluginStateEditor, LoadedPlugin, PluginInstance, PluginPackage,
    PluginStateStore, PluginStorage, SemanticParameterLinkContext,
    compile_semantic_parameter_links, validate_state_reference,
};
#[cfg(windows)]
use rackforge_midi_api::{
    MidiChannel, MidiPacket as RoutedMidiPacket, MidiSourceDescriptor, MidiSourceId, MidiSourceKey,
};
use rackforge_performance_api::{PERFORMANCE_SNAPSHOT_SCHEMA_VERSION, PerformanceSnapshot};
use rackforge_plugin_api::{
    HostPresetSummary, PROGRAM_EDITOR_SCHEMA_VERSION, PluginKind, PreparedProgram, PresetCatalog,
    ProgramDocument, ProgramEditRequest, ProgramEditorValue, ProgramFieldEditRequest,
};
use rackforge_repository::{
    InstalledPackage, LocalPackageInspection, MAX_PACKAGE_BYTES, PluginUserDataRemovalOptions,
    cleanup_uninstall_tombstones, inspect_local_archive, install_local_archive,
    install_local_archive_replacing, plugin_is_enabled, remove_plugin_user_data,
    set_plugin_enabled, uninstall_plugin,
};
use rackforge_session_api::{
    AuditionEndReason, BankSummary, CommandRef, DEFAULT_LIVE_SESSION_ID, EventEnvelope, InstanceId,
    MasterLevel, MasterPan, ParameterLink, PluginInstanceState, ProgramDraftState,
    RackForgeParameterMapper, RackForgeParameterValue, Revision, SESSION_SCHEMA_VERSION,
    SemanticControlProfile, SessionCommand, SessionEvent, SessionId, SessionState, SoundSummary,
    semantic_control_little_header,
};
use rackforge_surface_api::{SurfaceActivationRequest, SurfaceMode};
use rackforge_surface_runtime::{
    ActiveMode, Header, Input, Menu, MenuCommand, PlayPlugin, PlayPreset, PlaySound,
    ProgramExitDecision, ProgramExitDestination,
};
use semver::Version;
use shutdown::DesktopShutdown;
use startup::{Options, Startup, options_from_layout, parse_startup};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

include!(concat!(env!("OUT_DIR"), "/bundled_plugin.rs"));

const LONG_PRESS: Duration = Duration::from_millis(700);
const HOME_CHORD_SIMULTANEITY: Duration = Duration::from_millis(250);
const LITTLE_WIDTH: f32 = 760.0;
const LITTLE_HEIGHT: f32 = 270.0;

#[derive(Clone, Copy)]
struct LittleGeometry {
    outer: Rect,
    glass: Rect,
    header: Rect,
    footer: Rect,
    line_1: Pos2,
    line_2: Pos2,
    columns: [f32; 4],
}

impl LittleGeometry {
    fn new(outer: Rect) -> Self {
        let glass = outer.shrink2(Vec2::new(12.0, 12.0));
        let header = Rect::from_min_max(
            glass.min,
            Pos2::new(glass.max.x, glass.min.y + glass.height() * 0.22),
        );
        let footer = Rect::from_min_max(
            Pos2::new(glass.min.x, glass.max.y - glass.height() * 0.20),
            glass.max,
        );
        let body_height = footer.min.y - header.max.y;
        let column_width = glass.width() / 4.0;
        Self {
            outer,
            glass,
            header,
            footer,
            line_1: Pos2::new(glass.center().x, header.max.y + body_height * 0.39),
            line_2: Pos2::new(glass.center().x, header.max.y + body_height * 0.70),
            columns: std::array::from_fn(|index| glass.min.x + column_width * (index as f32 + 0.5)),
        }
    }
}

enum AppMode {
    Setup {
        state: Box<setup::SetupState>,
        web_preferences: web::WebServerPreferences,
        install_archives: Vec<PathBuf>,
    },
    Desktop(Box<DesktopApp>),
    Error(String),
}

struct RackForgeApp {
    mode: AppMode,
    shutdown: Option<DesktopShutdown>,
    #[cfg(windows)]
    webview: desktop_webview::DesktopWebView,
}

struct DesktopPlugin {
    instance_id: String,
    plugin_id: String,
    name: String,
    version: Version,
    runtime: &'static LoadedPlugin,
    config_available: bool,
    banks: Vec<BankSummary>,
    sound_summaries: Vec<SoundSummary>,
    sounds: Vec<PlaySound>,
    selected_sound_id: Option<String>,
    instance: PluginInstance<'static>,
    resources: BTreeMap<String, PathBuf>,
    resource_data_paths: BTreeMap<String, PathBuf>,
}

#[derive(Clone)]
struct RegisteredSemanticProfile {
    profile: Option<SemanticControlProfile>,
    runtime_source_id: Option<String>,
    runtime_source_name: Option<String>,
    host_controls: Vec<HostControlBinding>,
    host_actions: Vec<HostActionBinding>,
}

#[cfg(windows)]
fn compile_desktop_parameter_links(
    links: &[ParameterLink],
    plugins: &[DesktopPlugin],
    performance: &PerformanceRepository,
    semantic_profiles: &BTreeMap<String, RegisteredSemanticProfile>,
) -> Result<Vec<CompiledParameterLink>> {
    let mut compiled = links
        .iter()
        .map(|link| {
            let plugin_id = plugins
                .iter()
                .find(|plugin| plugin.instance_id == link.instance_id)
                .map(|plugin| plugin.plugin_id.as_str())
                .or_else(|| {
                    performance
                        .library()
                        .racks
                        .iter()
                        .flat_map(|rack| rack.slots.iter())
                        .find(|slot| slot.id.as_str() == link.instance_id)
                        .map(|slot| slot.plugin_id.as_str())
                })
                .with_context(|| {
                    format!(
                        "MIDI Link {} targets missing instance {}",
                        link.id, link.instance_id
                    )
                })?;
            let plugin = plugins
                .iter()
                .find(|plugin| plugin.plugin_id == plugin_id)
                .with_context(|| {
                    format!("MIDI Link {} targets inactive plugin {plugin_id}", link.id)
                })?;
            CompiledParameterLink::new(
                link.clone(),
                desktop_audio::stable_midi_source_key_from_id(&link.source.source_id),
                plugin.runtime.parameters(),
            )
            .with_context(|| format!("validating MIDI Link {}", link.id))
        })
        .collect::<Result<Vec<_>>>()?;

    for (controller_id, registered) in semantic_profiles {
        let Some(runtime_source_id) = &registered.runtime_source_id else {
            continue;
        };
        let Some(profile) = &registered.profile else {
            continue;
        };
        let source_id = MidiSourceId::new(runtime_source_id.clone())?;
        let source_key = desktop_audio::stable_midi_source_key_from_id(&source_id);
        for plugin in plugins {
            compiled.extend(
                compile_semantic_parameter_links(SemanticParameterLinkContext {
                    controller_id,
                    controller_name: registered
                        .runtime_source_name
                        .as_deref()
                        .unwrap_or(controller_id),
                    profile,
                    runtime_source_id: &source_id,
                    source_key,
                    instance_id: &plugin.instance_id,
                    schema: plugin.runtime.parameters(),
                    explicit_links: links,
                })
                .with_context(|| {
                    format!(
                        "compiling semantic controller {controller_id} for {}",
                        plugin.plugin_id
                    )
                })?,
            );
        }
    }
    Ok(compiled)
}

#[cfg(windows)]
fn virtual_midi_source_descriptor(client_id: &ClientId) -> MidiSourceDescriptor {
    let name = client_id.as_str().to_owned();
    MidiSourceDescriptor {
        // Client IDs and MIDI source IDs share RackForge's stable-ID syntax.
        // Keeping the same text lets a controller package reconnect to links
        // learned before a restart.
        id: MidiSourceId::new(client_id.as_str().to_owned())
            .expect("a valid client id is also a valid MIDI source id"),
        name,
        primary: false,
    }
}

#[cfg(windows)]
fn approved_midi_source(
    preferences: Option<&desktop_audio::AudioPreferences>,
    source_name: &str,
) -> Result<MidiSourceDescriptor, String> {
    let preferences = preferences.ok_or_else(|| "MIDI inputs are unavailable".to_owned())?;
    if !preferences
        .midi_inputs
        .iter()
        .any(|name| name == source_name)
    {
        return Err(format!(
            "MIDI input {source_name:?} is disabled in RackForge Audio & MIDI settings"
        ));
    }
    desktop_audio::midi_source_descriptor(source_name).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn approved_midi_source_statuses(
    preferences: Option<&desktop_audio::AudioPreferences>,
    present: &BTreeSet<String>,
) -> Vec<MidiSourceStatus> {
    preferences
        .map(|preferences| preferences.midi_inputs.clone())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| {
            desktop_audio::midi_source_descriptor(&name)
                .ok()
                .map(|source| MidiSourceStatus {
                    connected: present.contains(&name),
                    source,
                })
        })
        .collect()
}

#[derive(Clone, Default)]
struct VirtualMidiClientState {
    notes: BTreeSet<(u8, u8)>,
    channels: BTreeSet<u8>,
    /// Canonical host-owned source selected for this forwarding client.
    /// `None` means an actual virtual controller such as the touch keyboard.
    midi_source: Option<MidiSourceDescriptor>,
}

#[cfg(windows)]
struct DesktopMidiLearn {
    id: u64,
    started_at: Instant,
    candidate: Option<MidiLearnCandidate>,
}

enum PluginInstallEvent {
    Inspected {
        archive: PathBuf,
        bytes: Vec<u8>,
        inspection: LocalPackageInspection,
    },
    Installed {
        archive: PathBuf,
        inspection: LocalPackageInspection,
        installed: InstalledPackage,
        activation: PluginInstallActivation,
    },
}

#[derive(Debug, Eq, PartialEq)]
enum PluginInstallActivation {
    Reload,
    Restart,
    KeepCurrent { active_version: String },
}

fn plugin_install_activation(
    active_version: Option<&Version>,
    incoming_version: &Version,
) -> PluginInstallActivation {
    active_version.map_or(PluginInstallActivation::Reload, |active_version| {
        if incoming_version > active_version {
            PluginInstallActivation::Restart
        } else {
            PluginInstallActivation::KeepCurrent {
                active_version: active_version.to_string(),
            }
        }
    })
}

#[derive(Clone, Copy)]
enum PluginInstallPhase {
    Inspecting,
    Installing,
}

struct PluginInstallTask {
    phase: PluginInstallPhase,
    receiver: Receiver<Result<PluginInstallEvent, String>>,
}

impl PluginInstallPhase {
    fn label(self) -> &'static str {
        match self {
            Self::Inspecting => "inspection",
            Self::Installing => "installation",
        }
    }
}

fn validate_selected_archive(path: &Path) -> Result<(), String> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rfplugin"))
    {
        return Err(format!("{} is not a .rfplugin package", path.display()));
    }
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if metadata.len() == 0 {
        return Err(format!("{} is empty", path.display()));
    }
    if metadata.len() > MAX_PACKAGE_BYTES {
        return Err(format!(
            "{} is too large (maximum {})",
            path.display(),
            format_file_size(MAX_PACKAGE_BYTES)
        ));
    }
    Ok(())
}

fn read_plugin_archive_limited(path: &Path) -> Result<Vec<u8>, String> {
    validate_selected_archive(path)?;
    let file =
        File::open(path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let initial_size = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let mut bytes = Vec::with_capacity(initial_size.min(MAX_PACKAGE_BYTES) as usize);
    file.take(MAX_PACKAGE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    if bytes.is_empty() {
        return Err(format!("{} became empty while reading", path.display()));
    }
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(format!(
            "{} exceeded the {} limit while reading",
            path.display(),
            format_file_size(MAX_PACKAGE_BYTES)
        ));
    }
    Ok(bytes)
}

fn plugin_kind_label(kind: PluginKind) -> &'static str {
    match kind {
        PluginKind::Instrument => "Instrument",
        PluginKind::Effect => "Effect",
        PluginKind::MidiProcessor => "MIDI processor",
    }
}

fn format_file_size(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / MIB)
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} bytes")
    }
}

struct DesktopApp {
    /// Set when a live panel edit has not been persisted yet: the state is
    /// saved a breath after the last touch, and restored at the next start.
    live_state_dirty: Option<Instant>,
    menu: Menu,
    session: Arc<RwLock<SessionState>>,
    session_checkpoint: SessionCheckpointStore,
    button_down: [Option<Instant>; 4],
    keyboard_down: [Option<Instant>; 4],
    #[cfg(windows)]
    controller_button_down: [Option<Instant>; 4],
    #[cfg(windows)]
    controller_button_long_fired: [bool; 4],
    #[cfg(windows)]
    controller_home_chord_emitted: bool,
    #[cfg(windows)]
    controller_encoder_down: Option<Instant>,
    #[cfg(windows)]
    controller_header_restore_at: Option<Instant>,
    #[cfg(windows)]
    controller_parameter_mapper: RackForgeParameterMapper,
    web_url: String,
    web_servers: web::DesktopWebServers,
    web_control: Receiver<web::DesktopControlCall>,
    web_preferences: web::WebServerPreferences,
    web_config_path: PathBuf,
    status: String,
    plugins: Vec<DesktopPlugin>,
    options: Options,
    plugin_install: Option<PluginInstallTask>,
    performance_repository: PerformanceRepository,
    /// Mirrors the repository's revision for the web layer's sockets, so
    /// every connected client learns about an edit made by any of them.
    performance_revision_shared: Arc<RwLock<String>>,
    state_store: PluginStateStore,
    /// What PLAY was sounding before LIVE borrowed the voice.
    ///
    /// The Desktop renders one voice, so putting a Rack on stage overwrites
    /// the instrument and the sound the player had set up in PLAY. Leaving
    /// LIVE has to give it back: PLAY and LIVE are two modes, and a mode that
    /// forgets what you left in it is not a mode.
    play_voice: Option<(InstanceId, Vec<u8>)>,
    /// Runtime controller defaults. They are deliberately not persisted as
    /// user MIDI links; the signed controller package registers them again.
    controller_semantic_profiles: BTreeMap<String, RegisteredSemanticProfile>,
    virtual_midi: BTreeMap<ClientId, VirtualMidiClientState>,
    next_program_draft_id: u64,
    next_audition_lease_id: u64,
    #[cfg(windows)]
    audio: Option<desktop_audio::DesktopAudio>,
    #[cfg(windows)]
    audio_preferences: Option<desktop_audio::AudioPreferences>,
    #[cfg(windows)]
    audio_config_path: PathBuf,
    #[cfg(windows)]
    audio_recovery_at: Option<Instant>,
    #[cfg(windows)]
    audio_recovery_attempts: u32,
    /// Stall watchdog: the last callback count seen and when it last moved.
    /// A frozen counter is the only witness to an ASIO driver that stopped
    /// calling back (another client grabbed the hardware) -- that death
    /// reports no stream error at all.
    #[cfg(windows)]
    audio_watchdog: Option<(u64, Instant)>,
    /// When the last stall fired, so a device that dies over and over gets
    /// exponential patience instead of a tight reopen loop.
    #[cfg(windows)]
    audio_last_stall: Option<Instant>,
    /// Device inventory cache: enumerating instantiates every ASIO driver,
    /// and instantiating the driver that is currently streaming is asking a
    /// single-client driver for trouble. Settings reads within the TTL see
    /// the cached scan.
    #[cfg(windows)]
    audio_inventory_cache: Option<(Instant, desktop_audio::AudioInventory)>,
    #[cfg(windows)]
    midi_learn: Option<DesktopMidiLearn>,
    #[cfg(windows)]
    next_midi_learn_id: u64,
    /// Raised on exit so the controller supervisor reaps its drivers.
    controller_shutdown: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// Joined on exit so the controller driver can restore its hardware
    /// before Desktop releases the process and MIDI stack.
    controller_supervisor: Option<std::thread::JoinHandle<()>>,
}

impl DesktopApp {
    fn new(
        session: Arc<RwLock<SessionState>>,
        performance_revision_shared: Arc<RwLock<String>>,
        options: &Options,
        web_servers: web::DesktopWebServers,
        web_control: Receiver<web::DesktopControlCall>,
    ) -> Result<Self> {
        let (mut plugins, mut warnings) = load_desktop_plugins(options)?;
        let session_checkpoint = SessionCheckpointStore::live(&options.data_root);
        let session_id = session
            .read()
            .expect("session lock poisoned")
            .session_id
            .clone();
        let restored_mode = match session_checkpoint.active_mode(&session_id) {
            Ok(mode) => mode,
            Err(error) => {
                warnings.push(format!(
                    "Could not restore the previous session ({error:#}); using safe defaults"
                ));
                None
            }
        };
        let restored_active_instance = if restored_mode.is_some() {
            session_checkpoint
                .active_instance_id(&session_id)
                .unwrap_or_else(|error| {
                    warnings.push(format!(
                        "Could not restore the active plugin ({error:#}); using the first available plugin"
                    ));
                    None
                })
        } else {
            None
        };
        if restored_mode.is_some() {
            for plugin in &mut plugins {
                let restored_sound = session_checkpoint
                    .selected_sound(&session_id, &plugin.instance_id)
                    .unwrap_or_else(|error| {
                        warnings.push(format!(
                            "Could not restore {}'s program ({error:#})",
                            plugin.name
                        ));
                        None
                    });
                let Some(sound_id) = restored_sound else {
                    continue;
                };
                if !plugin.sounds.iter().any(|sound| sound.id == sound_id) {
                    warnings.push(format!(
                        "{} no longer provides saved program {sound_id:?}; using its first available program",
                        plugin.name
                    ));
                    continue;
                }
                match plugin.instance.load_preset(&sound_id) {
                    Ok(()) => plugin.selected_sound_id = Some(sound_id),
                    Err(error) => warnings.push(format!(
                        "Could not restore {} program {sound_id:?} ({error:#}); using its first available program",
                        plugin.name
                    )),
                }
            }
        }
        let active_instance_id = restored_active_instance
            .as_deref()
            .and_then(|restored| plugins.iter().find(|plugin| plugin.instance_id == restored))
            .or_else(|| {
                rackforge_core::choose_opening_instrument(&plugins, |plugin| {
                    plugin.plugin_id.as_str()
                })
            })
            .map(|plugin| plugin.instance_id.as_str());
        if let Some(restored) = restored_active_instance.as_deref()
            && active_instance_id != Some(restored)
        {
            warnings.push(format!(
                "Saved plugin instance {restored:?} is unavailable; using the first available plugin"
            ));
        }
        let restored_master_level =
            session_checkpoint
                .master_level(&session_id)
                .unwrap_or_else(|error| {
                    warnings.push(format!("Could not restore master volume: {error:#}"));
                    None
                });
        let restored_master_pan =
            session_checkpoint
                .master_pan(&session_id)
                .unwrap_or_else(|error| {
                    warnings.push(format!("Could not restore master pan: {error:#}"));
                    None
                });
        let restored_parameter_links = session_checkpoint
            .parameter_links(&session_id)
            .unwrap_or_else(|error| {
                warnings.push(format!("Could not restore MIDI parameter links: {error:#}"));
                Vec::new()
            });
        let performance_repository = PerformanceRepository::load_or_empty(Some(&options.data_root))
            .context("loading Desktop performance library")?;
        *performance_revision_shared
            .write()
            .expect("performance revision lock poisoned") =
            performance_repository.revision().as_str().to_owned();
        let state_store = PluginStateStore::new(Some(&options.data_root))
            .context("loading Desktop plugin-state store")?;
        #[cfg(windows)]
        let audio_config_path = options.rackforge_root.join("config/audio.toml");
        #[cfg(windows)]
        let (audio_preferences, audio) = match desktop_audio::AudioInventory::scan() {
            Ok(inventory) => match inventory.default_preferences() {
                Ok(defaults) => {
                    let preferences = match desktop_audio::AudioPreferences::load(
                        &audio_config_path,
                    ) {
                        Ok(Some(saved)) => match inventory.validate(&saved) {
                            Ok(()) => saved,
                            Err(error) => {
                                warnings.push(format!(
                                    "Saved audio configuration is unavailable ({error:#}); using the system default"
                                ));
                                // Audio outputs may be disconnected independently from MIDI
                                // controllers. Keep the user's MIDI selection so hot-plug can
                                // reconnect those devices after the fallback stream starts.
                                desktop_audio::fallback_preserving_midi(defaults, &saved)
                            }
                        },
                        Ok(None) => defaults,
                        Err(error) => {
                            warnings.push(format!(
                                "Could not load audio configuration ({error:#}); using the system default"
                            ));
                            defaults
                        }
                    };
                    match start_desktop_audio(
                        &plugins,
                        &preferences,
                        active_instance_id,
                        &options.data_root.join("states/live"),
                        external_controller_enabled(&options.rackforge_root),
                    ) {
                        Ok(audio) => (Some(preferences), Some(audio)),
                        Err(error) => {
                            let message = format!("Audio/MIDI unavailable: {error:#}");
                            eprintln!("DESKTOP_AUDIO_UNAVAILABLE {message}");
                            warnings.push(message.clone());
                            (Some(preferences), None)
                        }
                    }
                }
                Err(error) => {
                    warnings.push(format!("Audio/MIDI unavailable: {error:#}"));
                    (None, None)
                }
            },
            Err(error) => {
                warnings.push(format!("Audio/MIDI discovery failed: {error:#}"));
                (None, None)
            }
        };
        let mut menu = Menu::default();
        menu.set_play_plugins(
            plugins
                .iter()
                .map(|plugin| {
                    PlayPlugin::new(&plugin.instance_id, &plugin.plugin_id, &plugin.name)
                        .short_name(plugin.runtime.manifest().little_short_name())
                        .config_available(plugin.config_available)
                })
                .collect(),
            active_instance_id,
        );
        if let Some(plugin) = active_instance_id
            .and_then(|active| plugins.iter().find(|plugin| plugin.instance_id == active))
        {
            menu.sync_active_plugin(
                &plugin.instance_id,
                &plugin.plugin_id,
                &plugin.name,
                plugin.sounds.clone(),
                plugin.selected_sound_id.as_deref(),
            );
            if let Ok(presets) = state_store.list_presets(&plugin.plugin_id) {
                menu.set_plugin_presets(little_host_presets(presets), None);
            }
        }
        {
            let mut state = session.write().expect("session lock poisoned");
            if let Some(mode) = restored_mode {
                state.active_mode = mode;
            }
            if let Some(level) = restored_master_level {
                state.master_level = level;
            }
            if let Some(pan) = restored_master_pan {
                state.master_pan = pan;
            }
            state.active_instance_id = active_instance_id
                .map(InstanceId::new)
                .transpose()
                .map_err(anyhow::Error::msg)?;
            state.instances = plugins.iter().map(plugin_session_state).collect();
            state.parameter_links = restored_parameter_links.clone();
            menu.sync_active_mode(active_mode_from_surface(state.active_mode));
        }
        #[cfg(windows)]
        let controller_semantic_profiles = match audio_preferences.as_ref().map(|preferences| {
            declarative_semantic_profiles(&options.rackforge_root, &preferences.midi_inputs)
        }) {
            Some(Ok(profiles)) => profiles,
            Some(Err(error)) => {
                warnings.push(format!(
                    "Declarative controller mappings were disabled: {error:#}"
                ));
                BTreeMap::new()
            }
            None => BTreeMap::new(),
        };
        #[cfg(not(windows))]
        let controller_semantic_profiles = BTreeMap::new();
        #[cfg(windows)]
        if let Some(audio) = &audio {
            sync_desktop_audio(audio, &session, &menu)?;
            audio.replace_parameter_links(compile_desktop_parameter_links(
                &restored_parameter_links,
                &plugins,
                &performance_repository,
                &controller_semantic_profiles,
            )?)?;
        }
        if let Err(error) =
            session_checkpoint.save(&session.read().expect("session lock poisoned").clone())
        {
            warnings.push(format!("Could not save the restored session: {error:#}"));
        }

        let status = if plugins.is_empty() {
            if warnings.is_empty() {
                format!("No plugins installed in {}", options.plugins_root.display())
            } else {
                warnings.join(" · ")
            }
        } else if warnings.is_empty() {
            format!("{} plugin(s) ready", plugins.len())
        } else {
            format!(
                "{} plugin(s) ready · {}",
                plugins.len(),
                warnings.join(" · ")
            )
        };

        let web_url = web_servers.local_url().to_owned();
        // Point surface notes at the engine that just started, and hand over
        // the cell it writes each strike into. Both, and in the same breath:
        // handing over only the first meant the velocity square saw nothing
        // until something republished the runtime -- which is what pressing
        // Apply does, so the curve looked as though it needed applying before
        // it would follow the keyboard.
        web_servers.set_injected_midi(audio.as_ref().map(|audio| audio.injected_midi_sender()));
        web_servers.set_last_strike(audio.as_ref().map(|audio| audio.last_strike_cell()));
        #[cfg(windows)]
        let audio_recovery_at =
            if audio.is_none() && audio_preferences.is_some() && !plugins.is_empty() {
                Some(Instant::now() + Duration::from_secs(1))
            } else {
                None
            };
        let mut app = Self {
            menu,
            session,
            session_checkpoint,
            button_down: [None; 4],
            keyboard_down: [None; 4],
            #[cfg(windows)]
            controller_button_down: [None; 4],
            #[cfg(windows)]
            controller_button_long_fired: [false; 4],
            #[cfg(windows)]
            controller_home_chord_emitted: false,
            #[cfg(windows)]
            controller_encoder_down: None,
            #[cfg(windows)]
            controller_header_restore_at: None,
            #[cfg(windows)]
            controller_parameter_mapper: RackForgeParameterMapper::default(),
            web_url,
            web_servers,
            web_control,
            web_preferences: options.web_preferences.clone(),
            web_config_path: options.rackforge_root.join("config/web.toml"),
            status,
            plugins,
            options: options.clone(),
            plugin_install: None,
            performance_repository,
            performance_revision_shared,
            state_store,
            play_voice: None,
            live_state_dirty: None,
            controller_semantic_profiles,
            virtual_midi: BTreeMap::new(),
            next_program_draft_id: 1,
            next_audition_lease_id: 1,
            #[cfg(windows)]
            audio,
            #[cfg(windows)]
            audio_preferences,
            #[cfg(windows)]
            audio_config_path,
            #[cfg(windows)]
            audio_recovery_at,
            #[cfg(windows)]
            audio_recovery_attempts: 0,
            #[cfg(windows)]
            audio_watchdog: None,
            #[cfg(windows)]
            audio_last_stall: None,
            #[cfg(windows)]
            audio_inventory_cache: None,
            #[cfg(windows)]
            midi_learn: None,
            #[cfg(windows)]
            next_midi_learn_id: 1,
            controller_shutdown: None,
            controller_supervisor: None,
        };
        app.sync_little_plugin_parameters();
        Ok(app)
    }

    fn reload_plugins(&mut self) -> Result<Vec<String>> {
        let (previous_active, previous_mode, previous_sounds) = {
            let session = self.session.read().expect("session lock poisoned");
            (
                session
                    .active_instance_id
                    .as_ref()
                    .map(|id| id.as_str().to_owned()),
                session.active_mode,
                session
                    .instances
                    .iter()
                    .filter_map(|instance| {
                        instance
                            .selected_sound_id
                            .as_ref()
                            .map(|sound| (instance.instance_id.as_str().to_owned(), sound.clone()))
                    })
                    .collect::<BTreeMap<_, _>>(),
            )
        };
        let (mut plugins, mut warnings) = load_desktop_plugins(&self.options)?;
        for plugin in &mut plugins {
            let Some(sound_id) = previous_sounds.get(&plugin.instance_id) else {
                continue;
            };
            if plugin.sounds.iter().any(|sound| sound.id == *sound_id) {
                match plugin.instance.load_preset(sound_id) {
                    Ok(()) => plugin.selected_sound_id = Some(sound_id.clone()),
                    Err(error) => warnings.push(format!(
                        "Could not retain {} program {sound_id:?} after reload: {error:#}",
                        plugin.name
                    )),
                }
            }
        }
        let active_instance_id = previous_active
            .as_deref()
            .and_then(|previous| plugins.iter().find(|plugin| plugin.instance_id == previous))
            .or_else(|| {
                rackforge_core::choose_opening_instrument(&plugins, |plugin| {
                    plugin.plugin_id.as_str()
                })
            })
            .map(|plugin| plugin.instance_id.as_str());
        #[cfg(windows)]
        let mut replacement_audio = None;
        #[cfg(windows)]
        {
            // Same reason as the audio-settings path: the rebuilt instances
            // restore their live state, so it has to be current first.
            #[cfg(windows)]
            self.flush_live_state();
            // Catalog reloads replace every DSP instance. Retire the old
            // generation through the single audio hand-off path so Web MIDI
            // cannot keep a sender whose receiver dies with that generation.
            self.stop_audio_runtime();
            if let Some(preferences) = self.audio_preferences.as_ref() {
                match start_desktop_audio(
                    &plugins,
                    preferences,
                    active_instance_id,
                    &self.live_state_dir(),
                    external_controller_enabled(&self.options.rackforge_root),
                ) {
                    Ok(audio) => {
                        replacement_audio = Some(audio);
                    }
                    Err(error) => {
                        warnings.push(format!("Audio/MIDI unavailable: {error:#}"));
                        self.audio_recovery_at = Some(Instant::now() + Duration::from_secs(1));
                        self.audio_recovery_attempts = 0;
                    }
                }
            }
        }
        let mut menu = Menu::default();
        menu.set_play_plugins(
            plugins
                .iter()
                .map(|plugin| {
                    PlayPlugin::new(&plugin.instance_id, &plugin.plugin_id, &plugin.name)
                        .short_name(plugin.runtime.manifest().little_short_name())
                        .config_available(plugin.config_available)
                })
                .collect(),
            active_instance_id,
        );
        if let Some(plugin) = active_instance_id
            .and_then(|active| plugins.iter().find(|plugin| plugin.instance_id == active))
        {
            menu.sync_active_plugin(
                &plugin.instance_id,
                &plugin.plugin_id,
                &plugin.name,
                plugin.sounds.clone(),
                plugin.selected_sound_id.as_deref(),
            );
            if let Ok(presets) = self.state_store.list_presets(&plugin.plugin_id) {
                menu.set_plugin_presets(little_host_presets(presets), None);
            }
        }
        menu.sync_active_mode(active_mode_from_surface(previous_mode));
        {
            let mut state = self.session.write().expect("session lock poisoned");
            state.active_instance_id = active_instance_id
                .map(InstanceId::new)
                .transpose()
                .map_err(anyhow::Error::msg)?;
            state.instances = plugins.iter().map(plugin_session_state).collect();
            state.revision = Revision::new(state.revision.get().saturating_add(1));
        }
        self.menu = menu;
        self.plugins = plugins;
        #[cfg(windows)]
        if let Some(audio) = replacement_audio {
            match sync_desktop_audio(&audio, &self.session, &self.menu) {
                Ok(()) => {
                    self.publish_audio_runtime(audio);
                    self.audio_recovery_at = None;
                    self.audio_recovery_attempts = 0;
                }
                Err(error) => {
                    warnings.push(format!("Audio/MIDI synchronization failed: {error:#}"));
                    self.audio_recovery_at = Some(Instant::now() + Duration::from_secs(1));
                    self.audio_recovery_attempts = 0;
                }
            }
        }
        self.sync_little_plugin_parameters();
        self.persist_session_checkpoint();
        Ok(warnings)
    }

    #[cfg(windows)]
    /// Retires the current engine generation and makes every producer stop
    /// targeting it before its MIDI/audio receivers are dropped.
    fn stop_audio_runtime(&mut self) {
        self.web_servers.set_injected_midi(None);
        self.audio = None;
        self.audio_watchdog = None;
    }

    #[cfg(windows)]
    /// Publishes a fully started engine as one generation. Hardware MIDI is
    /// already connected by `DesktopAudio::start`; publishing its surface
    /// sender last makes Touch Controller switch to the same generation.
    fn publish_audio_runtime(&mut self, audio: desktop_audio::DesktopAudio) {
        let injected_midi = audio.injected_midi_sender();
        let last_strike = audio.last_strike_cell();
        self.audio = Some(audio);
        self.audio_watchdog = None;
        self.web_servers.set_injected_midi(Some(injected_midi));
        self.web_servers.set_last_strike(Some(last_strike));
    }

    #[cfg(windows)]
    fn poll_audio_error(&mut self) {
        // The stall watchdog. An ASIO driver whose hardware another client
        // grabbed (the Focusrite when a WASAPI session opens the same
        // interface, a control-panel reset, a sample-rate change) stops
        // calling back WITHOUT reporting anything: the stream object stays
        // alive, the error callback stays silent, and the app used to keep
        // showing a healthy summary over a dead engine while every played
        // note landed in a closed channel. A healthy stream renders blocks
        // continuously -- silence included -- so a counter that has not
        // moved in two seconds IS the failure, and it feeds the same
        // recovery path a reported error does.
        if let Some(audio) = &self.audio {
            let blocks = audio.callback_blocks();
            match self.audio_watchdog {
                Some((last, since)) if blocks == last => {
                    if since.elapsed() >= Duration::from_secs(2) {
                        self.stop_audio_runtime();
                        // A device that keeps dying earns exponential
                        // patience; one that stays up half a minute earns a
                        // fresh start.
                        let repeated = self
                            .audio_last_stall
                            .is_some_and(|at| at.elapsed() < Duration::from_secs(30));
                        if repeated {
                            self.audio_recovery_attempts =
                                self.audio_recovery_attempts.saturating_add(1);
                        } else {
                            self.audio_recovery_attempts = 0;
                        }
                        self.audio_last_stall = Some(Instant::now());
                        let exponent = self.audio_recovery_attempts.min(5);
                        let delay = Duration::from_millis(250_u64.saturating_mul(1 << exponent));
                        self.audio_recovery_at = Some(Instant::now() + delay);
                        self.status =
                            "Audio stream stalled (the driver stopped calling back) · reconnecting audio…"
                                .into();
                        eprintln!("DESKTOP_AUDIO_STALL_DETECTED blocks={blocks}");
                    }
                }
                Some((last, _)) if blocks != last => {
                    self.audio_watchdog = Some((blocks, Instant::now()));
                }
                None => {
                    self.audio_watchdog = Some((blocks, Instant::now()));
                }
                _ => {}
            }
        } else {
            self.audio_watchdog = None;
        }
        let stream_error = self.audio.as_ref().and_then(|audio| audio.take_error());
        if let Some(error) = stream_error {
            self.stop_audio_runtime();
            self.audio_recovery_attempts = 0;
            self.audio_recovery_at = Some(Instant::now() + Duration::from_millis(250));
            self.status = format!("{error} · reconnecting audio…");
            eprintln!("DESKTOP_AUDIO_RECOVERY_SCHEDULED error={error}");
        }
        let Some(retry_at) = self.audio_recovery_at else {
            return;
        };
        if Instant::now() < retry_at || self.audio.is_some() {
            return;
        }
        let Some(preferences) = self.audio_preferences.clone() else {
            self.audio_recovery_at = None;
            return;
        };
        let active = self
            .session
            .read()
            .expect("session lock poisoned")
            .active_instance_id
            .as_ref()
            .map(|id| id.as_str().to_owned());
        self.audio_recovery_at = None;
        match start_desktop_audio(
            &self.plugins,
            &preferences,
            active.as_deref(),
            &self.live_state_dir(),
            external_controller_enabled(&self.options.rackforge_root),
        ) {
            Ok(audio) => {
                if let Err(error) = sync_desktop_audio(&audio, &self.session, &self.menu) {
                    self.audio_recovery_attempts = self.audio_recovery_attempts.saturating_add(1);
                    self.audio_recovery_at = Some(Instant::now() + Duration::from_secs(1));
                    self.status =
                        format!("Audio reconnect synchronization failed · retrying · {error:#}");
                    return;
                }
                let summary = audio.summary().to_owned();
                self.publish_audio_runtime(audio);
                self.audio_recovery_attempts = 0;
                self.status = format!("Audio reconnected · {summary}");
                println!("DESKTOP_AUDIO_RECOVERED {summary}");
            }
            Err(error) => {
                self.audio_recovery_attempts = self.audio_recovery_attempts.saturating_add(1);
                let exponent = self.audio_recovery_attempts.min(5);
                let delay = Duration::from_millis(250_u64.saturating_mul(1_u64 << exponent));
                self.audio_recovery_at = Some(Instant::now() + delay);
                self.status = format!(
                    "Audio unavailable · retry {} in {:.1}s · {error:#}",
                    self.audio_recovery_attempts,
                    delay.as_secs_f32()
                );
                eprintln!(
                    "DESKTOP_AUDIO_RECOVERY_FAILED attempt={} retry_ms={} error={error:#}",
                    self.audio_recovery_attempts,
                    delay.as_millis()
                );
            }
        }
    }

    #[cfg(windows)]
    fn audio_summary(&self) -> String {
        self.audio.as_ref().map_or_else(
            || "Audio/MIDI unavailable".into(),
            |audio| audio.diagnostics(),
        )
    }

    #[cfg(windows)]
    /// The device inventory, without ever re-instantiating the ASIO driver
    /// that is streaming right now: enumerating instantiates every ASIO
    /// driver, and doing that to the live one stops the stream dead
    /// (measured on the Focusrite -- the callback froze the moment a scan
    /// ran). While ASIO is active, other backends are scanned fresh and the
    /// live driver's rows come from the cache; a short TTL keeps repeated
    /// settings reads from hammering the drivers either way.
    fn scan_inventory(&mut self) -> Result<desktop_audio::AudioInventory> {
        const INVENTORY_TTL: Duration = Duration::from_secs(10);
        if let Some((at, cached)) = &self.audio_inventory_cache
            && at.elapsed() < INVENTORY_TTL
        {
            return Ok(cached.clone());
        }
        let streaming_driver = if self.audio.is_some() {
            self.audio_preferences
                .as_ref()
                .map(|preferences| preferences.driver.clone())
                .filter(|driver| driver == "ASIO")
        } else {
            None
        };
        let inventory = match streaming_driver.as_deref() {
            Some(live) => {
                let mut fresh = desktop_audio::AudioInventory::scan_skipping(Some(live))?;
                match &self.audio_inventory_cache {
                    Some((_, cached)) => {
                        fresh
                            .drivers
                            .extend(cached.drivers.iter().filter(|d| d.name == live).cloned());
                        fresh
                            .outputs
                            .extend(cached.outputs.iter().filter(|o| o.driver == live).cloned());
                        fresh
                            .inputs
                            .extend(cached.inputs.iter().filter(|i| i.driver == live).cloned());
                    }
                    None => fresh.drivers.push(desktop_audio::AudioDriverInfo {
                        name: live.to_owned(),
                        available: true,
                        detail: "In use by the current stream".into(),
                    }),
                }
                fresh.outputs.sort_by(|left, right| {
                    left.driver
                        .cmp(&right.driver)
                        .then_with(|| right.is_default.cmp(&left.is_default))
                        .then_with(|| left.name.cmp(&right.name))
                });
                fresh.inputs.sort_by(|left, right| {
                    left.driver
                        .cmp(&right.driver)
                        .then_with(|| right.is_default.cmp(&left.is_default))
                        .then_with(|| left.name.cmp(&right.name))
                });
                fresh
            }
            None => desktop_audio::AudioInventory::scan()?,
        };
        self.audio_inventory_cache = Some((Instant::now(), inventory.clone()));
        Ok(inventory)
    }

    #[cfg(windows)]
    fn audio_settings_json(&mut self) -> Result<serde_json::Value> {
        let inventory = self.scan_inventory()?;
        let preferences = self
            .audio_preferences
            .clone()
            .map_or_else(|| inventory.default_preferences(), Ok)?;
        // Each port's identity beside its name, so the interface can tell
        // which keybed a strike came from without knowing how the identity is
        // derived.
        let midi_source_keys: serde_json::Map<String, serde_json::Value> = inventory
            .midi_inputs
            .iter()
            .map(|name| {
                (
                    name.clone(),
                    serde_json::Value::from(desktop_audio::stable_midi_source_key(name).get()),
                )
            })
            .collect();
        Ok(serde_json::json!({
            "status": "ok",
            "host": "desktop",
            "inventory": inventory,
            "preferences": preferences,
            "midi_source_keys": midi_source_keys,
            "runtime_status": self.audio_summary(),
        }))
    }

    #[cfg(windows)]
    fn apply_web_preferences(&mut self, preferences: web::WebServerPreferences) -> Result<String> {
        let previous = self.web_preferences.clone();
        self.web_servers.apply(preferences.clone())?;
        if let Err(error) = preferences.persist(&self.web_config_path) {
            let rollback = self.web_servers.apply(previous.clone());
            return match rollback {
                Ok(()) => Err(anyhow::anyhow!(
                    "Could not save HTTP server settings: {error:#}. The previous settings were restored"
                )),
                Err(rollback) => Err(anyhow::anyhow!(
                    "Could not save HTTP server settings: {error:#}. Restoring the previous server also failed: {rollback:#}"
                )),
            };
        }
        self.web_preferences = preferences.clone();
        Ok(if preferences.enabled {
            format!(
                "HTTP server is available on the network at port {}",
                preferences.port
            )
        } else {
            "HTTP server is disabled".into()
        })
    }

    #[cfg(windows)]
    fn apply_audio_preferences(
        &mut self,
        preferences: desktop_audio::AudioPreferences,
    ) -> Result<String> {
        // Dragging a point on a velocity curve is not a device change.
        // Everything below tears the stream down and scans the drivers, which
        // for a MIDI reading would mean a gap in the sound and, on
        // single-client ASIO hardware, a device that has to be re-acquired.
        // So if the readings are the only thing that moved, they go to the
        // audio thread as they are and nothing is rebuilt.
        if let Some(current) = self.audio_preferences.clone() {
            let mut probe = preferences.clone();
            probe.velocity_curve = current.velocity_curve;
            probe.velocity_curves = current.velocity_curves.clone();
            let readings_moved = preferences.velocity_curve != current.velocity_curve
                || preferences.velocity_curves != current.velocity_curves;
            if probe == current && readings_moved {
                if let Some(audio) = self.audio.as_ref() {
                    audio
                        .set_velocity_curves(
                            preferences.velocity_curve,
                            &preferences.velocity_curves,
                        )
                        .context("sending the velocity readings to the audio runtime")?;
                }
                preferences
                    .persist(&self.audio_config_path)
                    .context("saving the velocity readings")?;
                self.audio_preferences = Some(preferences);
                return Ok("Velocity curve applied".to_owned());
            }
        }
        let previous = self.audio_preferences.clone();
        let active = self
            .session
            .read()
            .expect("session lock poisoned")
            .active_instance_id
            .as_ref()
            .map(|id| id.as_str().to_owned());
        // Write down what is sounding before tearing it down. A restart
        // rebuilds each instance by loading its preset and THEN its live
        // state, so a state file older than the last program change puts
        // the old program back — the program a player picked seconds
        // before touching the audio settings would silently revert,
        // because the periodic flush had not come round yet.
        #[cfg(windows)]
        self.flush_live_state();
        // The stream comes down BEFORE the scan: enumerating instantiates
        // every ASIO driver, and instantiating the live one kills its
        // stream anyway (measured). Validation failures restore the
        // previous stream on the way out.
        self.stop_audio_runtime();
        let inventory = match desktop_audio::AudioInventory::scan() {
            Ok(inventory) => inventory,
            Err(error) => {
                return match self.restore_audio(previous.as_ref(), active.as_deref()) {
                    Ok(()) => Err(anyhow::anyhow!(
                        "Could not scan audio devices: {error:#}. The previous settings were restored"
                    )),
                    Err(rollback) => Err(anyhow::anyhow!(
                        "Could not scan audio devices: {error:#}. Reopening the previous audio settings also failed: {rollback:#}"
                    )),
                };
            }
        };
        self.audio_inventory_cache = Some((Instant::now(), inventory.clone()));
        if let Err(error) = inventory.validate(&preferences) {
            return match self.restore_audio(previous.as_ref(), active.as_deref()) {
                Ok(()) => Err(anyhow::anyhow!(
                    "{error:#}. The previous settings were kept"
                )),
                Err(rollback) => Err(anyhow::anyhow!(
                    "{error:#}. Reopening the previous audio settings also failed: {rollback:#}"
                )),
            };
        }
        let candidate = match start_desktop_audio(
            &self.plugins,
            &preferences,
            active.as_deref(),
            &self.live_state_dir(),
            external_controller_enabled(&self.options.rackforge_root),
        ) {
            Ok(audio) => audio,
            Err(error) => {
                return match self.restore_audio(previous.as_ref(), active.as_deref()) {
                    Ok(()) => Err(anyhow::anyhow!(
                        "Could not apply audio settings: {error:#}. The previous settings were restored"
                    )),
                    Err(rollback) => Err(anyhow::anyhow!(
                        "Could not apply audio settings: {error:#}. Reopening the previous audio settings also failed: {rollback:#}"
                    )),
                };
            }
        };
        if let Err(error) = sync_desktop_audio(&candidate, &self.session, &self.menu) {
            drop(candidate);
            return match self.restore_audio(previous.as_ref(), active.as_deref()) {
                Ok(()) => Err(anyhow::anyhow!(
                    "The new audio stream opened but could not synchronize: {error:#}. The previous settings were restored"
                )),
                Err(rollback) => Err(anyhow::anyhow!(
                    "The new audio stream opened but could not synchronize: {error:#}. Reopening the previous audio settings also failed: {rollback:#}"
                )),
            };
        }
        if let Err(error) = preferences.persist(&self.audio_config_path) {
            drop(candidate);
            return match self.restore_audio(previous.as_ref(), active.as_deref()) {
                Ok(()) => Err(anyhow::anyhow!(
                    "The new audio stream opened, but its settings could not be saved: {error:#}. The previous settings were restored"
                )),
                Err(rollback) => Err(anyhow::anyhow!(
                    "The new audio stream opened, but its settings could not be saved: {error:#}. Reopening the previous audio settings also failed: {rollback:#}"
                )),
            };
        }
        let summary = candidate.summary().to_owned();
        self.publish_audio_runtime(candidate);
        self.audio_preferences = Some(preferences);
        self.audio_recovery_at = None;
        self.audio_recovery_attempts = 0;
        self.status = format!("Audio settings applied · {summary}");
        Ok(format!("Settings applied · {summary}"))
    }

    #[cfg(windows)]
    fn restore_audio(
        &mut self,
        preferences: Option<&desktop_audio::AudioPreferences>,
        active_instance_id: Option<&str>,
    ) -> Result<()> {
        self.stop_audio_runtime();
        let restored = match preferences
            .map(|preferences| {
                start_desktop_audio(
                    &self.plugins,
                    preferences,
                    active_instance_id,
                    &self.options.data_root.join("states/live"),
                    external_controller_enabled(&self.options.rackforge_root),
                )
                .context("reopening the previous audio stream")
            })
            .transpose()
        {
            Ok(restored) => restored,
            Err(error) => {
                self.audio_recovery_attempts = 0;
                self.audio_recovery_at = Some(Instant::now() + Duration::from_secs(1));
                return Err(error);
            }
        };
        if let Some(audio) = &restored
            && let Err(error) = sync_desktop_audio(audio, &self.session, &self.menu)
        {
            self.audio_recovery_attempts = 0;
            self.audio_recovery_at = Some(Instant::now() + Duration::from_secs(1));
            return Err(error);
        }
        if let Some(audio) = restored {
            self.publish_audio_runtime(audio);
        }
        self.audio_recovery_at = None;
        self.audio_recovery_attempts = 0;
        Ok(())
    }

    #[cfg(windows)]
    fn test_audio_note(&mut self) -> Result<()> {
        let audio = self
            .audio
            .as_ref()
            .context("Audio is not running; apply a valid configuration first")?;
        audio.test_note()?;
        self.status = "Playing audio test note".into();
        Ok(())
    }

    #[cfg(windows)]
    fn choose_plugin_archive() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Install RackForge plugin")
            .add_filter("RackForge plugin", &["rfplugin"])
            .pick_file()
    }

    #[cfg(not(windows))]
    fn choose_plugin_archive() -> Option<PathBuf> {
        None
    }

    fn begin_plugin_install(&mut self) {
        if self.plugin_install.is_some() {
            self.status = "A plugin installation is already in progress".into();
            return;
        }
        let Some(store_root) = self.options.plugin_store_root.clone() else {
            self.status =
                "Local install requires --rackforge-root instead of legacy path flags".into();
            Self::show_install_error(&self.status);
            return;
        };
        let Some(archive) = Self::choose_plugin_archive() else {
            return;
        };
        if let Err(error) = validate_selected_archive(&archive) {
            self.status = error;
            Self::show_install_error(&self.status);
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let worker_archive = archive.clone();
        let spawn = thread::Builder::new()
            .name("rackforge-plugin-inspection".into())
            .spawn(move || {
                let result = (|| {
                    let bytes = read_plugin_archive_limited(&worker_archive)?;
                    let inspection = inspect_local_archive(&store_root, &bytes)
                        .map_err(|error| format!("Package validation failed: {error}"))?;
                    Ok(PluginInstallEvent::Inspected {
                        archive: worker_archive,
                        bytes,
                        inspection,
                    })
                })();
                let _ = sender.send(result);
            });
        match spawn {
            Ok(_) => {
                self.status = format!("Inspecting {}…", archive.display());
                self.plugin_install = Some(PluginInstallTask {
                    phase: PluginInstallPhase::Inspecting,
                    receiver,
                });
            }
            Err(error) => {
                self.status = format!("Could not start package inspection: {error}");
                Self::show_install_error(&self.status);
            }
        }
    }

    fn begin_validated_install(
        &mut self,
        archive: PathBuf,
        bytes: Vec<u8>,
        inspection: LocalPackageInspection,
    ) {
        let store_root = self
            .options
            .plugin_store_root
            .clone()
            .expect("validated local installation requires a plugin store");
        let incoming_version = Version::parse(&inspection.version)
            .expect("validated plugin manifest contains a semantic version");
        let active_version = self
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == inspection.plugin_id)
            .map(|plugin| &plugin.version);
        let activation = plugin_install_activation(active_version, &incoming_version);
        let label = format!("{} {}", inspection.plugin_name, inspection.version);
        let archive_label = archive.display().to_string();
        let (sender, receiver) = mpsc::channel();
        let spawn = thread::Builder::new()
            .name("rackforge-plugin-installation".into())
            .spawn(move || {
                let result = install_local_archive(&store_root, &bytes)
                    .map(|installed| PluginInstallEvent::Installed {
                        archive,
                        inspection,
                        installed,
                        activation,
                    })
                    .map_err(|error| format!("Could not install {archive_label}: {error}"));
                let _ = sender.send(result);
            });
        match spawn {
            Ok(_) => {
                self.status = format!("Installing {label}…");
                self.plugin_install = Some(PluginInstallTask {
                    phase: PluginInstallPhase::Installing,
                    receiver,
                });
            }
            Err(error) => {
                self.status = format!("Could not start package installation: {error}");
                Self::show_install_error(&self.status);
            }
        }
    }

    fn poll_plugin_install(&mut self, context: &egui::Context) -> bool {
        let Some(task) = self.plugin_install.take() else {
            return false;
        };
        let phase = task.phase;
        match task.receiver.try_recv() {
            Err(TryRecvError::Empty) => {
                self.plugin_install = Some(task);
                context.request_repaint_after(Duration::from_millis(50));
                false
            }
            Err(TryRecvError::Disconnected) => {
                self.status = format!("The plugin {} worker stopped unexpectedly", phase.label());
                Self::show_install_error(&self.status);
                false
            }
            Ok(Err(error)) => {
                self.status = error;
                Self::show_install_error(&self.status);
                false
            }
            Ok(Ok(PluginInstallEvent::Inspected {
                archive,
                bytes,
                inspection,
            })) => {
                if Self::confirm_plugin_install(&archive, &inspection) {
                    self.begin_validated_install(archive, bytes, inspection);
                } else {
                    self.status = "Plugin installation cancelled".into();
                }
                false
            }
            Ok(Ok(PluginInstallEvent::Installed {
                archive,
                inspection,
                installed,
                activation,
            })) => self.finish_plugin_install(&archive, &inspection, installed, activation),
        }
    }

    fn finish_plugin_install(
        &mut self,
        archive: &Path,
        inspection: &LocalPackageInspection,
        installed: InstalledPackage,
        activation: PluginInstallActivation,
    ) -> bool {
        let label = format!("{} {}", inspection.plugin_name, inspection.version);
        if installed.already_installed {
            self.status = format!("{label} is already installed");
            Self::show_install_info("Plugin already installed", &self.status);
            return false;
        }
        match activation {
            PluginInstallActivation::Restart => {
                self.status = format!(
                    "{label} was installed. Restart RackForge to activate this version safely."
                );
                Self::show_install_info("Plugin installed", &self.status);
                false
            }
            PluginInstallActivation::KeepCurrent { active_version } => {
                self.status = format!(
                    "{label} was installed side by side. Version {active_version} remains active because it is the same or newer."
                );
                Self::show_install_info("Plugin installed", &self.status);
                false
            }
            PluginInstallActivation::Reload => {
                self.status = format!(
                    "{label} was installed from {} and is inactive. Activate it from Plugin Manager when you are ready to use it.",
                    archive.display()
                );
                Self::show_install_info("Plugin installed", &self.status);
                false
            }
        }
    }

    fn install_in_progress(&self) -> bool {
        self.plugin_install.is_some()
    }

    fn window_title(&self) -> &'static str {
        match self.plugin_install.as_ref().map(|task| task.phase) {
            Some(PluginInstallPhase::Inspecting) => "RackForge — Inspecting Plugin…",
            Some(PluginInstallPhase::Installing) => "RackForge — Installing Plugin…",
            None => "RackForge Desktop",
        }
    }

    #[cfg(windows)]
    fn show_install_error(message: &str) {
        rfd::MessageDialog::new()
            .set_title("RackForge could not install the plugin")
            .set_description(message)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }

    #[cfg(not(windows))]
    fn show_install_error(_message: &str) {}

    #[cfg(windows)]
    fn show_install_info(title: &str, message: &str) {
        rfd::MessageDialog::new()
            .set_title(title)
            .set_description(message)
            .set_level(rfd::MessageLevel::Info)
            .show();
    }

    #[cfg(not(windows))]
    fn show_install_info(_title: &str, _message: &str) {}

    #[cfg(windows)]
    fn confirm_plugin_install(archive: &Path, inspection: &LocalPackageInspection) -> bool {
        let trust = if inspection.portable {
            "Portable WASM package (the same package can run on supported RackForge platforms)."
        } else {
            "Native code package. It will run inside RackForge with your user permissions; install only if you trust its source."
        };
        let description = format!(
            "Install this plugin?\n\nName: {}\nID: {}\nVersion: {}\nType: {}\nPlatform: {}\nSize: {}\nSHA-256: {}\nFile: {}\n\nLocal package: its structure and payload are validated, but its publisher identity is not verified.\n\n{}",
            inspection.plugin_name,
            inspection.plugin_id,
            inspection.version,
            plugin_kind_label(inspection.kind),
            inspection.platform,
            format_file_size(inspection.archive_bytes),
            inspection.artifact_sha256,
            archive.display(),
            trust
        );
        rfd::MessageDialog::new()
            .set_title("Install RackForge plugin")
            .set_description(description)
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes
    }

    #[cfg(not(windows))]
    fn confirm_plugin_install(_archive: &Path, _inspection: &LocalPackageInspection) -> bool {
        false
    }

    fn apply_input(&mut self, input: Input) {
        self.menu.apply_input(input);
        while let Some(command) = self.menu.take_command() {
            self.apply_command(command);
        }
        #[cfg(windows)]
        self.render_controller_screen();
    }

    #[cfg(windows)]
    fn render_controller_screen(&self) {
        if let Some(audio) = &self.audio {
            audio.render_little(self.menu.render());
        }
    }

    #[cfg(windows)]
    fn poll_controller(&mut self) {
        loop {
            let event = self
                .audio
                .as_ref()
                .and_then(desktop_audio::DesktopAudio::try_controller_event);
            let Some(event) = event else { break };
            self.handle_controller_event(event);
        }
        if !self.controller_home_chord_emitted
            && let (Some(ok), Some(back)) = (
                self.controller_button_down[0],
                self.controller_button_down[3],
            )
        {
            let separation = if ok >= back {
                ok.duration_since(back)
            } else {
                back.duration_since(ok)
            };
            let chord_started = ok.max(back);
            if separation <= HOME_CHORD_SIMULTANEITY && chord_started.elapsed() >= LONG_PRESS {
                self.controller_home_chord_emitted = true;
                self.controller_button_long_fired[0] = true;
                self.controller_button_long_fired[3] = true;
                self.menu.set_button_pressed(Input::Button1, false);
                self.menu.set_button_pressed(Input::Button4, false);
                self.apply_input(Input::HomeChord);
            }
        }
        let long_presses = (0..4)
            .filter(|index| {
                !self.controller_button_long_fired[*index]
                    && self.controller_button_down[*index]
                        .is_some_and(|started| started.elapsed() >= LONG_PRESS)
            })
            .collect::<Vec<_>>();
        for index in long_presses {
            self.controller_button_long_fired[index] = true;
            self.menu.set_button_pressed(short_input(index), false);
            self.apply_input(long_input(index));
        }
        if self
            .controller_header_restore_at
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.controller_header_restore_at = None;
            self.render_controller_screen();
        } else if self.controller_header_restore_at.is_none() {
            // The glass follows the machine's state, not the player's
            // fingers. A program chosen on the web surface, a Rack
            // activated from a phone, a preset loaded by a command — all of
            // them changed the menu underneath, and until now the panel
            // kept showing the old frame until someone pressed a button on
            // it. The compositor discards a frame that changed nothing, so
            // asking every cycle costs one comparison.
            //
            // A value message owns the header while it lasts; the repaint
            // stands back rather than wiping it mid-flight.
            self.render_controller_screen();
        }
    }

    #[cfg(windows)]
    fn observe_midi_learn(
        &mut self,
        source: MidiSourceKey,
        length: u8,
        data: [u8; 3],
        observed_at: Instant,
    ) {
        let names = self
            .audio_inventory_cache
            .as_ref()
            .map(|(_, inventory)| inventory.midi_inputs.clone())
            .or_else(|| {
                self.audio_preferences
                    .as_ref()
                    .map(|preferences| preferences.midi_inputs.clone())
            })
            .unwrap_or_default();
        let Some(name) = names
            .into_iter()
            .find(|name| desktop_audio::stable_midi_source_key(name) == source)
        else {
            return;
        };
        let Ok(descriptor) = desktop_audio::midi_source_descriptor(&name) else {
            return;
        };
        self.observe_midi_learn_from_source(descriptor, length, data, observed_at);
    }

    #[cfg(windows)]
    fn observe_midi_learn_from_source(
        &mut self,
        descriptor: MidiSourceDescriptor,
        length: u8,
        data: [u8; 3],
        observed_at: Instant,
    ) {
        if self
            .midi_learn
            .as_ref()
            .is_none_or(|learn| learn.candidate.is_some() || observed_at < learn.started_at)
        {
            return;
        }
        let Ok(packet) = RoutedMidiPacket::new(0, &data[..usize::from(length)]) else {
            return;
        };
        let message = match packet.kind() {
            rackforge_midi_api::MidiMessageKind::ControlChange => {
                ParameterLinkMessage::ControlChange {
                    controller: packet.data[1],
                }
            }
            rackforge_midi_api::MidiMessageKind::PitchBend => ParameterLinkMessage::PitchBend,
            rackforge_midi_api::MidiMessageKind::Note => ParameterLinkMessage::Note {
                note: packet.data[1],
            },
            rackforge_midi_api::MidiMessageKind::ChannelPressure => {
                ParameterLinkMessage::ChannelPressure
            }
            rackforge_midi_api::MidiMessageKind::PolyPressure => {
                ParameterLinkMessage::PolyPressure {
                    note: packet.data[1],
                }
            }
            rackforge_midi_api::MidiMessageKind::ProgramChange => return,
        };
        if let Some(learn) = self.midi_learn.as_mut() {
            learn.candidate = Some(MidiLearnCandidate {
                source: descriptor,
                channel: MidiChannel::from_zero_based(packet.data[0] & 0x0f)
                    .expect("MIDI packet channel is valid"),
                message,
            });
        }
    }

    #[cfg(windows)]
    fn handle_controller_event(&mut self, event: desktop_audio::DesktopControllerEvent) {
        use desktop_audio::DesktopControllerEvent;
        use keylab_essential_mk3::protocol::InputPhase;

        match event {
            DesktopControllerEvent::MidiObserved {
                source,
                length,
                data,
                observed_at,
            } => {
                self.observe_midi_learn(source, length, data, observed_at);
                let message = &data[..usize::from(length)];
                let registered = self
                    .controller_semantic_profiles
                    .values()
                    .find(|registered| {
                        registered
                            .runtime_source_id
                            .as_ref()
                            .is_some_and(|source_id| {
                                desktop_audio::stable_midi_source_key_from_id(
                                    &MidiSourceId::new(source_id.clone())
                                        .expect("stored declarative source id is valid"),
                                ) == source
                            })
                    })
                    .cloned();
                if let Some(registered) = registered {
                    let declarative = registered
                        .host_controls
                        .iter()
                        .find_map(|binding| {
                            Some(DeclarativeControllerInput::HostControl {
                                target: binding.target,
                                value: binding.midi_cc.value(message)?,
                            })
                        })
                        .or_else(|| {
                            registered.host_actions.iter().find_map(|binding| {
                                Some(DeclarativeControllerInput::HostAction {
                                    target: binding.target,
                                    phase: binding.midi_cc.phase(message)?,
                                })
                            })
                        });
                    match declarative {
                        Some(DeclarativeControllerInput::HostControl {
                            target: HostControlTarget::MasterLevel,
                            value,
                        }) => {
                            let _ = self.set_master_level(MasterLevel::from_midi(value), None);
                        }
                        Some(DeclarativeControllerInput::HostControl {
                            target: HostControlTarget::MasterPan,
                            value,
                        }) => {
                            let _ = self
                                .set_master_pan(MasterPan::from_midi_with_center_snap(value), None);
                        }
                        Some(DeclarativeControllerInput::HostAction {
                            target: HostActionTarget::KeyboardParts,
                            phase: ButtonPhase::Press,
                        }) => self.apply_input(Input::KeyboardParts),
                        Some(DeclarativeControllerInput::HostAction { .. }) => {}
                        Some(DeclarativeControllerInput::Semantic(_)) => unreachable!(),
                        None => {}
                    }
                    if let Some(profile) = registered.profile.as_ref() {
                        if let Some(input) = rackforge_parameter_input(profile, message) {
                            self.handle_controller_event(
                                DesktopControllerEvent::RackForgeParameter(input),
                            );
                        } else if let Some(input) = semantic_control_input(profile, message) {
                            self.handle_controller_event(DesktopControllerEvent::SemanticControl(
                                input,
                            ));
                        }
                    }
                }
            }
            DesktopControllerEvent::Connected => {
                self.status = "Arturia KeyLab connected · LITTLE active".into();
                self.render_controller_screen();
            }
            DesktopControllerEvent::Disconnected => {
                for index in 0..4 {
                    self.controller_button_down[index] = None;
                    self.controller_button_long_fired[index] = false;
                    self.menu.set_button_pressed(short_input(index), false);
                }
                self.controller_home_chord_emitted = false;
                self.controller_encoder_down = None;
                self.status = "Arturia KeyLab disconnected · held notes stopped".into();
            }
            DesktopControllerEvent::RackForgeParameter(input) => {
                let current_pan = self
                    .session
                    .read()
                    .expect("session lock poisoned")
                    .master_pan;
                let Some(parameter) = self.controller_parameter_mapper.apply(input, current_pan)
                else {
                    return;
                };
                let result = match parameter {
                    RackForgeParameterValue::MasterLevel(level) => {
                        self.set_master_level(level, None)
                    }
                    RackForgeParameterValue::MasterPan(pan) => self.set_master_pan(pan, None),
                };
                if let Err(error) = result {
                    self.status = error;
                    return;
                }
                self.show_controller_host_value(parameter.little_header());
            }
            DesktopControllerEvent::SemanticControl(input) => {
                self.show_controller_host_value(semantic_control_little_header(&input));
            }
            DesktopControllerEvent::Surface { input, phase } => match input {
                Input::Button1 | Input::Button2 | Input::Button3 | Input::Button4 => {
                    let index = match input {
                        Input::Button1 => 0,
                        Input::Button2 => 1,
                        Input::Button3 => 2,
                        Input::Button4 => 3,
                        _ => unreachable!(),
                    };
                    match phase {
                        InputPhase::Press => {
                            self.controller_button_down[index] = Some(Instant::now());
                            self.controller_button_long_fired[index] = false;
                            self.menu.set_button_pressed(short_input(index), true);
                            self.render_controller_screen();
                        }
                        InputPhase::Release => {
                            let started = self.controller_button_down[index].take();
                            let long_fired = std::mem::replace(
                                &mut self.controller_button_long_fired[index],
                                false,
                            );
                            self.menu.set_button_pressed(short_input(index), false);
                            if self.controller_button_down.iter().all(Option::is_none) {
                                self.controller_home_chord_emitted = false;
                            }
                            if long_fired {
                                self.render_controller_screen();
                            } else {
                                self.apply_input(
                                    if started.is_some_and(|time| time.elapsed() >= LONG_PRESS) {
                                        long_input(index)
                                    } else {
                                        short_input(index)
                                    },
                                );
                            }
                        }
                        InputPhase::Turn => {}
                    }
                }
                Input::EncoderLeft | Input::EncoderRight => {
                    if phase == InputPhase::Turn {
                        self.apply_input(input);
                    }
                }
                Input::EncoderPress => match phase {
                    InputPhase::Press => self.controller_encoder_down = Some(Instant::now()),
                    InputPhase::Release => {
                        self.controller_encoder_down = None;
                        self.apply_input(Input::EncoderPress);
                    }
                    InputPhase::Turn => {}
                },
                Input::KeyboardParts if phase == InputPhase::Press => {
                    self.apply_input(Input::KeyboardParts);
                }
                _ => {}
            },
        }
    }

    #[cfg(windows)]
    fn show_controller_host_value(&mut self, header: String) {
        let mut screen = self.menu.render();
        screen.header = Header::Visible(header);
        if let Some(audio) = &self.audio {
            audio.render_little(screen);
        }
        self.controller_header_restore_at = Some(Instant::now() + Duration::from_millis(1_500));
    }

    fn poll_web_control(&mut self) {
        while let Ok(call) = self.web_control.try_recv() {
            match call {
                web::DesktopControlCall::Session { request, response } => {
                    let _ = response.send(self.handle_web_control(request));
                }
                web::DesktopControlCall::Performance { request, response } => {
                    let _ = response.send(self.handle_performance_control(request));
                }
                web::DesktopControlCall::LoadResource {
                    plugin_id,
                    resource_id,
                    path,
                    persist,
                    preview,
                    response,
                } => {
                    let _ = response.send(self.load_plugin_resource(
                        &plugin_id,
                        &resource_id,
                        &path,
                        persist,
                        preview,
                    ));
                }
                web::DesktopControlCall::ClearResource {
                    plugin_id,
                    resource_id,
                    response,
                } => {
                    let _ = response.send(self.clear_plugin_resource(&plugin_id, &resource_id));
                }
                web::DesktopControlCall::ActivatePlugin {
                    plugin_id,
                    response,
                } => {
                    let _ = response.send(self.activate_plugin_id(&plugin_id));
                }
                web::DesktopControlCall::DeactivatePlugin {
                    plugin_id,
                    response,
                } => {
                    let _ = response.send(self.deactivate_plugin_id(&plugin_id));
                }
                web::DesktopControlCall::UninstallPlugin {
                    plugin_id,
                    delete_presets,
                    delete_plugin_data,
                    response,
                } => {
                    let _ = response.send(self.uninstall_plugin_id(
                        &plugin_id,
                        delete_presets,
                        delete_plugin_data,
                    ));
                }
                web::DesktopControlCall::AudioSettings { response } => {
                    #[cfg(windows)]
                    let _ = response.send(
                        self.audio_settings_json()
                            .map_err(|error| format!("{error:#}")),
                    );
                    #[cfg(not(windows))]
                    let _ = response.send(Err(
                        "This host does not publish Desktop audio settings.".into(),
                    ));
                }
                web::DesktopControlCall::ApplyAudioSettings {
                    preferences,
                    response,
                } => {
                    #[cfg(windows)]
                    let result =
                        serde_json::from_value::<desktop_audio::AudioPreferences>(preferences)
                            .map_err(|error| format!("Invalid Desktop audio settings: {error}"))
                            .and_then(|preferences| {
                                self.apply_audio_preferences(preferences)
                                    .map_err(|error| format!("{error:#}"))?;
                                self.audio_settings_json()
                                    .map_err(|error| format!("{error:#}"))
                            });
                    #[cfg(windows)]
                    let _ = response.send(result);
                    #[cfg(not(windows))]
                    let _ = {
                        let _ = preferences;
                        response.send(Err(
                            "This host does not publish Desktop audio settings.".into()
                        ))
                    };
                }
                web::DesktopControlCall::TestAudio { response } => {
                    #[cfg(windows)]
                    let _ =
                        response.send(self.test_audio_note().map_err(|error| format!("{error:#}")));
                    #[cfg(not(windows))]
                    let _ = response.send(Err(
                        "This host does not publish an audio test control.".into()
                    ));
                }
                web::DesktopControlCall::ApplyWebSettings {
                    preferences,
                    response,
                } => {
                    let result = self
                        .apply_web_preferences(preferences)
                        .map(|message| {
                            serde_json::json!({
                                "status": "ok",
                                "enabled": self.web_preferences.enabled,
                                "access": if self.web_preferences.enabled { "lan" } else { "local" },
                                "port": self.web_preferences.port,
                                "configurable": true,
                                "message": message,
                            })
                        })
                        .map_err(|error| format!("{error:#}"));
                    let _ = response.send(result);
                }
            }
        }
    }

    fn live_state_dir(&self) -> PathBuf {
        self.options.data_root.join("states/live")
    }

    /// Persists the active instrument's live state. Called a moment after
    /// the last panel edit, and at shutdown; restarts then mean what the
    /// player left, not the factory floor.
    fn flush_live_state(&mut self) {
        self.live_state_dirty = None;
        #[cfg(windows)]
        {
            let Some(audio) = self.audio.as_ref() else {
                return;
            };
            let active = self
                .session
                .read()
                .expect("session lock poisoned")
                .active_instance_id
                .clone();
            let Some(active) = active else { return };
            let Some(plugin) = self
                .plugins
                .iter()
                .find(|plugin| plugin.instance_id == active.as_str())
            else {
                return;
            };
            match audio.save_active_state() {
                Ok(bytes) => {
                    let dir = self.live_state_dir();
                    let path = live_state_path(&dir, &plugin.plugin_id);
                    if let Err(error) =
                        fs::create_dir_all(&dir).and_then(|_| fs::write(&path, &bytes))
                    {
                        eprintln!("DESKTOP_LIVE_STATE_WRITE_FAILED {error:#}");
                    }
                }
                Err(error) => eprintln!("DESKTOP_LIVE_STATE_SAVE_FAILED {error:#}"),
            }
        }
    }

    fn activate_plugin_id(&mut self, plugin_id: &str) -> Result<(), String> {
        let store_root = self
            .options
            .plugin_store_root
            .clone()
            .ok_or_else(|| "Desktop plugin storage is unavailable".to_owned())?;
        let was_enabled = plugin_is_enabled(&store_root, plugin_id)
            .map_err(|error| format!("Could not read plugin activation state: {error}"))?;
        set_plugin_enabled(&store_root, plugin_id, true)
            .map_err(|error| format!("Could not enable the installed plugin: {error}"))?;
        let activation = (|| {
            if !was_enabled
                || !self
                    .plugins
                    .iter()
                    .any(|plugin| plugin.plugin_id == plugin_id)
            {
                self.reload_plugins()
                    .map_err(|error| format!("Could not load the installed plugin: {error:#}"))?;
            }
            let instance_id = self
                .plugins
                .iter()
                .find(|plugin| plugin.plugin_id == plugin_id)
                .map(|plugin| plugin.instance_id.clone())
                .ok_or_else(|| {
                    format!("Installed plugin {plugin_id:?} is not compatible with Desktop")
                })?;
            let instance_id = InstanceId::new(instance_id)
                .map_err(|error| format!("Installed plugin has an invalid instance id: {error}"))?;
            self.select_plugin(&instance_id, None)?;
            self.apply_command(MenuCommand::SetActiveMode {
                mode: ActiveMode::Play,
            });
            Ok(())
        })();
        if let Err(error) = activation {
            if !was_enabled {
                let rollback = set_plugin_enabled(&store_root, plugin_id, false);
                let reload = self.reload_plugins();
                if let Err(rollback) = rollback {
                    return Err(format!(
                        "{error}. Restoring the inactive state also failed: {rollback}"
                    ));
                }
                if let Err(reload) = reload {
                    return Err(format!(
                        "{error}. The package was disabled again, but Desktop could not restore its plugin graph: {reload:#}"
                    ));
                }
            }
            return Err(error);
        }
        Ok(())
    }

    fn deactivate_plugin_id(&mut self, plugin_id: &str) -> Result<(), String> {
        let store_root = self
            .options
            .plugin_store_root
            .clone()
            .ok_or_else(|| "Desktop plugin storage is unavailable".to_owned())?;
        if self
            .options
            .plugins_root
            .join(plugin_id)
            .join("rackforge-plugin.toml")
            .is_file()
        {
            return Err("Built-in plugins cannot be deactivated".into());
        }
        if self
            .session
            .read()
            .is_ok_and(|session| session.audition.is_some() || session.program_draft.is_some())
        {
            return Err("Finish or cancel the active plugin edit before deactivating it".into());
        }
        let was_selected = self.session.read().is_ok_and(|session| {
            session
                .active_instance_id
                .as_ref()
                .is_some_and(|instance_id| {
                    self.plugins.iter().any(|plugin| {
                        plugin.plugin_id == plugin_id && plugin.instance_id == instance_id.as_str()
                    })
                })
        });
        if was_selected {
            self.apply_command(MenuCommand::SetActiveMode {
                mode: ActiveMode::Idle,
            });
        }
        set_plugin_enabled(&store_root, plugin_id, false)
            .map_err(|error| format!("Could not disable the plugin: {error}"))?;
        if let Err(error) = self.reload_plugins() {
            let rollback = set_plugin_enabled(&store_root, plugin_id, true);
            return Err(match rollback {
                Ok(()) => format!("Could not reload Desktop without the plugin: {error:#}"),
                Err(rollback) => format!(
                    "Could not reload Desktop without the plugin: {error:#}. Re-enabling it also failed: {rollback}"
                ),
            });
        }
        self.status = format!("Plugin deactivated: {plugin_id}");
        Ok(())
    }

    fn uninstall_plugin_id(
        &mut self,
        plugin_id: &str,
        delete_presets: bool,
        delete_plugin_data: bool,
    ) -> Result<serde_json::Value, String> {
        let store_root = self
            .options
            .plugin_store_root
            .clone()
            .ok_or_else(|| "Desktop plugin storage is unavailable".to_owned())?;
        if self
            .options
            .plugins_root
            .join(plugin_id)
            .join("rackforge-plugin.toml")
            .is_file()
        {
            return Err(
                "This plugin is part of the host installation and cannot be removed from the package manager."
                    .into(),
            );
        }
        if self
            .session
            .read()
            .is_ok_and(|session| session.audition.is_some() || session.program_draft.is_some())
        {
            return Err("Finish or cancel the active plugin edit before removing it".into());
        }

        // Drop every audio and DSP instance before renaming package roots.
        // Native libraries are process-lifetime by design, so Windows may
        // leave the tombstone for cleanup after exit, but it disappears from
        // discovery immediately and cannot be selected again.
        #[cfg(windows)]
        {
            self.stop_audio_runtime();
        }
        self.plugins.clear();
        let removed = match uninstall_plugin(&store_root, plugin_id) {
            Ok(removed) => removed,
            Err(error) => {
                let recovery = self.reload_plugins().err();
                return Err(match recovery {
                    Some(recovery) => format!(
                        "Could not remove plugin: {error}. Desktop also could not restore its runtime: {recovery:#}"
                    ),
                    None => format!("Could not remove plugin: {error}"),
                });
            }
        };
        let warnings = self.reload_plugins().map_err(|error| {
            format!("Plugin was removed, but Desktop could not reload: {error:#}")
        })?;
        let user_data_cleanup = remove_plugin_user_data(
            &self.options.data_root,
            plugin_id,
            PluginUserDataRemovalOptions {
                presets: delete_presets,
                plugin_data: delete_plugin_data,
            },
        );
        let (presets_deleted, plugin_data_deleted, user_data_cleanup_warning) =
            match user_data_cleanup {
                Ok(_) => (delete_presets, delete_plugin_data, None),
                Err(error) => (false, false, Some(error.to_string())),
            };
        if let Some(warning) = warnings.first() {
            self.status = format!("Plugin removed · {warning}");
        } else {
            self.status = format!("Plugin removed: {plugin_id}");
        }
        Ok(serde_json::json!({
            "status":"uninstalled",
            "plugin_id":removed.plugin_id,
            "removed_versions":removed.removed_versions,
            "cleanup_pending":removed.cleanup_pending,
            "restart_requested":false,
            "user_data_preserved":!presets_deleted && !plugin_data_deleted,
            "presets_deleted":presets_deleted,
            "plugin_data_deleted":plugin_data_deleted,
            "user_data_cleanup_warning":user_data_cleanup_warning
        }))
    }

    fn load_plugin_resource(
        &mut self,
        plugin_id: &str,
        resource_id: &str,
        path: &Path,
        persist: bool,
        preview: bool,
    ) -> Result<(), String> {
        if persist && preview {
            return Err("A resource preview cannot be persisted".into());
        }
        let index = self
            .plugins
            .iter()
            .position(|plugin| plugin.plugin_id == plugin_id)
            .ok_or_else(|| format!("Unknown Desktop plugin: {plugin_id}"))?;
        let live_state_dir = self.live_state_dir();
        let plugin = &mut self.plugins[index];
        let import_targets = plugin
            .runtime
            .manifest()
            .resources
            .iter()
            .find(|resource| resource.id == resource_id)
            .map(|resource| resource.import_targets.clone())
            .ok_or_else(|| format!("Plugin does not declare resource {resource_id:?}"))?;
        let mut candidate_resources = plugin.resources.clone();
        let mut previous = Vec::new();
        let installed_count;
        if !import_targets.is_empty() {
            if !persist {
                return Err(
                    "Resource archives must be installed into private plugin storage".into(),
                );
            }
            let imported = plugin
                .runtime
                .import_resource_archive(resource_id, path)
                .map_err(|error| format!("Selected archive was rejected: {error:#}"))?;
            let storage = PluginStorage::new(&self.options.data_root);
            installed_count = imported.len();
            for (target_id, bytes) in imported {
                let data_path = plugin.resource_data_paths.get(&target_id).ok_or_else(|| {
                    format!("Imported resource {target_id:?} has no private data_path")
                })?;
                let selected_path = storage
                    .write_atomic(plugin_id, data_path, &bytes)
                    .map_err(|error| format!("Could not install {target_id:?}: {error:#}"))?;
                if preview {
                    candidate_resources.insert(target_id, selected_path);
                } else {
                    previous.push((
                        target_id.clone(),
                        plugin.resources.insert(target_id, selected_path),
                    ));
                }
            }
        } else {
            let data_path = persist
                .then(|| {
                    plugin
                        .resource_data_paths
                        .get(resource_id)
                        .cloned()
                        .ok_or_else(|| {
                            format!(
                                "Resource {resource_id:?} cannot be installed because it has no private data_path"
                            )
                        })
                })
                .transpose()?;
            let selected_path = if let Some(data_path) = data_path.as_deref() {
                plugin
                    .runtime
                    .validate_resource_file(resource_id, path)
                    .map_err(|error| format!("Selected file was rejected: {error:#}"))?;
                PluginStorage::new(&self.options.data_root)
                    .copy_file_atomic(plugin_id, data_path, path)
                    .map_err(|error| format!("Could not install plugin resource: {error:#}"))?
            } else {
                path.to_path_buf()
            };
            installed_count = 1;
            if preview {
                candidate_resources.insert(resource_id.to_owned(), selected_path);
            } else {
                previous.push((
                    resource_id.to_owned(),
                    plugin
                        .resources
                        .insert(resource_id.to_owned(), selected_path),
                ));
            }
        }

        let prepare =
            (|| -> anyhow::Result<(PluginInstance<'static>, PresetCatalog, Option<String>)> {
                let resources = if preview {
                    &candidate_resources
                } else {
                    &plugin.resources
                };
                let mut instance = plugin
                    .runtime
                    .create_instance_with_resource_overrides(resources)?;
                let catalog = instance.preset_catalog()?;
                let selected_sound_id = (!preview)
                    .then_some(plugin.selected_sound_id.as_ref())
                    .flatten()
                    .filter(|id| {
                        catalog
                            .presets
                            .iter()
                            .any(|preset| preset.id == id.as_str())
                    })
                    .cloned()
                    .or_else(|| catalog.presets.first().map(|preset| preset.id.clone()));
                if let Some(preset_id) = selected_sound_id.as_deref() {
                    instance.load_preset(preset_id)?;
                }
                #[cfg(windows)]
                if let Some(audio) = &self.audio {
                    audio.replace_voice(desktop_audio::VoiceSpec {
                        instance_id: plugin.instance_id.clone(),
                        plugin: plugin.runtime,
                        preset_id: selected_sound_id.clone(),
                        resources: resources.clone(),
                        initial_state: read_live_state(&live_state_dir, &plugin.plugin_id),
                    })?;
                }
                Ok((instance, catalog, selected_sound_id))
            })();

        match prepare {
            Ok((instance, catalog, selected_sound_id)) => {
                plugin.instance = instance;
                if !preview {
                    let (banks, sound_summaries, sounds) = desktop_catalog_views(&catalog);
                    plugin.banks = banks;
                    plugin.sound_summaries = sound_summaries;
                    plugin.sounds = sounds;
                    plugin.selected_sound_id = selected_sound_id;
                    let next_session_state = plugin_session_state(plugin);
                    let mut session = self.session.write().expect("session lock poisoned");
                    if let Some(state) = session
                        .instances
                        .iter_mut()
                        .find(|state| state.instance_id.as_str() == plugin.instance_id)
                    {
                        *state = next_session_state;
                        session.revision = Revision::new(session.revision.get().saturating_add(1));
                    }
                }
                self.status = if preview {
                    format!("{} is auditioning an in-progress resource", plugin.name)
                } else {
                    format!(
                        "{} installed and activated {} recognized resource{} from {}",
                        plugin.name,
                        installed_count,
                        if installed_count == 1 { "" } else { "s" },
                        path.file_name()
                            .map(|name| name.to_string_lossy())
                            .unwrap_or_else(|| path.display().to_string().into())
                    )
                };
                Ok(())
            }
            Err(error) => {
                if persist {
                    self.status = format!(
                        "{} installed {}; waiting for the remaining compatible resources",
                        plugin.name, resource_id
                    );
                    return Ok(());
                }
                for (id, prior) in previous {
                    match prior {
                        Some(prior) => {
                            plugin.resources.insert(id, prior);
                        }
                        None => {
                            plugin.resources.remove(&id);
                        }
                    }
                }
                Err(format!("Could not load plugin resource: {error:#}"))
            }
        }
    }

    /// Removes one installed private resource and reactivates the plugin
    /// with the remaining set, so the package default plays again.
    fn clear_plugin_resource(&mut self, plugin_id: &str, resource_id: &str) -> Result<(), String> {
        let index = self
            .plugins
            .iter()
            .position(|plugin| plugin.plugin_id == plugin_id)
            .ok_or_else(|| format!("Unknown Desktop plugin: {plugin_id}"))?;
        let live_state_dir = self.live_state_dir();
        let plugin = &mut self.plugins[index];
        let required = plugin
            .runtime
            .manifest()
            .resources
            .iter()
            .any(|resource| resource.id == resource_id && resource.required);
        if required {
            return Err("A required resource cannot be cleared".into());
        }
        let data_path = plugin
            .resource_data_paths
            .get(resource_id)
            .cloned()
            .ok_or_else(|| format!("Resource {resource_id:?} has no private data_path"))?;
        PluginStorage::new(&self.options.data_root)
            .remove_file(plugin_id, &data_path)
            .map_err(|error| format!("Could not clear plugin resource: {error:#}"))?;
        plugin.resources.remove(resource_id);

        let prepare =
            (|| -> anyhow::Result<(PluginInstance<'static>, PresetCatalog, Option<String>)> {
                let mut instance = plugin
                    .runtime
                    .create_instance_with_resource_overrides(&plugin.resources)?;
                let catalog = instance.preset_catalog()?;
                let selected_sound_id = plugin
                    .selected_sound_id
                    .as_ref()
                    .filter(|id| {
                        catalog
                            .presets
                            .iter()
                            .any(|preset| preset.id == id.as_str())
                    })
                    .cloned()
                    .or_else(|| catalog.presets.first().map(|preset| preset.id.clone()));
                if let Some(preset_id) = selected_sound_id.as_deref() {
                    instance.load_preset(preset_id)?;
                }
                #[cfg(windows)]
                if let Some(audio) = &self.audio {
                    audio.replace_voice(desktop_audio::VoiceSpec {
                        instance_id: plugin.instance_id.clone(),
                        plugin: plugin.runtime,
                        preset_id: selected_sound_id.clone(),
                        resources: plugin.resources.clone(),
                        initial_state: read_live_state(&live_state_dir, &plugin.plugin_id),
                    })?;
                }
                Ok((instance, catalog, selected_sound_id))
            })();

        match prepare {
            Ok((instance, catalog, selected_sound_id)) => {
                let (banks, sound_summaries, sounds) = desktop_catalog_views(&catalog);
                plugin.instance = instance;
                plugin.banks = banks;
                plugin.sound_summaries = sound_summaries;
                plugin.sounds = sounds;
                plugin.selected_sound_id = selected_sound_id;
                let next_session_state = plugin_session_state(plugin);
                let mut session = self.session.write().expect("session lock poisoned");
                if let Some(state) = session
                    .instances
                    .iter_mut()
                    .find(|state| state.instance_id.as_str() == plugin.instance_id)
                {
                    *state = next_session_state;
                    session.revision = Revision::new(session.revision.get().saturating_add(1));
                }
                self.status = format!(
                    "{} cleared {} and restored the package default",
                    plugin.name, resource_id
                );
                Ok(())
            }
            Err(error) => Err(format!(
                "Could not activate after clearing the resource: {error:#}"
            )),
        }
    }

    fn replace_parameter_links(&mut self, links: Vec<ParameterLink>) -> Result<(), String> {
        #[cfg(windows)]
        {
            let compiled = compile_desktop_parameter_links(
                &links,
                &self.plugins,
                &self.performance_repository,
                &self.controller_semantic_profiles,
            )
            .map_err(|error| format!("Could not compile MIDI parameter links: {error:#}"))?;
            self.audio
                .as_ref()
                .ok_or_else(|| "Desktop audio is unavailable".to_owned())?
                .replace_parameter_links(compiled)
                .map_err(|error| format!("Could not apply MIDI parameter links: {error:#}"))
        }
        #[cfg(not(windows))]
        {
            let _ = links;
            Err("Desktop audio is unavailable".into())
        }
    }

    fn handle_web_control(&mut self, request: ControlRequest) -> ControlResponse {
        let envelope = match request {
            ControlRequest::Sequencer { command } => {
                let result = self
                    .audio
                    .as_ref()
                    .ok_or_else(|| "Desktop audio is unavailable".to_owned())
                    .and_then(|audio| {
                        audio
                            .sequencer_command(command)
                            .map_err(|error| error.to_string())
                            .and_then(|applied| applied)
                    });
                return match result {
                    Ok(()) => ControlResponse::SequencerAccepted,
                    Err(message) => ControlResponse::Error {
                        code: ControlErrorCode::InvalidRequest,
                        message,
                        current_revision: Some(
                            self.session.read().expect("session lock poisoned").revision,
                        ),
                    },
                };
            }
            ControlRequest::SequencerStatus => {
                let status = self
                    .audio
                    .as_ref()
                    .ok_or_else(|| "Desktop audio is unavailable".to_owned())
                    .and_then(|audio| audio.sequencer_status().map_err(|error| error.to_string()));
                return match status {
                    Ok(sequencer) => ControlResponse::SequencerStatus { sequencer },
                    Err(message) => ControlResponse::Error {
                        code: ControlErrorCode::Unavailable,
                        message,
                        current_revision: Some(
                            self.session.read().expect("session lock poisoned").revision,
                        ),
                    },
                };
            }
            ControlRequest::SequencerCaptureTake { lane } => {
                let notes = self
                    .audio
                    .as_ref()
                    .ok_or_else(|| "Desktop audio is unavailable".to_owned())
                    .and_then(|audio| {
                        audio
                            .sequencer_capture_take(lane)
                            .map_err(|error| error.to_string())
                    });
                return match notes {
                    Ok(notes) => ControlResponse::SequencerCapture { notes },
                    Err(message) => ControlResponse::Error {
                        code: ControlErrorCode::Unavailable,
                        message,
                        current_revision: Some(
                            self.session.read().expect("session lock poisoned").revision,
                        ),
                    },
                };
            }
            ControlRequest::VirtualMidi {
                client_id,
                source_name,
                message,
            } => {
                return self.accept_virtual_midi(client_id, source_name, message);
            }
            ControlRequest::ReleaseVirtualMidi { client_id } => {
                return self.release_virtual_midi(client_id);
            }
            ControlRequest::Dispatch { envelope } => envelope,
            _ => {
                return ControlResponse::Error {
                    code: ControlErrorCode::Unavailable,
                    message: "This Desktop operation is not connected yet.".into(),
                    current_revision: None,
                };
            }
        };
        let current_revision = self.session.read().expect("session lock poisoned").revision;
        if envelope.schema_version != SESSION_SCHEMA_VERSION {
            return ControlResponse::Error {
                code: ControlErrorCode::InvalidRequest,
                message: format!(
                    "unsupported session schema {}; expected {}",
                    envelope.schema_version, SESSION_SCHEMA_VERSION
                ),
                current_revision: Some(current_revision),
            };
        }
        if envelope
            .expected_revision
            .is_some_and(|expected| expected != current_revision)
        {
            return ControlResponse::Error {
                code: ControlErrorCode::Conflict,
                message: format!(
                    "session revision changed from {} to {}",
                    envelope
                        .expected_revision
                        .expect("checked expected revision")
                        .get(),
                    current_revision.get()
                ),
                current_revision: Some(current_revision),
            };
        }

        let client_id = envelope.client_id;
        let command_id = envelope.command_id;
        let command_ref = CommandRef {
            client_id: client_id.clone(),
            command_id,
        };
        let result = match envelope.command {
            SessionCommand::UpsertParameterLink { link } => {
                let mut links = self
                    .session
                    .read()
                    .expect("session lock poisoned")
                    .parameter_links
                    .clone();
                if let Some(existing) = links.iter_mut().find(|existing| existing.id == link.id) {
                    *existing = link.clone();
                } else {
                    links.push(link.clone());
                }
                self.replace_parameter_links(links).and_then(|()| {
                    let events = self.apply_program_events(
                        vec![SessionEvent::ParameterLinkUpserted { link }],
                        Some(command_ref),
                    )?;
                    self.persist_session_checkpoint();
                    Ok(events)
                })
            }
            SessionCommand::RemoveParameterLink { link_id } => {
                let current = self
                    .session
                    .read()
                    .expect("session lock poisoned")
                    .parameter_links
                    .clone();
                if !current.iter().any(|link| link.id == link_id) {
                    Err(format!("Unknown MIDI parameter link {link_id}"))
                } else {
                    let links = current
                        .into_iter()
                        .filter(|link| link.id != link_id)
                        .collect();
                    self.replace_parameter_links(links).and_then(|()| {
                        let events = self.apply_program_events(
                            vec![SessionEvent::ParameterLinkRemoved { link_id }],
                            Some(command_ref),
                        )?;
                        self.persist_session_checkpoint();
                        Ok(events)
                    })
                }
            }
            SessionCommand::SetMasterLevel { level } => {
                self.set_master_level(level, Some(command_ref))
            }
            SessionCommand::SetMasterPan { pan } => self.set_master_pan(pan, Some(command_ref)),
            SessionCommand::SetActiveMode { mode } => self.set_active_mode(mode, Some(command_ref)),
            SessionCommand::EmergencyStop => self.emergency_stop(Some(command_ref)),
            SessionCommand::SelectPlugin { instance_id } => {
                self.select_plugin(&instance_id, Some(command_ref))
            }
            SessionCommand::SelectSound {
                instance_id,
                sound_id,
            } => self.select_sound(&instance_id, &sound_id, Some(command_ref)),
            SessionCommand::ActivateSurface {
                instance_id,
                request,
            } => self.activate_surface(instance_id, request, Some(command_ref)),
            SessionCommand::BeginProgramEdit {
                instance_id,
                program_id,
            } => {
                let active_instance_id = self
                    .session
                    .read()
                    .expect("session lock poisoned")
                    .active_instance_id
                    .clone();
                if active_instance_id.as_ref() != Some(&instance_id) {
                    Err(format!(
                        "Plugin instance {instance_id} is not the active Desktop plugin"
                    ))
                } else {
                    self.begin_program_edit(program_id, Some(command_ref))
                }
            }
            SessionCommand::ReplaceProgramDraft {
                draft_id,
                document_json,
            } => serde_json::from_str::<ProgramDocument>(&document_json)
                .map_err(|error| format!("Program document is invalid: {error}"))
                .and_then(|document| {
                    self.replace_program_draft_document(
                        draft_id,
                        document,
                        true,
                        true,
                        Some(command_ref),
                    )
                }),
            SessionCommand::PreviewProgramDraft {
                draft_id,
                document_json,
            } => serde_json::from_str::<ProgramDocument>(&document_json)
                .map_err(|error| format!("Program preview document is invalid: {error}"))
                .and_then(|document| {
                    self.replace_program_draft_document(
                        draft_id,
                        document,
                        false,
                        false,
                        Some(command_ref),
                    )
                }),
            SessionCommand::EditProgramDraftField {
                draft_id,
                field_id,
                value,
                preview,
            } => {
                self.edit_program_draft_field(draft_id, field_id, value, preview, Some(command_ref))
            }
            SessionCommand::RestoreProgramDraftPreview { draft_id } => {
                self.restore_program_draft_preview(draft_id)
            }
            SessionCommand::SaveProgramDraft { draft_id } => {
                self.save_program_draft(draft_id, Some(command_ref))
            }
            SessionCommand::CancelProgramEdit { draft_id } => {
                self.cancel_program_edit(draft_id, Some(command_ref))
            }
            SessionCommand::KeepAuditionAlive { lease_id } => {
                let lease_matches = self
                    .session
                    .read()
                    .expect("session lock poisoned")
                    .audition
                    .as_ref()
                    .is_some_and(|audition| audition.lease_id == lease_id);
                if lease_matches {
                    Ok(Vec::new())
                } else {
                    Err("Program audition lease is missing or no longer valid".into())
                }
            }
            SessionCommand::RegisterHostBindings {
                controller_id,
                controls,
                actions,
                midi_source_name,
                semantic_profile,
            } => {
                // A controller driver reserving its host-control CCs. On this
                // host the driver owns its surface endpoint exclusively (the
                // desktop's MIDI capture yields it), so the reservation is
                // satisfied by construction: nothing else reads those CCs.
                // Validate and acknowledge.
                if controls
                    .iter()
                    .any(|binding| binding.midi_cc.validate().is_err())
                    || actions
                        .iter()
                        .any(|binding| binding.midi_cc.validate().is_err())
                {
                    Err("invalid reserved host binding registration".into())
                } else {
                    (|| -> Result<Vec<EventEnvelope>, String> {
                        if let Some(profile) = &semantic_profile {
                            profile.validate_against_reserved(&controls, &actions)?;
                        }
                        let previous = self
                            .controller_semantic_profiles
                            .get(&controller_id)
                            .cloned();
                        if semantic_profile.is_some() || !controls.is_empty() || !actions.is_empty()
                        {
                            #[cfg(windows)]
                            let resolved_source = midi_source_name.as_deref().and_then(|name| {
                                self.approved_midi_source(name)
                                    .ok()
                                    .map(|source| (source.id.as_str().to_owned(), source.name))
                            });
                            #[cfg(not(windows))]
                            let resolved_source: Option<(
                                String,
                                String,
                            )> = None;
                            let (runtime_source_id, runtime_source_name) = resolved_source
                                .map(|(id, name)| (Some(id), Some(name)))
                                .unwrap_or_else(|| {
                                    if midi_source_name.is_none() {
                                        previous.as_ref().map_or((None, None), |registered| {
                                            (
                                                registered.runtime_source_id.clone(),
                                                registered.runtime_source_name.clone(),
                                            )
                                        })
                                    } else {
                                        (None, None)
                                    }
                                });
                            self.controller_semantic_profiles.insert(
                                controller_id.clone(),
                                RegisteredSemanticProfile {
                                    profile: semantic_profile,
                                    runtime_source_id,
                                    runtime_source_name,
                                    host_controls: controls.clone(),
                                    host_actions: actions.clone(),
                                },
                            );
                        } else {
                            self.controller_semantic_profiles.remove(&controller_id);
                        }
                        let explicit_links = self
                            .session
                            .read()
                            .expect("session lock poisoned")
                            .parameter_links
                            .clone();
                        if self.audio.is_some()
                            && let Err(error) = self.replace_parameter_links(explicit_links)
                        {
                            match previous {
                                Some(profile) => {
                                    self.controller_semantic_profiles
                                        .insert(controller_id.clone(), profile);
                                }
                                None => {
                                    self.controller_semantic_profiles.remove(&controller_id);
                                }
                            }
                            return Err(error);
                        }
                        println!(
                            "DESKTOP_HOST_BINDINGS_RESERVED controller={} controls={} actions={} semantic={}",
                            controller_id,
                            controls.len(),
                            actions.len(),
                            self.controller_semantic_profiles
                                .get(&controller_id)
                                .and_then(|registered| registered.profile.as_ref())
                                .map_or(0, |profile| profile.controls.len())
                        );
                        Ok(Vec::new())
                    })()
                }
            }
            SessionCommand::SetLiveBrowseMode { mode } => self.apply_program_events(
                vec![SessionEvent::LiveBrowseModeChanged { mode }],
                Some(command_ref),
            ),
            SessionCommand::ActivateLiveTarget { location } => {
                self.activate_live_target(location, Some(command_ref))
            }
            other => Err(format!(
                "Desktop does not support {} yet",
                serde_json::to_value(&other)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("type")
                            .and_then(|name| name.as_str())
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| "this command".to_owned())
            )),
        };
        match result {
            Ok(events) => {
                let revision = self.session.read().expect("session lock poisoned").revision;
                ControlResponse::CommandApplied {
                    client_id,
                    command_id,
                    revision,
                    events,
                }
            }
            Err(message) => ControlResponse::Error {
                code: ControlErrorCode::Rejected,
                message,
                current_revision: Some(
                    self.session.read().expect("session lock poisoned").revision,
                ),
            },
        }
    }

    #[cfg(windows)]
    fn approved_midi_source(&self, source_name: &str) -> Result<MidiSourceDescriptor, String> {
        approved_midi_source(self.audio_preferences.as_ref(), source_name)
    }

    fn accept_virtual_midi(
        &mut self,
        client_id: ClientId,
        source_name: Option<String>,
        message: VirtualMidiMessage,
    ) -> ControlResponse {
        if let Err(message) = message.validate() {
            return ControlResponse::Error {
                code: ControlErrorCode::InvalidRequest,
                message: message.into(),
                current_revision: Some(
                    self.session.read().expect("session lock poisoned").revision,
                ),
            };
        }
        #[cfg(windows)]
        let midi_source = match source_name
            .as_deref()
            .map(|name| self.approved_midi_source(name))
            .transpose()
        {
            Ok(descriptor) => descriptor,
            Err(message) => {
                return ControlResponse::Error {
                    code: ControlErrorCode::Unavailable,
                    message,
                    current_revision: Some(
                        self.session.read().expect("session lock poisoned").revision,
                    ),
                };
            }
        };
        #[cfg(not(windows))]
        let midi_source: Option<MidiSourceDescriptor> = None;
        #[cfg(windows)]
        let result = {
            let routing_descriptor = midi_source
                .clone()
                .unwrap_or_else(|| virtual_midi_source_descriptor(&client_id));
            let source = desktop_audio::stable_midi_source_key_from_id(&routing_descriptor.id);
            if let Some(descriptor) = midi_source.clone() {
                self.observe_midi_learn_from_source(descriptor, 3, message.bytes(), Instant::now());
            }
            self.audio
                .as_ref()
                .ok_or_else(|| "Desktop audio is unavailable".to_owned())
                .and_then(|audio| {
                    audio
                        .inject_midi_messages_from(source, vec![message.bytes()])
                        .map_err(|error| error.to_string())
                })
        };
        #[cfg(not(windows))]
        let result: Result<(), String> = Err("Desktop audio is unavailable".into());
        if let Err(message) = result {
            return ControlResponse::Error {
                code: ControlErrorCode::Unavailable,
                message,
                current_revision: Some(
                    self.session.read().expect("session lock poisoned").revision,
                ),
            };
        }
        let state = self.virtual_midi.entry(client_id.clone()).or_default();
        state.midi_source = midi_source;
        let channel = message.channel();
        state.channels.insert(channel);
        if let Some(note) = message.note_on() {
            state.notes.insert((channel, note));
        } else if let Some(note) = message.note_off() {
            state.notes.remove(&(channel, note));
        }
        ControlResponse::VirtualMidiAccepted {
            client_id,
            active_notes: u16::try_from(state.notes.len()).unwrap_or(u16::MAX),
        }
    }

    fn release_virtual_midi(&mut self, client_id: ClientId) -> ControlResponse {
        let state = self.virtual_midi.remove(&client_id).unwrap_or_default();
        let mut messages = Vec::with_capacity(state.notes.len() + state.channels.len() * 3);
        for (channel, note) in &state.notes {
            messages.push([0x80 | *channel, *note, 0]);
        }
        let channels = if state.channels.is_empty() {
            vec![0]
        } else {
            state.channels.iter().copied().collect()
        };
        for channel in channels {
            for controller in [64, 123, 120] {
                messages.push([0xb0 | channel, controller, 0]);
            }
        }
        #[cfg(windows)]
        let result = {
            let descriptor = state
                .midi_source
                .clone()
                .unwrap_or_else(|| virtual_midi_source_descriptor(&client_id));
            let source = desktop_audio::stable_midi_source_key_from_id(&descriptor.id);
            self.audio
                .as_ref()
                .ok_or_else(|| "Desktop audio is unavailable".to_owned())
                .and_then(|audio| {
                    audio
                        .inject_midi_messages_from(source, messages)
                        .map_err(|error| error.to_string())
                })
        };
        #[cfg(not(windows))]
        let result: Result<(), String> = {
            let _ = messages;
            Err("Desktop audio is unavailable".into())
        };
        if let Err(message) = result {
            self.virtual_midi.insert(client_id.clone(), state);
            return ControlResponse::Error {
                code: ControlErrorCode::Unavailable,
                message,
                current_revision: Some(
                    self.session.read().expect("session lock poisoned").revision,
                ),
            };
        }
        ControlResponse::VirtualMidiReleased { client_id }
    }

    fn handle_performance_control(&mut self, request: ControlRequest) -> ControlResponse {
        match request {
            ControlRequest::OutputMeter => {
                #[cfg(windows)]
                {
                    ControlResponse::OutputMeter {
                        meter: self
                            .audio
                            .as_ref()
                            .map(desktop_audio::DesktopAudio::take_output_meter)
                            .unwrap_or_default(),
                    }
                }
                #[cfg(not(windows))]
                {
                    ControlResponse::OutputMeter {
                        meter: Default::default(),
                    }
                }
            }
            ControlRequest::MidiSources => {
                #[cfg(windows)]
                {
                    let inventory = match self.scan_inventory() {
                        Ok(inventory) => inventory,
                        Err(error) => {
                            return ControlResponse::Error {
                                code: ControlErrorCode::Unavailable,
                                message: format!("Could not scan MIDI inputs: {error:#}"),
                                current_revision: Some(
                                    self.session.read().expect("session lock poisoned").revision,
                                ),
                            };
                        }
                    };
                    let present = inventory
                        .midi_inputs
                        .iter()
                        .cloned()
                        .collect::<BTreeSet<_>>();
                    let sources =
                        approved_midi_source_statuses(self.audio_preferences.as_ref(), &present);
                    ControlResponse::MidiSources { sources }
                }
                #[cfg(not(windows))]
                {
                    ControlResponse::MidiSources {
                        sources: Vec::new(),
                    }
                }
            }
            ControlRequest::BeginMidiLearn {
                instance_id,
                parameter_index,
            } => {
                #[cfg(windows)]
                {
                    let plugin_id = self
                        .plugins
                        .iter()
                        .find(|plugin| plugin.instance_id == instance_id)
                        .map(|plugin| plugin.plugin_id.as_str())
                        .or_else(|| {
                            self.performance_repository
                                .library()
                                .racks
                                .iter()
                                .flat_map(|rack| rack.slots.iter())
                                .find(|slot| slot.id.as_str() == instance_id)
                                .map(|slot| slot.plugin_id.as_str())
                        });
                    let parameter = plugin_id
                        .and_then(|plugin_id| {
                            self.plugins
                                .iter()
                                .find(|plugin| plugin.plugin_id == plugin_id)
                        })
                        .and_then(|plugin| {
                            plugin
                                .runtime
                                .parameters()
                                .parameters
                                .iter()
                                .find(|parameter| parameter.index == parameter_index)
                        });
                    if parameter.is_none_or(|parameter| {
                        parameter.flags.read_only
                            || matches!(
                                parameter.kind,
                                rackforge_plugin_api::ParameterKind::Meter { .. }
                            )
                    }) {
                        return ControlResponse::Error {
                            code: ControlErrorCode::InvalidRequest,
                            message: format!("Parameter {parameter_index} is missing or read-only"),
                            current_revision: Some(
                                self.session.read().expect("session lock poisoned").revision,
                            ),
                        };
                    }
                    let learn_id = self.next_midi_learn_id;
                    self.next_midi_learn_id = self.next_midi_learn_id.wrapping_add(1).max(1);
                    self.midi_learn = Some(DesktopMidiLearn {
                        id: learn_id,
                        started_at: Instant::now(),
                        candidate: None,
                    });
                    ControlResponse::MidiLearnStarted { learn_id }
                }
                #[cfg(not(windows))]
                {
                    ControlResponse::Error {
                        code: ControlErrorCode::Unavailable,
                        message: "MIDI Learn is unavailable".into(),
                        current_revision: None,
                    }
                }
            }
            ControlRequest::MidiLearnStatus { learn_id } => {
                #[cfg(windows)]
                {
                    match self
                        .midi_learn
                        .as_ref()
                        .filter(|learn| learn.id == learn_id)
                    {
                        Some(learn) => ControlResponse::MidiLearnStatus {
                            learn_id,
                            candidate: learn.candidate.clone(),
                        },
                        None => ControlResponse::Error {
                            code: ControlErrorCode::NotFound,
                            message: format!("MIDI Learn session {learn_id} does not exist"),
                            current_revision: Some(
                                self.session.read().expect("session lock poisoned").revision,
                            ),
                        },
                    }
                }
                #[cfg(not(windows))]
                {
                    ControlResponse::Error {
                        code: ControlErrorCode::Unavailable,
                        message: "MIDI Learn is unavailable".into(),
                        current_revision: None,
                    }
                }
            }
            ControlRequest::CancelMidiLearn { learn_id } => {
                #[cfg(windows)]
                {
                    if self
                        .midi_learn
                        .as_ref()
                        .is_some_and(|learn| learn.id == learn_id)
                    {
                        self.midi_learn = None;
                        ControlResponse::MidiLearnCancelled { learn_id }
                    } else {
                        ControlResponse::Error {
                            code: ControlErrorCode::NotFound,
                            message: format!("MIDI Learn session {learn_id} does not exist"),
                            current_revision: Some(
                                self.session.read().expect("session lock poisoned").revision,
                            ),
                        }
                    }
                }
                #[cfg(not(windows))]
                {
                    ControlResponse::Error {
                        code: ControlErrorCode::Unavailable,
                        message: "MIDI Learn is unavailable".into(),
                        current_revision: None,
                    }
                }
            }
            ControlRequest::PerformanceSnapshot => ControlResponse::PerformanceSnapshot {
                snapshot: Box::new(self.performance_snapshot()),
            },
            ControlRequest::EditPerformance {
                expected_revision,
                edit,
            } => {
                let current_revision = self.performance_repository.revision();
                if expected_revision != current_revision {
                    return ControlResponse::Error {
                        code: ControlErrorCode::Conflict,
                        message: format!(
                            "performance library changed from {} to {}",
                            expected_revision.as_str(),
                            current_revision.as_str()
                        ),
                        current_revision: Some(
                            self.session.read().expect("session lock poisoned").revision,
                        ),
                    };
                }
                let mut live = self
                    .session
                    .read()
                    .expect("session lock poisoned")
                    .live
                    .clone();
                let previous_live = live.clone();
                match self
                    .performance_repository
                    .apply_edit(&expected_revision, edit, &mut live)
                {
                    Ok(()) => {
                        if live != previous_live {
                            if let Err(error) = self.apply_program_events(
                                vec![SessionEvent::LiveStateReconciled { live }],
                                None,
                            ) {
                                return ControlResponse::Error {
                                    code: ControlErrorCode::Internal,
                                    message: format!(
                                        "Performance changed, but LIVE navigation could not be saved: {error}"
                                    ),
                                    current_revision: Some(
                                        self.session
                                            .read()
                                            .expect("session lock poisoned")
                                            .revision,
                                    ),
                                };
                            }
                            self.persist_session_checkpoint();
                        }
                        self.publish_performance_revision();
                        ControlResponse::PerformanceEdited {
                            snapshot: Box::new(self.performance_snapshot()),
                        }
                    }
                    Err(error) => ControlResponse::Error {
                        code: ControlErrorCode::Rejected,
                        message: format!("Could not save performance library: {error:#}"),
                        current_revision: Some(
                            self.session.read().expect("session lock poisoned").revision,
                        ),
                    },
                }
            }
            ControlRequest::ExportLiveShow { name } => {
                let exported = rackforge_core::live_show::now_unix_ms().and_then(|now| {
                    rackforge_core::live_show::assemble_live_show(
                        &name,
                        self.performance_repository.library(),
                        &self.state_store,
                        now,
                    )
                });
                match exported {
                    Ok(mut file) => {
                        if let Some(status) = self
                            .audio
                            .as_ref()
                            .and_then(|audio| audio.sequencer_status().ok())
                        {
                            file.tempo_bpm = Some(status.tempo_bpm);
                            file.beats_per_bar = Some(status.beats_per_bar);
                            file.beat_unit = Some(status.beat_unit);
                        }
                        file.live = Some(
                            self.session
                                .read()
                                .expect("session lock poisoned")
                                .live
                                .clone(),
                        );
                        ControlResponse::LiveShowExported {
                            file_name: rackforge_core::live_show::live_show_file_name(&name),
                            file: Box::new(file),
                        }
                    }
                    Err(error) => ControlResponse::Error {
                        code: ControlErrorCode::Rejected,
                        message: format!("Could not export the show: {error:#}"),
                        current_revision: Some(
                            self.session.read().expect("session lock poisoned").revision,
                        ),
                    },
                }
            }
            ControlRequest::InspectLiveShow { file } => {
                match rackforge_core::live_show::inspect_live_show(
                    &file,
                    &self.installed_plugin_versions(),
                    self.performance_repository.library(),
                ) {
                    Ok(preview) => ControlResponse::LiveShowInspected {
                        preview: Box::new(preview),
                    },
                    Err(error) => ControlResponse::Error {
                        code: ControlErrorCode::InvalidRequest,
                        message: format!("Could not validate the .rflive file: {error:#}"),
                        current_revision: Some(
                            self.session.read().expect("session lock poisoned").revision,
                        ),
                    },
                }
            }
            ControlRequest::ImportLiveShow { file } => self.import_live_show(&file),
            ControlRequest::PluginPresets { plugin_id } => {
                match self.state_store.list_presets(&plugin_id) {
                    Ok(presets) => ControlResponse::PluginPresets { plugin_id, presets },
                    Err(error) => ControlResponse::Error {
                        code: ControlErrorCode::InvalidRequest,
                        message: format!("Could not list RackForge presets: {error:#}"),
                        current_revision: Some(
                            self.session.read().expect("session lock poisoned").revision,
                        ),
                    },
                }
            }
            ControlRequest::SavePluginPreset { instance_id, name } => {
                self.save_host_preset(&instance_id, &name)
            }
            ControlRequest::LoadPluginPreset {
                instance_id,
                preset_id,
            } => self.load_host_preset(&instance_id, &preset_id),
            ControlRequest::RenamePluginPreset {
                plugin_id,
                preset_id,
                name,
            } => match self
                .state_store
                .rename_preset(&plugin_id, &preset_id, &name)
            {
                Ok(preset) => match self.state_store.list_presets(&plugin_id) {
                    Ok(presets) => ControlResponse::PluginPresetRenamed {
                        preset: Box::new(preset),
                        presets,
                    },
                    Err(error) => self.preset_error(ControlErrorCode::Internal, error),
                },
                Err(error) => self.preset_error(ControlErrorCode::InvalidRequest, error),
            },
            ControlRequest::DeletePluginPreset {
                plugin_id,
                preset_id,
            } => match self.state_store.delete_preset(&plugin_id, &preset_id) {
                Ok(_) => match self.state_store.list_presets(&plugin_id) {
                    Ok(presets) => ControlResponse::PluginPresetDeleted {
                        plugin_id,
                        preset_id,
                        presets,
                    },
                    Err(error) => self.preset_error(ControlErrorCode::Internal, error),
                },
                Err(error) => self.preset_error(ControlErrorCode::Rejected, error),
            },
            ControlRequest::PluginPreset {
                plugin_id,
                preset_id,
            } => match self.state_store.preset(&plugin_id, &preset_id) {
                Ok(preset) => ControlResponse::PluginPreset {
                    preset: Box::new(preset),
                },
                Err(error) => ControlResponse::Error {
                    code: ControlErrorCode::NotFound,
                    message: format!("Could not read RackForge preset: {error:#}"),
                    current_revision: Some(
                        self.session.read().expect("session lock poisoned").revision,
                    ),
                },
            },
            ControlRequest::ExportPluginPreset {
                plugin_id,
                preset_id,
            } => {
                let plugin_name = self
                    .plugins
                    .iter()
                    .find(|plugin| plugin.plugin_id == plugin_id)
                    .map(|plugin| plugin.name.as_str())
                    .unwrap_or(&plugin_id);
                match self
                    .state_store
                    .export_preset_file(&plugin_id, &preset_id, plugin_name)
                {
                    Ok((file_name, file)) => ControlResponse::PluginPresetExported {
                        file_name,
                        file: Box::new(file),
                    },
                    Err(error) => ControlResponse::Error {
                        code: ControlErrorCode::Rejected,
                        message: format!("Could not export RackForge preset: {error:#}"),
                        current_revision: Some(
                            self.session.read().expect("session lock poisoned").revision,
                        ),
                    },
                }
            }
            ControlRequest::InspectPluginPreset {
                target_plugin_id,
                file,
            } => {
                let plugin = self
                    .plugins
                    .iter()
                    .find(|plugin| plugin.plugin_id == target_plugin_id);
                match plugin {
                    Some(plugin) => match self.state_store.inspect_preset_file(
                        &target_plugin_id,
                        &plugin.version.to_string(),
                        plugin.runtime.manifest().state_version,
                        &file,
                    ) {
                        Ok(preview) => ControlResponse::PluginPresetInspected {
                            preview: Box::new(preview),
                        },
                        Err(error) => ControlResponse::Error {
                            code: ControlErrorCode::InvalidRequest,
                            message: format!("Could not validate .rfpreset: {error:#}"),
                            current_revision: Some(
                                self.session.read().expect("session lock poisoned").revision,
                            ),
                        },
                    },
                    None => ControlResponse::Error {
                        code: ControlErrorCode::Unavailable,
                        message: format!("Plugin {target_plugin_id} is not active on Desktop"),
                        current_revision: Some(
                            self.session.read().expect("session lock poisoned").revision,
                        ),
                    },
                }
            }
            ControlRequest::ImportPluginPreset {
                target_plugin_id,
                file,
                conflict_policy,
            } => {
                let compatibility = self
                    .plugins
                    .iter()
                    .find(|plugin| plugin.plugin_id == target_plugin_id)
                    .map(|plugin| {
                        (
                            plugin.version.to_string(),
                            plugin.runtime.manifest().state_version,
                        )
                    });
                match compatibility {
                    Some((version, state_version)) => match self.state_store.import_preset_file(
                        &target_plugin_id,
                        &version,
                        state_version,
                        &file,
                        conflict_policy,
                    ) {
                        Ok(preset) => match self.state_store.list_presets(&target_plugin_id) {
                            Ok(presets) => ControlResponse::PluginPresetImported {
                                preset: Box::new(preset),
                                presets,
                            },
                            Err(error) => ControlResponse::Error {
                                code: ControlErrorCode::Internal,
                                message: error.to_string(),
                                current_revision: Some(
                                    self.session.read().expect("session lock poisoned").revision,
                                ),
                            },
                        },
                        Err(error) => ControlResponse::Error {
                            code: ControlErrorCode::Rejected,
                            message: format!("Could not import .rfpreset: {error:#}"),
                            current_revision: Some(
                                self.session.read().expect("session lock poisoned").revision,
                            ),
                        },
                    },
                    None => ControlResponse::Error {
                        code: ControlErrorCode::Unavailable,
                        message: format!("Plugin {target_plugin_id} is not active on Desktop"),
                        current_revision: Some(
                            self.session.read().expect("session lock poisoned").revision,
                        ),
                    },
                }
            }
            ControlRequest::MaterializePluginState {
                plugin_id,
                sound_id,
            } => self.materialize_plugin_state(&plugin_id, sound_id),
            ControlRequest::PluginParameters { instance_id } => {
                self.plugin_parameters(&instance_id)
            }
            ControlRequest::SetPluginParameter {
                instance_id,
                parameter_index,
                value,
            } => self.set_plugin_parameter(&instance_id, parameter_index, value),
            ControlRequest::PluginStateParameters { state } => self.plugin_state_parameters(&state),
            ControlRequest::SetPluginStateParameter {
                state,
                parameter_index,
                value,
            } => self.set_plugin_state_parameter(&state, parameter_index, value),
            _ => ControlResponse::Error {
                code: ControlErrorCode::InvalidRequest,
                message: "Unsupported Desktop performance request".into(),
                current_revision: Some(
                    self.session.read().expect("session lock poisoned").revision,
                ),
            },
        }
    }

    fn materialize_plugin_state(
        &mut self,
        plugin_id: &str,
        requested_sound_id: Option<String>,
    ) -> ControlResponse {
        let Some(plugin) = self
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == plugin_id)
        else {
            return ControlResponse::Error {
                code: ControlErrorCode::Unavailable,
                message: format!("Plugin {plugin_id} is not active on Desktop"),
                current_revision: Some(
                    self.session.read().expect("session lock poisoned").revision,
                ),
            };
        };
        if let Some(sound_id) = requested_sound_id.as_deref()
            && !plugin
                .sound_summaries
                .iter()
                .any(|sound| sound.id == sound_id)
        {
            return ControlResponse::Error {
                code: ControlErrorCode::NotFound,
                message: format!("Plugin {plugin_id} does not provide sound {sound_id:?}"),
                current_revision: Some(
                    self.session.read().expect("session lock poisoned").revision,
                ),
            };
        }
        let sound_id = requested_sound_id
            .or_else(|| plugin.sound_summaries.first().map(|sound| sound.id.clone()));
        let state = (|| -> Result<_> {
            let mut isolated = plugin
                .runtime
                .create_instance_with_resource_overrides(&plugin.resources)?;
            if let Some(sound_id) = sound_id.as_deref() {
                isolated.load_preset(sound_id)?;
            }
            let bytes = isolated.save_state()?;
            self.state_store.put(
                plugin_id,
                &plugin.version.to_string(),
                plugin.runtime.manifest().state_version,
                sound_id,
                &bytes,
            )
        })();
        match state {
            Ok(state) => ControlResponse::PluginStateMaterialized {
                state: Box::new(state),
            },
            Err(error) => ControlResponse::Error {
                code: ControlErrorCode::Rejected,
                message: format!("Could not materialize Rack Slot state: {error:#}"),
                current_revision: Some(
                    self.session.read().expect("session lock poisoned").revision,
                ),
            },
        }
    }

    fn preset_error(
        &self,
        code: ControlErrorCode,
        error: impl std::fmt::Display,
    ) -> ControlResponse {
        ControlResponse::Error {
            code,
            message: format!("RackForge preset operation failed: {error}"),
            current_revision: Some(self.session.read().expect("session lock poisoned").revision),
        }
    }

    fn save_host_preset(&mut self, instance_id: &InstanceId, name: &str) -> ControlResponse {
        let state = self.session.read().expect("session lock poisoned").clone();
        if state.active_instance_id.as_ref() != Some(instance_id) {
            return self.preset_error(
                ControlErrorCode::Rejected,
                format!("plugin instance {instance_id} is not active in PLAY"),
            );
        }
        let Some(plugin) = self
            .plugins
            .iter()
            .find(|plugin| plugin.instance_id == instance_id.as_str())
        else {
            return self.preset_error(ControlErrorCode::NotFound, "plugin instance is missing");
        };
        #[cfg(windows)]
        let bytes = match self
            .audio
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Desktop audio is unavailable"))
            .and_then(|audio| audio.save_active_state())
        {
            Ok(bytes) => bytes,
            Err(error) => return self.preset_error(ControlErrorCode::Unavailable, error),
        };
        #[cfg(not(windows))]
        let bytes = match plugin.instance.save_state() {
            Ok(bytes) => bytes,
            Err(error) => return self.preset_error(ControlErrorCode::Rejected, error),
        };
        let reference = match self.state_store.put(
            &plugin.plugin_id,
            &plugin.version.to_string(),
            plugin.runtime.manifest().state_version,
            plugin.selected_sound_id.clone(),
            &bytes,
        ) {
            Ok(reference) => reference,
            Err(error) => return self.preset_error(ControlErrorCode::Rejected, error),
        };
        let plugin_id = plugin.plugin_id.clone();
        match self.state_store.save_preset(name, reference) {
            Ok(preset) => match self.state_store.list_presets(&plugin_id) {
                Ok(presets) => ControlResponse::PluginPresetSaved {
                    preset: Box::new(preset),
                    presets,
                },
                Err(error) => self.preset_error(ControlErrorCode::Internal, error),
            },
            Err(error) => self.preset_error(ControlErrorCode::InvalidRequest, error),
        }
    }

    fn load_host_preset(&mut self, instance_id: &InstanceId, preset_id: &str) -> ControlResponse {
        let active = self
            .session
            .read()
            .expect("session lock poisoned")
            .active_instance_id
            .clone();
        if active.as_ref() != Some(instance_id) {
            return self.preset_error(
                ControlErrorCode::Rejected,
                format!("plugin instance {instance_id} is not active in PLAY"),
            );
        }
        let Some(index) = self
            .plugins
            .iter()
            .position(|plugin| plugin.instance_id == instance_id.as_str())
        else {
            return self.preset_error(ControlErrorCode::NotFound, "plugin instance is missing");
        };
        let plugin_id = self.plugins[index].plugin_id.clone();
        let preset = match self.state_store.preset(&plugin_id, preset_id) {
            Ok(preset) => preset,
            Err(error) => return self.preset_error(ControlErrorCode::NotFound, error),
        };
        let installed_state_version = self.plugins[index].runtime.manifest().state_version;
        if preset.state.state_version != installed_state_version {
            return self.preset_error(
                ControlErrorCode::Rejected,
                format!(
                    "preset state v{} is incompatible with installed state v{}",
                    preset.state.state_version, installed_state_version
                ),
            );
        }
        let bytes = match self.state_store.read(&preset.state) {
            Ok(bytes) => bytes,
            Err(error) => return self.preset_error(ControlErrorCode::Rejected, error),
        };
        let validation = self.plugins[index]
            .runtime
            .create_instance_with_resource_overrides(&self.plugins[index].resources)
            .and_then(|mut instance| instance.load_state(&bytes));
        if let Err(error) = validation {
            return self.preset_error(ControlErrorCode::Rejected, error);
        }
        #[cfg(windows)]
        if let Err(error) = self
            .audio
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Desktop audio is unavailable"))
            .and_then(|audio| audio.restore_state(instance_id.as_str(), bytes.clone()))
        {
            return self.preset_error(ControlErrorCode::Unavailable, error);
        }
        if let Err(error) = self.plugins[index].instance.load_state(&bytes) {
            return self.preset_error(ControlErrorCode::Rejected, error);
        }
        self.plugins[index].selected_sound_id = preset.state.selected_sound_id.clone();
        let revision = {
            let mut session = self.session.write().expect("session lock poisoned");
            let revision = Revision::new(session.revision.get().saturating_add(1));
            let event = EventEnvelope {
                schema_version: SESSION_SCHEMA_VERSION,
                revision,
                command: None,
                event: SessionEvent::PluginStateRestored {
                    instance_id: instance_id.clone(),
                    selected_sound_id: preset.state.selected_sound_id.clone(),
                },
            };
            if let Err(error) = session.apply(&event) {
                return self.preset_error(ControlErrorCode::Internal, error);
            }
            revision
        };
        self.menu.set_play_sounds(
            self.plugins[index].sounds.clone(),
            preset.state.selected_sound_id.as_deref(),
        );
        self.menu.complete_plugin_preset_load(preset_id);
        self.sync_little_plugin_parameters();
        self.live_state_dirty = Some(Instant::now());
        self.persist_session_checkpoint();
        ControlResponse::PluginPresetLoaded {
            preset: Box::new(preset),
            revision,
        }
    }

    fn plugin_parameters(&mut self, instance_id: &InstanceId) -> ControlResponse {
        let state = self.session.read().expect("session lock poisoned");
        let revision = state.revision;
        let mut instance_id = instance_id.clone();
        if state.active_instance_id.as_ref() != Some(&instance_id) {
            // The panel this came from may have been opened before its plugin
            // was reinstalled or reactivated, which mints a new instance id.
            // The panel keeps working -- and every control on it silently
            // stops doing anything, because this guard rejected each edit
            // with an error only a status line showed. If the ACTIVE instance
            // is the same plugin, the edit is for it.
            let stale_plugin = self
                .plugins
                .iter()
                .find(|plugin| plugin.instance_id == instance_id.as_str())
                .map(|plugin| plugin.plugin_id.clone());
            let active = state.active_instance_id.clone();
            let retarget = match (stale_plugin, &active) {
                (Some(plugin_id), Some(active_id)) => self
                    .plugins
                    .iter()
                    .any(|plugin| {
                        plugin.instance_id == active_id.as_str() && plugin.plugin_id == plugin_id
                    })
                    .then(|| active_id.clone()),
                // The stale id is gone entirely: if the active instance
                // exists at all, route the edit there rather than nowhere.
                (None, Some(active_id)) => Some(active_id.clone()),
                _ => None,
            };
            match retarget {
                Some(target) => instance_id = target,
                None => {
                    return ControlResponse::Error {
                        code: ControlErrorCode::Rejected,
                        message: format!(
                            "Plugin instance {instance_id} is not the active Desktop plugin"
                        ),
                        current_revision: Some(revision),
                    };
                }
            }
        }
        drop(state);

        #[cfg(windows)]
        let result = self
            .audio
            .as_ref()
            .ok_or_else(|| "Desktop audio is unavailable".to_owned())
            .and_then(|audio| {
                audio
                    .plugin_parameters(instance_id.as_str())
                    .map_err(|error| error.to_string())
            });
        #[cfg(not(windows))]
        let result: std::result::Result<
            (
                rackforge_plugin_api::ParameterSchema,
                Vec<rackforge_control_api::PluginParameterValue>,
            ),
            String,
        > = Err("Desktop audio is unavailable".into());

        match result {
            Ok((schema, values)) => ControlResponse::PluginParameters {
                instance_id: instance_id.clone(),
                schema: Box::new(schema),
                values,
            },
            Err(message) => ControlResponse::Error {
                code: ControlErrorCode::Unavailable,
                message: format!("Could not read plugin parameters: {message}"),
                current_revision: Some(revision),
            },
        }
    }

    fn sync_little_plugin_parameters(&mut self) {
        let active_id = self
            .session
            .read()
            .expect("session lock poisoned")
            .active_instance_id
            .clone();
        let Some(instance_id) = active_id else {
            return;
        };
        if let ControlResponse::PluginParameters { schema, values, .. } =
            self.plugin_parameters(&instance_id)
        {
            self.menu.sync_plugin_parameters(
                *schema,
                values.into_iter().map(|value| (value.index, value.value)),
            );
        }
    }

    fn set_plugin_parameter(
        &mut self,
        instance_id: &InstanceId,
        parameter_index: u32,
        value: f64,
    ) -> ControlResponse {
        let state = self.session.read().expect("session lock poisoned");
        let revision = state.revision;
        if state.active_instance_id.as_ref() != Some(instance_id) {
            return ControlResponse::Error {
                code: ControlErrorCode::Rejected,
                message: format!("Plugin instance {instance_id} is not the active Desktop plugin"),
                current_revision: Some(revision),
            };
        }
        drop(state);
        if !value.is_finite() {
            return ControlResponse::Error {
                code: ControlErrorCode::InvalidRequest,
                message: "Plugin parameter value must be finite".into(),
                current_revision: Some(revision),
            };
        }

        #[cfg(windows)]
        let result = self
            .audio
            .as_ref()
            .ok_or_else(|| "Desktop audio is unavailable".to_owned())
            .and_then(|audio| {
                audio
                    .set_plugin_parameter(instance_id.as_str(), parameter_index, value)
                    .map_err(|error| error.to_string())
            });
        #[cfg(not(windows))]
        let result: std::result::Result<f64, String> = Err("Desktop audio is unavailable".into());

        match result {
            Ok(value) => {
                self.live_state_dirty = Some(Instant::now());
                ControlResponse::PluginParameterSet {
                    instance_id: instance_id.clone(),
                    parameter_index,
                    value,
                }
            }
            Err(message) => ControlResponse::Error {
                code: ControlErrorCode::Rejected,
                message: format!("Could not set plugin parameter: {message}"),
                current_revision: Some(revision),
            },
        }
    }

    fn plugin_state_parameters(
        &mut self,
        state: &rackforge_plugin_api::PluginStateReference,
    ) -> ControlResponse {
        let revision = self.session.read().expect("session lock poisoned").revision;
        let Some(plugin) = self
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == state.plugin_id)
        else {
            return ControlResponse::Error {
                code: ControlErrorCode::Unavailable,
                message: format!("Plugin {} is not active on Desktop", state.plugin_id),
                current_revision: Some(revision),
            };
        };
        if let Err(error) = validate_state_reference(plugin.runtime, state) {
            return ControlResponse::Error {
                code: ControlErrorCode::Rejected,
                message: format!("Incompatible Rack Slot state: {error:#}"),
                current_revision: Some(revision),
            };
        }
        let bytes = match self.state_store.read(state) {
            Ok(bytes) => bytes,
            Err(error) => {
                return ControlResponse::Error {
                    code: ControlErrorCode::NotFound,
                    message: format!("Rack Slot state is unavailable: {error:#}"),
                    current_revision: Some(revision),
                };
            }
        };
        let result = (|| -> Result<_> {
            let mut editor =
                IsolatedPluginStateEditor::open(plugin.runtime, &plugin.resources, &bytes)?;
            editor.parameters()
        })();
        match result {
            Ok((schema, values)) => ControlResponse::PluginStateParameters {
                state: Box::new(state.clone()),
                schema: Box::new(schema),
                values,
            },
            Err(error) => ControlResponse::Error {
                code: ControlErrorCode::Rejected,
                message: format!("Could not read Rack Slot parameters: {error:#}"),
                current_revision: Some(revision),
            },
        }
    }

    fn set_plugin_state_parameter(
        &mut self,
        state: &rackforge_plugin_api::PluginStateReference,
        parameter_index: u32,
        value: f64,
    ) -> ControlResponse {
        let revision = self.session.read().expect("session lock poisoned").revision;
        if !value.is_finite() {
            return ControlResponse::Error {
                code: ControlErrorCode::InvalidRequest,
                message: "Plugin parameter value must be finite".into(),
                current_revision: Some(revision),
            };
        }
        let Some(plugin) = self
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == state.plugin_id)
        else {
            return ControlResponse::Error {
                code: ControlErrorCode::Unavailable,
                message: format!("Plugin {} is not active on Desktop", state.plugin_id),
                current_revision: Some(revision),
            };
        };
        if let Err(error) = validate_state_reference(plugin.runtime, state) {
            return ControlResponse::Error {
                code: ControlErrorCode::Rejected,
                message: format!("Incompatible Rack Slot state: {error:#}"),
                current_revision: Some(revision),
            };
        }
        let bytes = match self.state_store.read(state) {
            Ok(bytes) => bytes,
            Err(error) => {
                return ControlResponse::Error {
                    code: ControlErrorCode::NotFound,
                    message: format!("Rack Slot state is unavailable: {error:#}"),
                    current_revision: Some(revision),
                };
            }
        };
        let edited = (|| -> Result<_> {
            let mut editor =
                IsolatedPluginStateEditor::open(plugin.runtime, &plugin.resources, &bytes)?;
            let canonical = editor.set_parameter(parameter_index, value)?;
            let bytes = editor.save_state()?;
            Ok((canonical, bytes))
        })();
        let (canonical, bytes) = match edited {
            Ok(edited) => edited,
            Err(error) => {
                return ControlResponse::Error {
                    code: ControlErrorCode::Rejected,
                    message: format!("Could not edit Rack Slot parameter: {error:#}"),
                    current_revision: Some(revision),
                };
            }
        };
        let manifest = plugin.runtime.manifest();
        match self.state_store.put(
            &manifest.id,
            &manifest.version,
            manifest.state_version,
            state.selected_sound_id.clone(),
            &bytes,
        ) {
            Ok(next_state) => ControlResponse::PluginStateParameterSet {
                state: Box::new(next_state),
                parameter_index,
                value: canonical,
            },
            Err(error) => ControlResponse::Error {
                code: ControlErrorCode::Rejected,
                message: format!("Could not store edited Rack Slot state: {error:#}"),
                current_revision: Some(revision),
            },
        }
    }

    fn installed_plugin_versions(&self) -> std::collections::BTreeMap<String, String> {
        self.plugins
            .iter()
            .map(|plugin| (plugin.plugin_id.clone(), plugin.version.to_string()))
            .collect()
    }

    /// Imports a `.rflive` show: authenticate and store the embedded
    /// states first (so every reference resolves), then upsert each
    /// document through the library's own edit machinery, reconciling the
    /// LIVE state once at the end the way a single edit would.
    fn import_live_show(&mut self, file: &rackforge_control_api::RfLiveFile) -> ControlResponse {
        let error_response =
            |code: ControlErrorCode, message: String, revision: Revision| ControlResponse::Error {
                code,
                message,
                current_revision: Some(revision),
            };
        let revision = self.session.read().expect("session lock poisoned").revision;
        let preview = match rackforge_core::live_show::inspect_live_show(
            file,
            &self.installed_plugin_versions(),
            self.performance_repository.library(),
        ) {
            Ok(preview) => preview,
            Err(error) => {
                return error_response(
                    ControlErrorCode::InvalidRequest,
                    format!("Could not validate the .rflive file: {error:#}"),
                    revision,
                );
            }
        };
        if let Err(error) =
            rackforge_core::live_show::store_live_show_states(file, &mut self.state_store)
        {
            return error_response(
                ControlErrorCode::Rejected,
                format!("Could not store the show's plugin states: {error:#}"),
                revision,
            );
        }
        let mut live = self
            .session
            .read()
            .expect("session lock poisoned")
            .live
            .clone();
        let previous_live = live.clone();
        for edit in rackforge_core::live_show::live_show_edits(file) {
            let current_revision = self.performance_repository.revision();
            if let Err(error) =
                self.performance_repository
                    .apply_edit(&current_revision, edit, &mut live)
            {
                return error_response(
                    ControlErrorCode::Rejected,
                    format!("Could not import the show: {error:#}"),
                    revision,
                );
            }
        }
        if live != previous_live {
            if let Err(error) =
                self.apply_program_events(vec![SessionEvent::LiveStateReconciled { live }], None)
            {
                return error_response(
                    ControlErrorCode::Internal,
                    format!("Show imported, but LIVE navigation could not be saved: {error}"),
                    revision,
                );
            }
            self.persist_session_checkpoint();
        }
        self.publish_performance_revision();
        ControlResponse::LiveShowImported {
            preview: Box::new(preview),
            snapshot: Box::new(self.performance_snapshot()),
        }
    }

    /// Publishes the library's revision to the web layer; each session
    /// socket compares and pushes a fresh snapshot to its own client.
    fn publish_performance_revision(&self) {
        let revision = self.performance_repository.revision();
        let mut shared = self
            .performance_revision_shared
            .write()
            .expect("performance revision lock poisoned");
        *shared = revision.as_str().to_owned();
    }

    fn performance_snapshot(&self) -> PerformanceSnapshot {
        PerformanceSnapshot {
            schema_version: PERFORMANCE_SNAPSHOT_SCHEMA_VERSION,
            revision: self.performance_repository.revision(),
            library: self.performance_repository.library().clone(),
            live: self
                .session
                .read()
                .expect("session lock poisoned")
                .live
                .clone(),
        }
    }

    fn persist_session_checkpoint(&mut self) {
        let snapshot = self.session.read().expect("session lock poisoned").clone();
        if let Err(error) = self.session_checkpoint.save(&snapshot) {
            eprintln!("SESSION_CHECKPOINT_ERROR {error:#}");
            self.status = format!("Session changed, but could not be saved: {error:#}");
        }
    }

    fn set_master_level(
        &mut self,
        level: MasterLevel,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        #[cfg(windows)]
        if let Some(audio) = &self.audio {
            audio
                .set_master_level(level)
                .map_err(|error| format!("Could not update master volume: {error:#}"))?;
        }
        let event = {
            let mut session = self.session.write().expect("session lock poisoned");
            let revision = session.revision.next()?;
            let event = EventEnvelope {
                schema_version: SESSION_SCHEMA_VERSION,
                revision,
                command,
                event: SessionEvent::MasterLevelChanged { level },
            };
            session.apply(&event)?;
            event
        };
        let percent = (u32::from(level.get()) + 5) / 10;
        self.status = format!("Master volume: {percent}%");
        self.persist_session_checkpoint();
        Ok(vec![event])
    }

    fn set_master_pan(
        &mut self,
        pan: MasterPan,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        #[cfg(windows)]
        if let Some(audio) = &self.audio {
            audio
                .set_master_pan(pan)
                .map_err(|error| format!("Could not update master pan: {error:#}"))?;
        }
        let event = {
            let mut session = self.session.write().expect("session lock poisoned");
            let revision = session.revision.next()?;
            let event = EventEnvelope {
                schema_version: SESSION_SCHEMA_VERSION,
                revision,
                command,
                event: SessionEvent::MasterPanChanged { pan },
            };
            session.apply(&event)?;
            event
        };
        let value = pan.get();
        self.status = if value == 0 {
            "Master pan: center".into()
        } else {
            let side = if value < 0 { 'L' } else { 'R' };
            let percent = (u32::from(value.unsigned_abs()) + 5) / 10;
            format!("Master pan: {side} {percent}%")
        };
        self.persist_session_checkpoint();
        Ok(vec![event])
    }

    /// Puts a LIVE target on stage: the session state and the Part's
    /// sequencer freight. The Desktop keeps playing its active voice —
    /// multi-Slot Rack audio remains the appliance's — so the Part's
    /// patterns sound through it, quantised to the next bar.
    fn activate_live_target(
        &mut self,
        location: rackforge_performance_api::LiveLocation,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        let active_mode = self
            .session
            .read()
            .expect("session lock poisoned")
            .active_mode;
        if active_mode != SurfaceMode::Live {
            return Err("LIVE targets can only be activated while LIVE is active".into());
        }
        let (rack_id, part_commands, sounding, unsounded_slots) = {
            let library = self.performance_repository.library();
            let rack = library
                .resolve_playable(&location)
                .map_err(|error| error.to_string())?;
            if !rack.enabled {
                return Err("the selected Rack is disabled".into());
            }
            let commands = library
                .resolve_part(&location)
                .map(|part| {
                    rackforge_core::sequencer::part_launch_commands(part, &library.patterns)
                })
                .unwrap_or_default();
            // The Desktop renders one voice at a time, so a Rack sounds
            // through its first enabled Slot. Mixing several Slots is the
            // appliance's, and the Slot order is the Rack's own.
            let enabled = rack.slots.iter().filter(|slot| slot.enabled);
            let mut enabled = enabled.peekable();
            let first = enabled
                .next()
                .map(|slot| (slot.plugin_id.clone(), slot.state.clone()));
            let remaining = enabled.count();
            (rack.id.clone(), commands, first, remaining)
        };
        // A binding that fails must never fail the activation: the show
        // goes on with the lanes that resolve.
        for part_command in part_commands {
            match self
                .audio
                .as_ref()
                .map(|audio| audio.sequencer_command(part_command))
            {
                Some(Ok(Ok(()))) => {}
                Some(Ok(Err(reason))) => {
                    eprintln!("SEQUENCER_PART_QUEUE_REJECTED reason={reason}");
                }
                Some(Err(error)) => {
                    eprintln!("SEQUENCER_PART_QUEUE_UNREACHABLE reason={error:#}");
                    break;
                }
                None => break,
            }
        }
        // Until here the activation only moved LIVE's state, which is how a
        // Rack could be shown on stage while PLAY's instrument kept sounding:
        // the Desktop engine has no notion of a Rack, so nobody ever pointed
        // the voice at the one the player chose.
        let mut events = Vec::new();
        if let Some((plugin_id, state)) = sounding {
            let instance_id = self
                .plugins
                .iter()
                .find(|plugin| plugin.plugin_id == plugin_id)
                .map(|plugin| plugin.instance_id.clone())
                .ok_or_else(|| {
                    format!("The Rack needs {plugin_id}, which is not installed here")
                })?;
            let instance_id = InstanceId::new(instance_id)
                .map_err(|error| format!("The Rack's instrument is unusable: {error}"))?;
            let previous = self
                .session
                .read()
                .expect("session lock poisoned")
                .active_instance_id
                .clone();
            let already_sounding = previous.as_ref() == Some(&instance_id);
            // Take the snapshot before anything is overwritten, and only the
            // first time: a second Rack must not record the first Rack's
            // sound as the one PLAY was holding.
            if self.play_voice.is_none()
                && let Some(previous) = previous.clone()
            {
                {
                    let saved = {
                        #[cfg(windows)]
                        {
                            self.audio
                                .as_ref()
                                .and_then(|audio| audio.save_active_state().ok())
                        }
                        #[cfg(not(windows))]
                        {
                            None::<Vec<u8>>
                        }
                    };
                    // A voice whose state cannot be read is still worth
                    // remembering by name; the player gets their instrument
                    // back even if its knobs do not survive.
                    println!(
                        "PLAY_VOICE_BORROWED instrument={} state_bytes={}",
                        previous.as_str(),
                        saved.as_ref().map_or(0, Vec::len)
                    );
                    self.play_voice = Some((previous, saved.unwrap_or_default()));
                }
            }
            if !already_sounding {
                events.extend(self.select_plugin(&instance_id, command.clone())?);
            }
            // The Slot carries its sound with it. Loading the instrument
            // without its state would hand the player the right box making
            // the wrong noise.
            if let Some(reference) = state {
                let bytes = self
                    .state_store
                    .read(&reference)
                    .map_err(|error| format!("Could not read the Slot's sound: {error:#}"))?;
                #[cfg(windows)]
                if let Some(audio) = &self.audio {
                    audio
                        .restore_state(instance_id.as_str(), bytes)
                        .map_err(|error| format!("Could not load the Slot's sound: {error:#}"))?;
                }
            }
            if unsounded_slots > 0 {
                // Said out loud rather than mixed silently into nothing.
                println!(
                    "LIVE_RACK_PARTIAL sounding={} silent_slots={unsounded_slots} reason=desktop-renders-one-voice",
                    instance_id.as_str()
                );
                self.status =
                    format!("LIVE: {unsounded_slots} more Slot(s) in this Rack stay silent here");
            }
        }
        events.extend(self.apply_program_events(
            vec![SessionEvent::LiveTargetActivated { location, rack_id }],
            command,
        )?);
        self.persist_session_checkpoint();
        Ok(events)
    }

    /// Puts PLAY's own instrument and sound back under the player's hands.
    ///
    /// Separate from [`Self::set_active_mode`] so that its failure is a
    /// failure to restore, not a failure to change mode.
    fn restore_play_voice(
        &mut self,
        instance_id: &InstanceId,
        state: Vec<u8>,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        let (known, sounding) = {
            let session = self.session.read().expect("session lock poisoned");
            (
                session.instance(instance_id).is_some(),
                session.active_instance_id.as_ref() == Some(instance_id),
            )
        };
        if !known {
            return Err(format!("{} is no longer loaded", instance_id.as_str()));
        }
        let events = if sounding {
            Vec::new()
        } else {
            self.select_plugin(instance_id, command)?
        };
        if !state.is_empty() {
            #[cfg(windows)]
            if let Some(audio) = &self.audio {
                audio
                    .restore_state(instance_id.as_str(), state)
                    .map_err(|error| format!("could not load its sound: {error:#}"))?;
            }
        }
        Ok(events)
    }

    fn set_active_mode(
        &mut self,
        mode: SurfaceMode,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        #[cfg(windows)]
        if let Some(audio) = &self.audio {
            audio
                .set_running(mode != SurfaceMode::Idle)
                .map_err(|error| format!("Could not change Desktop audio mode: {error:#}"))?;
            // Conducting is a LIVE gesture. A key-follow lane that kept
            // listening in PLAY did worse than linger: claiming a note keeps
            // it from the instrument, so notes vanished into a lane the
            // player could not see from where they were standing.
            audio
                .set_conducting(mode == SurfaceMode::Live)
                .map_err(|error| format!("Could not change Desktop conducting: {error:#}"))?;
            if mode != SurfaceMode::Live {
                // Keys held as the mode changes have their note-offs filtered
                // out on the way in. Panic stops the transport and flushes
                // every lane, so nothing is left sounding by a key that was
                // down when the player walked off.
                let _ = audio
                    .sequencer_command(rackforge_control_api::SequencerCommand::TransportPanic);
            }
        }

        let events = {
            let mut session = self.session.write().expect("session lock poisoned");
            let session_events = desktop_active_mode_events(&session, mode);

            let mut events = Vec::with_capacity(session_events.len());
            for session_event in session_events {
                let revision = session.revision.next()?;
                let event = EventEnvelope {
                    schema_version: SESSION_SCHEMA_VERSION,
                    revision,
                    command: command.clone(),
                    event: session_event,
                };
                session.apply(&event)?;
                events.push(event);
            }
            events
        };

        let mut events = events;

        let active_mode = active_mode_from_surface(mode);
        self.menu.sync_active_mode(active_mode);
        if mode == SurfaceMode::Play {
            let snapshot = self.performance_snapshot();
            self.menu.sync_performance_snapshot(snapshot);
        }
        // Returning to PLAY restores the instrument and the sound LIVE
        // borrowed the voice from. A restore that cannot happen must not take
        // the mode change down with it, and must not throw the memory away
        // either: the player asked to be in PLAY, and the next attempt still
        // has something to give them back.
        if mode == SurfaceMode::Play
            && let Some((instance_id, state)) = self.play_voice.clone()
        {
            match self.restore_play_voice(&instance_id, state, command.clone()) {
                Ok(restored) => {
                    println!(
                        "PLAY_VOICE_RESTORED instrument={} events={}",
                        instance_id.as_str(),
                        restored.len()
                    );
                    events.extend(restored);
                    self.play_voice = None;
                }
                Err(error) => {
                    eprintln!(
                        "PLAY_VOICE_RESTORE_FAILED instrument={} error={error}",
                        instance_id.as_str()
                    );
                }
            }
        }

        self.status = format!("Active mode: {active_mode:?}");
        self.persist_session_checkpoint();
        Ok(events)
    }

    fn select_plugin(
        &mut self,
        instance_id: &InstanceId,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        {
            let session = self.session.read().expect("session lock poisoned");
            if session.instance(instance_id).is_none() {
                return Err(format!("Unknown plugin instance: {instance_id}"));
            }
            if session.audition.is_some() || session.program_draft.is_some() {
                return Err(
                    "Finish or cancel the active plugin edit before changing plugins".into(),
                );
            }
        }

        let index = self
            .plugins
            .iter()
            .position(|plugin| plugin.instance_id == instance_id.as_str())
            .ok_or_else(|| format!("Unknown plugin instance: {instance_id}"))?;

        #[cfg(windows)]
        if let Some(audio) = &self.audio {
            audio
                .select_plugin(instance_id.as_str())
                .map_err(|error| format!("Could not select plugin audio: {error:#}"))?;
        }

        let event = {
            let mut session = self.session.write().expect("session lock poisoned");
            let revision = session
                .revision
                .next()
                .map_err(|error| format!("Could not advance session revision: {error}"))?;
            let event = EventEnvelope {
                schema_version: SESSION_SCHEMA_VERSION,
                revision,
                command,
                event: SessionEvent::ActiveInstanceChanged {
                    instance_id: instance_id.clone(),
                },
            };
            session.apply(&event)?;
            event
        };

        let plugin_name = self.plugins[index].name.clone();
        self.menu.sync_active_plugin(
            &self.plugins[index].instance_id,
            &self.plugins[index].plugin_id,
            &self.plugins[index].name,
            self.plugins[index].sounds.clone(),
            self.plugins[index].selected_sound_id.as_deref(),
        );
        if let Ok(presets) = self
            .state_store
            .list_presets(&self.plugins[index].plugin_id)
        {
            self.menu
                .set_plugin_presets(little_host_presets(presets), None);
        }
        self.sync_little_plugin_parameters();
        self.status = format!("{plugin_name} selected");
        self.persist_session_checkpoint();
        Ok(vec![event])
    }

    fn select_sound(
        &mut self,
        instance_id: &InstanceId,
        sound_id: &str,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        {
            let session = self.session.read().expect("session lock poisoned");
            let instance = session
                .instance(instance_id)
                .ok_or_else(|| format!("Unknown plugin instance: {instance_id}"))?;
            if !instance.sounds.iter().any(|sound| sound.id == sound_id) {
                return Err(format!(
                    "Unknown program {sound_id:?} for plugin instance {instance_id}"
                ));
            }
        }
        let index = self
            .plugins
            .iter()
            .position(|plugin| plugin.instance_id == instance_id.as_str())
            .ok_or_else(|| format!("Unknown plugin instance: {instance_id}"))?;
        self.plugins[index]
            .instance
            .load_preset(sound_id)
            .map_err(|error| format!("Could not load {sound_id}: {error:#}"))?;
        self.plugins[index].selected_sound_id = Some(sound_id.to_owned());

        let active = self
            .session
            .read()
            .expect("session lock poisoned")
            .active_instance_id
            .as_ref()
            == Some(instance_id);
        if active {
            self.menu
                .set_play_sounds(self.plugins[index].sounds.clone(), Some(sound_id));
        }

        let event = {
            let mut session = self.session.write().expect("session lock poisoned");
            let revision = session
                .revision
                .next()
                .map_err(|error| format!("Could not advance session revision: {error}"))?;
            let event = EventEnvelope {
                schema_version: SESSION_SCHEMA_VERSION,
                revision,
                command,
                event: SessionEvent::SoundSelected {
                    instance_id: instance_id.clone(),
                    sound_id: sound_id.to_owned(),
                },
            };
            session.apply(&event)?;
            event
        };

        self.status = format!("Loaded {sound_id}");
        self.live_state_dirty = Some(Instant::now());
        #[cfg(windows)]
        if active
            && let Some(audio) = &self.audio
            && let Err(error) = audio.select_sound(instance_id.as_str(), sound_id)
        {
            self.status = format!("Loaded {sound_id}, but audio did not switch: {error:#}");
        }
        if active {
            self.sync_little_plugin_parameters();
        }
        self.persist_session_checkpoint();
        Ok(vec![event])
    }

    fn activate_surface(
        &mut self,
        instance_id: InstanceId,
        request: SurfaceActivationRequest,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        {
            let session = self.session.read().expect("session lock poisoned");
            validate_desktop_surface_activation(&session, &instance_id, &request)?;
        }

        #[cfg(windows)]
        let response = self
            .audio
            .as_ref()
            .ok_or_else(|| "Desktop audio is unavailable".to_owned())?
            .activate_surface(instance_id.as_str(), request.clone())
            .map_err(|error| format!("Could not activate the plugin surface: {error:#}"))?;
        #[cfg(not(windows))]
        let response: rackforge_surface_api::SurfaceActivationResponse = {
            let _ = (&instance_id, &request);
            return Err("Desktop plugin surfaces require the Windows audio runtime".into());
        };
        response
            .validate()
            .map_err(|error| format!("Plugin returned an invalid surface response: {error}"))?;

        let events = self.apply_program_events(
            vec![SessionEvent::SurfaceActivated {
                instance_id,
                request,
                response,
            }],
            command,
        )?;
        self.status = "Returned to the active LITTLE surface".into();
        Ok(events)
    }

    fn emergency_stop(
        &mut self,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        #[cfg(windows)]
        if let Some(audio) = &self.audio {
            audio
                .emergency_stop()
                .map_err(|error| format!("Could not stop Desktop audio: {error:#}"))?;
        }
        let events = {
            let session = self.session.read().expect("session lock poisoned");
            desktop_emergency_stop_events(&session)
        };
        let events = self.apply_program_events(events, command)?;
        self.menu.sync_program_edit(None, None);
        self.menu.sync_active_mode(ActiveMode::Idle);
        self.status = "Emergency HOME · audio stopped".into();
        self.persist_session_checkpoint();
        Ok(events)
    }

    fn allocate_program_id(counter: &mut u64) -> u64 {
        let id = (*counter).max(1);
        *counter = id.checked_add(1).unwrap_or(1);
        id
    }

    fn apply_program_events(
        &mut self,
        events: Vec<SessionEvent>,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        let mut current = self.session.write().expect("session lock poisoned");
        let mut session = current.clone();
        let mut envelopes = Vec::with_capacity(events.len());
        for event in events {
            let revision = session
                .revision
                .next()
                .map_err(|error| format!("Could not advance session revision: {error}"))?;
            let envelope = EventEnvelope {
                schema_version: SESSION_SCHEMA_VERSION,
                revision,
                command: command.clone(),
                event,
            };
            session.apply(&envelope)?;
            envelopes.push(envelope);
        }
        *current = session;
        Ok(envelopes)
    }

    fn preview_audio_program(
        &self,
        instance_id: &str,
        prepared: PreparedProgram,
        reset: bool,
    ) -> Result<(), String> {
        #[cfg(windows)]
        {
            let audio = self.audio.as_ref().ok_or_else(|| {
                "Audio/MIDI is unavailable; program preview cannot start".to_owned()
            })?;
            audio
                .preview_program(instance_id, prepared, reset)
                .map_err(|error| format!("Could not preview the program: {error:#}"))
        }
        #[cfg(not(windows))]
        {
            let _ = (instance_id, prepared, reset);
            Err("Desktop program preview requires the Windows audio runtime".into())
        }
    }

    fn install_audio_program(
        &self,
        instance_id: &str,
        prepared: PreparedProgram,
    ) -> Result<PresetCatalog, String> {
        #[cfg(windows)]
        {
            let audio = self.audio.as_ref().ok_or_else(|| {
                "Audio/MIDI is unavailable; the program cannot be installed".to_owned()
            })?;
            audio
                .install_program(instance_id, prepared)
                .map_err(|error| format!("Could not install the program in audio: {error:#}"))
        }
        #[cfg(not(windows))]
        {
            let _ = (instance_id, prepared);
            Err("Desktop program installation requires the Windows audio runtime".into())
        }
    }

    fn restore_audio_program(
        &self,
        instance_id: &str,
        sound_id: Option<&str>,
    ) -> Result<(), String> {
        #[cfg(windows)]
        {
            let audio = self.audio.as_ref().ok_or_else(|| {
                "Audio/MIDI is unavailable; the previous program cannot be restored".to_owned()
            })?;
            audio
                .restore_program(instance_id, sound_id)
                .map_err(|error| format!("Could not restore the previous program: {error:#}"))
        }
        #[cfg(not(windows))]
        {
            let _ = (instance_id, sound_id);
            Err("Desktop program restoration requires the Windows audio runtime".into())
        }
    }

    fn active_program_draft(
        &self,
        draft_id: u64,
    ) -> Result<(ProgramDraftState, u64, Option<String>), String> {
        let session = self.session.read().expect("session lock poisoned");
        let draft = session
            .program_draft
            .as_ref()
            .filter(|draft| draft.draft_id == draft_id)
            .cloned()
            .ok_or_else(|| "Program draft is missing or no longer valid".to_owned())?;
        let audition = session
            .audition
            .as_ref()
            .filter(|audition| audition.instance_id == draft.instance_id)
            .ok_or_else(|| "Program draft lost its audio audition lease".to_owned())?;
        Ok((draft, audition.lease_id, audition.previous_sound_id.clone()))
    }

    fn begin_program_edit(
        &mut self,
        program_id: Option<String>,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        let (instance_id, previous_sound_id) = {
            let session = self.session.read().expect("session lock poisoned");
            if session.program_draft.is_some() || session.audition.is_some() {
                return Err("Another program edit is already active".into());
            }
            let instance_id = session
                .active_instance_id
                .clone()
                .ok_or_else(|| "No active plugin to edit".to_owned())?;
            let previous_sound_id = session
                .instance(&instance_id)
                .and_then(|instance| instance.selected_sound_id.clone());
            (instance_id, previous_sound_id)
        };
        let index = self
            .plugins
            .iter()
            .position(|plugin| plugin.instance_id == instance_id.as_str())
            .ok_or_else(|| format!("Unknown plugin instance: {instance_id}"))?;
        if !self.plugins[index].instance.supports_program_editing() {
            return Err(format!(
                "{} does not expose the RackForge program editor",
                self.plugins[index].name
            ));
        }
        let (prepared, editor) = {
            let plugin = &mut self.plugins[index];
            let prepared = plugin
                .instance
                .begin_program_edit(&ProgramEditRequest::new(program_id.clone()))
                .map_err(|error| format!("Could not begin program editing: {error:#}"))?;
            let editor = plugin
                .instance
                .program_editor_view(&prepared.document)
                .map_err(|error| format!("Could not build the program editor: {error:#}"))?;
            (prepared, editor)
        };
        self.preview_audio_program(instance_id.as_str(), prepared.clone(), true)?;

        let draft_id = Self::allocate_program_id(&mut self.next_program_draft_id);
        let lease_id = Self::allocate_program_id(&mut self.next_audition_lease_id);
        let draft = desktop_program_draft_state(
            draft_id,
            instance_id.clone(),
            program_id,
            &prepared,
            editor,
            false,
        )?;
        let events = self.apply_program_events(
            vec![
                SessionEvent::AuditionStarted {
                    lease_id,
                    instance_id: instance_id.clone(),
                    previous_sound_id,
                },
                SessionEvent::ProgramEditStarted {
                    draft: draft.clone(),
                },
            ],
            command,
        )?;
        self.menu.sync_program_edit(Some(draft), Some(lease_id));
        Ok(events)
    }

    fn edit_program_draft_field(
        &mut self,
        draft_id: u64,
        field_id: String,
        value: ProgramEditorValue,
        preview: bool,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        let (draft, lease_id, _) = self.active_program_draft(draft_id)?;
        let document: ProgramDocument = serde_json::from_str(&draft.document_json)
            .map_err(|error| format!("Stored program draft is invalid: {error}"))?;
        let index = self
            .plugins
            .iter()
            .position(|plugin| plugin.instance_id == draft.instance_id.as_str())
            .ok_or_else(|| format!("Unknown plugin instance: {}", draft.instance_id))?;
        let (prepared, editor) = {
            let plugin = &mut self.plugins[index];
            let prepared = plugin
                .instance
                .apply_program_edit(&ProgramFieldEditRequest {
                    schema_version: PROGRAM_EDITOR_SCHEMA_VERSION,
                    document,
                    field_id,
                    value,
                })
                .map_err(|error| format!("Could not edit the program: {error:#}"))?;
            let editor = plugin
                .instance
                .program_editor_view(&prepared.document)
                .map_err(|error| format!("Could not refresh the program editor: {error:#}"))?;
            (prepared, editor)
        };
        self.preview_audio_program(draft.instance_id.as_str(), prepared.clone(), false)?;
        if !preview {
            let updated = desktop_program_draft_state(
                draft_id,
                draft.instance_id,
                draft.original_program_id,
                &prepared,
                editor,
                true,
            )?;
            let events = self.apply_program_events(
                vec![SessionEvent::ProgramDraftUpdated {
                    draft: updated.clone(),
                }],
                command,
            )?;
            self.menu.sync_program_edit(Some(updated), Some(lease_id));
            return Ok(events);
        }
        Ok(Vec::new())
    }

    fn replace_program_draft_document(
        &mut self,
        draft_id: u64,
        document: ProgramDocument,
        dirty: bool,
        persist: bool,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        let (draft, lease_id, _) = self.active_program_draft(draft_id)?;
        let confirmed: ProgramDocument = serde_json::from_str(&draft.document_json)
            .map_err(|error| format!("Stored program draft is invalid: {error}"))?;
        if document.id != confirmed.id || document.plugin_id != confirmed.plugin_id {
            return Err("Program identity cannot change during editing".into());
        }
        let index = self
            .plugins
            .iter()
            .position(|plugin| plugin.instance_id == draft.instance_id.as_str())
            .ok_or_else(|| format!("Unknown plugin instance: {}", draft.instance_id))?;
        let (prepared, editor) = {
            let plugin = &mut self.plugins[index];
            let prepared = plugin
                .instance
                .prepare_program_save(&document)
                .map_err(|error| format!("Could not prepare the program: {error:#}"))?;
            let editor = plugin
                .instance
                .program_editor_view(&prepared.document)
                .map_err(|error| format!("Could not refresh the program editor: {error:#}"))?;
            (prepared, editor)
        };
        self.preview_audio_program(draft.instance_id.as_str(), prepared.clone(), false)?;
        if !persist {
            return Ok(Vec::new());
        }
        let updated = desktop_program_draft_state(
            draft_id,
            draft.instance_id,
            draft.original_program_id,
            &prepared,
            editor,
            dirty,
        )?;
        let events = self.apply_program_events(
            vec![SessionEvent::ProgramDraftUpdated {
                draft: updated.clone(),
            }],
            command,
        )?;
        self.menu.sync_program_edit(Some(updated), Some(lease_id));
        Ok(events)
    }

    fn restore_program_draft_preview(
        &mut self,
        draft_id: u64,
    ) -> Result<Vec<EventEnvelope>, String> {
        let (draft, _, _) = self.active_program_draft(draft_id)?;
        let document: ProgramDocument = serde_json::from_str(&draft.document_json)
            .map_err(|error| format!("Stored program draft is invalid: {error}"))?;
        let index = self
            .plugins
            .iter()
            .position(|plugin| plugin.instance_id == draft.instance_id.as_str())
            .ok_or_else(|| format!("Unknown plugin instance: {}", draft.instance_id))?;
        let prepared = self.plugins[index]
            .instance
            .prepare_program_save(&document)
            .map_err(|error| format!("Could not restore the draft preview: {error:#}"))?;
        self.preview_audio_program(draft.instance_id.as_str(), prepared, false)?;
        Ok(Vec::new())
    }

    fn set_program_draft_name(
        &mut self,
        draft_id: u64,
        name: String,
    ) -> Result<Vec<EventEnvelope>, String> {
        let (draft, _, _) = self.active_program_draft(draft_id)?;
        let mut document: ProgramDocument = serde_json::from_str(&draft.document_json)
            .map_err(|error| format!("Stored program draft is invalid: {error}"))?;
        document.name = name;
        self.replace_program_draft_document(draft_id, document, true, true, None)
    }

    fn restore_control_program(
        &mut self,
        plugin_index: usize,
        sound_id: Option<&str>,
    ) -> Result<(), String> {
        self.plugins[plugin_index]
            .instance
            .reset()
            .map_err(|error| format!("Could not reset the program editor instance: {error:#}"))?;
        if let Some(sound_id) = sound_id {
            self.plugins[plugin_index]
                .instance
                .load_preset(sound_id)
                .map_err(|error| format!("Could not restore {sound_id}: {error:#}"))?;
        }
        Ok(())
    }

    fn save_program_draft(
        &mut self,
        draft_id: u64,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        let (draft, lease_id, previous_sound_id) = self.active_program_draft(draft_id)?;
        let document: ProgramDocument = serde_json::from_str(&draft.document_json)
            .map_err(|error| format!("Stored program draft is invalid: {error}"))?;
        let index = self
            .plugins
            .iter()
            .position(|plugin| plugin.instance_id == draft.instance_id.as_str())
            .ok_or_else(|| format!("Unknown plugin instance: {}", draft.instance_id))?;
        let prepared = self.plugins[index]
            .instance
            .prepare_program_save(&document)
            .map_err(|error| format!("Could not prepare the program for saving: {error:#}"))?;
        PluginStorage::new(&self.options.data_root)
            .save_prepared_program(&prepared)
            .map_err(|error| format!("Could not save the program: {error:#}"))?;
        self.plugins[index]
            .instance
            .install_program(&prepared)
            .map_err(|error| format!("Could not install the saved program: {error:#}"))?;
        let catalog = self.install_audio_program(draft.instance_id.as_str(), prepared.clone())?;
        let preset_id = format!("custom.{}", prepared.document.id);
        if !catalog.presets.iter().any(|preset| preset.id == preset_id) {
            return Err("Installed program is missing from the plugin catalog".into());
        }
        self.restore_audio_program(draft.instance_id.as_str(), previous_sound_id.as_deref())?;
        self.restore_control_program(index, previous_sound_id.as_deref())?;

        let (banks, sound_summaries, sounds) = desktop_catalog_views(&catalog);
        let saved_sound = sound_summaries
            .iter()
            .find(|sound| sound.id == preset_id)
            .cloned()
            .ok_or_else(|| "Installed program has no Desktop catalog entry".to_owned())?;
        let events = self.apply_program_events(
            vec![
                SessionEvent::ProgramSaved {
                    draft_id,
                    instance_id: draft.instance_id.clone(),
                    sound: saved_sound,
                },
                SessionEvent::AuditionEnded {
                    lease_id,
                    instance_id: draft.instance_id.clone(),
                    restored_sound_id: previous_sound_id.clone(),
                    reason: AuditionEndReason::Released,
                },
            ],
            command,
        )?;
        self.plugins[index].banks = banks.clone();
        self.plugins[index].sound_summaries = sound_summaries.clone();
        self.plugins[index].sounds = sounds.clone();
        if let Some(instance) = self
            .session
            .write()
            .expect("session lock poisoned")
            .instances
            .iter_mut()
            .find(|instance| instance.instance_id == draft.instance_id)
        {
            instance.banks = banks;
            instance.sounds = sound_summaries;
        }
        self.menu.sync_active_plugin(
            &self.plugins[index].instance_id,
            &self.plugins[index].plugin_id,
            &self.plugins[index].name,
            sounds,
            previous_sound_id.as_deref(),
        );
        self.menu.sync_program_edit(None, None);
        self.persist_session_checkpoint();
        Ok(events)
    }

    fn cancel_program_edit(
        &mut self,
        draft_id: u64,
        command: Option<CommandRef>,
    ) -> Result<Vec<EventEnvelope>, String> {
        let (draft, lease_id, previous_sound_id) = self.active_program_draft(draft_id)?;
        let index = self
            .plugins
            .iter()
            .position(|plugin| plugin.instance_id == draft.instance_id.as_str())
            .ok_or_else(|| format!("Unknown plugin instance: {}", draft.instance_id))?;
        self.restore_audio_program(draft.instance_id.as_str(), previous_sound_id.as_deref())?;
        self.restore_control_program(index, previous_sound_id.as_deref())?;
        let events = self.apply_program_events(
            vec![
                SessionEvent::ProgramEditCancelled {
                    draft_id,
                    instance_id: draft.instance_id.clone(),
                },
                SessionEvent::AuditionEnded {
                    lease_id,
                    instance_id: draft.instance_id,
                    restored_sound_id: previous_sound_id,
                    reason: AuditionEndReason::Cancelled,
                },
            ],
            command,
        )?;
        self.menu.sync_program_edit(None, None);
        Ok(events)
    }

    fn apply_command(&mut self, command: MenuCommand) {
        match command {
            MenuCommand::SetActiveMode { mode } => {
                let surface_mode = match mode {
                    ActiveMode::Idle => SurfaceMode::Idle,
                    ActiveMode::Live => SurfaceMode::Live,
                    ActiveMode::Play => SurfaceMode::Play,
                };
                if let Err(error) = self.set_active_mode(surface_mode, None) {
                    self.status = error;
                }
            }
            MenuCommand::SelectPlugin { instance_id } => {
                let instance_id = match InstanceId::new(instance_id) {
                    Ok(instance_id) => instance_id,
                    Err(error) => {
                        self.status = format!("Invalid plugin instance: {error}");
                        return;
                    }
                };
                if let Err(error) = self.select_plugin(&instance_id, None) {
                    self.status = error;
                }
            }
            MenuCommand::SelectSound { id } => {
                let Some(active_id) = self
                    .session
                    .read()
                    .expect("session lock poisoned")
                    .active_instance_id
                    .clone()
                else {
                    self.status = "No active plugin".into();
                    return;
                };
                if let Err(error) = self.select_sound(&active_id, &id, None) {
                    self.status = error;
                }
            }
            MenuCommand::LoadPluginPreset { preset_id } => {
                let Some(active_id) = self
                    .session
                    .read()
                    .expect("session lock poisoned")
                    .active_instance_id
                    .clone()
                else {
                    self.status = "No active plugin".into();
                    return;
                };
                match self.load_host_preset(&active_id, &preset_id) {
                    ControlResponse::PluginPresetLoaded { .. } => {
                        self.menu.complete_plugin_preset_load(&preset_id);
                        self.status = format!("RackForge preset loaded: {preset_id}");
                    }
                    ControlResponse::Error { message, .. } => self.status = message,
                    response => {
                        self.status = format!(
                            "Unexpected response while loading RackForge preset: {response:?}"
                        )
                    }
                }
            }
            MenuCommand::SavePluginPreset { name } => {
                let Some(active_id) = self
                    .session
                    .read()
                    .expect("session lock poisoned")
                    .active_instance_id
                    .clone()
                else {
                    self.menu
                        .complete_plugin_preset_save(Err("No active plugin is available".into()));
                    return;
                };
                match self.save_host_preset(&active_id, &name) {
                    ControlResponse::PluginPresetSaved { preset, presets } => {
                        let saved = PlayPreset::new(
                            preset.id.clone(),
                            preset.name.clone(),
                            format!("v{}", preset.state.plugin_version),
                        );
                        self.menu
                            .complete_plugin_preset_save(Ok((saved, little_host_presets(presets))));
                        self.status = format!("RackForge preset saved: {}", preset.name);
                    }
                    ControlResponse::Error { message, .. } => {
                        self.status = message.clone();
                        self.menu.complete_plugin_preset_save(Err(message));
                    }
                    response => {
                        let message = format!(
                            "Unexpected response while saving RackForge preset: {response:?}"
                        );
                        self.status = message.clone();
                        self.menu.complete_plugin_preset_save(Err(message));
                    }
                }
            }
            MenuCommand::SetPluginParameter {
                instance_id,
                parameter_index,
                value,
            } => {
                let Ok(instance_id) = InstanceId::new(instance_id) else {
                    self.status = "LITTLE produced an invalid plugin instance".into();
                    return;
                };
                match self.set_plugin_parameter(&instance_id, parameter_index, value) {
                    ControlResponse::PluginParameterSet { value, .. } => {
                        self.menu
                            .complete_plugin_parameter_set(parameter_index, value);
                    }
                    ControlResponse::Error { message, .. } => self.status = message,
                    response => {
                        self.status = format!("Unexpected parameter response: {response:?}")
                    }
                }
            }
            MenuCommand::TriggerPluginParameter {
                instance_id,
                parameter_index,
            } => {
                let Ok(instance_id) = InstanceId::new(instance_id) else {
                    self.status = "LITTLE produced an invalid plugin instance".into();
                    return;
                };
                let pressed = self.set_plugin_parameter(&instance_id, parameter_index, 1.0);
                let released = self.set_plugin_parameter(&instance_id, parameter_index, 0.0);
                match (pressed, released) {
                    (
                        ControlResponse::PluginParameterSet { .. },
                        ControlResponse::PluginParameterSet { value, .. },
                    ) => self
                        .menu
                        .complete_plugin_parameter_set(parameter_index, value),
                    (ControlResponse::Error { message, .. }, _)
                    | (_, ControlResponse::Error { message, .. }) => self.status = message,
                    _ => self.status = "Unexpected trigger response".into(),
                }
            }
            MenuCommand::BeginProgramEdit { program_id } => {
                match self.begin_program_edit(program_id, None) {
                    Ok(_) => {
                        self.status = "Program editor ready · changes are auditioned live".into()
                    }
                    Err(error) => self.status = error,
                }
            }
            MenuCommand::EditProgramDraftField {
                draft_id,
                field_id,
                value,
                preview,
            } => match self.edit_program_draft_field(draft_id, field_id, value, preview, None) {
                Ok(_) if preview => self.status = "Previewing program change".into(),
                Ok(_) => self.status = "Program draft updated".into(),
                Err(error) => self.status = error,
            },
            MenuCommand::RestoreProgramDraftPreview { draft_id } => {
                match self.restore_program_draft_preview(draft_id) {
                    Ok(_) => self.status = "Restored the confirmed draft preview".into(),
                    Err(error) => self.status = error,
                }
            }
            MenuCommand::SetProgramDraftName { draft_id, name } => {
                match self.set_program_draft_name(draft_id, name) {
                    Ok(_) => self.status = "Program name updated".into(),
                    Err(error) => self.status = error,
                }
            }
            MenuCommand::SaveProgramDraft { draft_id } => {
                match self.save_program_draft(draft_id, None) {
                    Ok(_) => self.status = "Program saved".into(),
                    Err(error) => self.status = error,
                }
            }
            MenuCommand::CancelProgramEdit { draft_id } => {
                match self.cancel_program_edit(draft_id, None) {
                    Ok(_) => self.status = "Program edit cancelled".into(),
                    Err(error) => self.status = error,
                }
            }
            MenuCommand::ResolveProgramExit {
                draft_id,
                decision,
                destination,
            } => {
                let result = match decision {
                    ProgramExitDecision::Save => self.save_program_draft(draft_id, None),
                    ProgramExitDecision::Discard => self.cancel_program_edit(draft_id, None),
                };
                if let Err(error) = result {
                    self.status = error;
                    return;
                }
                match destination {
                    ProgramExitDestination::CustomPrograms => {
                        self.status = match decision {
                            ProgramExitDecision::Save => "Program saved".into(),
                            ProgramExitDecision::Discard => "Program changes discarded".into(),
                        };
                    }
                    ProgramExitDestination::ActiveMode {
                        mode,
                        selected_sound_id,
                    } => self.apply_command(MenuCommand::ReturnToActiveMode {
                        mode,
                        cancel_draft_id: None,
                        selected_sound_id,
                    }),
                }
            }
            MenuCommand::ReturnToActiveMode {
                mode,
                cancel_draft_id,
                selected_sound_id,
            } => {
                if let Some(draft_id) = cancel_draft_id
                    && let Err(error) = self.cancel_program_edit(draft_id, None)
                {
                    self.status = error;
                    return;
                }
                let mut focus_sound_id = selected_sound_id;
                if mode == ActiveMode::Play {
                    let active_id = self
                        .session
                        .read()
                        .expect("session lock poisoned")
                        .active_instance_id
                        .as_ref()
                        .map(|id| id.as_str().to_owned());
                    let Some(plugin) = active_id.as_deref().and_then(|active| {
                        self.plugins
                            .iter()
                            .find(|plugin| plugin.instance_id == active)
                    }) else {
                        self.status = "No active plugin to return to".into();
                        return;
                    };
                    focus_sound_id = focus_sound_id.or_else(|| plugin.selected_sound_id.clone());
                    self.menu.sync_active_plugin(
                        &plugin.instance_id,
                        &plugin.plugin_id,
                        &plugin.name,
                        plugin.sounds.clone(),
                        focus_sound_id.as_deref(),
                    );
                }
                self.menu
                    .complete_return_to_active_mode(mode, focus_sound_id.as_deref());
                self.status = match mode {
                    ActiveMode::Idle => "Returned to RackForge home".into(),
                    ActiveMode::Live => "Returned to active LIVE performance".into(),
                    ActiveMode::Play => "Returned to the active PLAY plugin".into(),
                };
            }
            MenuCommand::ForceHome => {
                let active_draft_id = self
                    .session
                    .read()
                    .expect("session lock poisoned")
                    .program_draft
                    .as_ref()
                    .map(|draft| draft.draft_id);
                if let Some(draft_id) = active_draft_id
                    && let Err(error) = self.cancel_program_edit(draft_id, None)
                {
                    self.status = format!("Could not cancel the active program edit: {error}");
                    return;
                }
                {
                    let mut session = self.session.write().expect("session lock poisoned");
                    session.active_mode = SurfaceMode::Idle;
                    session.revision = Revision::new(session.revision.get().saturating_add(1));
                }
                self.persist_session_checkpoint();
                #[cfg(windows)]
                if let Some(audio) = &self.audio
                    && let Err(error) = audio.emergency_stop()
                {
                    self.status =
                        format!("Emergency home activated, but audio reset failed: {error:#}");
                    return;
                }
                self.status = "Emergency HOME · audio stopped".into();
            }
            other => {
                self.status = format!("Desktop bridge pending: {other:?}");
            }
        }
    }

    fn keyboard(&mut self, context: &egui::Context) {
        let keys = [Key::Q, Key::W, Key::E, Key::R];
        for (index, key) in keys.into_iter().enumerate() {
            let down = context.input(|input| input.key_down(key));
            match (down, self.keyboard_down[index]) {
                (true, None) => {
                    self.keyboard_down[index] = Some(Instant::now());
                    self.menu.set_button_pressed(short_input(index), true);
                }
                (false, Some(started)) => {
                    self.keyboard_down[index] = None;
                    self.menu.set_button_pressed(short_input(index), false);
                    self.apply_input(if started.elapsed() >= LONG_PRESS {
                        long_input(index)
                    } else {
                        short_input(index)
                    });
                }
                _ => {}
            }
        }
        if context.input(|input| input.key_pressed(Key::ArrowLeft)) {
            self.apply_input(Input::Button2);
        }
        if context.input(|input| input.key_pressed(Key::ArrowRight)) {
            self.apply_input(Input::Button3);
        }
        if context.input(|input| input.key_pressed(Key::Enter)) {
            self.apply_input(Input::Button1);
        }
        if context.input(|input| input.key_pressed(Key::Escape)) {
            self.apply_input(Input::Button4);
        }
    }

    fn little_display(&mut self, ui: &mut egui::Ui) {
        let screen = self.menu.render();
        let width = ui.available_width().min(LITTLE_WIDTH);
        let (outer, _) = ui.allocate_exact_size(Vec2::new(width, LITTLE_HEIGHT), Sense::hover());
        let geometry = LittleGeometry::new(outer);
        let painter = ui.painter_at(geometry.outer);

        painter.rect_filled(
            geometry.outer.translate(Vec2::new(0.0, 5.0)),
            18.0,
            Color32::from_black_alpha(72),
        );
        painter.rect(
            geometry.outer,
            18.0,
            Color32::from_rgb(37, 43, 47),
            Stroke::new(1.5_f32, Color32::from_rgb(91, 101, 106)),
            StrokeKind::Inside,
        );

        let glass = painter.with_clip_rect(geometry.glass);
        glass.rect_filled(geometry.glass, 9.0, Color32::from_rgb(222, 228, 216));

        let mut scan_y = geometry.glass.min.y + 3.0;
        while scan_y < geometry.glass.max.y {
            glass.line_segment(
                [
                    Pos2::new(geometry.glass.min.x, scan_y),
                    Pos2::new(geometry.glass.max.x, scan_y),
                ],
                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(42, 55, 45, 9)),
            );
            scan_y += 5.0;
        }

        glass.rect_filled(geometry.header, 0.0, Color32::from_rgb(16, 20, 22));
        if let Some(title) = screen
            .header
            .text(rackforge_surface_runtime::DISPLAY_COLUMNS)
        {
            glass.text(
                Pos2::new(geometry.header.min.x + 18.0, geometry.header.center().y),
                Align2::LEFT_CENTER,
                title,
                FontId::monospace(20.0),
                Color32::from_rgb(244, 247, 240),
            );
        }

        glass.text(
            geometry.line_1,
            Align2::CENTER_CENTER,
            &screen.line_1,
            FontId::monospace(30.0),
            Color32::from_rgb(10, 16, 14),
        );
        glass.text(
            geometry.line_2,
            Align2::CENTER_CENTER,
            &screen.line_2,
            FontId::monospace(22.0),
            Color32::from_rgb(45, 57, 50),
        );

        glass.rect_filled(
            geometry.footer,
            0.0,
            Color32::from_rgba_unmultiplied(94, 108, 96, 22),
        );
        glass.line_segment(
            [geometry.footer.left_top(), geometry.footer.right_top()],
            Stroke::new(1.0_f32, Color32::from_rgb(151, 162, 151)),
        );
        let button_width = geometry.footer.width() / 4.0;
        for index in 0..4 {
            let center = Pos2::new(geometry.columns[index], geometry.footer.center().y);
            if self.button_is_down(index) {
                let highlight = Rect::from_center_size(center, Vec2::new(button_width - 8.0, 34.0));
                glass.rect_filled(highlight, 5.0, Color32::from_rgb(16, 20, 22));
            }
            glass.text(
                center,
                Align2::CENTER_CENTER,
                &screen.footer[index].label,
                FontId::monospace(16.0),
                if self.button_is_down(index) {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(25, 30, 32)
                },
            );
        }

        painter.rect_stroke(
            geometry.glass,
            9.0,
            Stroke::new(2.0_f32, Color32::from_rgb(7, 10, 11)),
            StrokeKind::Inside,
        );
        painter.line_segment(
            [
                Pos2::new(geometry.glass.min.x + 10.0, geometry.glass.min.y + 3.0),
                Pos2::new(geometry.glass.max.x - 10.0, geometry.glass.min.y + 3.0),
            ],
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 34)),
        );
    }

    fn virtual_buttons(&mut self, ui: &mut egui::Ui) {
        let screen = self.menu.render();
        let width = ui.available_width().min(LITTLE_WIDTH);
        let (row, _) = ui.allocate_exact_size(Vec2::new(width, 66.0), Sense::hover());
        let mapped = row.shrink2(Vec2::new(12.0, 0.0));
        let column_width = mapped.width() / 4.0;
        for index in 0..4 {
            let center = Pos2::new(
                mapped.min.x + column_width * (index as f32 + 0.5),
                row.center().y,
            );
            let button_rect = Rect::from_center_size(center, Vec2::new(column_width - 20.0, 56.0));
            let label = &screen.footer[index].label;
            let response = ui.put(
                button_rect,
                egui::Button::new(RichText::new(label).size(17.0).strong())
                    .fill(Color32::from_rgb(47, 54, 59))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(94, 105, 111)))
                    .corner_radius(9.0),
            );
            let down = response.is_pointer_button_down_on();
            match (down, self.button_down[index]) {
                (true, None) => {
                    self.button_down[index] = Some(Instant::now());
                    self.menu.set_button_pressed(short_input(index), true);
                }
                (false, Some(started)) => {
                    self.button_down[index] = None;
                    self.menu.set_button_pressed(short_input(index), false);
                    self.apply_input(if started.elapsed() >= LONG_PRESS {
                        long_input(index)
                    } else {
                        short_input(index)
                    });
                }
                _ => {}
            }
            response.on_hover_text(format!(
                "Button {} · hold for long press · keyboard {}",
                index + 1,
                ["Q", "W", "E", "R"][index]
            ));
        }
    }

    fn button_is_down(&self, index: usize) -> bool {
        self.button_down[index].is_some() || self.keyboard_down[index].is_some() || {
            #[cfg(windows)]
            {
                self.controller_button_down[index].is_some()
            }
            #[cfg(not(windows))]
            {
                false
            }
        }
    }
}

impl eframe::App for DesktopApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(touched) = self.live_state_dirty
            && touched.elapsed() >= Duration::from_millis(1500)
        {
            self.flush_live_state();
        }
        #[cfg(windows)]
        self.poll_audio_error();
        #[cfg(windows)]
        self.poll_controller();
        let _ = self.poll_plugin_install(context);
        self.keyboard(context);
        context.request_repaint_after(Duration::from_millis(16));
        egui::TopBottomPanel::top("desktop-toolbar").show(context, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("RACKFORGE").color(Color32::from_rgb(58, 216, 224)));
                ui.label(RichText::new("DESKTOP HOST · windows-x86-64").weak());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Open Web UI").clicked() {
                        match webbrowser::open(&self.web_url) {
                            Ok(()) => self.status = format!("Opened {}", self.web_url),
                            Err(error) => self.status = format!("Could not open browser: {error}"),
                        }
                    }
                    if ui
                        .add_enabled(
                            !self.install_in_progress(),
                            egui::Button::new("Install .rfplugin"),
                        )
                        .clicked()
                    {
                        self.begin_plugin_install();
                    }
                    ui.monospace(&self.web_url);
                });
            });
        });
        egui::CentralPanel::default().show(context, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(28.0);
                self.little_display(ui);
                ui.add_space(24.0);
                self.virtual_buttons(ui);
                ui.add_space(18.0);
                ui.label(RichText::new(&self.status).weak());
                ui.label(
                    RichText::new(format!(
                        "RackForge Root · {}",
                        self.options.rackforge_root.display()
                    ))
                    .small()
                    .weak(),
                );
            });
        });
    }
}

fn short_input(index: usize) -> Input {
    [
        Input::Button1,
        Input::Button2,
        Input::Button3,
        Input::Button4,
    ][index]
}

fn long_input(index: usize) -> Input {
    [
        Input::Button1Long,
        Input::Button2Long,
        Input::Button3Long,
        Input::Button4Long,
    ][index]
}

fn active_mode_from_surface(mode: SurfaceMode) -> ActiveMode {
    match mode {
        SurfaceMode::Idle => ActiveMode::Idle,
        SurfaceMode::Live => ActiveMode::Live,
        SurfaceMode::Play => ActiveMode::Play,
    }
}

fn validate_desktop_surface_activation(
    session: &SessionState,
    instance_id: &InstanceId,
    request: &SurfaceActivationRequest,
) -> Result<(), String> {
    request.validate().map_err(|error| error.to_string())?;
    let instance = session
        .instance(instance_id)
        .ok_or_else(|| format!("Unknown plugin instance: {instance_id}"))?;
    if session.active_instance_id.as_ref() != Some(instance_id) {
        return Err(format!("Plugin instance {instance_id} is not active"));
    }
    if !instance
        .ui_layouts
        .iter()
        .any(|layout| layout == &request.layout_id)
    {
        return Err(format!(
            "Plugin {} does not expose layout {}",
            instance.plugin_id, request.layout_id
        ));
    }
    Ok(())
}

fn desktop_emergency_stop_events(session: &SessionState) -> Vec<SessionEvent> {
    let mut events = Vec::with_capacity(3);
    if let Some(draft) = session.program_draft.as_ref() {
        events.push(SessionEvent::ProgramEditCancelled {
            draft_id: draft.draft_id,
            instance_id: draft.instance_id.clone(),
        });
    }
    if let Some(audition) = session.audition.as_ref() {
        events.push(SessionEvent::AuditionEnded {
            lease_id: audition.lease_id,
            instance_id: audition.instance_id.clone(),
            restored_sound_id: None,
            reason: AuditionEndReason::Cancelled,
        });
    }
    events.push(SessionEvent::ActiveModeChanged {
        mode: SurfaceMode::Idle,
    });
    events
}

fn desktop_active_mode_events(session: &SessionState, mode: SurfaceMode) -> Vec<SessionEvent> {
    let mut events = vec![SessionEvent::ActiveModeChanged { mode }];
    if mode == SurfaceMode::Play && session.live.active.is_some() {
        let mut live = session.live.clone();
        live.deactivate();
        events.push(SessionEvent::LiveStateReconciled { live });
    }
    events
}

fn plugin_session_state(plugin: &DesktopPlugin) -> PluginInstanceState {
    PluginInstanceState {
        instance_id: InstanceId::new(plugin.instance_id.clone())
            .expect("desktop plugin instance id is validated during loading"),
        plugin_id: plugin.plugin_id.clone(),
        plugin_name: plugin.name.clone(),
        plugin_short_name: plugin.runtime.manifest().little_short_name(),
        ui_layouts: vec!["little@1".into()],
        config_available: plugin.config_available,
        banks: plugin.banks.clone(),
        sounds: plugin.sound_summaries.clone(),
        selected_sound_id: plugin.selected_sound_id.clone(),
    }
}

fn little_host_presets(presets: Vec<HostPresetSummary>) -> Vec<PlayPreset> {
    presets
        .into_iter()
        .map(|preset| {
            PlayPreset::new(
                preset.id,
                preset.name,
                format!("v{}", preset.plugin_version),
            )
        })
        .collect()
}

fn desktop_catalog_views(
    catalog: &PresetCatalog,
) -> (Vec<BankSummary>, Vec<SoundSummary>, Vec<PlaySound>) {
    let bank_names = catalog
        .banks
        .iter()
        .map(|bank| (bank.id.as_str(), bank.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let banks = catalog
        .banks
        .iter()
        .map(|bank| BankSummary {
            id: bank.id.clone(),
            name: bank.name.clone(),
            order: bank.order,
        })
        .collect();
    let sound_summaries = catalog
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
        .collect();
    let sounds = catalog
        .presets
        .iter()
        .map(|preset| {
            let bank = preset
                .bank
                .as_deref()
                .and_then(|id| bank_names.get(id).copied())
                .unwrap_or("Factory");
            let detail = preset
                .category
                .as_deref()
                .or(preset.description.as_deref())
                .unwrap_or("Preset");
            PlaySound::new(&preset.id, &preset.name, bank, detail).editable(preset.editable)
        })
        .collect();
    (banks, sound_summaries, sounds)
}

fn desktop_program_draft_state(
    draft_id: u64,
    instance_id: InstanceId,
    original_program_id: Option<String>,
    prepared: &PreparedProgram,
    editor: rackforge_plugin_api::ProgramEditorView,
    dirty: bool,
) -> Result<ProgramDraftState, String> {
    prepared
        .validate()
        .map_err(|error| format!("Plugin returned an invalid prepared program: {error}"))?;
    editor
        .validate()
        .map_err(|error| format!("Plugin returned an invalid program editor: {error}"))?;
    let document_json = serde_json::to_string(&prepared.document)
        .map_err(|error| format!("Could not serialize the prepared program: {error}"))?;
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

#[cfg(windows)]
fn desktop_audio_specs(
    plugins: &[DesktopPlugin],
    live_state_dir: &Path,
) -> Vec<desktop_audio::VoiceSpec> {
    plugins
        .iter()
        .map(|plugin| desktop_audio::VoiceSpec {
            instance_id: plugin.instance_id.clone(),
            plugin: plugin.runtime,
            preset_id: plugin.selected_sound_id.clone(),
            resources: plugin.resources.clone(),
            initial_state: read_live_state(live_state_dir, &plugin.plugin_id),
        })
        .collect()
}

/// Where a plugin's live panel state sleeps between sessions.
fn live_state_path(dir: &Path, plugin_id: &str) -> PathBuf {
    dir.join(format!("{plugin_id}.rfstate"))
}

fn read_live_state(dir: &Path, plugin_id: &str) -> Option<Vec<u8>> {
    fs::read(live_state_path(dir, plugin_id)).ok()
}

#[cfg(windows)]
fn start_desktop_audio(
    plugins: &[DesktopPlugin],
    preferences: &desktop_audio::AudioPreferences,
    active_instance_id: Option<&str>,
    live_state_dir: &Path,
    external_controller: bool,
) -> Result<desktop_audio::DesktopAudio> {
    desktop_audio::DesktopAudio::start(
        desktop_audio_specs(plugins, live_state_dir),
        preferences,
        active_instance_id,
        external_controller,
        live_state_dir
            .parent()
            .and_then(Path::parent)
            .unwrap_or(live_state_dir),
    )
}

/// True when an installed, enabled controller package should own the
/// hardware surface: the built-in KeyLab handling stands down for it.
/// The KeyLab ships WITH RackForge: its manifest is embedded (the driver
/// crate carries it) and its driver binary travels beside the desktop exe.
/// First boot installs it into the controller store like any package --
/// the same contract Android's bundled install honors -- so a fresh
/// machine has its controller without anyone running a command.
fn ensure_bundled_controller(rackforge_root: &Path) {
    let store = rackforge_controller_package::PackageStore::new(rackforge_root.join("controllers"));
    let driver = BUNDLED_CONTROLLER_DRIVER
        .map(|bytes| bytes.to_vec())
        .or_else(|| {
            std::env::current_exe().ok().and_then(|exe| {
                fs::read(
                    exe.parent()?
                        .join("rackforge-arturia-keylab-essential-mk3-driver.exe"),
                )
                .ok()
            })
        });
    let Some(driver) = driver else {
        eprintln!("DESKTOP_BUNDLED_CONTROLLER_SKIPPED reason=driver-binary-unavailable");
        return;
    };
    let manifest_text = match rackforge_controller_package::stamp_bundled_manifest(
        keylab_essential_mk3::controller::PACKAGE_MANIFEST,
        &[("windows-x86-64", driver.as_slice())],
    ) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("DESKTOP_BUNDLED_CONTROLLER_SKIPPED reason=manifest-invalid error={error}");
            return;
        }
    };
    let Ok(manifest) =
        toml::from_str::<rackforge_controller_package::ControllerPackageManifest>(&manifest_text)
    else {
        eprintln!("DESKTOP_BUNDLED_CONTROLLER_SKIPPED reason=stamped-manifest-invalid");
        return;
    };
    let already = store
        .list()
        .map(|installed| {
            installed.iter().any(|controller| {
                controller.record.id == manifest.id && controller.record.version == manifest.version
            })
        })
        .unwrap_or(false);
    if already {
        return;
    }
    let staging = rackforge_root
        .join("controllers")
        .join("staging")
        .join(&manifest.id);
    let staged = (|| -> std::io::Result<()> {
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        let bin = staging.join("bin").join("windows-x86-64");
        fs::create_dir_all(&bin)?;
        fs::write(
            staging.join(rackforge_controller_package::CONTROLLER_MANIFEST_FILE),
            &manifest_text,
        )?;
        fs::write(
            bin.join("rackforge-arturia-keylab-essential-mk3-driver.exe"),
            &driver,
        )?;
        Ok(())
    })();
    if let Err(error) = staged {
        eprintln!("DESKTOP_BUNDLED_CONTROLLER_SKIPPED reason=staging error={error}");
        return;
    }
    match store.install_directory(
        &staging,
        rackforge_controller_package::PackageTrust::Official,
    ) {
        Ok(installed) => println!(
            "DESKTOP_BUNDLED_CONTROLLER_INSTALLED id={} version={}",
            installed.record.id, installed.record.version
        ),
        Err(error) => eprintln!("DESKTOP_BUNDLED_CONTROLLER_SKIPPED reason=install error={error}"),
    }
    let _ = fs::remove_dir_all(rackforge_root.join("controllers").join("staging"));
}

fn external_controller_enabled(rackforge_root: &Path) -> bool {
    let root = rackforge_root.join("controllers");
    if !root.join("packages").exists() {
        return false;
    }
    rackforge_controller_package::PackageStore::new(root)
        .list()
        .map(|installed| installed.iter().any(|controller| controller.record.enabled))
        .unwrap_or(false)
}

#[cfg(windows)]
fn declarative_semantic_profiles(
    rackforge_root: &Path,
    approved_midi_inputs: &[String],
) -> Result<BTreeMap<String, RegisteredSemanticProfile>> {
    let store = rackforge_controller_package::PackageStore::new(rackforge_root.join("controllers"));
    let mut profiles = BTreeMap::new();
    for endpoint_name in approved_midi_inputs {
        let Some(binding) = store
            .resolve_declarative_input(endpoint_name)
            .with_context(|| format!("resolving declarative controller for {endpoint_name:?}"))?
        else {
            continue;
        };
        let descriptor = desktop_audio::midi_source_descriptor(endpoint_name)?;
        if profiles.contains_key(&binding.controller_id) {
            bail!(
                "declarative controller {} matches more than one enabled MIDI input; make its endpoint matcher more specific",
                binding.controller_id
            );
        }
        profiles.insert(
            binding.controller_id,
            RegisteredSemanticProfile {
                profile: binding.semantic_profile,
                runtime_source_id: Some(descriptor.id.as_str().into()),
                runtime_source_name: Some(descriptor.name),
                host_controls: binding.host_controls,
                host_actions: binding.host_actions,
            },
        );
    }
    Ok(profiles)
}

#[cfg(windows)]
fn sync_desktop_audio(
    audio: &desktop_audio::DesktopAudio,
    session: &Arc<RwLock<SessionState>>,
    menu: &Menu,
) -> Result<()> {
    let state = session.read().expect("session lock poisoned");
    audio.set_master_level(state.master_level)?;
    audio.set_master_pan(state.master_pan)?;
    audio.set_running(state.active_mode != SurfaceMode::Idle)?;
    drop(state);
    audio.render_little(menu.render());
    Ok(())
}

fn load_desktop_plugins(options: &Options) -> Result<(Vec<DesktopPlugin>, Vec<String>)> {
    fs::create_dir_all(&options.plugins_root).with_context(|| {
        format!(
            "creating plugin directory {}",
            options.plugins_root.display()
        )
    })?;
    fs::create_dir_all(&options.data_root).with_context(|| {
        format!(
            "creating plugin data directory {}",
            options.data_root.display()
        )
    })?;
    if let Some(store_root) = options.plugin_store_root.as_deref() {
        let _ = cleanup_uninstall_tombstones(store_root);
    }

    let mut package_roots = direct_package_roots(&options.plugins_root)?;
    if let Some(store_root) = options.plugin_store_root.as_deref() {
        package_roots.extend(versioned_package_roots(store_root)?);
    }
    package_roots.sort();

    let mut selected = BTreeMap::<String, (Version, PluginPackage)>::new();
    let mut warnings = Vec::new();
    for root in package_roots {
        let package = match PluginPackage::open(&root) {
            Ok(package) => package,
            Err(error) => {
                warnings.push(format!("{}: {error:#}", root.display()));
                continue;
            }
        };
        let version = match Version::parse(&package.manifest().version) {
            Ok(version) => version,
            Err(error) => {
                warnings.push(format!(
                    "{}: invalid plugin version {:?}: {error}",
                    root.display(),
                    package.manifest().version
                ));
                continue;
            }
        };
        let replace = selected
            .get(&package.manifest().id)
            .is_none_or(|(current, _)| version >= *current);
        if replace {
            selected.insert(package.manifest().id.clone(), (version, package));
        }
    }

    let mut plugins = Vec::new();
    for (_, package) in selected.into_values() {
        match load_desktop_plugin(&package, &options.data_root) {
            Ok(plugin) => plugins.push(plugin),
            Err(error) => warnings.push(format!("{}: {error:#}", package.root().display())),
        }
    }
    Ok((plugins, warnings))
}

fn direct_package_roots(root: &Path) -> Result<Vec<PathBuf>> {
    Ok(fs::read_dir(root)
        .with_context(|| format!("reading plugin directory {}", root.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        })
        .filter(|root| root.join("rackforge-plugin.toml").is_file())
        .collect())
}

fn versioned_package_roots(store_root: &Path) -> Result<Vec<PathBuf>> {
    versioned_package_roots_with_activation(store_root, true)
}

pub(crate) fn all_versioned_package_roots(store_root: &Path) -> Result<Vec<PathBuf>> {
    versioned_package_roots_with_activation(store_root, false)
}

fn versioned_package_roots_with_activation(
    store_root: &Path,
    enabled_only: bool,
) -> Result<Vec<PathBuf>> {
    let packages_root = store_root.join("packages");
    fs::create_dir_all(&packages_root)
        .with_context(|| format!("creating plugin store {}", packages_root.display()))?;
    let mut roots = Vec::new();
    for plugin in fs::read_dir(&packages_root)
        .with_context(|| format!("reading plugin store {}", packages_root.display()))?
        .flatten()
    {
        if !plugin.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let plugin_id = plugin.file_name().to_string_lossy().into_owned();
        if enabled_only
            && !plugin_is_enabled(store_root, &plugin_id)
                .with_context(|| format!("reading activation state for {plugin_id}"))?
        {
            continue;
        }
        roots.extend(
            fs::read_dir(plugin.path())
                .into_iter()
                .flatten()
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path())
                .filter(|root| root.join("rackforge-plugin.toml").is_file()),
        );
    }
    Ok(roots)
}

fn load_desktop_plugin(package: &PluginPackage, data_root: &Path) -> Result<DesktopPlugin> {
    if !matches!(
        package.manifest().kind,
        PluginKind::Instrument | PluginKind::Effect
    ) {
        bail!(
            "Desktop PLAY accepts instrument and effect plugins, found {:?}",
            package.manifest().kind
        );
    }
    let instance_id = format!("desktop.{}", package.manifest().id);
    InstanceId::new(instance_id.clone()).map_err(anyhow::Error::msg)?;
    let version = Version::parse(&package.manifest().version)
        .with_context(|| format!("invalid plugin version {:?}", package.manifest().version))?;

    // SAFETY: Desktop only scans the user's installed RackForge plugin root.
    // Native packages are trusted by the same boundary as the appliance host.
    let loaded = unsafe { LoadedPlugin::load(package, None, &BTreeMap::new(), Some(data_root)) }?;
    // Native plugin libraries are process-lifetime objects. Leaking this box is
    // intentional: unloading while an instance may hold ABI pointers is unsafe.
    let loaded: &'static LoadedPlugin = Box::leak(Box::new(loaded));
    let mut instance = loaded.create_instance()?;
    let catalog = instance.preset_catalog()?;
    let (banks, sound_summaries, sounds) = desktop_catalog_views(&catalog);
    let selected_sound_id = sounds.first().map(|sound| sound.id.clone());
    if let Some(id) = selected_sound_id.as_deref() {
        instance
            .load_preset(id)
            .with_context(|| format!("loading initial preset {id:?}"))?;
    }

    Ok(DesktopPlugin {
        instance_id,
        plugin_id: package.manifest().id.clone(),
        name: package.manifest().name.clone(),
        version,
        runtime: loaded,
        config_available: package.manifest().config_mode,
        banks,
        sound_summaries,
        sounds,
        selected_sound_id,
        instance,
        resources: BTreeMap::new(),
        resource_data_paths: package
            .manifest()
            .resources
            .iter()
            .filter_map(|resource| {
                resource
                    .data_path
                    .as_deref()
                    .map(|path| (resource.id.clone(), PathBuf::from(path)))
            })
            .collect(),
    })
}

fn create_desktop(options: Options) -> Result<DesktopApp> {
    let startup = rackforge_core::startup::StartupTimeline::new("desktop");
    if !options.install_archives.is_empty() {
        let store_root = options
            .plugin_store_root
            .as_deref()
            .context("--install-plugin requires a RackForge Root with a plugin store")?;
        for archive in &options.install_archives {
            let bytes = read_plugin_archive_limited(archive).map_err(anyhow::Error::msg)?;
            install_local_archive(store_root, &bytes)
                .with_context(|| format!("installing plugin archive {}", archive.display()))?;
        }
    }

    install_bundled_default_plugin(&options)?;
    install_bundled_official_plugins(&options)?;

    let session = Arc::new(RwLock::new(SessionState::new(
        SessionId::new(DEFAULT_LIVE_SESSION_ID).expect("valid live session id"),
    )));
    let (web_control_sender, web_control) = web::control_channel();
    let performance_revision_shared = Arc::new(RwLock::new(String::new()));
    let web_servers = web::start(
        Arc::clone(&session),
        Arc::clone(&performance_revision_shared),
        &options,
        web::WebServerPreferences {
            enabled: false,
            ..options.web_preferences.clone()
        },
        web_control_sender,
    )?;
    let controller_root = options.rackforge_root.join("controllers");
    let controller_address = web_servers.control_bridge_addr().to_string();
    let mut app = DesktopApp::new(
        Arc::clone(&session),
        performance_revision_shared,
        &options,
        web_servers,
        web_control,
    )?;
    #[cfg(windows)]
    if app.audio.is_some() {
        startup.advance(rackforge_core::startup::StartupPhase::AudioReady)?;
    }
    ensure_bundled_controller(&options.rackforge_root);

    // The controller supervisor: every enabled .rfcontroller package runs
    // only after DesktopAudio has completed its device/plugin generation.
    // Its driver points back at this host through the TCP control bridge. The
    // loop exits on its own when the store holds nothing runnable.
    let controller_shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (controller_ready_sender, controller_ready_receiver) = mpsc::sync_channel(1);
    let controller_supervisor = {
        let root = controller_root;
        let address = controller_address;
        let shutdown = Arc::clone(&controller_shutdown);
        std::thread::Builder::new()
            .name("rackforge-controller-supervisor".into())
            .spawn(move || {
                if !root.join("packages").exists() {
                    let _ = controller_ready_sender.send(());
                    return;
                }
                let options = rackforge_controller_package::supervise::SuperviseOptions {
                    allow_community: false,
                    extra_env: vec![
                        ("RACKFORGE_CONTROL_ADDR".into(), address),
                        // WinMM gives the packaged controller exclusive
                        // ownership of its main input. Return ordinary MIDI
                        // to Desktop while the driver retains surface events.
                        ("RACKFORGE_FORWARD_MIDI".into(), "1".into()),
                    ],
                    shutdown,
                    on_ready: Some(Arc::new(move || {
                        let _ = controller_ready_sender.try_send(());
                    })),
                };
                match rackforge_controller_package::supervise::supervise(&root, &options) {
                    Ok(0) => println!("DESKTOP_CONTROLLERS_NONE"),
                    Ok(count) => println!("DESKTOP_CONTROLLERS_STOPPED count={count}"),
                    Err(error) => eprintln!("DESKTOP_CONTROLLERS_ERROR {error}"),
                }
            })
            .context("starting the controller supervisor")?
    };
    #[cfg(windows)]
    if app.audio.is_some() {
        if let Err(error) = controller_ready_receiver.recv_timeout(Duration::from_secs(2)) {
            eprintln!("DESKTOP_CONTROLLER_STARTUP_DEGRADED error={error}");
        }
        startup.advance(rackforge_core::startup::StartupPhase::ControlReady)?;
        if let Err(error) = app.web_servers.apply(options.web_preferences.clone()) {
            eprintln!("DESKTOP_BACKGROUND_WEB_FAILED error={error:#}");
            app.web_preferences.enabled = false;
        }
        startup.advance(rackforge_core::startup::StartupPhase::BackgroundReady)?;
    }
    app.controller_shutdown = Some(controller_shutdown);
    app.controller_supervisor = Some(controller_supervisor);
    Ok(app)
}

fn install_bundled_default_plugin(options: &Options) -> Result<()> {
    let Some(bytes) = BUNDLED_DEFAULT_PLUGIN else {
        return Ok(());
    };
    let Some(store_root) = options.plugin_store_root.as_deref() else {
        return Ok(());
    };
    let marker = options
        .rackforge_root
        .join("state")
        .join("bundled-default-initialized");
    if marker.is_file() {
        return Ok(());
    }
    let packages_root = store_root.join("packages");
    if fs::read_dir(&packages_root)
        .ok()
        .is_some_and(|mut entries| entries.next().is_some())
    {
        if let Some(parent) = marker.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&marker, b"1\n")?;
        return Ok(());
    }
    let installed = install_local_archive(store_root, bytes)
        .context("installing the bundled default instrument")?;
    eprintln!(
        "DESKTOP_DEFAULT_PLUGIN id={} version={} path={} existing={}",
        installed.record.plugin_id,
        installed.record.version,
        installed.path.display(),
        installed.already_installed
    );
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&marker, b"1\n")?;
    Ok(())
}

fn install_bundled_official_plugins(options: &Options) -> Result<()> {
    let Some(store_root) = options.plugin_store_root.as_deref() else {
        return Ok(());
    };
    for (archive_name, bytes) in BUNDLED_OFFICIAL_PLUGINS {
        let inspection = inspect_local_archive(store_root, bytes)
            .with_context(|| format!("validating bundled official plugin {archive_name}"))?;
        let known_plugin = store_root
            .join("packages")
            .join(&inspection.plugin_id)
            .is_dir();
        // The official set this build carries is pinned upstream and checked
        // against a SHA-256 the source tree holds, so it is the authority on
        // what its versions contain. A same-version copy left by an earlier
        // build is corrected rather than kept: keeping it meant a player ran
        // a stale instrument that no release could ever replace.
        let installed = install_local_archive_replacing(store_root, bytes)
            .with_context(|| format!("installing bundled official plugin {archive_name}"))?;
        if !known_plugin {
            set_plugin_enabled(store_root, &inspection.plugin_id, true).with_context(|| {
                format!("enabling bundled official plugin {}", inspection.plugin_id)
            })?;
        }
        eprintln!(
            "DESKTOP_OFFICIAL_PLUGIN id={} version={} path={} existing={} enabled_by_build={}",
            installed.record.plugin_id,
            installed.record.version,
            installed.path.display(),
            installed.already_installed,
            !known_plugin
        );
    }
    Ok(())
}

impl RackForgeApp {
    fn initial_mode(startup: Startup) -> Result<AppMode> {
        let mode = match startup {
            Startup::Ready(options) => AppMode::Desktop(Box::new(create_desktop(options)?)),
            Startup::FirstStart {
                web_preferences,
                default_root,
                executable_directory,
                install_archives,
            } => AppMode::Setup {
                state: Box::new(setup::SetupState::new(default_root, executable_directory)),
                web_preferences,
                install_archives,
            },
        };
        Ok(mode)
    }

    #[cfg(windows)]
    fn new(startup: Startup, creation: &eframe::CreationContext<'_>) -> Result<Self> {
        Ok(Self {
            mode: Self::initial_mode(startup)?,
            shutdown: None,
            webview: desktop_webview::DesktopWebView::new(creation)?,
        })
    }

    #[cfg(not(windows))]
    fn new(startup: Startup, _creation: &eframe::CreationContext<'_>) -> Result<Self> {
        Ok(Self {
            mode: Self::initial_mode(startup)?,
            shutdown: None,
        })
    }
}

impl eframe::App for RackForgeApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.shutdown.is_some() {
            return;
        }
        // The player's last touches survive the window closing.
        if let AppMode::Desktop(app) = &mut self.mode {
            if app.live_state_dirty.is_some() {
                app.flush_live_state();
            }
            if let Some(shutdown) = &app.controller_shutdown {
                shutdown.store(true, std::sync::atomic::Ordering::Release);
            }
            if let Some(supervisor) = app.controller_supervisor.take()
                && supervisor.join().is_err()
            {
                eprintln!("DESKTOP_CONTROLLERS_JOIN_FAILED");
            }
        }
    }

    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        let close_requested = context.input(|input| input.viewport().close_requested());
        if close_requested
            && self.shutdown.is_none()
            && let AppMode::Desktop(app) = &mut self.mode
        {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            #[cfg(windows)]
            {
                let _ = self.webview.hide();
            }
            self.shutdown = Some(DesktopShutdown::begin(app));
        }

        if let Some(shutdown) = self.shutdown.as_mut() {
            if !shutdown.is_complete() && close_requested {
                context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
            if let AppMode::Desktop(app) = &mut self.mode {
                shutdown.poll(app);
            }
            shutdown.render(context);
            context.request_repaint_after(Duration::from_millis(16));
            if shutdown.is_complete() {
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }

        let transition = match &mut self.mode {
            AppMode::Setup {
                state,
                web_preferences,
                install_archives,
            } => {
                #[cfg(windows)]
                {
                    let _ = self.webview.hide();
                }
                state.update(context).map(|result| {
                    result.and_then(|layout| {
                        let options = options_from_layout(
                            web_preferences.clone(),
                            layout,
                            std::mem::take(install_archives),
                        );
                        create_desktop(options)
                    })
                })
            }
            AppMode::Desktop(app) => {
                #[cfg(windows)]
                app.poll_audio_error();
                #[cfg(windows)]
                app.poll_controller();
                app.poll_web_control();
                context.request_repaint_after(Duration::from_millis(16));
                let reload_web = app.poll_plugin_install(context);
                context.send_viewport_cmd(egui::ViewportCommand::Title(app.window_title().into()));
                #[cfg(windows)]
                {
                    let web_rect = egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(context, |ui| ui.available_rect_before_wrap())
                        .inner;
                    if reload_web && let Err(error) = self.webview.reload() {
                        app.status = format!("Could not reload Web UI: {error:#}");
                    }
                    if let Err(error) = self.webview.show(&app.web_url, web_rect) {
                        app.status = format!("Could not show embedded Web UI: {error:#}");
                    }
                }
                #[cfg(not(windows))]
                egui::CentralPanel::default().show(context, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label(format!("Open RackForge Web at {}", app.web_url));
                    });
                });
                None
            }
            AppMode::Error(message) => {
                #[cfg(windows)]
                {
                    let _ = self.webview.hide();
                }
                egui::CentralPanel::default().show(context, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.heading(
                            RichText::new("RACKFORGE COULD NOT START")
                                .color(Color32::from_rgb(235, 105, 105)),
                        );
                        ui.add_space(16.0);
                        ui.label(message.as_str());
                        ui.add_space(16.0);
                        if ui.button("Close").clicked() {
                            context.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
                None
            }
        };

        if let Some(result) = transition {
            self.mode = match result {
                Ok(app) => AppMode::Desktop(Box::new(app)),
                Err(error) => AppMode::Error(format!("{error:#}")),
            };
            context.request_repaint();
        }
    }
}

fn main() {
    if let Err(error) = run() {
        show_startup_error(&format!("{error:#}"));
    }
}

fn run() -> Result<()> {
    #[cfg(windows)]
    let _single_instance = match single_instance::acquire()? {
        single_instance::AcquireOutcome::Acquired(guard) => guard,
        single_instance::AcquireOutcome::AlreadyRunning => {
            show_already_running();
            return Ok(());
        }
    };
    let startup = parse_startup()?;
    // Installing from the command line is a job, not a session. This used to
    // fall through into the full app boot -- audio stream, MIDI capture,
    // window, the lot -- so every scripted `--install-plugin` left a complete
    // instrument running. With a real session also open, whichever process
    // held the audio kept playing a plugin nobody's panel controlled: the
    // user heard a second piano that no fader could touch.
    if let Startup::Ready(options) = &startup
        && !options.install_archives.is_empty()
    {
        let store_root = options
            .plugin_store_root
            .as_deref()
            .context("--install-plugin requires a RackForge Root with a plugin store")?;
        for archive in &options.install_archives {
            let bytes = read_plugin_archive_limited(archive).map_err(anyhow::Error::msg)?;
            install_local_archive(store_root, &bytes)
                .with_context(|| format!("installing plugin archive {}", archive.display()))?;
            println!("RFPLUGIN_INSTALLED {}", archive.display());
        }
        return Ok(());
    }
    // Controllers are packages too, and their install is the same job-not-
    // session shape as the plugins'. Local installs carry official trust:
    // this path takes a directory the user built or unpacked themselves.
    if let Startup::Ready(options) = &startup
        && !options.install_controllers.is_empty()
    {
        let store = rackforge_controller_package::PackageStore::new(
            options.rackforge_root.join("controllers"),
        );
        for package in &options.install_controllers {
            let installed = store
                .install_directory(
                    package,
                    rackforge_controller_package::PackageTrust::Official,
                )
                .map_err(|error| {
                    anyhow::anyhow!("installing controller {}: {error}", package.display())
                })?;
            println!(
                "RFCONTROLLER_INSTALLED id={} version={}",
                installed.record.id, installed.record.version
            );
        }
        return Ok(());
    }
    let app_icon = eframe::icon_data::from_png_bytes(include_bytes!(
        "../../../assets/brand/rackforge-mark-256.png"
    ))
    .context("loading the embedded RackForge app icon")?;
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RackForge Desktop")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_icon(Arc::new(app_icon)),
        ..Default::default()
    };
    eframe::run_native(
        "RackForge Desktop",
        native,
        Box::new(move |creation| Ok(Box::new(RackForgeApp::new(startup, creation)?))),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))
}

#[cfg(windows)]
fn show_already_running() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};

    let title = "RackForge is already running"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let message =
        "Another RackForge instance is already running. The audio engine was not started."
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
    // SAFETY: both pointers reference NUL-terminated UTF-16 buffers for the
    // duration of the synchronous MessageBoxW call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

#[cfg(windows)]
fn show_startup_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

    let title = "RackForge could not start"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let message = message
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: both pointers reference NUL-terminated UTF-16 buffers for the
    // duration of the synchronous MessageBoxW call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn show_startup_error(message: &str) {
    eprintln!("RackForge could not start: {message}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_performance_api::{LiveLocation, RackId};
    use rackforge_plugin_api::{
        BankDescriptor, PRESET_CATALOG_SCHEMA_VERSION, PROGRAM_EDIT_SCHEMA_VERSION,
        PROGRAM_SCHEMA_VERSION, PresetDescriptor, ProgramEditorField, ProgramEditorFieldKind,
        ProgramEditorPage, ProgramEditorView,
    };

    #[test]
    fn chooses_safe_activation_for_plugin_versions() {
        let current = Version::parse("2.0.0").unwrap();
        assert_eq!(
            plugin_install_activation(None, &Version::parse("1.0.0").unwrap()),
            PluginInstallActivation::Reload
        );
        assert_eq!(
            plugin_install_activation(Some(&current), &Version::parse("2.1.0").unwrap()),
            PluginInstallActivation::Restart
        );
        assert_eq!(
            plugin_install_activation(Some(&current), &Version::parse("1.9.0").unwrap()),
            PluginInstallActivation::KeepCurrent {
                active_version: "2.0.0".into()
            }
        );
    }

    #[test]
    fn desktop_session_keeps_dynamic_bank_ids_and_labels_distinct() {
        let catalog = PresetCatalog {
            schema_version: PRESET_CATALOG_SCHEMA_VERSION,
            banks: vec![
                BankDescriptor {
                    id: "factory-programs".into(),
                    name: "M1 Factory Programs".into(),
                    order: 0,
                },
                BankDescriptor {
                    id: "plus1-card-combinations".into(),
                    name: "M1 Plus+1 Card Combinations".into(),
                    order: 3,
                },
            ],
            presets: vec![PresetDescriptor {
                id: "plus1.combination.000".into(),
                name: "C00 The Cutter".into(),
                description: None,
                bank: Some("plus1-card-combinations".into()),
                category: None,
                order: 0,
                tags: vec!["plus1".into()],
                editable: false,
            }],
        };

        let (banks, summaries, menu_sounds) = desktop_catalog_views(&catalog);

        assert_eq!(banks.len(), 2);
        assert_eq!(banks[1].id, "plus1-card-combinations");
        assert_eq!(banks[1].name, "M1 Plus+1 Card Combinations");
        assert_eq!(
            summaries[0].bank.as_deref(),
            Some("plus1-card-combinations")
        );
        assert_eq!(menu_sounds[0].bank, "M1 Plus+1 Card Combinations");
    }

    #[test]
    fn desktop_program_draft_preserves_plugin_owned_document_and_editor() {
        let prepared = PreparedProgram {
            schema_version: PROGRAM_EDIT_SCHEMA_VERSION,
            storage_path: "org.rackforge.demo/programs/stage.rfprogram".into(),
            preview_sound_id: "custom.stage".into(),
            document: ProgramDocument {
                schema_version: PROGRAM_SCHEMA_VERSION,
                id: "stage".into(),
                name: "Stage Piano".into(),
                plugin_id: "org.rackforge.demo".into(),
                plugin_version: "1.0.0".into(),
                plugin_state_version: 1,
                payload_version: 1,
                category: None,
                tags: Vec::new(),
                payload: serde_json::json!({ "bright": true }),
            },
            artifacts: Vec::new(),
        };
        let editor = ProgramEditorView {
            schema_version: PROGRAM_EDITOR_SCHEMA_VERSION,
            title: "Program Editor".into(),
            pages: vec![ProgramEditorPage {
                id: "tone".into(),
                label: "Tone".into(),
                detail: "Program tone".into(),
                enabled: true,
                pages: Vec::new(),
                fields: vec![ProgramEditorField {
                    id: "bright".into(),
                    label: "Bright".into(),
                    detail: "Bright tone".into(),
                    value: ProgramEditorValue::Boolean(true),
                    kind: ProgramEditorFieldKind::Toggle,
                    live_preview: true,
                }],
            }],
        };

        let draft = desktop_program_draft_state(
            7,
            InstanceId::new("desktop.org.rackforge.demo").unwrap(),
            Some("factory.stage".into()),
            &prepared,
            editor.clone(),
            true,
        )
        .unwrap();

        assert_eq!(draft.name, "Stage Piano");
        assert_eq!(draft.preview_sound_id, "custom.stage");
        assert_eq!(draft.editor, editor);
        assert!(draft.dirty);
        assert_eq!(
            serde_json::from_str::<ProgramDocument>(&draft.document_json).unwrap(),
            prepared.document
        );
    }

    #[test]
    fn desktop_play_transition_is_a_recorded_session_event() {
        let session = SessionState::new(SessionId::new("test.desktop-mode").unwrap());
        assert_eq!(
            desktop_active_mode_events(&session, SurfaceMode::Play),
            vec![SessionEvent::ActiveModeChanged {
                mode: SurfaceMode::Play
            }]
        );
    }

    #[test]
    fn desktop_accepts_little_return_for_the_active_compatible_plugin() {
        let instance_id = InstanceId::new("desktop.org.rackforge.rf-106").unwrap();
        let mut session = SessionState::new(SessionId::new("test.desktop-little").unwrap());
        session.active_mode = SurfaceMode::Play;
        session.active_instance_id = Some(instance_id.clone());
        session.instances.push(PluginInstanceState {
            instance_id: instance_id.clone(),
            plugin_id: "org.rackforge.rf-106".into(),
            plugin_name: "RF-106".into(),
            plugin_short_name: "RF-106".into(),
            ui_layouts: vec!["little@1".into()],
            config_available: false,
            banks: Vec::new(),
            sounds: vec![SoundSummary {
                id: "factory.rf106.002".into(),
                name: "A13 Trumpet".into(),
                bank: None,
                detail: None,
                category: None,
                tags: Vec::new(),
                editable: false,
            }],
            selected_sound_id: Some("factory.rf106.002".into()),
        });
        let request = SurfaceActivationRequest::return_to(
            "little@1",
            SurfaceMode::Play,
            Some("factory.rf106.002".into()),
        );

        assert!(validate_desktop_surface_activation(&session, &instance_id, &request).is_ok());

        let wrong_layout = SurfaceActivationRequest::return_to(
            "unknown@1",
            SurfaceMode::Play,
            Some("factory.rf106.002".into()),
        );
        assert!(
            validate_desktop_surface_activation(&session, &instance_id, &wrong_layout)
                .unwrap_err()
                .contains("does not expose layout")
        );
    }

    #[test]
    fn desktop_emergency_home_always_publishes_idle_mode() {
        let mut session = SessionState::new(SessionId::new("test.desktop-emergency").unwrap());
        session.active_mode = SurfaceMode::Play;

        assert_eq!(
            desktop_emergency_stop_events(&session),
            vec![SessionEvent::ActiveModeChanged {
                mode: SurfaceMode::Idle
            }]
        );
    }

    #[test]
    fn desktop_play_transition_deactivates_the_live_target() {
        let mut session = SessionState::new(SessionId::new("test.desktop-live-mode").unwrap());
        let rack_id = RackId::new("rack.stage").unwrap();
        session.live.activate(
            LiveLocation::Rack {
                rack_id: rack_id.clone(),
            },
            rack_id,
        );

        let events = desktop_active_mode_events(&session, SurfaceMode::Play);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0],
            SessionEvent::ActiveModeChanged {
                mode: SurfaceMode::Play
            }
        ));
        let SessionEvent::LiveStateReconciled { live } = &events[1] else {
            panic!("PLAY must reconcile LIVE navigation");
        };
        assert!(live.active.is_none());
        assert!(live.active_rack_id.is_none());
    }

    #[test]
    fn desktop_live_transition_preserves_the_selected_live_target() {
        let mut session = SessionState::new(SessionId::new("test.desktop-live-mode").unwrap());
        let rack_id = RackId::new("rack.stage").unwrap();
        session.live.activate(
            LiveLocation::Rack {
                rack_id: rack_id.clone(),
            },
            rack_id,
        );

        assert_eq!(
            desktop_active_mode_events(&session, SurfaceMode::Live),
            vec![SessionEvent::ActiveModeChanged {
                mode: SurfaceMode::Live
            }]
        );
    }

    #[cfg(windows)]
    #[test]
    fn midi_source_api_exposes_only_the_settings_allowlist() {
        let preferences = desktop_audio::AudioPreferences {
            schema_version: 1,
            driver: "WASAPI".into(),
            output_device: "Speakers".into(),
            sample_rate_hz: 48_000,
            buffer_frames: None,
            output_gain_db: 0,
            input_device: None,
            input_channels: Vec::new(),
            input_gain_db: 0,
            midi_inputs: vec!["Enabled Keyboard".into(), "Disconnected Keyboard".into()],
            velocity_curve: Default::default(),
            velocity_curves: Default::default(),
        };
        let present = BTreeSet::from(["Enabled Keyboard".into(), "Hidden Keyboard".into()]);
        let sources = approved_midi_source_statuses(Some(&preferences), &present);

        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].source.name, "Enabled Keyboard");
        assert!(sources[0].connected);
        assert_eq!(sources[1].source.name, "Disconnected Keyboard");
        assert!(!sources[1].connected);
        assert!(
            sources
                .iter()
                .all(|source| source.source.name != "Hidden Keyboard")
        );
    }

    #[cfg(windows)]
    #[test]
    fn forwarded_controller_must_resolve_to_an_approved_physical_input() {
        let preferences = desktop_audio::AudioPreferences {
            schema_version: 1,
            driver: "WASAPI".into(),
            output_device: "Speakers".into(),
            sample_rate_hz: 48_000,
            buffer_frames: None,
            output_gain_db: 0,
            input_device: None,
            input_channels: Vec::new(),
            input_gain_db: 0,
            midi_inputs: vec!["KL Essential 61 mk3 MIDI".into()],
            velocity_curve: Default::default(),
            velocity_curves: Default::default(),
        };
        let physical =
            approved_midi_source(Some(&preferences), "KL Essential 61 mk3 MIDI").unwrap();
        assert_eq!(physical.name, "KL Essential 61 mk3 MIDI");
        assert!(approved_midi_source(Some(&preferences), "Hidden MIDI").is_err());
    }
}
