use axum::{
    Json, Router,
    body::Body,
    extract::{State, WebSocketUpgrade, ws::Message},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use include_dir::{Dir, include_dir};
use rackforge_control_api::{ControlRequest, ControlResponse};
use rackforge_performance_api::{
    LibraryRevision, PERFORMANCE_SNAPSHOT_SCHEMA_VERSION, PerformanceLibrary, PerformanceSnapshot,
};
use rackforge_session_api::SessionState;
use serde_json::{Value, json};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, RwLock};

static WEB_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../web/dist");

#[derive(Clone)]
struct WebState {
    session: Arc<RwLock<SessionState>>,
    port: u16,
    lan: bool,
}

pub fn start(session: Arc<RwLock<SessionState>>, port: u16, lan: bool) -> anyhow::Result<()> {
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
                let state = WebState { session, port, lan };
                let app = Router::new()
                    .route("/api/v1/health", get(health))
                    .route("/api/v1/auth/status", get(auth_status))
                    .route("/api/v1/config", get(config))
                    .route("/api/v1/plugins", get(plugins))
                    .route("/api/v1/repositories", get(empty_repositories))
                    .route("/api/v1/store/catalog", get(empty_store))
                    .route("/ws/v1/session", get(session_socket))
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

async fn plugins() -> Json<Value> {
    Json(json!([]))
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
