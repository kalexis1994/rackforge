use anyhow::Context;
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path as AxumPath, Query, State, WebSocketUpgrade, ws::Message},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use include_dir::{Dir, include_dir};
use rackforge_control_api::{ClientId, ControlRequest, ControlResponse};
use rackforge_core::PluginPackage;
use rackforge_repository::{
    LocalPackageInspection, MAX_PACKAGE_BYTES, inspect_local_archive,
    install_local_archive_cancellable,
};
use rackforge_resource_api::{
    BindResourceRequest, BindSelectionRequest, BrowseGrantRequest, ClearInstalledResourceRequest,
    ListGrantsRequest, LoadGrantedResourceRequest, MAX_CLIENT_UPLOAD_BYTES, ResourceBrowser,
    ResourceBundleKind, ResourceEntryKind, ResourceError, SelectHostEntryRequest,
};
use rackforge_resource_host::NativeResourceBrowser;
use rackforge_session_api::SessionState;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

use crate::Options;

static WEB_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../web/dist");
const DESKTOP_CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
const DESKTOP_SETTINGS_TIMEOUT: Duration = Duration::from_secs(20);
static CLIENT_UPLOAD_SERIAL: AtomicU64 = AtomicU64::new(1);
static PLUGIN_INSTALL_CANCELLATIONS: OnceLock<Mutex<BTreeMap<String, Arc<AtomicBool>>>> =
    OnceLock::new();

type PluginWebPackageCache = Arc<Mutex<Option<(u64, BTreeMap<String, PluginWebPackage>)>>>;

fn plugin_install_cancellations() -> &'static Mutex<BTreeMap<String, Arc<AtomicBool>>> {
    PLUGIN_INSTALL_CANCELLATIONS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[derive(Clone)]
struct WebState {
    session: Arc<RwLock<SessionState>>,
    plugin_catalog_revision: Arc<AtomicU64>,
    legacy_plugins_root: PathBuf,
    plugin_store_root: Option<PathBuf>,
    data_root: PathBuf,
    public_server: Arc<RwLock<WebServerPreferences>>,
    control: Sender<DesktopControlCall>,
    resource_browser: Arc<NativeResourceBrowser>,
    resource_upload_root: PathBuf,
    /// The web-package discovery walks every installed package directory and
    /// parses every manifest -- dozens of file opens. Doing that once per
    /// ASSET REQUEST made the splash and the icons crawl. The result only
    /// changes when the catalog does, so it is cached against the catalog
    /// revision; `active` flags are refreshed from the session per call.
    web_packages_cache: PluginWebPackageCache,
    /// Bumped only when the set of installed packages actually changes
    /// (install, uninstall). `plugin_catalog_revision` also bumps on
    /// ACTIVATION for the clients' sake, and keying the scan cache on it
    /// meant the first splash after every activation paid the full package
    /// walk again.
    package_scan_revision: Arc<AtomicU64>,
    /// Root of the controller package store (`<root>/controllers`).
    controllers_root: PathBuf,
    /// Notes from a surface go straight to the audio thread through this.
    /// Routing them through the GUI thread capped them at one frame each —
    /// about sixty a second — so fast playing on a touch surface queued up
    /// and arrived late. It is refreshed whenever the audio engine restarts.
    injected_midi: Arc<Mutex<Option<SyncSender<crate::desktop_audio::MidiPacket>>>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadResourceQuery {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallSelectedPluginRequest {
    selection_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeResourcePickRequest {
    kind: ResourceEntryKind,
    #[serde(default)]
    extensions: Vec<String>,
}

const MAX_PORTABLE_TEXT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeTextReadRequest {
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default = "default_portable_text_limit")]
    maximum_bytes: usize,
}

fn default_portable_text_limit() -> usize {
    MAX_PORTABLE_TEXT_BYTES
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeTextWriteRequest {
    file_name: String,
    mime_type: String,
    text: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UninstallPluginRequest {
    delete_presets: bool,
    delete_plugin_data: bool,
}

pub enum DesktopControlCall {
    Session {
        request: ControlRequest,
        response: Sender<ControlResponse>,
    },
    Performance {
        request: ControlRequest,
        response: Sender<ControlResponse>,
    },
    LoadResource {
        plugin_id: String,
        resource_id: String,
        path: PathBuf,
        persist: bool,
        preview: bool,
        response: Sender<Result<(), String>>,
    },
    ClearResource {
        plugin_id: String,
        resource_id: String,
        response: Sender<Result<(), String>>,
    },
    ActivatePlugin {
        plugin_id: String,
        response: Sender<Result<(), String>>,
    },
    DeactivatePlugin {
        plugin_id: String,
        response: Sender<Result<(), String>>,
    },
    UninstallPlugin {
        plugin_id: String,
        delete_presets: bool,
        delete_plugin_data: bool,
        response: Sender<Result<Value, String>>,
    },
    AudioSettings {
        response: Sender<Result<Value, String>>,
    },
    ApplyAudioSettings {
        preferences: Value,
        response: Sender<Result<Value, String>>,
    },
    TestAudio {
        response: Sender<Result<(), String>>,
    },
    ApplyWebSettings {
        preferences: WebServerPreferences,
        response: Sender<Result<Value, String>>,
    },
}

pub fn control_channel() -> (Sender<DesktopControlCall>, Receiver<DesktopControlCall>) {
    mpsc::channel()
}

const WEB_SERVER_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebServerPreferences {
    pub schema_version: u32,
    pub enabled: bool,
    pub port: u16,
}

impl Default for WebServerPreferences {
    fn default() -> Self {
        Self {
            schema_version: WEB_SERVER_SCHEMA_VERSION,
            enabled: false,
            port: 8787,
        }
    }
}

impl WebServerPreferences {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == WEB_SERVER_SCHEMA_VERSION,
            "unsupported Web server configuration schema {}",
            self.schema_version
        );
        anyhow::ensure!(
            self.port >= 1024,
            "HTTP server port must be in 1024..=65535"
        );
        Ok(())
    }

    pub fn load(path: &std::path::Path) -> anyhow::Result<Option<Self>> {
        if !path.is_file() {
            return Ok(None);
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading Web server configuration {}", path.display()))?;
        let preferences: Self = toml::from_str(&text)
            .with_context(|| format!("parsing Web server configuration {}", path.display()))?;
        preferences.validate()?;
        Ok(Some(preferences))
    }

    pub fn persist(&self, path: &std::path::Path) -> anyhow::Result<()> {
        self.validate()?;
        let parent = path
            .parent()
            .context("Web server configuration has no parent")?;
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        if temporary.exists() {
            fs::remove_file(&temporary)
                .with_context(|| format!("removing stale {}", temporary.display()))?;
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .with_context(|| format!("creating {}", temporary.display()))?;
        file.write_all(toml::to_string_pretty(self)?.as_bytes())?;
        file.sync_all()?;
        drop(file);
        if path.exists() {
            fs::remove_file(path).with_context(|| format!("replacing {}", path.display()))?;
        }
        fs::rename(&temporary, path)
            .with_context(|| format!("activating Web server configuration {}", path.display()))
    }
}

struct RunningServer {
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl RunningServer {
    fn request_shutdown(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }

    fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    fn finish(&mut self, wait: bool) {
        self.request_shutdown();
        let Some(thread) = self.thread.take() else {
            return;
        };
        if wait || thread.is_finished() {
            let _ = thread.join();
        }
        // Dropping an unfinished handle detaches it. The process is already
        // committed to exit and the server received its shutdown signal; a
        // broken Tokio task must never freeze the Desktop window forever.
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.finish(true);
    }
}

pub struct DesktopWebServers {
    _local: RunningServer,
    public: Option<RunningServer>,
    state: WebState,
    preferences: Arc<RwLock<WebServerPreferences>>,
    local_url: String,
    control_bridge_addr: SocketAddr,
}

impl DesktopWebServers {
    /// Where the framed control protocol listens; handed to controller
    /// drivers as `RACKFORGE_CONTROL_ADDR`.
    pub fn control_bridge_addr(&self) -> SocketAddr {
        self.control_bridge_addr
    }
}

impl DesktopWebServers {
    /// Points surface notes at the running audio engine. Called when the
    /// engine starts and again after every recovery, since a restarted
    /// engine owns a new queue.
    pub fn set_injected_midi(&self, sender: Option<SyncSender<crate::desktop_audio::MidiPacket>>) {
        if let Ok(mut slot) = self.state.injected_midi.lock() {
            *slot = sender;
        }
    }

    pub fn local_url(&self) -> &str {
        &self.local_url
    }

    pub fn apply(&mut self, preferences: WebServerPreferences) -> anyhow::Result<()> {
        preferences.validate()?;
        let current = self
            .preferences
            .read()
            .expect("Web settings lock poisoned")
            .clone();
        if current == preferences {
            return Ok(());
        }

        let replacement = if preferences.enabled {
            Some(bind_public_server(self.state.clone(), preferences.port)?)
        } else {
            None
        };
        *self
            .preferences
            .write()
            .expect("Web settings lock poisoned") = preferences;
        self.public = replacement;
        Ok(())
    }

    pub fn request_shutdown(&mut self) {
        self._local.request_shutdown();
        if let Some(public) = self.public.as_mut() {
            public.request_shutdown();
        }
    }

    pub fn shutdown_complete(&self) -> bool {
        self._local.is_finished() && self.public.as_ref().is_none_or(RunningServer::is_finished)
    }

    pub fn finish_shutdown(&mut self, wait: bool) {
        self._local.finish(wait);
        if let Some(public) = self.public.as_mut() {
            public.finish(wait);
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct PublicWebSurface {
    kind: rackforge_plugin_api::WebSurfaceKind,
    entry_url: String,
}

#[derive(Clone, Debug, Serialize)]
struct PublicPluginBranding {
    icon_url: String,
    banner_url: String,
    splash_url: String,
    background_color: Option<String>,
    accent_color: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct PublicPluginWeb {
    plugin_id: String,
    plugin_name: String,
    version: String,
    kind: rackforge_plugin_api::PluginKind,
    active: bool,
    managed: bool,
    api_version: u16,
    branding: Option<PublicPluginBranding>,
    surfaces: Vec<PublicWebSurface>,
    resources: Vec<rackforge_plugin_api::ResourceRequirement>,
}

#[derive(Clone)]
struct PluginWebPackage {
    root: PathBuf,
    public: PublicPluginWeb,
}

pub fn start(
    session: Arc<RwLock<SessionState>>,
    options: &Options,
    preferences: WebServerPreferences,
    control: Sender<DesktopControlCall>,
) -> anyhow::Result<DesktopWebServers> {
    preferences.validate()?;
    let shared_preferences = Arc::new(RwLock::new(preferences.clone()));
    let injected_midi = Arc::new(Mutex::new(None));
    let state = WebState {
        injected_midi: Arc::clone(&injected_midi),
        session,
        plugin_catalog_revision: Arc::new(AtomicU64::new(0)),
        web_packages_cache: Arc::new(Mutex::new(None)),
        package_scan_revision: Arc::new(AtomicU64::new(0)),
        controllers_root: options.rackforge_root.join("controllers"),
        legacy_plugins_root: options.plugins_root.clone(),
        plugin_store_root: options.plugin_store_root.clone(),
        data_root: options.data_root.clone(),
        public_server: Arc::clone(&shared_preferences),
        control,
        resource_browser: Arc::new(NativeResourceBrowser::platform_defaults_persistent(
            options.rackforge_root.join("state/resource-grants.json"),
        )?),
        resource_upload_root: options.rackforge_root.join("state/resource-uploads"),
    };
    let local_listener =
        std::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))?;
    let local_port = local_listener.local_addr()?.port();
    let local = spawn_server(
        local_listener,
        state.clone(),
        "rackforge-desktop-web-local",
        true,
    )?;
    let public = preferences
        .enabled
        .then(|| bind_public_server(state.clone(), preferences.port))
        .transpose()?;
    // The control bridge: the framed control protocol (one JSON line in,
    // one out per connection) on TCP loopback, so controller drivers on
    // hosts without a Unix control socket -- this one -- reach the same
    // session dispatch every other client uses. The supervisor hands
    // drivers the address through RACKFORGE_CONTROL_ADDR.
    let bridge_listener =
        std::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))?;
    let control_bridge_addr = bridge_listener.local_addr()?;
    {
        let bridge_state = state.clone();
        std::thread::Builder::new()
            .name("rackforge-control-bridge".into())
            .spawn(move || control_bridge(bridge_listener, bridge_state))
            .context("starting the control bridge")?;
    }
    Ok(DesktopWebServers {
        _local: local,
        public,
        state,
        preferences: shared_preferences,
        local_url: format!("http://127.0.0.1:{local_port}"),
        control_bridge_addr,
    })
}

fn control_bridge(listener: std::net::TcpListener, state: WebState) {
    for connection in listener.incoming() {
        let Ok(stream) = connection else {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        let connection_state = state.clone();
        let _ = std::thread::Builder::new()
            .name("rackforge-control-client".into())
            .spawn(move || handle_control_bridge_connection(stream, connection_state));
    }
}

fn handle_control_bridge_connection(mut stream: std::net::TcpStream, state: WebState) {
    use std::io::{BufRead, BufReader, Read, Write};
    const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let Ok(reader_stream) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader_stream);
    let mut bytes = Vec::new();
    let mut virtual_midi_clients = std::collections::BTreeSet::<ClientId>::new();
    loop {
        bytes.clear();
        match reader
            .by_ref()
            .take(MAX_REQUEST_BYTES + 1)
            .read_until(b'\n', &mut bytes)
        {
            Ok(0) | Err(_) => break,
            Ok(_) if bytes.len() as u64 > MAX_REQUEST_BYTES => break,
            Ok(_) => {}
        }
        let response = match serde_json::from_slice::<ControlRequest>(&bytes) {
            Ok(request) => {
                match &request {
                    ControlRequest::VirtualMidi { client_id, .. } => {
                        virtual_midi_clients.insert(client_id.clone());
                    }
                    ControlRequest::ReleaseVirtualMidi { client_id } => {
                        virtual_midi_clients.remove(client_id);
                    }
                    _ => {}
                }
                response_for(request, &state)
            }
            Err(error) => json!({
                "status": "error",
                "code": "invalid_request",
                "message": error.to_string(),
            }),
        };
        let mut line = response.to_string().into_bytes();
        line.push(b'\n');
        if stream.write_all(&line).is_err() {
            break;
        }
    }
    for client_id in virtual_midi_clients {
        release_injected_midi(&state);
        let _ = response_for(ControlRequest::ReleaseVirtualMidi { client_id }, &state);
    }
}

fn release_injected_midi(state: &WebState) {
    let Ok(slot) = state.injected_midi.lock() else {
        return;
    };
    let Some(sender) = slot.as_ref() else {
        return;
    };
    for channel in 0..16 {
        for (controller, value) in [(64, 0), (123, 0)] {
            let _ = sender.try_send(crate::desktop_audio::MidiPacket {
                source: crate::desktop_audio::VIRTUAL_MIDI_SOURCE_KEY,
                length: 3,
                data: [0xb0 | channel, controller, value],
            });
        }
    }
}

/// Attempts the low-latency surface route. A disconnected sender belongs to
/// an engine generation that has already retired; remove it and let the
/// caller fall back to the Desktop control loop, which always sees the
/// current generation. Queue pressure is different: the engine is alive, so
/// report it instead of silently reordering the note through a slower path.
fn try_injected_midi(
    state: &WebState,
    packet: crate::desktop_audio::MidiPacket,
) -> Result<bool, String> {
    let mut slot = state
        .injected_midi
        .lock()
        .map_err(|_| "the audio route is unavailable".to_owned())?;
    let Some(sender) = slot.as_ref() else {
        return Ok(false);
    };
    match sender.try_send(packet) {
        Ok(()) => Ok(true),
        Err(mpsc::TrySendError::Full(_)) => {
            Err("the audio queue is full; try the note again".into())
        }
        Err(mpsc::TrySendError::Disconnected(_)) => {
            *slot = None;
            Ok(false)
        }
    }
}

fn bind_public_server(state: WebState, port: u16) -> anyhow::Result<RunningServer> {
    let listener =
        std::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port))
            .with_context(|| format!("opening public HTTP server on port {port}"))?;
    spawn_server(listener, state, "rackforge-desktop-web-public", false)
}

fn spawn_server(
    listener: std::net::TcpListener,
    state: WebState,
    thread_name: &str,
    allow_native_resources: bool,
) -> anyhow::Result<RunningServer> {
    listener.set_nonblocking(true)?;
    let (shutdown, shutdown_receiver) = tokio::sync::oneshot::channel();
    let thread = std::thread::Builder::new()
        .name(thread_name.into())
        .spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("create desktop Web runtime");
            runtime.block_on(async move {
                // Warm the web-package cache off the request path: the first
                // discovery walks every installed package (measured ~1.4 s
                // cold), and paying that on the first splash request is why
                // opening an instrument used to stall on a black panel.
                {
                    let warm_state = state.clone();
                    tokio::task::spawn_blocking(move || {
                        let _ = discover_web_packages(&warm_state);
                    });
                }
                let app = router(state, allow_native_resources);
                let listener = tokio::net::TcpListener::from_std(listener)
                    .expect("adopt RackForge Desktop Web listener");
                tokio::select! {
                    result = axum::serve(listener, app) => {
                        if let Err(error) = result {
                            eprintln!("RACKFORGE_DESKTOP_WEB_ERROR {error}");
                        }
                    }
                    _ = shutdown_receiver => {}
                }
            });
        })?;
    Ok(RunningServer {
        shutdown: Some(shutdown),
        thread: Some(thread),
    })
}

fn router(state: WebState, allow_native_resources: bool) -> Router {
    let router = Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/auth/status", get(auth_status))
        .route(
            "/api/v1/host/audio",
            get(audio_settings).put(apply_audio_settings),
        )
        .route("/api/v1/host/audio/test", post(test_audio))
        .route("/api/v1/plugins", get(plugin_catalog))
        .route(
            "/api/v1/plugins/{plugin_id}",
            get(plugin_descriptor).delete(uninstall_managed_plugin),
        )
        .route(
            "/api/v1/plugins/{plugin_id}/activate",
            post(activate_plugin),
        )
        .route(
            "/api/v1/plugins/{plugin_id}/deactivate",
            post(deactivate_plugin),
        )
        .route("/ws/v1/session", get(session_socket))
        .route("/plugin-assets/{plugin_id}/{*asset}", get(plugin_asset))
        .route("/api/v1/controllers", get(controller_catalog))
        .route(
            "/api/v1/controllers/{controller_id}/settings",
            axum::routing::put(apply_controller_settings),
        );
    let router = if allow_native_resources {
        router
            .route("/api/v1/config", get(local_config).put(apply_web_settings))
            .route("/api/v1/resources/mounts", get(resource_mounts))
            .route(
                "/api/v1/resources/mounts/{mount_id}/root",
                get(resource_mount_root),
            )
            .route(
                "/api/v1/resources/entries/{parent_id}",
                get(resource_entries),
            )
            .route("/api/v1/resources/bind", post(bind_resource))
            .route(
                "/api/v1/resources/bind-selection",
                post(bind_resource_selection),
            )
            .route("/api/v1/resources/grants", post(resource_grants))
            .route("/api/v1/resources/status", post(resource_status))
            .route("/api/v1/resources/browse", post(browse_resource_grant))
            .route("/api/v1/resources/load", post(load_granted_resource))
            .route("/api/v1/resources/clear", post(clear_installed_resource))
            .route(
                "/api/v1/resources/uploads",
                post(upload_client_resource)
                    .layer(DefaultBodyLimit::max(MAX_CLIENT_UPLOAD_BYTES as usize)),
            )
            .route("/api/v1/resources/selections", post(select_host_resource))
            .route("/api/v1/resources/native-pick", post(pick_native_resource))
            .route(
                "/api/v1/resources/native-read-text",
                post(read_native_text_file).layer(DefaultBodyLimit::max(MAX_PORTABLE_TEXT_BYTES)),
            )
            .route(
                "/api/v1/resources/native-write-text",
                post(write_native_text_file).layer(DefaultBodyLimit::max(MAX_PORTABLE_TEXT_BYTES)),
            )
            .route(
                "/api/v1/resources/selections/release",
                post(release_resource_selection),
            )
            .route("/api/v1/plugins/inspect", post(inspect_selected_plugin))
            .route("/api/v1/plugins/install", post(install_selected_plugin))
            .route(
                "/api/v1/plugins/install/cancel",
                post(cancel_selected_plugin_install),
            )
    } else {
        router.route("/api/v1/config", get(config))
    };
    router.fallback(get(static_asset)).with_state(state)
}

async fn health() -> Json<Value> {
    let ui_revision = WEB_ASSETS
        .get_file("ui-revision.txt")
        .and_then(|file| file.contents_utf8())
        .map(str::trim)
        .unwrap_or("unknown");
    Json(json!({
        "status": "ok",
        "core_connected": true,
        "schema_version": 1,
        "host": "desktop",
        "revision": env!("RACKFORGE_REVISION"),
        "ui_revision": ui_revision,
    }))
}

async fn auth_status() -> Json<Value> {
    // Desktop serves itself to whoever is already sitting at the machine, so
    // there is nothing to unlock and no PIN to manage. Saying so keeps the
    // interface from offering a control that would do nothing here.
    Json(json!({
        "status": "ok",
        "pin_managed": false,
        "requires_pin": false,
        "unlocked": true,
        "pin_state": "set",
        "locked_for": 0
    }))
}

async fn config(State(state): State<WebState>) -> Json<Value> {
    config_response(&state, false)
}

async fn local_config(State(state): State<WebState>) -> Json<Value> {
    config_response(&state, true)
}

fn config_response(state: &WebState, configurable: bool) -> Json<Value> {
    let preferences = state
        .public_server
        .read()
        .expect("Web settings lock poisoned")
        .clone();
    Json(json!({
        "enabled": preferences.enabled,
        "access": if preferences.enabled { "lan" } else { "local" },
        "port": preferences.port,
        "configurable": configurable
    }))
}

async fn apply_web_settings(
    State(state): State<WebState>,
    Json(preferences): Json<WebServerPreferences>,
) -> Response {
    desktop_settings_response(&state, |response| DesktopControlCall::ApplyWebSettings {
        preferences,
        response,
    })
}

async fn audio_settings(State(state): State<WebState>) -> Response {
    desktop_settings_response(&state, |response| DesktopControlCall::AudioSettings {
        response,
    })
}

async fn apply_audio_settings(
    State(state): State<WebState>,
    Json(preferences): Json<Value>,
) -> Response {
    desktop_settings_response(&state, |response| DesktopControlCall::ApplyAudioSettings {
        preferences,
        response,
    })
}

async fn test_audio(State(state): State<WebState>) -> Response {
    let (response_sender, response_receiver) = mpsc::channel();
    if state
        .control
        .send(DesktopControlCall::TestAudio {
            response: response_sender,
        })
        .is_err()
    {
        return desktop_settings_error("Desktop runtime is shutting down".into());
    }
    match response_receiver.recv_timeout(DESKTOP_SETTINGS_TIMEOUT) {
        Ok(Ok(())) => Json(json!({"status":"ok"})).into_response(),
        Ok(Err(message)) => desktop_settings_error(message),
        Err(_) => desktop_settings_error(
            "Desktop runtime did not answer the audio test request in time.".into(),
        ),
    }
}

fn desktop_settings_response(
    state: &WebState,
    call: impl FnOnce(Sender<Result<Value, String>>) -> DesktopControlCall,
) -> Response {
    let (response_sender, response_receiver) = mpsc::channel();
    if state.control.send(call(response_sender)).is_err() {
        return desktop_settings_error("Desktop runtime is shutting down".into());
    }
    match response_receiver.recv_timeout(DESKTOP_SETTINGS_TIMEOUT) {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(message)) => desktop_settings_error(message),
        Err(_) => desktop_settings_error(
            "Desktop runtime did not answer the audio settings request in time.".into(),
        ),
    }
}

fn desktop_settings_error(message: String) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"status":"error", "message":message})),
    )
        .into_response()
}

async fn plugin_catalog(State(state): State<WebState>) -> Response {
    match discover_web_packages(&state) {
        Ok(packages) => Json(
            packages
                .into_values()
                .map(|package| package.public)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(error) => internal_error(error),
    }
}

async fn plugin_descriptor(
    AxumPath(plugin_id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    match discover_web_packages(&state) {
        Ok(mut packages) => packages.remove(&plugin_id).map_or_else(
            || StatusCode::NOT_FOUND.into_response(),
            |package| Json(package.public).into_response(),
        ),
        Err(error) => internal_error(error),
    }
}

async fn activate_plugin(
    AxumPath(plugin_id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    if !plugin_is_installed(&state, &plugin_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"status":"error", "message":"Plugin is not installed."})),
        )
            .into_response();
    }
    let (response_sender, response_receiver) = mpsc::channel();
    if state
        .control
        .send(DesktopControlCall::ActivatePlugin {
            plugin_id: plugin_id.clone(),
            response: response_sender,
        })
        .is_err()
    {
        return resource_error(ResourceError::Backend(
            "Desktop runtime is shutting down".into(),
        ));
    }
    match tokio::task::spawn_blocking(move || {
        response_receiver.recv_timeout(Duration::from_secs(45))
    })
    .await
    {
        Ok(Ok(Ok(()))) => {
            state.plugin_catalog_revision.fetch_add(1, Ordering::AcqRel);
            Json(json!({"status":"active", "plugin_id":plugin_id})).into_response()
        }
        Ok(Ok(Err(message))) => (
            StatusCode::CONFLICT,
            Json(json!({"status":"error", "message":message})),
        )
            .into_response(),
        _ => resource_error(ResourceError::Backend(
            "Desktop runtime did not finish activating the plugin".into(),
        )),
    }
}

async fn deactivate_plugin(
    AxumPath(plugin_id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    let package = match discover_web_packages(&state) {
        Ok(packages) => packages.get(&plugin_id).cloned(),
        Err(error) => return internal_error(error),
    };
    let Some(package) = package else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"status":"error", "message":"Plugin is not installed."})),
        )
            .into_response();
    };
    if !package.public.managed {
        return (
            StatusCode::CONFLICT,
            Json(json!({"status":"error", "message":"Built-in plugins cannot be deactivated."})),
        )
            .into_response();
    }
    let (response_sender, response_receiver) = mpsc::channel();
    if state
        .control
        .send(DesktopControlCall::DeactivatePlugin {
            plugin_id: plugin_id.clone(),
            response: response_sender,
        })
        .is_err()
    {
        return resource_error(ResourceError::Backend(
            "Desktop runtime is shutting down".into(),
        ));
    }
    match tokio::task::spawn_blocking(move || {
        response_receiver.recv_timeout(Duration::from_secs(45))
    })
    .await
    {
        Ok(Ok(Ok(()))) => {
            state.plugin_catalog_revision.fetch_add(1, Ordering::AcqRel);
            state.package_scan_revision.fetch_add(1, Ordering::AcqRel);
            Json(json!({"status":"inactive", "plugin_id":plugin_id})).into_response()
        }
        Ok(Ok(Err(message))) => (
            StatusCode::CONFLICT,
            Json(json!({"status":"error", "message":message})),
        )
            .into_response(),
        _ => resource_error(ResourceError::Backend(
            "Desktop runtime did not finish deactivating the plugin".into(),
        )),
    }
}

async fn uninstall_managed_plugin(
    AxumPath(plugin_id): AxumPath<String>,
    State(state): State<WebState>,
    request: Option<Json<UninstallPluginRequest>>,
) -> Response {
    let request = request.map(|Json(request)| request).unwrap_or_default();
    let package = match discover_web_packages(&state) {
        Ok(packages) => packages
            .get(&plugin_id)
            .map(|package| package.public.managed),
        Err(error) => return internal_error(error),
    };
    match package {
        None => return StatusCode::NOT_FOUND.into_response(),
        Some(false) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "status":"error",
                    "message":"This plugin is part of the host installation and cannot be removed from the package manager."
                })),
            )
                .into_response();
        }
        Some(true) => {}
    }
    let (response_sender, response_receiver) = mpsc::channel();
    if state
        .control
        .send(DesktopControlCall::UninstallPlugin {
            plugin_id,
            delete_presets: request.delete_presets,
            delete_plugin_data: request.delete_plugin_data,
            response: response_sender,
        })
        .is_err()
    {
        return resource_error(ResourceError::Backend(
            "Desktop runtime is shutting down".into(),
        ));
    }
    match tokio::task::spawn_blocking(move || {
        response_receiver.recv_timeout(Duration::from_secs(45))
    })
    .await
    {
        Ok(Ok(Ok(mut result))) => {
            if request.delete_plugin_data
                && result
                    .get("plugin_data_deleted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                && let Err(error) = state.resource_browser.release_plugin_grants(
                    result
                        .get("plugin_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                )
            {
                result["user_data_cleanup_warning"] = Value::String(format!(
                    "private data was deleted, but resource grants could not be revoked: {error}"
                ));
            }
            state.plugin_catalog_revision.fetch_add(1, Ordering::AcqRel);
            state.package_scan_revision.fetch_add(1, Ordering::AcqRel);
            {
                let warm_state = state.clone();
                tokio::task::spawn_blocking(move || {
                    let _ = discover_web_packages(&warm_state);
                });
            }
            Json(result).into_response()
        }
        Ok(Ok(Err(message))) => (
            StatusCode::CONFLICT,
            Json(json!({"status":"error", "message":message})),
        )
            .into_response(),
        _ => resource_error(ResourceError::Backend(
            "Desktop runtime did not finish removing the plugin".into(),
        )),
    }
}

async fn resource_mounts(State(state): State<WebState>) -> Response {
    match state.resource_browser.mounts() {
        Ok(mounts) => Json(mounts).into_response(),
        Err(error) => resource_error(error),
    }
}

async fn resource_mount_root(
    State(state): State<WebState>,
    AxumPath(mount_id): AxumPath<String>,
) -> Response {
    match state.resource_browser.mount_root(&mount_id) {
        Ok(entry) => Json(entry).into_response(),
        Err(error) => resource_error(error),
    }
}

async fn resource_entries(
    State(state): State<WebState>,
    AxumPath(parent_id): AxumPath<String>,
) -> Response {
    match state.resource_browser.entries(&parent_id) {
        Ok(entries) => Json(entries).into_response(),
        Err(error) => resource_error(error),
    }
}

async fn upload_client_resource(
    State(state): State<WebState>,
    Query(query): Query<UploadResourceQuery>,
    body: Body,
) -> Response {
    let display_name = query.name.trim();
    if display_name.is_empty() {
        return resource_error(ResourceError::InvalidRequest(
            "upload name is required".into(),
        ));
    }
    if let Err(error) = tokio::fs::create_dir_all(&state.resource_upload_root).await {
        return resource_error(ResourceError::Backend(error.to_string()));
    }
    let serial = CLIENT_UPLOAD_SERIAL.fetch_add(1, Ordering::Relaxed);
    let stage = state
        .resource_upload_root
        .join(format!(".client-upload-{}-{serial}", std::process::id()));
    let mut file = match tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&stage)
        .await
    {
        Ok(file) => file,
        Err(error) => return resource_error(ResourceError::Backend(error.to_string())),
    };
    let mut stream = body.into_data_stream();
    let mut size = 0_u64;
    let streamed = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| error.to_string())?;
            size = size
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| "upload size overflow".to_owned())?;
            if size > MAX_CLIENT_UPLOAD_BYTES {
                return Err(format!(
                    "uploaded file exceeds the {} byte limit",
                    MAX_CLIENT_UPLOAD_BYTES
                ));
            }
            file.write_all(&chunk)
                .await
                .map_err(|error| error.to_string())?;
        }
        if size == 0 {
            return Err("uploaded file is empty".to_owned());
        }
        file.flush().await.map_err(|error| error.to_string())?;
        file.sync_all().await.map_err(|error| error.to_string())
    }
    .await;
    drop(file);
    if let Err(message) = streamed {
        let _ = tokio::fs::remove_file(&stage).await;
        return resource_error(ResourceError::InvalidRequest(message));
    }
    match state.resource_browser.register_client_upload_file(
        display_name,
        &stage,
        &state.resource_upload_root,
    ) {
        Ok(selection) => Json(selection).into_response(),
        Err(error) => {
            let _ = tokio::fs::remove_file(&stage).await;
            resource_error(error)
        }
    }
}

async fn select_host_resource(
    State(state): State<WebState>,
    Json(request): Json<SelectHostEntryRequest>,
) -> Response {
    match state.resource_browser.select_host_entry(&request.entry_id) {
        Ok(selection) => Json(selection).into_response(),
        Err(error) => resource_error(error),
    }
}

async fn pick_native_resource(
    State(state): State<WebState>,
    Json(request): Json<NativeResourcePickRequest>,
) -> Response {
    #[cfg(target_os = "windows")]
    {
        let picked = match tokio::task::spawn_blocking(move || {
            let mut dialog = rfd::FileDialog::new().set_title(match request.kind {
                ResourceEntryKind::File => "Choose a file",
                ResourceEntryKind::Directory => "Choose a folder",
            });
            if matches!(request.kind, ResourceEntryKind::File) && !request.extensions.is_empty() {
                let extensions = request
                    .extensions
                    .iter()
                    .map(|extension| extension.trim().trim_start_matches('.'))
                    .filter(|extension| {
                        !extension.is_empty()
                            && extension
                                .chars()
                                .all(|character| character.is_ascii_alphanumeric())
                    })
                    .collect::<Vec<_>>();
                if !extensions.is_empty() {
                    dialog = dialog.add_filter("Supported files", &extensions);
                }
            }
            match request.kind {
                ResourceEntryKind::File => dialog.pick_file(),
                ResourceEntryKind::Directory => dialog.pick_folder(),
            }
        })
        .await
        {
            Ok(Some(path)) => path,
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"status":"cancelled", "message":"Resource selection was cancelled."})),
                )
                    .into_response();
            }
            Err(error) => return internal_error(error),
        };
        match state.resource_browser.register_native_selection(picked) {
            Ok(selection) => Json(selection).into_response(),
            Err(error) => resource_error(error),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (state, request);
        resource_error(ResourceError::Backend(
            "The native resource picker is unavailable on this Desktop build".into(),
        ))
    }
}

async fn read_native_text_file(Json(request): Json<NativeTextReadRequest>) -> Response {
    #[cfg(target_os = "windows")]
    {
        let maximum = request.maximum_bytes.clamp(1, MAX_PORTABLE_TEXT_BYTES);
        match tokio::task::spawn_blocking(move || -> Result<Value, String> {
            let mut dialog = rfd::FileDialog::new().set_title("Import RackForge preset");
            let extensions = request
                .extensions
                .iter()
                .map(|extension| extension.trim().trim_start_matches('.'))
                .filter(|extension| {
                    !extension.is_empty() && extension.chars().all(|c| c.is_ascii_alphanumeric())
                })
                .collect::<Vec<_>>();
            if !extensions.is_empty() {
                dialog = dialog.add_filter("RackForge preset", &extensions);
            }
            let path = dialog
                .pick_file()
                .ok_or_else(|| "File selection was cancelled.".to_owned())?;
            let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.len() == 0
                || metadata.len() > maximum as u64
            {
                return Err("Selected preset has an invalid size or file type.".into());
            }
            let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
            Ok(json!({
                "file_name": path.file_name().unwrap_or_default().to_string_lossy(),
                "text": text,
            }))
        })
        .await
        {
            Ok(Ok(value)) => Json(value).into_response(),
            Ok(Err(message)) => {
                (StatusCode::BAD_REQUEST, Json(json!({"message": message}))).into_response()
            }
            Err(error) => internal_error(error),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = request;
        resource_error(ResourceError::Backend(
            "Native preset import is unavailable.".into(),
        ))
    }
}

async fn write_native_text_file(Json(request): Json<NativeTextWriteRequest>) -> Response {
    #[cfg(target_os = "windows")]
    {
        if request.text.is_empty() || request.text.len() > MAX_PORTABLE_TEXT_BYTES {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"message":"Preset file has an invalid size."})),
            )
                .into_response();
        }
        if request.file_name.contains(['/', '\\']) || !request.file_name.ends_with(".rfpreset") {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"message":"Preset file name is invalid."})),
            )
                .into_response();
        }
        let result = tokio::task::spawn_blocking(move || -> Result<Value, String> {
            let path = rfd::FileDialog::new()
                .set_title("Export RackForge preset")
                .set_file_name(&request.file_name)
                .add_filter("RackForge preset", &["rfpreset"])
                .save_file()
                .ok_or_else(|| "Preset export was cancelled.".to_owned())?;
            std::fs::write(&path, request.text.as_bytes()).map_err(|error| error.to_string())?;
            Ok(json!({"saved": true, "path": path.to_string_lossy(), "mime_type": request.mime_type}))
        }).await;
        match result {
            Ok(Ok(value)) => Json(value).into_response(),
            Ok(Err(message)) => {
                (StatusCode::BAD_REQUEST, Json(json!({"message": message}))).into_response()
            }
            Err(error) => internal_error(error),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = request;
        resource_error(ResourceError::Backend(
            "Native preset export is unavailable.".into(),
        ))
    }
}

async fn release_resource_selection(
    State(state): State<WebState>,
    Json(request): Json<InstallSelectedPluginRequest>,
) -> Response {
    match state
        .resource_browser
        .release_selection(request.selection_id.trim())
    {
        Ok(()) => Json(json!({"status": "released"})).into_response(),
        Err(ResourceError::UnknownHandle) => Json(json!({"status": "released"})).into_response(),
        Err(error) => resource_error(error),
    }
}

fn plugin_inspection_response(selection_id: &str, inspection: &LocalPackageInspection) -> Value {
    let branding = inspection.branding.as_ref().map(|branding| {
        json!({
            "banner_data_url": format!(
                "data:image/png;base64,{}",
                STANDARD.encode(&branding.banner_png)
            ),
            "background_color": branding.background_color,
            "accent_color": branding.accent_color,
        })
    });
    json!({
        "selection_id": selection_id,
        "plugin_id": inspection.plugin_id,
        "plugin_name": inspection.plugin_name,
        "vendor": inspection.vendor,
        "version": inspection.version,
        "description": inspection.description,
        "kind": inspection.kind,
        "platform": inspection.platform,
        "portable": inspection.portable,
        "archive_bytes": inspection.archive_bytes,
        "branding": branding,
    })
}

async fn inspect_selected_plugin(
    State(state): State<WebState>,
    Json(request): Json<InstallSelectedPluginRequest>,
) -> Response {
    let selection_id = request.selection_id.trim().to_owned();
    if selection_id.is_empty() {
        return resource_error(ResourceError::InvalidRequest(
            "selection_id is required".into(),
        ));
    }
    let Some(store_root) = state.plugin_store_root.clone() else {
        return resource_error(ResourceError::Backend(
            "Desktop plugin storage is unavailable".into(),
        ));
    };
    let path = match state.resource_browser.resolve_selection_file(&selection_id) {
        Ok(path) => path,
        Err(error) => return resource_error(error),
    };
    if fs::metadata(&path).map_or(true, |metadata| metadata.len() > MAX_PACKAGE_BYTES) {
        return resource_error(ResourceError::InvalidRequest(
            "plugin package exceeds RackForge's 512 MB limit".into(),
        ));
    }
    match tokio::task::spawn_blocking(move || {
        fs::read(path)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| inspect_local_archive(store_root, &bytes).map_err(Into::into))
    })
    .await
    {
        Ok(Ok(inspection)) => {
            Json(plugin_inspection_response(&selection_id, &inspection)).into_response()
        }
        Ok(Err(error)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"status":"error", "message":error.to_string()})),
        )
            .into_response(),
        Err(error) => internal_error(error),
    }
}

async fn install_selected_plugin(
    State(state): State<WebState>,
    Json(request): Json<InstallSelectedPluginRequest>,
) -> Response {
    let selection_id = request.selection_id.trim().to_owned();
    if selection_id.is_empty() {
        return resource_error(ResourceError::InvalidRequest(
            "selection_id is required".into(),
        ));
    }
    let Some(store_root) = state.plugin_store_root.clone() else {
        return resource_error(ResourceError::Backend(
            "Desktop plugin storage is unavailable".into(),
        ));
    };
    let selection = match state.resource_browser.consume_selection_file(&selection_id) {
        Ok(selection) => selection,
        Err(error) => return resource_error(error),
    };
    if fs::metadata(selection.path()).map_or(true, |metadata| metadata.len() > MAX_PACKAGE_BYTES) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status":"error",
                "message":"plugin package exceeds RackForge's 512 MB limit"
            })),
        )
            .into_response();
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    plugin_install_cancellations()
        .lock()
        .expect("plugin install cancellation lock poisoned")
        .insert(selection_id.clone(), cancelled.clone());
    let worker_cancelled = cancelled.clone();
    let result = tokio::task::spawn_blocking(move || {
        fs::read(selection.path())
            .map_err(anyhow::Error::from)
            .and_then(|bytes| {
                install_local_archive_cancellable(store_root, &bytes, &worker_cancelled)
                    .map_err(Into::into)
            })
    })
    .await;
    plugin_install_cancellations()
        .lock()
        .expect("plugin install cancellation lock poisoned")
        .remove(&selection_id);
    match result {
        Ok(Ok(installed)) => {
            state.plugin_catalog_revision.fetch_add(1, Ordering::AcqRel);
            state.package_scan_revision.fetch_add(1, Ordering::AcqRel);
            {
                let warm_state = state.clone();
                tokio::task::spawn_blocking(move || {
                    let _ = discover_web_packages(&warm_state);
                });
            }
            Json(json!({
                "plugin_id": installed.record.plugin_id,
                "version": installed.record.version,
                "already_installed": installed.already_installed,
                "activation_required": true,
            }))
            .into_response()
        }
        Ok(Err(error)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"status":"error", "message":error.to_string()})),
        )
            .into_response(),
        Err(error) => internal_error(error),
    }
}

async fn cancel_selected_plugin_install(
    Json(request): Json<InstallSelectedPluginRequest>,
) -> Response {
    let selection_id = request.selection_id.trim();
    if selection_id.is_empty() {
        return resource_error(ResourceError::InvalidRequest(
            "selection_id is required".into(),
        ));
    }
    let requested = plugin_install_cancellations()
        .lock()
        .expect("plugin install cancellation lock poisoned")
        .get(selection_id)
        .map(|cancelled| {
            cancelled.store(true, Ordering::Release);
            true
        })
        .unwrap_or(false);
    Json(json!({
        "status": if requested { "cancellation_requested" } else { "not_running" },
        "selection_id": selection_id,
    }))
    .into_response()
}

async fn bind_resource(
    State(state): State<WebState>,
    Json(request): Json<BindResourceRequest>,
) -> Response {
    let packages = match discover_web_packages(&state) {
        Ok(packages) => packages,
        Err(error) => return internal_error(error),
    };
    let Some(package) = packages.get(&request.plugin_id) else {
        return resource_error(ResourceError::InvalidRequest(
            "plugin is not installed".into(),
        ));
    };
    let Some(requirement) = package
        .public
        .resources
        .iter()
        .find(|resource| resource.id == request.resource_id)
    else {
        return resource_error(ResourceError::InvalidRequest(
            "resource is not declared by this plugin".into(),
        ));
    };
    match state.resource_browser.bind(&request) {
        Ok(grant)
            if matches!(
                (requirement.kind, grant.kind),
                (
                    rackforge_plugin_api::ResourceKind::File,
                    ResourceEntryKind::File
                ) | (
                    rackforge_plugin_api::ResourceKind::Directory,
                    ResourceEntryKind::Directory
                )
            ) =>
        {
            Json(grant).into_response()
        }
        Ok(_) => resource_error(ResourceError::InvalidRequest(
            "selected entry has the wrong resource kind".into(),
        )),
        Err(error) => resource_error(error),
    }
}

async fn bind_resource_selection(
    State(state): State<WebState>,
    Json(request): Json<BindSelectionRequest>,
) -> Response {
    let packages = match discover_web_packages(&state) {
        Ok(packages) => packages,
        Err(error) => return internal_error(error),
    };
    let Some(package) = packages.get(&request.plugin_id) else {
        return resource_error(ResourceError::InvalidRequest(
            "plugin is not installed".into(),
        ));
    };
    let Some(requirement) = package
        .public
        .resources
        .iter()
        .find(|resource| resource.id == request.resource_id)
    else {
        return resource_error(ResourceError::InvalidRequest(
            "resource is not declared by this plugin".into(),
        ));
    };
    let expected_kind = match requirement.kind {
        rackforge_plugin_api::ResourceKind::File => ResourceEntryKind::File,
        rackforge_plugin_api::ResourceKind::Directory => ResourceEntryKind::Directory,
    };
    match state
        .resource_browser
        .bind_selection(&request, expected_kind)
    {
        Ok(grant) => Json(grant).into_response(),
        Err(error) => resource_error(error),
    }
}

async fn resource_grants(
    State(state): State<WebState>,
    Json(request): Json<ListGrantsRequest>,
) -> Response {
    if !plugin_is_installed(&state, &request.plugin_id) {
        return resource_error(ResourceError::InvalidRequest(
            "plugin is not installed".into(),
        ));
    }
    match state.resource_browser.grants(&request.plugin_id) {
        Ok(grants) => Json(grants).into_response(),
        Err(error) => resource_error(error),
    }
}

async fn resource_status(
    State(state): State<WebState>,
    Json(request): Json<ListGrantsRequest>,
) -> Response {
    let packages = match discover_web_packages(&state) {
        Ok(packages) => packages,
        Err(error) => return internal_error(error),
    };
    let Some(package) = packages.get(&request.plugin_id) else {
        return resource_error(ResourceError::InvalidRequest(
            "plugin is not installed".into(),
        ));
    };
    Json(resource_install_statuses(&package.public, &state.data_root)).into_response()
}

fn resource_install_statuses(package: &PublicPluginWeb, data_root: &Path) -> Vec<Value> {
    let plugin_root = fs::canonicalize(data_root.join("plugins").join(&package.plugin_id)).ok();
    package
        .resources
        .iter()
        .filter_map(|resource| {
            let relative = resource.data_path.as_deref()?;
            let installed = plugin_root.as_ref().is_some_and(|root| {
                let candidate = root.join(relative);
                fs::symlink_metadata(&candidate).is_ok_and(|metadata| {
                    !metadata.file_type().is_symlink()
                        && metadata.is_file()
                        && fs::canonicalize(&candidate).is_ok_and(|path| path.starts_with(root))
                })
            });
            Some(json!({
                "resource_id": resource.id,
                "installed": installed,
            }))
        })
        .collect()
}

async fn browse_resource_grant(
    State(state): State<WebState>,
    Json(request): Json<BrowseGrantRequest>,
) -> Response {
    if !plugin_is_installed(&state, &request.plugin_id) {
        return resource_error(ResourceError::InvalidRequest(
            "plugin is not installed".into(),
        ));
    }
    match state.resource_browser.grant_entries(&request) {
        Ok(entries) => Json(entries).into_response(),
        Err(error) => resource_error(error),
    }
}

async fn load_granted_resource(
    State(state): State<WebState>,
    Json(request): Json<LoadGrantedResourceRequest>,
) -> Response {
    if request.persist && request.preview {
        return resource_error(ResourceError::InvalidRequest(
            "a resource preview cannot be persisted".into(),
        ));
    }
    let packages = match discover_web_packages(&state) {
        Ok(packages) => packages,
        Err(error) => return internal_error(error),
    };
    let Some(package) = packages.get(&request.plugin_id) else {
        return resource_error(ResourceError::InvalidRequest(
            "plugin is not installed".into(),
        ));
    };
    let owns_instance = state.session.read().is_ok_and(|session| {
        session.instances.iter().any(|instance| {
            instance.instance_id.as_str() == request.instance_id
                && instance.plugin_id == request.plugin_id
        })
    });
    if !owns_instance {
        return resource_error(ResourceError::InvalidRequest(
            "plugin does not own this instance".into(),
        ));
    }
    let valid_target = package.public.resources.iter().any(|resource| {
        resource.id == request.target_resource_id
            && resource.kind == rackforge_plugin_api::ResourceKind::File
    });
    if !valid_target {
        return resource_error(ResourceError::InvalidRequest(
            "target is not a declared file resource".into(),
        ));
    }
    let bundled;
    let path = match request.bundle {
        Some(ResourceBundleKind::NkiDependencies) => {
            match state.resource_browser.bundle_granted_nki(
                &request.plugin_id,
                &request.grant_id,
                request.entry_id.as_deref(),
            ) {
                Ok(staged) => {
                    let path = staged.path().to_path_buf();
                    bundled = Some(staged);
                    path
                }
                Err(error) => return resource_error(error),
            }
        }
        Some(ResourceBundleKind::SfzDependencies) => {
            match state.resource_browser.bundle_granted_sfz(
                &request.plugin_id,
                &request.grant_id,
                request.entry_id.as_deref(),
            ) {
                Ok(staged) => {
                    let path = staged.path().to_path_buf();
                    bundled = Some(staged);
                    path
                }
                Err(error) => return resource_error(error),
            }
        }
        None => match state.resource_browser.resolve_granted_file(
            &request.plugin_id,
            &request.grant_id,
            request.entry_id.as_deref(),
        ) {
            Ok(path) => {
                bundled = None;
                path
            }
            Err(error) => return resource_error(error),
        },
    };
    let uploaded_grant = (request.persist || request.preview)
        .then(|| (request.plugin_id.clone(), request.grant_id.clone()));
    let (response_sender, response_receiver) = mpsc::channel();
    if state
        .control
        .send(DesktopControlCall::LoadResource {
            plugin_id: request.plugin_id,
            resource_id: request.target_resource_id,
            path,
            persist: request.persist,
            preview: request.preview,
            response: response_sender,
        })
        .is_err()
    {
        return resource_error(ResourceError::Backend(
            "Desktop runtime is shutting down".into(),
        ));
    }
    let response = match tokio::task::spawn_blocking(move || {
        response_receiver.recv_timeout(Duration::from_secs(30))
    })
    .await
    {
        Ok(Ok(Ok(()))) => Json(json!({"status":"ok"})).into_response(),
        Ok(Ok(Err(message))) => resource_error(ResourceError::Backend(message)),
        _ => resource_error(ResourceError::Backend(
            "Desktop runtime did not finish loading the resource".into(),
        )),
    };
    if let Some((plugin_id, grant_id)) = uploaded_grant {
        let _ = state
            .resource_browser
            .release_owned_grant(&plugin_id, &grant_id);
    }
    drop(bundled);
    response
}

async fn clear_installed_resource(
    State(state): State<WebState>,
    Json(request): Json<ClearInstalledResourceRequest>,
) -> Response {
    let packages = match discover_web_packages(&state) {
        Ok(packages) => packages,
        Err(error) => return internal_error(error),
    };
    let Some(package) = packages.get(&request.plugin_id) else {
        return resource_error(ResourceError::InvalidRequest(
            "plugin is not installed".into(),
        ));
    };
    let owns_instance = state.session.read().is_ok_and(|session| {
        session.instances.iter().any(|instance| {
            instance.instance_id.as_str() == request.instance_id
                && instance.plugin_id == request.plugin_id
        })
    });
    if !owns_instance {
        return resource_error(ResourceError::InvalidRequest(
            "plugin does not own this instance".into(),
        ));
    }
    let valid_target = package.public.resources.iter().any(|resource| {
        resource.id == request.target_resource_id
            && resource.kind == rackforge_plugin_api::ResourceKind::File
            && resource.data_path.is_some()
    });
    if !valid_target {
        return resource_error(ResourceError::InvalidRequest(
            "target is not a declared installable file resource".into(),
        ));
    }
    let (response_sender, response_receiver) = mpsc::channel();
    if state
        .control
        .send(DesktopControlCall::ClearResource {
            plugin_id: request.plugin_id,
            resource_id: request.target_resource_id,
            response: response_sender,
        })
        .is_err()
    {
        return resource_error(ResourceError::Backend(
            "Desktop runtime is shutting down".into(),
        ));
    }
    match tokio::task::spawn_blocking(move || {
        response_receiver.recv_timeout(Duration::from_secs(30))
    })
    .await
    {
        Ok(Ok(Ok(()))) => Json(json!({"status":"ok"})).into_response(),
        Ok(Ok(Err(message))) => resource_error(ResourceError::Backend(message)),
        _ => resource_error(ResourceError::Backend(
            "Desktop runtime did not finish clearing the resource".into(),
        )),
    }
}

fn plugin_is_installed(state: &WebState, plugin_id: &str) -> bool {
    discover_web_packages(state).is_ok_and(|packages| packages.contains_key(plugin_id))
}

fn resource_error(error: ResourceError) -> Response {
    let status = match &error {
        ResourceError::UnknownHandle => StatusCode::NOT_FOUND,
        ResourceError::OutsideMount
        | ResourceError::NotDirectory
        | ResourceError::Unreadable
        | ResourceError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
        ResourceError::Backend(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(json!({"status":"error", "message":error.to_string()})),
    )
        .into_response()
}

/// The installed controller packages: the plugins tab lists these beside
/// the instruments. Read-only for now; enable/disable and configuration
/// arrive with the settings schema (see docs/architecture/controller-plugins.md).
async fn controller_catalog(State(state): State<WebState>) -> Response {
    let store = rackforge_controller_package::PackageStore::new(&state.controllers_root);
    let installed = match store.list() {
        Ok(installed) => installed,
        Err(error) => return internal_error(error),
    };
    let controllers: Vec<serde_json::Value> = installed
        .iter()
        .map(|controller| {
            let manifest = controller.package.manifest();
            let stored = read_controller_settings(&state, &controller.record.id);
            let settings: Vec<serde_json::Value> = manifest
                .settings
                .iter()
                .map(|setting| {
                    json!({
                        "id": setting.id,
                        "name": setting.name,
                        "kind": format!("{:?}", setting.kind).to_ascii_lowercase(),
                        "default": setting.default,
                        "page": setting.page,
                        "value": stored
                            .get(&setting.id)
                            .cloned()
                            .unwrap_or_else(|| setting.default.clone()),
                    })
                })
                .collect();
            json!({
                "id": controller.record.id,
                "name": manifest.name,
                "version": controller.record.version,
                "enabled": controller.record.enabled,
                "trust": format!("{:?}", controller.record.trust).to_ascii_lowercase(),
                "runtime": format!("{:?}", manifest.runtime.kind),
                "devices": manifest.devices.len(),
                "settings": settings,
            })
        })
        .collect();
    Json(json!({"status": "ok", "controllers": controllers})).into_response()
}

fn controller_settings_path(state: &WebState, controller_id: &str) -> PathBuf {
    state
        .controllers_root
        .join("state")
        .join(controller_id)
        .join("settings.toml")
}

fn read_controller_settings(state: &WebState, controller_id: &str) -> BTreeMap<String, String> {
    fs::read_to_string(controller_settings_path(state, controller_id))
        .ok()
        .and_then(|text| toml::from_str::<BTreeMap<String, String>>(&text).ok())
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct ControllerSettingsRequest {
    values: BTreeMap<String, String>,
}

/// Persists user values for a controller's declared settings. Every key must
/// exist in the manifest and every value must satisfy its kind; the driver
/// watches the file and applies the change to the hardware within a second.
async fn apply_controller_settings(
    AxumPath(controller_id): AxumPath<String>,
    State(state): State<WebState>,
    Json(request): Json<ControllerSettingsRequest>,
) -> Response {
    let store = rackforge_controller_package::PackageStore::new(&state.controllers_root);
    let installed = match store.resolve(&controller_id) {
        Ok(installed) => installed,
        Err(error) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"status": "error", "message": error.to_string()})),
            )
                .into_response();
        }
    };
    let manifest = installed.package.manifest();
    let mut values = read_controller_settings(&state, &controller_id);
    for (id, value) in &request.values {
        let Some(setting) = manifest.settings.iter().find(|setting| &setting.id == id) else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "status": "error",
                    "message": format!("this controller declares no setting {id:?}"),
                })),
            )
                .into_response();
        };
        if let Err(error) = setting.validate_value(value) {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": error})),
            )
                .into_response();
        }
        values.insert(id.clone(), value.clone());
    }
    let path = controller_settings_path(&state, &controller_id);
    let write = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = values
            .iter()
            .map(|(key, value)| format!("{key} = {value:?}\n"))
            .collect::<String>();
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        fs::write(&temporary, body)?;
        fs::rename(&temporary, &path)?;
        Ok(())
    })();
    match write {
        Ok(()) => Json(json!({"status": "ok", "values": values})).into_response(),
        Err(error) => internal_error(error),
    }
}

async fn plugin_asset(
    AxumPath((plugin_id, asset)): AxumPath<(String, String)>,
    headers: axum::http::HeaderMap,
    State(state): State<WebState>,
) -> Response {
    let packages = match discover_web_packages(&state) {
        Ok(packages) => packages,
        Err(error) => return internal_error(error),
    };
    let Some(package) = packages.get(&plugin_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = match fs::canonicalize(package.root.join(&asset)) {
        Ok(path) if path.starts_with(&package.root) && path.is_file() => path,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    // Conditional serving instead of `no-store`. The splash is megabytes and
    // was re-downloaded on EVERY panel open; with a validator the browser
    // keeps its copy and each open costs one 304 round trip on localhost.
    // Not `immutable`, deliberately: development edits files in place
    // without bumping the version, and a year-long cache would hide them.
    let etag = match fs::metadata(&path) {
        Ok(meta) => {
            let modified = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_millis());
            format!("\"{}-{}\"", meta.len(), modified)
        }
        Err(error) => return internal_error(error),
    };
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        == Some(etag.as_str())
    {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, etag)
            .header(header::CACHE_CONTROL, "public, max-age=0, must-revalidate")
            .body(Body::empty())
            .expect("valid not-modified response");
    }
    match fs::read(&path) {
        Ok(bytes) => Response::builder()
            .header(
                header::CONTENT_TYPE,
                mime_guess::from_path(&path)
                    .first_or_octet_stream()
                    .as_ref(),
            )
            .header(header::ETAG, etag)
            .header(header::CACHE_CONTROL, "public, max-age=0, must-revalidate")
            .body(Body::from(bytes))
            .expect("valid plugin asset response"),
        Err(error) => internal_error(error),
    }
}

async fn session_socket(ws: WebSocketUpgrade, State(state): State<WebState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: axum::extract::ws::WebSocket, state: WebState) {
    let (mut sender, mut receiver) = socket.split();
    let mut virtual_midi_clients = std::collections::BTreeSet::<ClientId>::new();
    let mut published_revision = state.session.read().expect("session lock").revision;
    let mut published_catalog_revision = state.plugin_catalog_revision.load(Ordering::Acquire);
    if sender
        .send(Message::Text(snapshot_json(&state).into()))
        .await
        .is_err()
    {
        return;
    }
    loop {
        let message = match tokio::time::timeout(Duration::from_millis(100), receiver.next()).await
        {
            Ok(Some(Ok(message))) => message,
            Ok(Some(Err(_))) | Ok(None) => break,
            Err(_) => {
                let revision = state.session.read().expect("session lock").revision;
                if revision != published_revision {
                    if sender
                        .send(Message::Text(snapshot_json(&state).into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    published_revision = revision;
                }
                let catalog_revision = state.plugin_catalog_revision.load(Ordering::Acquire);
                if catalog_revision != published_catalog_revision {
                    if sender
                        .send(Message::Text(
                            json!({
                                "status":"plugin_catalog_changed",
                                "revision":catalog_revision,
                            })
                            .to_string()
                            .into(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    published_catalog_revision = catalog_revision;
                }
                continue;
            }
        };
        match message {
            Message::Text(text) => {
                let request = serde_json::from_str::<ControlRequest>(&text);
                if let Ok(request) = &request {
                    match request {
                        ControlRequest::VirtualMidi { client_id, .. } => {
                            virtual_midi_clients.insert(client_id.clone());
                        }
                        ControlRequest::ReleaseVirtualMidi { client_id } => {
                            virtual_midi_clients.remove(client_id);
                        }
                        _ => {}
                    }
                }
                let sends_snapshot = request
                    .as_ref()
                    .is_ok_and(|request| matches!(request, ControlRequest::Dispatch { .. }));
                let is_snapshot_request = request
                    .as_ref()
                    .is_ok_and(|request| matches!(request, ControlRequest::Snapshot));
                let response = request
                    .map(|request| response_for(request, &state))
                    .unwrap_or_else(
                        |error| json!({"status":"gateway_error", "message":error.to_string()}),
                    );
                if sender
                    .send(Message::Text(response.to_string().into()))
                    .await
                    .is_err()
                {
                    break;
                }
                if is_snapshot_request
                    && response.get("status").and_then(Value::as_str) == Some("snapshot")
                {
                    published_revision = state.session.read().expect("session lock").revision;
                }
                if sends_snapshot
                    && response.get("status").and_then(Value::as_str) == Some("command_applied")
                    && sender
                        .send(Message::Text(snapshot_json(&state).into()))
                        .await
                        .is_err()
                {
                    break;
                }
                if sends_snapshot
                    && response.get("status").and_then(Value::as_str) == Some("command_applied")
                {
                    published_revision = state.session.read().expect("session lock").revision;
                }
            }
            Message::Ping(bytes) => {
                if sender.send(Message::Pong(bytes)).await.is_err() {
                    break;
                }
            }
            Message::Close(frame) => {
                // Complete the WebSocket close handshake. Dropping the TCP
                // stream directly makes Chromium report an error in addition
                // to the expected close event, which used to surface as a
                // misleading Core interruption banner during UI handovers.
                let _ = sender.send(Message::Close(frame)).await;
                break;
            }
            _ => {}
        }
    }
    for client_id in virtual_midi_clients {
        let _ = response_for(ControlRequest::ReleaseVirtualMidi { client_id }, &state);
    }
}

fn response_for(request: ControlRequest, state: &WebState) -> Value {
    match request {
        ControlRequest::Snapshot => {
            serde_json::from_str(&snapshot_json(state)).expect("snapshot JSON")
        }
        request @ (ControlRequest::PerformanceSnapshot
        | ControlRequest::EditPerformance { .. }
        | ControlRequest::PluginPresets { .. }
        | ControlRequest::PluginPreset { .. }
        | ControlRequest::SavePluginPreset { .. }
        | ControlRequest::LoadPluginPreset { .. }
        | ControlRequest::RenamePluginPreset { .. }
        | ControlRequest::DeletePluginPreset { .. }
        | ControlRequest::ExportPluginPreset { .. }
        | ControlRequest::InspectPluginPreset { .. }
        | ControlRequest::ImportPluginPreset { .. }
        | ControlRequest::MaterializePluginState { .. }
        | ControlRequest::PluginParameters { .. }
        | ControlRequest::SetPluginParameter { .. }
        | ControlRequest::PluginStateParameters { .. }
        | ControlRequest::SetPluginStateParameter { .. }
        | ControlRequest::MidiSources
        | ControlRequest::BeginMidiLearn { .. }
        | ControlRequest::MidiLearnStatus { .. }
        | ControlRequest::CancelMidiLearn { .. }
        | ControlRequest::OutputMeter) => {
            let (response_sender, response_receiver) = mpsc::channel();
            if state
                .control
                .send(DesktopControlCall::Performance {
                    request,
                    response: response_sender,
                })
                .is_err()
            {
                return json!({"status":"error", "code":"unavailable", "message":"The Desktop runtime is shutting down."});
            }
            match response_receiver.recv_timeout(DESKTOP_CONTROL_TIMEOUT) {
                Ok(response) => serde_json::to_value(response).expect("control response"),
                Err(_) => {
                    json!({"status":"error", "code":"timeout", "message":"The Desktop runtime did not answer the performance request in time."})
                }
            }
        }
        ControlRequest::Events { .. } => {
            let revision = state.session.read().expect("session lock").revision;
            serde_json::to_value(ControlResponse::Events {
                current_revision: revision,
                events: Vec::new(),
            })
            .expect("events response")
        }
        request @ (ControlRequest::Dispatch { .. }
        | ControlRequest::VirtualMidi { .. }
        | ControlRequest::ReleaseVirtualMidi { .. }) => {
            // A note is not a session command: it must reach the audio thread
            // now, not on the next GUI frame. Everything else in this arm
            // still takes the ordinary route.
            if let ControlRequest::VirtualMidi { message, .. } = &request
                && let Err(error) = message.validate()
            {
                return json!({"status":"error", "code":"invalid_request", "message": error});
            }
            if let ControlRequest::VirtualMidi {
                client_id,
                source_name: None,
                message,
            } = &request
            {
                let packet = crate::desktop_audio::MidiPacket {
                    source: crate::desktop_audio::VIRTUAL_MIDI_SOURCE_KEY,
                    length: 3,
                    data: message.bytes(),
                };
                match try_injected_midi(state, packet) {
                    Ok(true) => {
                        return serde_json::to_value(ControlResponse::VirtualMidiAccepted {
                            client_id: client_id.clone(),
                            active_notes: 0,
                        })
                        .expect("virtual MIDI response");
                    }
                    Err(message) => {
                        return json!({
                            "status":"error",
                            "code":"unavailable",
                            "message": message,
                        });
                    }
                    Ok(false) => {}
                }
            }
            let (response_sender, response_receiver) = mpsc::channel();
            if state
                .control
                .send(DesktopControlCall::Session {
                    request,
                    response: response_sender,
                })
                .is_err()
            {
                return json!({"status":"error", "code":"unavailable", "message":"The Desktop runtime is shutting down."});
            }
            match response_receiver.recv_timeout(DESKTOP_CONTROL_TIMEOUT) {
                Ok(response) => serde_json::to_value(response).expect("control response"),
                Err(_) => {
                    json!({"status":"error", "code":"timeout", "message":"The Desktop runtime did not answer the command in time."})
                }
            }
        }
        _ => {
            json!({"status":"error", "code":"unavailable", "message":"This operation is not connected to the Desktop runtime yet."})
        }
    }
}

fn discover_web_packages(state: &WebState) -> anyhow::Result<BTreeMap<String, PluginWebPackage>> {
    let active = state
        .session
        .read()
        .expect("session lock poisoned")
        .instances
        .iter()
        .map(|instance| instance.plugin_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let revision = state.package_scan_revision.load(Ordering::Acquire);
    if let Some((cached_revision, cached)) = state
        .web_packages_cache
        .lock()
        .expect("web package cache")
        .as_ref()
        && *cached_revision == revision
    {
        let mut packages = cached.clone();
        for package in packages.values_mut() {
            package.public.active = if package.public.managed {
                state.plugin_store_root.as_ref().is_some_and(|store| {
                    rackforge_repository::plugin_is_enabled(store, &package.public.plugin_id)
                        .unwrap_or(false)
                })
            } else {
                active.contains(&package.public.plugin_id)
            };
        }
        return Ok(packages);
    }
    let mut roots = crate::direct_package_roots(&state.legacy_plugins_root)?;
    if let Some(store_root) = state.plugin_store_root.as_deref() {
        roots.extend(crate::all_versioned_package_roots(store_root)?);
    }
    let mut packages = BTreeMap::<String, (Version, PluginWebPackage)>::new();
    for root in roots {
        let package = match PluginPackage::open(&root) {
            Ok(package) => package,
            Err(_) => continue,
        };
        let manifest = package.manifest();
        let version = match Version::parse(&manifest.version) {
            Ok(version) => version,
            Err(_) => continue,
        };
        let root = fs::canonicalize(package.root())?;
        let mut surfaces = Vec::new();
        if let Some(web_ui) = &manifest.web_ui {
            for surface in &web_ui.surfaces {
                let entry = fs::canonicalize(root.join(&surface.entry))?;
                if !entry.starts_with(&root) || !entry.is_file() {
                    anyhow::bail!("plugin web entry escapes package: {:?}", surface.entry);
                }
                surfaces.push(PublicWebSurface {
                    kind: surface.kind,
                    entry_url: format!(
                        "/plugin-assets/{}/{}?v={}",
                        manifest.id,
                        surface.entry.replace('\\', "/"),
                        manifest.version,
                    ),
                });
            }
        }
        let managed = state.plugin_store_root.as_ref().is_some_and(|store| {
            fs::canonicalize(store.join("packages"))
                .is_ok_and(|packages| root.starts_with(&packages) && root != packages)
        });
        let candidate = PluginWebPackage {
            root,
            public: PublicPluginWeb {
                plugin_id: manifest.id.clone(),
                plugin_name: manifest.name.clone(),
                version: manifest.version.clone(),
                kind: manifest.kind,
                active: if managed {
                    state.plugin_store_root.as_ref().is_some_and(|store| {
                        rackforge_repository::plugin_is_enabled(store, &manifest.id)
                            .unwrap_or(false)
                    })
                } else {
                    active.contains(&manifest.id)
                },
                managed,
                api_version: manifest.web_ui.as_ref().map_or(0, |web| web.api_version),
                branding: manifest
                    .branding
                    .as_ref()
                    .map(|branding| PublicPluginBranding {
                        icon_url: plugin_asset_url(&manifest.id, &branding.icon, &manifest.version),
                        banner_url: plugin_asset_url(
                            &manifest.id,
                            &branding.banner,
                            &manifest.version,
                        ),
                        splash_url: plugin_asset_url(
                            &manifest.id,
                            &branding.splash,
                            &manifest.version,
                        ),
                        background_color: branding.background_color.clone(),
                        accent_color: branding.accent_color.clone(),
                    }),
                surfaces,
                resources: manifest.resources.clone(),
            },
        };
        let replace = packages
            .get(&manifest.id)
            .is_none_or(|(current, _)| version >= *current);
        if replace {
            packages.insert(manifest.id.clone(), (version, candidate));
        }
    }
    let packages: BTreeMap<String, PluginWebPackage> = packages
        .into_iter()
        .map(|(id, (_, package))| (id, package))
        .collect();
    *state.web_packages_cache.lock().expect("web package cache") =
        Some((revision, packages.clone()));
    Ok(packages)
}

fn plugin_asset_url(plugin_id: &str, asset: &str, version: &str) -> String {
    format!(
        "/plugin-assets/{plugin_id}/{}?v={version}",
        asset.replace('\\', "/")
    )
}

fn internal_error(error: impl std::fmt::Display) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"status":"error", "message":error.to_string()})),
    )
        .into_response()
}

fn snapshot_json(state: &WebState) -> String {
    serde_json::to_string(&ControlResponse::Snapshot {
        snapshot: Box::new(state.session.read().expect("session lock").clone()),
    })
    .expect("serialize desktop snapshot")
}

async fn static_asset(uri: axum::http::Uri) -> Response {
    let requested = uri.path().trim_start_matches('/');
    let path = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };
    let asset = WEB_ASSETS
        .get_file(path)
        .or_else(|| WEB_ASSETS.get_file("index.html"));
    let Some(asset) = asset else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = mime_guess::from_path(asset.path()).first_or_octet_stream();
    // Vite writes content-hashed filenames under assets/, so those can be
    // cached forever; index.html is the one entry that must stay fresh.
    let cache_control = if asset.path().starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, cache_control)
        .body(Body::from(asset.contents().to_vec()))
        .expect("valid embedded asset response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_control_api::ControlErrorCode;
    use rackforge_session_api::{
        ClientId, CommandEnvelope, DEFAULT_LIVE_INSTANCE_ID, DEFAULT_LIVE_SESSION_ID, InstanceId,
        Revision, SessionCommand, SessionId,
    };
    use rackforge_surface_api::SurfaceMode;

    #[test]
    fn network_http_server_is_disabled_by_default() {
        let preferences = WebServerPreferences::default();
        assert!(!preferences.enabled);
        assert_eq!(preferences.port, 8787);
        preferences.validate().unwrap();
    }

    #[test]
    fn web_server_preferences_round_trip() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-desktop-web-settings-{}",
            std::process::id()
        ));
        let path = root.join("config/web.toml");
        let preferences = WebServerPreferences {
            enabled: true,
            port: 9123,
            ..WebServerPreferences::default()
        };
        preferences.persist(&path).unwrap();
        assert_eq!(
            WebServerPreferences::load(&path).unwrap(),
            Some(preferences)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn privileged_http_ports_are_rejected() {
        let preferences = WebServerPreferences {
            port: 80,
            ..WebServerPreferences::default()
        };
        assert!(preferences.validate().is_err());
    }

    #[test]
    fn surface_midi_discards_a_retired_audio_generation_and_accepts_the_next_one() {
        let (control, _receiver) = control_channel();
        let state = WebState {
            session: Arc::new(RwLock::new(SessionState::new(
                SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            ))),
            plugin_catalog_revision: Arc::new(AtomicU64::new(0)),
            legacy_plugins_root: PathBuf::new(),
            plugin_store_root: None,
            data_root: PathBuf::new(),
            public_server: Arc::new(RwLock::new(WebServerPreferences::default())),
            control,
            resource_browser: Arc::new(NativeResourceBrowser::new([]).unwrap()),
            resource_upload_root: PathBuf::new(),
            web_packages_cache: Arc::new(Mutex::new(None)),
            package_scan_revision: Arc::new(AtomicU64::new(0)),
            controllers_root: PathBuf::new(),
            injected_midi: Arc::new(Mutex::new(None)),
        };
        let packet = crate::desktop_audio::MidiPacket {
            source: crate::desktop_audio::VIRTUAL_MIDI_SOURCE_KEY,
            length: 3,
            data: [0x90, 60, 100],
        };

        let (retired_sender, retired_receiver) = mpsc::sync_channel(1);
        *state.injected_midi.lock().unwrap() = Some(retired_sender);
        drop(retired_receiver);
        assert!(!try_injected_midi(&state, packet).unwrap());
        assert!(state.injected_midi.lock().unwrap().is_none());

        let (current_sender, current_receiver) = mpsc::sync_channel(1);
        *state.injected_midi.lock().unwrap() = Some(current_sender);
        assert!(try_injected_midi(&state, packet).unwrap());
        let delivered = current_receiver.recv().unwrap();
        assert_eq!(delivered.length, packet.length);
        assert_eq!(delivered.data, packet.data);
    }

    #[test]
    fn forwarded_physical_midi_bypasses_the_touch_fast_path() {
        let (control, receiver) = control_channel();
        let (audio_sender, audio_receiver) = mpsc::sync_channel(1);
        let state = WebState {
            session: Arc::new(RwLock::new(SessionState::new(
                SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            ))),
            plugin_catalog_revision: Arc::new(AtomicU64::new(0)),
            legacy_plugins_root: PathBuf::new(),
            plugin_store_root: None,
            data_root: PathBuf::new(),
            public_server: Arc::new(RwLock::new(WebServerPreferences::default())),
            control,
            resource_browser: Arc::new(NativeResourceBrowser::new([]).unwrap()),
            resource_upload_root: PathBuf::new(),
            web_packages_cache: Arc::new(Mutex::new(None)),
            package_scan_revision: Arc::new(AtomicU64::new(0)),
            controllers_root: PathBuf::new(),
            injected_midi: Arc::new(Mutex::new(Some(audio_sender))),
        };
        let client_id = ClientId::new("controller.test.forwarder").unwrap();
        let request = ControlRequest::VirtualMidi {
            client_id: client_id.clone(),
            source_name: Some("Enabled Controller".into()),
            message: rackforge_control_api::VirtualMidiMessage {
                status: 0xb0,
                data1: 74,
                data2: 96,
            },
        };
        let responder = std::thread::spawn(move || {
            let DesktopControlCall::Session { request, response } = receiver.recv().unwrap() else {
                panic!("forwarded physical MIDI must reach the Desktop host");
            };
            assert!(matches!(
                request,
                ControlRequest::VirtualMidi {
                    source_name: Some(ref name),
                    ..
                } if name == "Enabled Controller"
            ));
            response
                .send(ControlResponse::VirtualMidiAccepted {
                    client_id,
                    active_notes: 0,
                })
                .unwrap();
        });

        let response = response_for(request, &state);
        responder.join().unwrap();
        assert_eq!(
            response.get("status").and_then(Value::as_str),
            Some("virtual_midi_accepted")
        );
        assert!(audio_receiver.try_recv().is_err());
    }

    #[test]
    fn dispatches_session_changes_to_the_desktop_runtime() {
        let (control, receiver) = control_channel();
        let state = WebState {
            session: Arc::new(RwLock::new(SessionState::new(
                SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            ))),
            plugin_catalog_revision: Arc::new(AtomicU64::new(0)),
            legacy_plugins_root: PathBuf::new(),
            plugin_store_root: None,
            data_root: PathBuf::new(),
            public_server: Arc::new(RwLock::new(WebServerPreferences::default())),
            control,
            resource_browser: Arc::new(NativeResourceBrowser::new([]).unwrap()),
            resource_upload_root: PathBuf::new(),
            web_packages_cache: Arc::new(Mutex::new(None)),
            package_scan_revision: Arc::new(AtomicU64::new(0)),
            controllers_root: PathBuf::new(),
            injected_midi: Arc::new(Mutex::new(None)),
        };
        let instance_id = InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).unwrap();
        let request = ControlRequest::Dispatch {
            envelope: CommandEnvelope::new(
                ClientId::new("test.desktop-web").unwrap(),
                7,
                SessionCommand::SelectSound {
                    instance_id: instance_id.clone(),
                    sound_id: "xp10.piano".into(),
                },
            ),
        };
        let responder = std::thread::spawn(move || {
            let call = receiver.recv().unwrap();
            let DesktopControlCall::Session { request, response } = call else {
                panic!("unexpected Desktop control call");
            };
            assert!(matches!(
                request,
                ControlRequest::Dispatch {
                    envelope: CommandEnvelope {
                        command: SessionCommand::SelectSound { .. },
                        ..
                    }
                }
            ));
            response
                .send(ControlResponse::CommandApplied {
                    client_id: ClientId::new("test.desktop-web").unwrap(),
                    command_id: 7,
                    revision: Revision::new(1),
                    events: Vec::new(),
                })
                .unwrap();
        });
        let response = response_for(request, &state);
        responder.join().unwrap();
        assert_eq!(
            response.get("status").and_then(Value::as_str),
            Some("command_applied")
        );
    }

    #[test]
    fn dispatches_play_mode_changes_to_the_desktop_runtime() {
        let (control, receiver) = control_channel();
        let state = WebState {
            session: Arc::new(RwLock::new(SessionState::new(
                SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            ))),
            plugin_catalog_revision: Arc::new(AtomicU64::new(0)),
            legacy_plugins_root: PathBuf::new(),
            plugin_store_root: None,
            data_root: PathBuf::new(),
            public_server: Arc::new(RwLock::new(WebServerPreferences::default())),
            control,
            resource_browser: Arc::new(NativeResourceBrowser::new([]).unwrap()),
            resource_upload_root: PathBuf::new(),
            web_packages_cache: Arc::new(Mutex::new(None)),
            package_scan_revision: Arc::new(AtomicU64::new(0)),
            controllers_root: PathBuf::new(),
            injected_midi: Arc::new(Mutex::new(None)),
        };
        let request = ControlRequest::Dispatch {
            envelope: CommandEnvelope::new(
                ClientId::new("test.desktop-mode-web").unwrap(),
                17,
                SessionCommand::SetActiveMode {
                    mode: SurfaceMode::Play,
                },
            ),
        };
        let responder = std::thread::spawn(move || {
            let DesktopControlCall::Session { request, response } = receiver.recv().unwrap() else {
                panic!("unexpected Desktop control call");
            };
            assert!(matches!(
                request,
                ControlRequest::Dispatch {
                    envelope: CommandEnvelope {
                        command: SessionCommand::SetActiveMode {
                            mode: SurfaceMode::Play
                        },
                        ..
                    }
                }
            ));
            response
                .send(ControlResponse::CommandApplied {
                    client_id: ClientId::new("test.desktop-mode-web").unwrap(),
                    command_id: 17,
                    revision: Revision::new(1),
                    events: Vec::new(),
                })
                .unwrap();
        });

        let response = response_for(request, &state);
        responder.join().unwrap();
        assert_eq!(
            response.get("status").and_then(Value::as_str),
            Some("command_applied")
        );
    }

    #[test]
    fn dispatches_program_edit_commands_to_the_desktop_runtime() {
        let (control, receiver) = control_channel();
        let state = WebState {
            session: Arc::new(RwLock::new(SessionState::new(
                SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            ))),
            plugin_catalog_revision: Arc::new(AtomicU64::new(0)),
            legacy_plugins_root: PathBuf::new(),
            plugin_store_root: None,
            data_root: PathBuf::new(),
            public_server: Arc::new(RwLock::new(WebServerPreferences::default())),
            control,
            resource_browser: Arc::new(NativeResourceBrowser::new([]).unwrap()),
            resource_upload_root: PathBuf::new(),
            web_packages_cache: Arc::new(Mutex::new(None)),
            package_scan_revision: Arc::new(AtomicU64::new(0)),
            controllers_root: PathBuf::new(),
            injected_midi: Arc::new(Mutex::new(None)),
        };
        let request = ControlRequest::Dispatch {
            envelope: CommandEnvelope::new(
                ClientId::new("test.desktop-program-web").unwrap(),
                11,
                SessionCommand::ReplaceProgramDraft {
                    draft_id: 3,
                    document_json: r#"{"schema_version":1}"#.into(),
                },
            ),
        };
        let responder = std::thread::spawn(move || {
            let DesktopControlCall::Session { request, response } = receiver.recv().unwrap() else {
                panic!("unexpected Desktop control call");
            };
            assert!(matches!(
                request,
                ControlRequest::Dispatch {
                    envelope: CommandEnvelope {
                        command: SessionCommand::ReplaceProgramDraft { draft_id: 3, .. },
                        ..
                    }
                }
            ));
            response
                .send(ControlResponse::CommandApplied {
                    client_id: ClientId::new("test.desktop-program-web").unwrap(),
                    command_id: 11,
                    revision: Revision::new(4),
                    events: Vec::new(),
                })
                .unwrap();
        });

        let response = response_for(request, &state);

        responder.join().unwrap();
        assert_eq!(
            response.get("status").and_then(Value::as_str),
            Some("command_applied")
        );
    }

    #[test]
    fn dispatches_preset_mutations_to_the_desktop_runtime() {
        let (control, receiver) = control_channel();
        let state = WebState {
            session: Arc::new(RwLock::new(SessionState::new(
                SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            ))),
            plugin_catalog_revision: Arc::new(AtomicU64::new(0)),
            legacy_plugins_root: PathBuf::new(),
            plugin_store_root: None,
            data_root: PathBuf::new(),
            public_server: Arc::new(RwLock::new(WebServerPreferences::default())),
            control,
            resource_browser: Arc::new(NativeResourceBrowser::new([]).unwrap()),
            resource_upload_root: PathBuf::new(),
            web_packages_cache: Arc::new(Mutex::new(None)),
            package_scan_revision: Arc::new(AtomicU64::new(0)),
            controllers_root: PathBuf::new(),
            injected_midi: Arc::new(Mutex::new(None)),
        };
        let instance_id = InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).unwrap();
        let responder = std::thread::spawn(move || {
            let DesktopControlCall::Performance { request, response } = receiver.recv().unwrap()
            else {
                panic!("preset mutation must reach Desktop performance control");
            };
            assert!(matches!(
                request,
                ControlRequest::SavePluginPreset { ref name, .. } if name == "Warm Strings"
            ));
            response
                .send(ControlResponse::Error {
                    code: ControlErrorCode::Rejected,
                    message: "routed to Desktop preset control".into(),
                    current_revision: None,
                })
                .unwrap();
        });

        let response = response_for(
            ControlRequest::SavePluginPreset {
                instance_id,
                name: "Warm Strings".into(),
            },
            &state,
        );
        responder.join().unwrap();
        assert_eq!(
            response.get("message").and_then(Value::as_str),
            Some("routed to Desktop preset control")
        );
    }

    #[test]
    fn dispatches_live_plugin_parameter_requests_to_the_desktop_runtime() {
        let (control, receiver) = control_channel();
        let state = WebState {
            session: Arc::new(RwLock::new(SessionState::new(
                SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            ))),
            plugin_catalog_revision: Arc::new(AtomicU64::new(0)),
            legacy_plugins_root: PathBuf::new(),
            plugin_store_root: None,
            data_root: PathBuf::new(),
            public_server: Arc::new(RwLock::new(WebServerPreferences::default())),
            control,
            resource_browser: Arc::new(NativeResourceBrowser::new([]).unwrap()),
            resource_upload_root: PathBuf::new(),
            web_packages_cache: Arc::new(Mutex::new(None)),
            package_scan_revision: Arc::new(AtomicU64::new(0)),
            controllers_root: PathBuf::new(),
            injected_midi: Arc::new(Mutex::new(None)),
        };
        let instance_id = InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).unwrap();
        let responder = std::thread::spawn(move || {
            for expected_write in [false, true] {
                let DesktopControlCall::Performance { request, response } =
                    receiver.recv().unwrap()
                else {
                    panic!("unexpected Desktop control call");
                };
                assert_eq!(
                    matches!(request, ControlRequest::SetPluginParameter { .. }),
                    expected_write
                );
                response
                    .send(ControlResponse::Error {
                        code: ControlErrorCode::Rejected,
                        message: "routed to Desktop performance control".into(),
                        current_revision: None,
                    })
                    .unwrap();
            }
        });

        for request in [
            ControlRequest::PluginParameters {
                instance_id: instance_id.clone(),
            },
            ControlRequest::SetPluginParameter {
                instance_id,
                parameter_index: 7,
                value: 0.5,
            },
        ] {
            let response = response_for(request, &state);
            assert_eq!(
                response.get("message").and_then(Value::as_str),
                Some("routed to Desktop performance control")
            );
        }

        responder.join().unwrap();
    }
}
