#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod desktop_audio;
#[cfg(windows)]
mod desktop_webview;
mod paths;
mod setup;
#[cfg(windows)]
mod single_instance;
mod startup;
mod web;

use anyhow::{Context, Result, bail};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Key, Layout, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, Vec2,
};
use rackforge_control_api::{
    ClientId, ControlErrorCode, ControlRequest, ControlResponse, VirtualMidiMessage,
};
use rackforge_core::performance::PerformanceRepository;
use rackforge_core::session_checkpoint::SessionCheckpointStore;
use rackforge_core::{
    IsolatedPluginStateEditor, LoadedPlugin, PluginInstance, PluginPackage, PluginStateStore,
    PluginStorage, validate_state_reference,
};
use rackforge_performance_api::{PERFORMANCE_SNAPSHOT_SCHEMA_VERSION, PerformanceSnapshot};
use rackforge_plugin_api::{
    PROGRAM_EDITOR_SCHEMA_VERSION, PluginKind, PreparedProgram, PresetCatalog, ProgramDocument,
    ProgramEditRequest, ProgramEditorValue, ProgramFieldEditRequest,
};
use rackforge_repository::{
    InstalledPackage, LocalPackageInspection, MAX_PACKAGE_BYTES, PluginUserDataRemovalOptions,
    cleanup_uninstall_tombstones, inspect_local_archive, install_local_archive,
    remove_plugin_user_data, uninstall_plugin,
};
use rackforge_session_api::{
    AuditionEndReason, BankSummary, CommandRef, DEFAULT_LIVE_SESSION_ID, EventEnvelope, InstanceId,
    MasterLevel, MasterPan, PluginInstanceState, ProgramDraftState, Revision,
    SESSION_SCHEMA_VERSION, SessionCommand, SessionEvent, SessionId, SessionState, SoundSummary,
};
use rackforge_surface_api::SurfaceMode;
use rackforge_surface_runtime::{
    ActiveMode, Header, Input, Menu, MenuCommand, PlayPlugin, PlaySound, ProgramExitDecision,
    ProgramExitDestination,
};
use semver::Version;
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

#[derive(Clone, Default)]
struct VirtualMidiClientState {
    notes: BTreeSet<(u8, u8)>,
    channels: BTreeSet<u8>,
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
    state_store: PluginStateStore,
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
    /// Raised on exit so the controller supervisor reaps its drivers.
    controller_shutdown: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl DesktopApp {
    fn new(
        session: Arc<RwLock<SessionState>>,
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
            .or_else(|| plugins.first())
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
        let performance_repository = PerformanceRepository::load_or_empty(Some(&options.data_root))
            .context("loading Desktop performance library")?;
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
            menu.sync_active_mode(active_mode_from_surface(state.active_mode));
        }
        #[cfg(windows)]
        if let Some(audio) = &audio {
            sync_desktop_audio(audio, &session, &menu)?;
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
        // Point surface notes at the engine that just started.
        web_servers.set_injected_midi(audio.as_ref().map(|audio| audio.injected_midi_sender()));
        #[cfg(windows)]
        let audio_recovery_at =
            if audio.is_none() && audio_preferences.is_some() && !plugins.is_empty() {
                Some(Instant::now() + Duration::from_secs(1))
            } else {
                None
            };
        Ok(Self {
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
            state_store,
            live_state_dirty: None,
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
            controller_shutdown: None,
        })
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
            .or_else(|| plugins.first())
            .map(|plugin| plugin.instance_id.as_str());
        #[cfg(windows)]
        {
            self.audio = None;
            if let Some(preferences) = self.audio_preferences.as_ref() {
                match start_desktop_audio(
                    &plugins,
                    preferences,
                    active_instance_id,
                    &self.live_state_dir(),
                    external_controller_enabled(&self.options.rackforge_root),
                ) {
                    Ok(audio) => {
                        self.audio = Some(audio);
                        self.audio_recovery_at = None;
                        self.audio_recovery_attempts = 0;
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
        if let Some(audio) = &self.audio {
            sync_desktop_audio(audio, &self.session, &self.menu)?;
        }
        self.persist_session_checkpoint();
        Ok(warnings)
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
                        self.web_servers.set_injected_midi(None);
                        self.audio = None;
                        self.audio_watchdog = None;
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
            self.web_servers.set_injected_midi(None);
            self.audio = None;
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
                    self.status =
                        format!("Audio reconnected, but controller sync failed: {error:#}");
                }
                let summary = audio.summary().to_owned();
                self.web_servers
                    .set_injected_midi(Some(audio.injected_midi_sender()));
                self.audio = Some(audio);
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
        Ok(serde_json::json!({
            "status": "ok",
            "host": "desktop",
            "inventory": inventory,
            "preferences": preferences,
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
        let previous = self.audio_preferences.clone();
        let active = self
            .session
            .read()
            .expect("session lock poisoned")
            .active_instance_id
            .as_ref()
            .map(|id| id.as_str().to_owned());
        // The stream comes down BEFORE the scan: enumerating instantiates
        // every ASIO driver, and instantiating the live one kills its
        // stream anyway (measured). Validation failures restore the
        // previous stream on the way out.
        self.audio = None;
        self.audio_watchdog = None;
        let inventory = desktop_audio::AudioInventory::scan()?;
        self.audio_inventory_cache = Some((Instant::now(), inventory.clone()));
        if let Err(error) = inventory.validate(&preferences) {
            return match self.restore_audio(previous.as_ref(), active.as_deref()) {
                Ok(()) => Err(anyhow::anyhow!("{error:#}. The previous settings were kept")),
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
        sync_desktop_audio(&candidate, &self.session, &self.menu)?;
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
        self.audio = Some(candidate);
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
        self.audio = preferences
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
            .transpose()?;
        if let Some(audio) = &self.audio {
            sync_desktop_audio(audio, &self.session, &self.menu)?;
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
            PluginInstallActivation::Reload => match self.activate_plugin_id(&inspection.plugin_id)
            {
                Ok(()) => {
                    self.status = format!("{label} installed and active");
                    Self::show_install_info("Plugin installed", &self.status);
                    true
                }
                Err(error) => {
                    self.status = format!(
                        "{label} was installed from {}, but could not be activated: {error:#}. Restart RackForge to try again.",
                        archive.display()
                    );
                    Self::show_install_error(&self.status);
                    false
                }
            },
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
        }
    }

    #[cfg(windows)]
    fn handle_controller_event(&mut self, event: desktop_audio::DesktopControllerEvent) {
        use desktop_audio::DesktopControllerEvent;
        use keylab_essential_mk3::protocol::InputPhase;

        match event {
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
            DesktopControllerEvent::MasterLevel(value) => {
                let level = MasterLevel::from_midi(value);
                if let Err(error) = self.set_master_level(level, None) {
                    self.status = error;
                    return;
                }
                self.show_controller_host_value(
                    keylab_essential_mk3::protocol::host_control_header(
                        rackforge_session_api::HostControlTarget::MasterLevel,
                        value,
                    ),
                );
            }
            DesktopControllerEvent::MasterPan(value) => {
                let pan = MasterPan::from_midi_with_center_snap(value);
                if let Err(error) = self.set_master_pan(pan, None) {
                    self.status = error;
                    return;
                }
                self.show_controller_host_value(
                    keylab_essential_mk3::protocol::host_control_header(
                        rackforge_session_api::HostControlTarget::MasterPan,
                        value,
                    ),
                );
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
                Input::KeyboardParts => {
                    if phase == InputPhase::Press {
                        self.apply_input(Input::KeyboardParts);
                    }
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
                    response,
                } => {
                    let _ = response.send(self.load_plugin_resource(
                        &plugin_id,
                        &resource_id,
                        &path,
                        persist,
                    ));
                }
                web::DesktopControlCall::ActivatePlugin {
                    plugin_id,
                    response,
                } => {
                    let _ = response.send(self.activate_plugin_id(&plugin_id));
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
            let Some(audio) = self.audio.as_ref() else { return };
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
        if !self
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
            .as_deref()
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
            self.audio = None;
        }
        self.plugins.clear();
        let removed = match uninstall_plugin(store_root, plugin_id) {
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
    ) -> Result<(), String> {
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
                previous.push((
                    target_id.clone(),
                    plugin.resources.insert(target_id, selected_path),
                ));
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
            previous.push((
                resource_id.to_owned(),
                plugin
                    .resources
                    .insert(resource_id.to_owned(), selected_path),
            ));
        }

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
                    "{} installed and activated {} recognized resource{} from {}",
                    plugin.name,
                    installed_count,
                    if installed_count == 1 { "" } else { "s" },
                    path.file_name()
                        .map(|name| name.to_string_lossy())
                        .unwrap_or_else(|| path.display().to_string().into())
                );
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

    fn handle_web_control(&mut self, request: ControlRequest) -> ControlResponse {
        let envelope = match request {
            ControlRequest::VirtualMidi { client_id, message } => {
                return self.accept_virtual_midi(client_id, message);
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
            SessionCommand::SetMasterLevel { level } => {
                self.set_master_level(level, Some(command_ref))
            }
            SessionCommand::SetMasterPan { pan } => self.set_master_pan(pan, Some(command_ref)),
            SessionCommand::SelectPlugin { instance_id } => {
                self.select_plugin(&instance_id, Some(command_ref))
            }
            SessionCommand::SelectSound {
                instance_id,
                sound_id,
            } => self.select_sound(&instance_id, &sound_id, Some(command_ref)),
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
            } => {
                // A controller driver reserving its host-control CCs. On this
                // host the driver owns its surface endpoint exclusively (the
                // desktop's MIDI capture yields it), so the reservation is
                // satisfied by construction: nothing else reads those CCs.
                // Validate and acknowledge.
                if controls.iter().any(|binding| binding.midi_cc.validate().is_err())
                    || actions.iter().any(|binding| binding.midi_cc.validate().is_err())
                {
                    Err("invalid reserved host binding registration".into())
                } else {
                    println!(
                        "DESKTOP_HOST_BINDINGS_RESERVED controller={} controls={} actions={}",
                        controller_id,
                        controls.len(),
                        actions.len()
                    );
                    Ok(Vec::new())
                }
            }
            other => Err(format!(
                "Desktop does not support this command yet: {other:?}"
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

    fn accept_virtual_midi(
        &mut self,
        client_id: ClientId,
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
        let result = self
            .audio
            .as_ref()
            .ok_or_else(|| "Desktop audio is unavailable".to_owned())
            .and_then(|audio| {
                audio
                    .inject_midi_messages(vec![message.bytes()])
                    .map_err(|error| error.to_string())
            });
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
        let result = self
            .audio
            .as_ref()
            .ok_or_else(|| "Desktop audio is unavailable".to_owned())
            .and_then(|audio| {
                audio
                    .inject_midi_messages(messages)
                    .map_err(|error| error.to_string())
            });
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
        sound_id: Option<String>,
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
        if let Some(sound_id) = sound_id.as_deref()
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
                        plugin.instance_id == active_id.as_str()
                            && plugin.plugin_id == plugin_id
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

        let plugin = &self.plugins[index];
        self.menu.sync_active_plugin(
            &plugin.instance_id,
            &plugin.plugin_id,
            &plugin.name,
            plugin.sounds.clone(),
            plugin.selected_sound_id.as_deref(),
        );
        self.status = format!("{} selected", plugin.name);
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
        self.persist_session_checkpoint();
        Ok(vec![event])
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
                self.menu.sync_active_mode(mode);
                let mut session = self.session.write().expect("session lock poisoned");
                session.active_mode = surface_mode;
                let live_target_deactivated =
                    surface_mode == SurfaceMode::Play && session.live.active.is_some();
                if live_target_deactivated {
                    session.live.deactivate();
                }
                session.revision = Revision::new(session.revision.get().saturating_add(1));
                self.status = format!("Active mode: {mode:?}");
                drop(session);
                if live_target_deactivated {
                    let snapshot = self.performance_snapshot();
                    self.menu.sync_performance_snapshot(snapshot);
                }
                #[cfg(windows)]
                if let Some(audio) = &self.audio
                    && let Err(error) = audio.set_running(mode != ActiveMode::Idle)
                {
                    self.status =
                        format!("Mode changed, but audio state did not follow: {error:#}");
                }
                self.persist_session_checkpoint();
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
        if let Header::Visible(title) = &screen.header {
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

fn plugin_session_state(plugin: &DesktopPlugin) -> PluginInstanceState {
    PluginInstanceState {
        instance_id: InstanceId::new(plugin.instance_id.clone())
            .expect("desktop plugin instance id is validated during loading"),
        plugin_id: plugin.plugin_id.clone(),
        plugin_name: plugin.name.clone(),
        ui_layouts: vec!["little@1".into()],
        config_available: plugin.config_available,
        banks: plugin.banks.clone(),
        sounds: plugin.sound_summaries.clone(),
        selected_sound_id: plugin.selected_sound_id.clone(),
    }
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
    )
}

/// True when an installed, enabled controller package should own the
/// hardware surface: the built-in KeyLab handling stands down for it.
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
    if package.manifest().kind != PluginKind::Instrument {
        bail!(
            "Desktop PLAY currently accepts instrument plugins, found {:?}",
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

    let session = Arc::new(RwLock::new(SessionState::new(
        SessionId::new(DEFAULT_LIVE_SESSION_ID).expect("valid live session id"),
    )));
    let (web_control_sender, web_control) = web::control_channel();
    let web_servers = web::start(
        Arc::clone(&session),
        &options,
        options.web_preferences.clone(),
        web_control_sender,
    )?;
    // The controller supervisor: every enabled .rfcontroller package runs
    // its driver, pointed back at this host through the TCP control bridge.
    // The loop exits on its own when the store holds nothing runnable.
    let controller_shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let root = options.rackforge_root.join("controllers");
        let address = web_servers.control_bridge_addr().to_string();
        let shutdown = Arc::clone(&controller_shutdown);
        std::thread::Builder::new()
            .name("rackforge-controller-supervisor".into())
            .spawn(move || {
                if !root.join("packages").exists() {
                    return;
                }
                let options = rackforge_controller_package::supervise::SuperviseOptions {
                    allow_community: false,
                    extra_env: vec![("RACKFORGE_CONTROL_ADDR".into(), address)],
                    shutdown,
                };
                match rackforge_controller_package::supervise::supervise(&root, &options) {
                    Ok(0) => println!("DESKTOP_CONTROLLERS_NONE"),
                    Ok(count) => println!("DESKTOP_CONTROLLERS_STOPPED count={count}"),
                    Err(error) => eprintln!("DESKTOP_CONTROLLERS_ERROR {error}"),
                }
            })
            .context("starting the controller supervisor")?;
    }
    let mut app = DesktopApp::new(Arc::clone(&session), &options, web_servers, web_control)?;
    app.controller_shutdown = Some(controller_shutdown);
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
            webview: desktop_webview::DesktopWebView::new(creation)?,
        })
    }

    #[cfg(not(windows))]
    fn new(startup: Startup, _creation: &eframe::CreationContext<'_>) -> Result<Self> {
        Ok(Self {
            mode: Self::initial_mode(startup)?,
        })
    }
}

impl eframe::App for RackForgeApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // The player's last touches survive the window closing.
        if let AppMode::Desktop(app) = &mut self.mode {
            if app.live_state_dirty.is_some() {
                app.flush_live_state();
            }
            if let Some(shutdown) = &app.controller_shutdown {
                shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
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
}
