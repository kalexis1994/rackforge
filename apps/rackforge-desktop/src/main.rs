#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod audio_settings;
#[cfg(windows)]
mod desktop_audio;
#[cfg(windows)]
mod desktop_webview;
#[cfg(windows)]
mod native_menu;
mod paths;
mod setup;
mod web;

use anyhow::{Context, Result, bail};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Key, Layout, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, Vec2,
};
use rackforge_control_api::{ControlErrorCode, ControlRequest, ControlResponse};
use rackforge_core::{LoadedPlugin, PluginInstance, PluginPackage};
use rackforge_plugin_api::PluginKind;
use rackforge_repository::{
    InstalledPackage, LocalPackageInspection, MAX_PACKAGE_BYTES, inspect_local_archive,
    install_local_archive,
};
use rackforge_session_api::{
    CommandRef, DEFAULT_LIVE_SESSION_ID, EventEnvelope, InstanceId, MasterLevel, MasterPan,
    PluginInstanceState, Revision, SESSION_SCHEMA_VERSION, SessionCommand, SessionEvent, SessionId,
    SessionState, SoundSummary,
};
use rackforge_surface_api::SurfaceMode;
use rackforge_surface_runtime::{
    ActiveMode, Header, Input, Menu, MenuCommand, PlayPlugin, PlaySound,
};
use semver::Version;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

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

#[derive(Clone, Debug)]
struct Options {
    web_preferences: web::WebServerPreferences,
    rackforge_root: PathBuf,
    plugins_root: PathBuf,
    plugin_store_root: Option<PathBuf>,
    data_root: PathBuf,
    install_archives: Vec<PathBuf>,
}

#[derive(Debug)]
struct CliOptions {
    port: Option<u16>,
    lan: bool,
    rackforge_root: Option<PathBuf>,
    plugins_root: Option<PathBuf>,
    data_root: Option<PathBuf>,
    install_archives: Vec<PathBuf>,
}

enum Startup {
    Ready(Options),
    FirstStart {
        web_preferences: web::WebServerPreferences,
        default_root: PathBuf,
        executable_directory: PathBuf,
        install_archives: Vec<PathBuf>,
    },
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
    audio_settings: Option<audio_settings::AudioSettingsState>,
    #[cfg(windows)]
    webview: desktop_webview::DesktopWebView,
    #[cfg(windows)]
    native_menu: native_menu::NativeMenu,
}

struct DesktopPlugin {
    instance_id: String,
    plugin_id: String,
    name: String,
    version: Version,
    runtime: &'static LoadedPlugin,
    config_available: bool,
    sounds: Vec<PlaySound>,
    selected_sound_id: Option<String>,
    instance: PluginInstance<'static>,
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
    menu: Menu,
    session: Arc<RwLock<SessionState>>,
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
}

impl DesktopApp {
    fn new(
        session: Arc<RwLock<SessionState>>,
        options: &Options,
        web_servers: web::DesktopWebServers,
        web_control: Receiver<web::DesktopControlCall>,
    ) -> Result<Self> {
        let (plugins, mut warnings) = load_desktop_plugins(options)?;
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
                                defaults
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
                    let active = plugins.first().map(|plugin| plugin.instance_id.as_str());
                    match start_desktop_audio(&plugins, &preferences, active) {
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
        let active_instance_id = plugins.first().map(|plugin| plugin.instance_id.as_str());
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
        if let Some(plugin) = plugins.first() {
            menu.sync_active_plugin(
                &plugin.instance_id,
                &plugin.plugin_id,
                &plugin.name,
                plugin.sounds.clone(),
                plugin.selected_sound_id.as_deref(),
            );
        }
        #[cfg(windows)]
        if let Some(audio) = &audio {
            sync_desktop_audio(audio, &session, &menu)?;
        }

        {
            let mut state = session.write().expect("session lock poisoned");
            state.active_instance_id = active_instance_id
                .map(InstanceId::new)
                .transpose()
                .map_err(anyhow::Error::msg)?;
            state.instances = plugins.iter().map(plugin_session_state).collect();
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
        })
    }

    fn reload_plugins(&mut self) -> Result<Vec<String>> {
        let previous_active = self
            .session
            .read()
            .expect("session lock poisoned")
            .active_instance_id
            .as_ref()
            .map(|id| id.as_str().to_owned());
        let (plugins, mut warnings) = load_desktop_plugins(&self.options)?;
        #[cfg(windows)]
        {
            self.audio = None;
            if let Some(preferences) = self.audio_preferences.as_ref() {
                match start_desktop_audio(&plugins, preferences, previous_active.as_deref()) {
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
        let active_instance_id = previous_active
            .as_deref()
            .and_then(|previous| plugins.iter().find(|plugin| plugin.instance_id == previous))
            .or_else(|| plugins.first())
            .map(|plugin| plugin.instance_id.as_str());
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
        Ok(warnings)
    }

    #[cfg(windows)]
    fn show_audio_error(message: &str) {
        rfd::MessageDialog::new()
            .set_title("RackForge audio is unavailable")
            .set_description(message)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }

    #[cfg(windows)]
    fn poll_audio_error(&mut self) {
        let stream_error = self.audio.as_ref().and_then(|audio| audio.take_error());
        if let Some(error) = stream_error {
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
        match start_desktop_audio(&self.plugins, &preferences, active.as_deref()) {
            Ok(audio) => {
                if let Err(error) = sync_desktop_audio(&audio, &self.session, &self.menu) {
                    self.status =
                        format!("Audio reconnected, but controller sync failed: {error:#}");
                }
                let summary = audio.summary().to_owned();
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
    fn audio_settings_state(&self) -> Result<audio_settings::AudioSettingsState> {
        let inventory = desktop_audio::AudioInventory::scan()?;
        let preferences = self
            .audio_preferences
            .clone()
            .map_or_else(|| inventory.default_preferences(), Ok)?;
        let mut state = audio_settings::AudioSettingsState::new(
            inventory,
            preferences,
            self.web_preferences.clone(),
        );
        state.set_runtime_status(self.audio_summary());
        Ok(state)
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
        let inventory = desktop_audio::AudioInventory::scan()?;
        inventory.validate(&preferences)?;
        let previous = self.audio_preferences.clone();
        let active = self
            .session
            .read()
            .expect("session lock poisoned")
            .active_instance_id
            .as_ref()
            .map(|id| id.as_str().to_owned());
        self.audio = None;
        let candidate = match start_desktop_audio(&self.plugins, &preferences, active.as_deref()) {
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
                start_desktop_audio(&self.plugins, preferences, active_instance_id)
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
            PluginInstallActivation::Reload => match self.reload_plugins() {
                Ok(warnings) => {
                    self.status = if warnings.is_empty() {
                        format!("{label} installed and ready")
                    } else {
                        format!("{label} installed · {}", warnings.join(" · "))
                    };
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
            let response = self.handle_web_control(call.request);
            let _ = call.response.send(response);
        }
    }

    fn handle_web_control(&mut self, request: ControlRequest) -> ControlResponse {
        let ControlRequest::Dispatch { envelope } = request else {
            return ControlResponse::Error {
                code: ControlErrorCode::Unavailable,
                message: "This Desktop operation is not connected yet.".into(),
                current_revision: None,
            };
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
        #[cfg(windows)]
        if active
            && let Some(audio) = &self.audio
            && let Err(error) = audio.select_sound(instance_id.as_str(), sound_id)
        {
            self.status = format!("Loaded {sound_id}, but audio did not switch: {error:#}");
        }
        Ok(vec![event])
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
                session.revision = Revision::new(session.revision.get().saturating_add(1));
                self.status = format!("Active mode: {mode:?}");
                drop(session);
                #[cfg(windows)]
                if let Some(audio) = &self.audio
                    && let Err(error) = audio.set_running(mode != ActiveMode::Idle)
                {
                    self.status =
                        format!("Mode changed, but audio state did not follow: {error:#}");
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
            MenuCommand::ReturnToActiveMode {
                mode,
                cancel_draft_id,
                selected_sound_id,
            } => {
                if cancel_draft_id.is_some() {
                    self.status =
                        "Program draft cancellation is not available on Desktop yet".into();
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
                {
                    let mut session = self.session.write().expect("session lock poisoned");
                    session.active_mode = SurfaceMode::Idle;
                    session.revision = Revision::new(session.revision.get().saturating_add(1));
                }
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

fn plugin_session_state(plugin: &DesktopPlugin) -> PluginInstanceState {
    PluginInstanceState {
        instance_id: InstanceId::new(plugin.instance_id.clone())
            .expect("desktop plugin instance id is validated during loading"),
        plugin_id: plugin.plugin_id.clone(),
        plugin_name: plugin.name.clone(),
        ui_layouts: vec!["little@1".into()],
        config_available: plugin.config_available,
        banks: Vec::new(),
        sounds: plugin
            .sounds
            .iter()
            .map(|sound| SoundSummary {
                id: sound.id.clone(),
                name: sound.name.clone(),
                bank: Some(sound.bank.clone()),
                detail: Some(sound.detail.clone()),
                category: None,
                tags: Vec::new(),
                editable: sound.editable,
            })
            .collect(),
        selected_sound_id: plugin.selected_sound_id.clone(),
    }
}

#[cfg(windows)]
fn desktop_audio_specs(plugins: &[DesktopPlugin]) -> Vec<desktop_audio::VoiceSpec> {
    plugins
        .iter()
        .map(|plugin| desktop_audio::VoiceSpec {
            instance_id: plugin.instance_id.clone(),
            plugin: plugin.runtime,
            preset_id: plugin.selected_sound_id.clone(),
        })
        .collect()
}

#[cfg(windows)]
fn start_desktop_audio(
    plugins: &[DesktopPlugin],
    preferences: &desktop_audio::AudioPreferences,
    active_instance_id: Option<&str>,
) -> Result<desktop_audio::DesktopAudio> {
    desktop_audio::DesktopAudio::start(
        desktop_audio_specs(plugins),
        preferences,
        active_instance_id,
    )
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
    let banks = catalog
        .banks
        .iter()
        .map(|bank| (bank.id.as_str(), bank.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let sounds = catalog
        .presets
        .iter()
        .map(|preset| {
            let bank = preset
                .bank
                .as_deref()
                .and_then(|id| banks.get(id).copied())
                .unwrap_or("Factory");
            let detail = preset
                .category
                .as_deref()
                .or(preset.description.as_deref())
                .unwrap_or("Preset");
            PlaySound::new(&preset.id, &preset.name, bank, detail).editable(preset.editable)
        })
        .collect::<Vec<_>>();
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
        sounds,
        selected_sound_id,
        instance,
    })
}

fn parse_startup() -> Result<Startup> {
    resolve_options(parse_cli_options(std::env::args().skip(1))?)
}

fn parse_cli_options(arguments: impl IntoIterator<Item = String>) -> Result<CliOptions> {
    let mut options = CliOptions {
        port: None,
        lan: false,
        rackforge_root: None,
        plugins_root: None,
        data_root: None,
        install_archives: Vec::new(),
    };
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--port" => {
                let port = arguments
                    .next()
                    .context("--port requires a value")?
                    .parse()
                    .context("invalid --port")?;
                if port < 1024 {
                    bail!("--port must be in 1024..=65535");
                }
                options.port = Some(port);
            }
            "--lan" => options.lan = true,
            "--rackforge-root" => {
                options.rackforge_root = Some(PathBuf::from(
                    arguments
                        .next()
                        .context("--rackforge-root requires a directory")?,
                ));
            }
            "--plugins-root" => {
                options.plugins_root = Some(PathBuf::from(
                    arguments
                        .next()
                        .context("--plugins-root requires a directory")?,
                ));
            }
            "--data-root" => {
                options.data_root = Some(PathBuf::from(
                    arguments
                        .next()
                        .context("--data-root requires a directory")?,
                ));
            }
            "--install-plugin" => {
                options.install_archives.push(PathBuf::from(
                    arguments
                        .next()
                        .context("--install-plugin requires an .rfplugin file")?,
                ));
            }
            _ => bail!("unknown argument: {argument}"),
        }
    }
    Ok(options)
}

fn resolve_options(cli: CliOptions) -> Result<Startup> {
    let default_root = paths::default_root()?;
    let executable_directory = paths::executable_directory()?;
    let uses_legacy_paths =
        cli.rackforge_root.is_none() && (cli.plugins_root.is_some() || cli.data_root.is_some());

    let layout = if let Some(root) = cli.rackforge_root.as_deref() {
        paths::DesktopPaths::initialize(root)?
    } else if uses_legacy_paths {
        paths::DesktopPaths::initialize(&default_root)?
    } else if let Some(choice) = paths::load_choice(&default_root, &executable_directory)? {
        paths::DesktopPaths::initialize(choice.root)?
    } else {
        let mut web_preferences = web::WebServerPreferences::default();
        if let Some(port) = cli.port {
            web_preferences.port = port;
        }
        if cli.lan {
            web_preferences.enabled = true;
        }
        return Ok(Startup::FirstStart {
            web_preferences,
            default_root,
            executable_directory,
            install_archives: cli.install_archives,
        });
    };

    let web_config_path = layout.root.join("config/web.toml");
    let mut web_preferences =
        web::WebServerPreferences::load(&web_config_path)?.unwrap_or_default();
    if let Some(port) = cli.port {
        web_preferences.port = port;
    }
    if cli.lan {
        web_preferences.enabled = true;
    }
    Ok(Startup::Ready(Options {
        web_preferences,
        rackforge_root: layout.root,
        plugins_root: cli.plugins_root.unwrap_or(layout.legacy_plugins_root),
        plugin_store_root: (!uses_legacy_paths).then_some(layout.plugin_store_root),
        data_root: cli.data_root.unwrap_or(layout.data_root),
        install_archives: cli.install_archives,
    }))
}

fn options_from_layout(
    web_preferences: web::WebServerPreferences,
    layout: paths::DesktopPaths,
    install_archives: Vec<PathBuf>,
) -> Options {
    Options {
        web_preferences,
        rackforge_root: layout.root,
        plugins_root: layout.legacy_plugins_root,
        plugin_store_root: Some(layout.plugin_store_root),
        data_root: layout.data_root,
        install_archives,
    }
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
    let app = DesktopApp::new(Arc::clone(&session), &options, web_servers, web_control)?;
    Ok(app)
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
            audio_settings: None,
            webview: desktop_webview::DesktopWebView::new(creation)?,
            native_menu: native_menu::NativeMenu::new(creation)?,
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
                let mut install_plugin = false;
                let mut reload_web = false;
                let mut open_browser = false;
                let mut open_root = false;
                let mut open_settings = false;
                #[cfg(windows)]
                let mut web_route: Option<(&'static str, ActiveMode)> = None;
                #[cfg(windows)]
                {
                    let mut popup_request = None;
                    egui::TopBottomPanel::top("rackforge-desktop-menu-bar")
                        .exact_height(42.0)
                        .frame(
                            egui::Frame::new()
                                .fill(Color32::from_rgb(9, 24, 34))
                                .inner_margin(egui::Margin::symmetric(8, 4))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(49, 91, 106))),
                        )
                        .show(context, |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.horizontal(|ui| {
                                for (section, label, width) in [
                                    (native_menu::NativeMenuSection::File, "File", 70.0),
                                    (native_menu::NativeMenuSection::Settings, "Settings", 100.0),
                                    (native_menu::NativeMenuSection::View, "View", 70.0),
                                    (native_menu::NativeMenuSection::Help, "Help", 70.0),
                                ] {
                                    let (rect, response) = ui.allocate_exact_size(
                                        Vec2::new(width, 34.0),
                                        Sense::click(),
                                    );
                                    response.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            label,
                                        )
                                    });
                                    let selected = response.hovered() || response.has_focus();
                                    ui.painter().rect(
                                        rect,
                                        0.0,
                                        if selected {
                                            Color32::from_rgb(92, 226, 245)
                                        } else {
                                            Color32::from_rgb(12, 34, 46)
                                        },
                                        Stroke::new(1.0_f32, Color32::from_rgb(53, 91, 105)),
                                        StrokeKind::Inside,
                                    );
                                    ui.painter().text(
                                        rect.center(),
                                        Align2::CENTER_CENTER,
                                        label,
                                        FontId::proportional(13.0),
                                        if selected {
                                            Color32::from_rgb(2, 16, 22)
                                        } else {
                                            Color32::from_rgb(226, 242, 245)
                                        },
                                    );
                                    let keyboard_open = response.has_focus()
                                        && ui.input(|input| {
                                            input.key_pressed(Key::Enter)
                                                || input.key_pressed(Key::Space)
                                        });
                                    if response.clicked() || keyboard_open {
                                        popup_request = Some((section, rect.left_bottom()));
                                    }
                                }
                            });
                        });
                    if let Some((section, anchor)) = popup_request
                        && let Err(error) =
                            self.native_menu
                                .popup(section, anchor, context.pixels_per_point())
                    {
                        app.status = format!("Could not show application menu: {error:#}");
                    }
                    for command in self.native_menu.drain() {
                        match command {
                            native_menu::NativeMenuCommand::InstallPlugin => install_plugin = true,
                            native_menu::NativeMenuCommand::OpenRoot => open_root = true,
                            native_menu::NativeMenuCommand::Settings => open_settings = true,
                            native_menu::NativeMenuCommand::Exit => {
                                context.send_viewport_cmd(egui::ViewportCommand::Close)
                            }
                            native_menu::NativeMenuCommand::Reload => reload_web = true,
                            native_menu::NativeMenuCommand::OpenBrowser => open_browser = true,
                            native_menu::NativeMenuCommand::DeveloperTools => {
                                self.webview.open_devtools()
                            }
                            native_menu::NativeMenuCommand::AudioMidiStatus => {
                                rfd::MessageDialog::new()
                                    .set_title("RackForge Audio & MIDI")
                                    .set_description(app.audio_summary())
                                    .set_level(rfd::MessageLevel::Info)
                                    .show();
                            }
                            native_menu::NativeMenuCommand::Play => {
                                web_route = Some(("play", ActiveMode::Play));
                            }
                            native_menu::NativeMenuCommand::Live => {
                                web_route = Some(("live", ActiveMode::Live));
                            }
                            native_menu::NativeMenuCommand::About => {
                                rfd::MessageDialog::new()
                                    .set_title("About RackForge")
                                    .set_description(format!(
                                        "RackForge Desktop\nRust host · Embedded Web workspace\n\n{}",
                                        app.audio_summary()
                                    ))
                                    .set_level(rfd::MessageLevel::Info)
                                    .show();
                            }
                        }
                    }
                }

                #[cfg(windows)]
                if let Some((route, mode)) = web_route {
                    app.apply_command(MenuCommand::SetActiveMode { mode });
                    let url = format!("{}/{route}", app.web_url.trim_end_matches('/'));
                    if let Err(error) = self.webview.navigate(&url) {
                        app.status = format!("Could not open {route}: {error:#}");
                    }
                }

                if install_plugin {
                    app.begin_plugin_install();
                }
                reload_web |= app.poll_plugin_install(context);
                context.send_viewport_cmd(egui::ViewportCommand::Title(app.window_title().into()));
                #[cfg(windows)]
                self.native_menu
                    .set_install_enabled(!app.install_in_progress());
                if open_browser {
                    match webbrowser::open(&app.web_url) {
                        Ok(()) => app.status = format!("Opened {}", app.web_url),
                        Err(error) => app.status = format!("Could not open browser: {error}"),
                    }
                }
                if open_root {
                    match std::process::Command::new("explorer.exe")
                        .arg(&app.options.rackforge_root)
                        .spawn()
                    {
                        Ok(_) => {
                            app.status = format!(
                                "Opened RackForge Root {}",
                                app.options.rackforge_root.display()
                            )
                        }
                        Err(error) => {
                            app.status = format!("Could not open RackForge Root: {error}")
                        }
                    }
                }

                #[cfg(windows)]
                if open_settings {
                    match app.audio_settings_state() {
                        Ok(state) => self.audio_settings = Some(state),
                        Err(error) => {
                            app.status = format!("Could not open Audio & MIDI settings: {error:#}");
                            DesktopApp::show_audio_error(&app.status);
                        }
                    }
                }

                #[cfg(windows)]
                if let Some(settings) = self.audio_settings.as_mut() {
                    settings.set_runtime_status(app.audio_summary());
                }
                #[cfg(windows)]
                let settings_action = self
                    .audio_settings
                    .as_mut()
                    .map(|settings| settings.show(context));
                #[cfg(windows)]
                if let Some(action) = settings_action {
                    match action {
                        audio_settings::AudioSettingsAction::None => {}
                        audio_settings::AudioSettingsAction::Close => {
                            self.audio_settings = None;
                            context.request_repaint();
                        }
                        audio_settings::AudioSettingsAction::Rescan => {
                            match desktop_audio::AudioInventory::scan() {
                                Ok(inventory) => {
                                    if let Some(settings) = self.audio_settings.as_mut() {
                                        settings.replace_inventory(inventory);
                                    }
                                }
                                Err(error) => {
                                    if let Some(settings) = self.audio_settings.as_mut() {
                                        settings.error(format!("Device rescan failed: {error:#}"));
                                    }
                                }
                            }
                        }
                        audio_settings::AudioSettingsAction::TestNote => {
                            if let Err(error) = app.test_audio_note()
                                && let Some(settings) = self.audio_settings.as_mut()
                            {
                                settings.error(format!("Could not play test note: {error:#}"));
                            }
                        }
                        audio_settings::AudioSettingsAction::Apply(preferences) => {
                            match app.apply_audio_preferences(preferences.clone()) {
                                Ok(message) => {
                                    if let Some(settings) = self.audio_settings.as_mut() {
                                        settings.applied(preferences, message);
                                    }
                                }
                                Err(error) => {
                                    if let Some(settings) = self.audio_settings.as_mut() {
                                        settings.error(format!("{error:#}"));
                                    }
                                }
                            }
                        }
                        audio_settings::AudioSettingsAction::ApplyWeb(preferences) => {
                            match app.apply_web_preferences(preferences.clone()) {
                                Ok(message) => {
                                    if let Some(settings) = self.audio_settings.as_mut() {
                                        settings.web_applied(preferences, message);
                                    }
                                }
                                Err(error) => {
                                    if let Some(settings) = self.audio_settings.as_mut() {
                                        settings.error(format!("{error:#}"));
                                    }
                                }
                            }
                        }
                    }
                }

                #[cfg(windows)]
                {
                    if self.audio_settings.is_some() {
                        if let Err(error) = self.webview.hide() {
                            app.status = format!("Could not hide embedded Web UI: {error:#}");
                        }
                    } else {
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
    let startup = parse_startup()?;
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RackForge Desktop")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0]),
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

    #[test]
    fn parses_root_and_repeatable_plugin_install_arguments() {
        let parsed = parse_cli_options([
            "--port".to_owned(),
            "9123".to_owned(),
            "--lan".to_owned(),
            "--rackforge-root".to_owned(),
            "D:\\RackForge".to_owned(),
            "--install-plugin".to_owned(),
            "one.rfplugin".to_owned(),
            "--install-plugin".to_owned(),
            "two.rfplugin".to_owned(),
        ])
        .unwrap();

        assert_eq!(parsed.port, Some(9123));
        assert!(parsed.lan);
        assert_eq!(parsed.rackforge_root, Some(PathBuf::from("D:\\RackForge")));
        assert_eq!(
            parsed.install_archives,
            vec![PathBuf::from("one.rfplugin"), PathBuf::from("two.rfplugin")]
        );
    }

    #[test]
    fn keeps_legacy_plugin_and_data_arguments() {
        let parsed = parse_cli_options([
            "--plugins-root".to_owned(),
            "legacy-plugins".to_owned(),
            "--data-root".to_owned(),
            "legacy-data".to_owned(),
        ])
        .unwrap();

        assert_eq!(parsed.plugins_root, Some(PathBuf::from("legacy-plugins")));
        assert_eq!(parsed.data_root, Some(PathBuf::from("legacy-data")));
        assert!(parsed.rackforge_root.is_none());
    }

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
}
