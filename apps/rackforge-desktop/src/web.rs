use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, State, WebSocketUpgrade, ws::Message},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use include_dir::{Dir, include_dir};
use rackforge_control_api::{ControlRequest, ControlResponse};
use rackforge_core::PluginPackage;
use rackforge_performance_api::{
    LibraryRevision, PERFORMANCE_SNAPSHOT_SCHEMA_VERSION, PerformanceLibrary, PerformanceSnapshot,
};
use rackforge_session_api::SessionState;
use semver::Version;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::Options;

static WEB_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../web/dist");

#[derive(Clone)]
struct WebState {
    session: Arc<RwLock<SessionState>>,
    legacy_plugins_root: PathBuf,
    plugin_store_root: Option<PathBuf>,
    port: u16,
    lan: bool,
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
}

struct PluginWebPackage {
    root: PathBuf,
    public: PublicPluginWeb,
}

pub fn start(session: Arc<RwLock<SessionState>>, options: &Options) -> anyhow::Result<()> {
    let port = options.port;
    let lan = options.lan;
    let legacy_plugins_root = options.plugins_root.clone();
    let plugin_store_root = options.plugin_store_root.clone();
    let address = SocketAddr::new(
        if lan {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        },
        port,
    );
    let listener = std::net::TcpListener::bind(address)?;
    listener.set_nonblocking(true)?;
    std::thread::Builder::new()
        .name("rackforge-desktop-web".into())
        .spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("create desktop Web runtime");
            runtime.block_on(async move {
                let state = WebState {
                    session,
                    legacy_plugins_root,
                    plugin_store_root,
                    port,
                    lan,
                };
                let app = Router::new()
                    .route("/api/v1/health", get(health))
                    .route("/api/v1/auth/status", get(auth_status))
                    .route("/api/v1/config", get(config))
                    .route("/api/v1/plugins", get(plugin_catalog))
                    .route("/api/v1/plugins/{plugin_id}", get(plugin_descriptor))
                    .route("/api/v1/repositories", get(empty_repositories))
                    .route("/api/v1/store/catalog", get(empty_store))
                    .route("/ws/v1/session", get(session_socket))
                    .route("/plugin-assets/{plugin_id}/{*asset}", get(plugin_asset))
                    .fallback(get(static_asset))
                    .with_state(state);
                let listener = tokio::net::TcpListener::from_std(listener)
                    .expect("adopt RackForge Desktop Web listener");
                if let Err(error) = axum::serve(listener, app).await {
                    eprintln!("RACKFORGE_DESKTOP_WEB_ERROR {error}");
                }
            });
        })?;
    Ok(())
}

async fn health() -> Json<Value> {
    Json(json!({"status":"ok", "core_connected":true, "schema_version":1, "host":"desktop"}))
}

async fn auth_status() -> Json<Value> {
    Json(json!({"status":"ok", "requires_pairing":false, "paired":true, "pairing_active":false}))
}

async fn config(State(state): State<WebState>) -> Json<Value> {
    Json(json!({
        "enabled": true,
        "access": if state.lan { "lan" } else { "local" },
        "port": state.port
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
                let response = serde_json::from_str::<ControlRequest>(&text)
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
        ControlRequest::PerformanceSnapshot => {
            serde_json::to_value(ControlResponse::PerformanceSnapshot {
                snapshot: Box::new(empty_performance()),
            })
            .expect("performance response")
        }
        ControlRequest::Events { .. } => {
            let revision = state.session.read().expect("session lock").revision;
            serde_json::to_value(ControlResponse::Events {
                current_revision: revision,
                events: Vec::new(),
            })
            .expect("events response")
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

fn empty_performance() -> PerformanceSnapshot {
    PerformanceSnapshot {
        schema_version: PERFORMANCE_SNAPSHOT_SCHEMA_VERSION,
        revision: LibraryRevision::new("0".repeat(64)).expect("valid empty revision"),
        library: PerformanceLibrary::empty(),
        live: Default::default(),
    }
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
