use anyhow::Context;
use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, State, WebSocketUpgrade, ws::Message},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use include_dir::{Dir, include_dir};
use rackforge_control_api::{ControlRequest, ControlResponse};
use rackforge_core::PluginPackage;
use rackforge_resource_api::{
    BindResourceRequest, BrowseGrantRequest, ListGrantsRequest, LoadGrantedResourceRequest,
    ResourceBrowser, ResourceEntryKind, ResourceError,
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
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::Options;

static WEB_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../web/dist");
const DESKTOP_CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct WebState {
    session: Arc<RwLock<SessionState>>,
    legacy_plugins_root: PathBuf,
    plugin_store_root: Option<PathBuf>,
    public_server: Arc<RwLock<WebServerPreferences>>,
    control: Sender<DesktopControlCall>,
    resource_browser: Arc<NativeResourceBrowser>,
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
        response: Sender<Result<(), String>>,
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

impl Drop for RunningServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct DesktopWebServers {
    _local: RunningServer,
    public: Option<RunningServer>,
    state: WebState,
    preferences: Arc<RwLock<WebServerPreferences>>,
    local_url: String,
}

impl DesktopWebServers {
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
}

#[derive(Clone, Debug, Serialize)]
struct PublicWebSurface {
    kind: rackforge_plugin_api::WebSurfaceKind,
    entry_url: String,
}

#[derive(Clone, Debug, Serialize)]
struct PublicPluginWeb {
    plugin_id: String,
    plugin_name: String,
    version: String,
    active: bool,
    api_version: u16,
    surfaces: Vec<PublicWebSurface>,
    resources: Vec<rackforge_plugin_api::ResourceRequirement>,
}

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
    let state = WebState {
        session,
        legacy_plugins_root: options.plugins_root.clone(),
        plugin_store_root: options.plugin_store_root.clone(),
        public_server: Arc::clone(&shared_preferences),
        control,
        resource_browser: Arc::new(NativeResourceBrowser::platform_defaults_persistent(
            options.rackforge_root.join("state/resource-grants.json"),
        )?),
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
    Ok(DesktopWebServers {
        _local: local,
        public,
        state,
        preferences: shared_preferences,
        local_url: format!("http://127.0.0.1:{local_port}"),
    })
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
        .route("/api/v1/config", get(config))
        .route("/api/v1/plugins", get(plugin_catalog))
        .route("/api/v1/plugins/{plugin_id}", get(plugin_descriptor))
        .route("/api/v1/repositories", get(empty_repositories))
        .route("/api/v1/store/catalog", get(empty_store))
        .route("/ws/v1/session", get(session_socket))
        .route("/plugin-assets/{plugin_id}/{*asset}", get(plugin_asset));
    let router = if allow_native_resources {
        router
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
            .route("/api/v1/resources/grants", post(resource_grants))
            .route("/api/v1/resources/browse", post(browse_resource_grant))
            .route("/api/v1/resources/load", post(load_granted_resource))
    } else {
        router
    };
    router.fallback(get(static_asset)).with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({"status":"ok", "core_connected":true, "schema_version":1, "host":"desktop"}))
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
    let preferences = state
        .public_server
        .read()
        .expect("Web settings lock poisoned")
        .clone();
    Json(json!({
        "enabled": preferences.enabled,
        "access": if preferences.enabled { "lan" } else { "local" },
        "port": preferences.port
    }))
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
    let path = match state.resource_browser.resolve_granted_file(
        &request.plugin_id,
        &request.grant_id,
        &request.entry_id,
    ) {
        Ok(path) => path,
        Err(error) => return resource_error(error),
    };
    let (response_sender, response_receiver) = mpsc::channel();
    if state
        .control
        .send(DesktopControlCall::LoadResource {
            plugin_id: request.plugin_id,
            resource_id: request.target_resource_id,
            path,
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
            "Desktop runtime did not finish loading the resource".into(),
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

async fn plugin_asset(
    AxumPath((plugin_id, asset)): AxumPath<(String, String)>,
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
    match fs::read(&path) {
        Ok(bytes) => Response::builder()
            .header(
                header::CONTENT_TYPE,
                mime_guess::from_path(&path)
                    .first_or_octet_stream()
                    .as_ref(),
            )
            .body(Body::from(bytes))
            .expect("valid plugin asset response"),
        Err(error) => internal_error(error),
    }
}

async fn empty_repositories() -> Json<Value> {
    Json(json!({"schema_version":1, "repositories":[]}))
}

async fn empty_store() -> Json<Value> {
    Json(json!({"repositories":[]}))
}

async fn session_socket(ws: WebSocketUpgrade, State(state): State<WebState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: axum::extract::ws::WebSocket, state: WebState) {
    let (mut sender, mut receiver) = socket.split();
    if sender
        .send(Message::Text(snapshot_json(&state).into()))
        .await
        .is_err()
    {
        return;
    }
    while let Some(Ok(message)) = receiver.next().await {
        match message {
            Message::Text(text) => {
                let request = serde_json::from_str::<ControlRequest>(&text);
                let sends_snapshot = request
                    .as_ref()
                    .is_ok_and(|request| matches!(request, ControlRequest::Dispatch { .. }));
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
                if sends_snapshot
                    && response.get("status").and_then(Value::as_str) == Some("command_applied")
                    && sender
                        .send(Message::Text(snapshot_json(&state).into()))
                        .await
                        .is_err()
                {
                    break;
                }
            }
            Message::Ping(bytes) => {
                if sender.send(Message::Pong(bytes)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

fn response_for(request: ControlRequest, state: &WebState) -> Value {
    match request {
        ControlRequest::Snapshot => {
            serde_json::from_str(&snapshot_json(state)).expect("snapshot JSON")
        }
        request
        @ (ControlRequest::PerformanceSnapshot | ControlRequest::EditPerformance { .. }) => {
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
        request @ ControlRequest::Dispatch { .. } => {
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
    let mut roots = crate::direct_package_roots(&state.legacy_plugins_root)?;
    if let Some(store_root) = state.plugin_store_root.as_deref() {
        roots.extend(crate::versioned_package_roots(store_root)?);
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
                        "/plugin-assets/{}/{}",
                        manifest.id,
                        surface.entry.replace('\\', "/")
                    ),
                });
            }
        }
        let candidate = PluginWebPackage {
            root,
            public: PublicPluginWeb {
                plugin_id: manifest.id.clone(),
                plugin_name: manifest.name.clone(),
                version: manifest.version.clone(),
                active: active.contains(&manifest.id),
                api_version: manifest.web_ui.as_ref().map_or(0, |web| web.api_version),
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
    Ok(packages
        .into_iter()
        .map(|(id, (_, package))| (id, package))
        .collect())
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
    Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(asset.contents().to_vec()))
        .expect("valid embedded asset response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_session_api::{
        ClientId, CommandEnvelope, DEFAULT_LIVE_INSTANCE_ID, DEFAULT_LIVE_SESSION_ID, InstanceId,
        Revision, SessionCommand, SessionId,
    };

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
    fn dispatches_session_changes_to_the_desktop_runtime() {
        let (control, receiver) = control_channel();
        let state = WebState {
            session: Arc::new(RwLock::new(SessionState::new(
                SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            ))),
            legacy_plugins_root: PathBuf::new(),
            plugin_store_root: None,
            public_server: Arc::new(RwLock::new(WebServerPreferences::default())),
            control,
            resource_browser: Arc::new(NativeResourceBrowser::new([]).unwrap()),
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
    fn dispatches_program_edit_commands_to_the_desktop_runtime() {
        let (control, receiver) = control_channel();
        let state = WebState {
            session: Arc::new(RwLock::new(SessionState::new(
                SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            ))),
            legacy_plugins_root: PathBuf::new(),
            plugin_store_root: None,
            public_server: Arc::new(RwLock::new(WebServerPreferences::default())),
            control,
            resource_browser: Arc::new(NativeResourceBrowser::new([]).unwrap()),
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
}
