use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, State, WebSocketUpgrade, ws::Message},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{
            CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, COOKIE, HOST, ORIGIN, SET_COOKIE,
        },
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use rackforge_control_api::CONTROL_SOCKET_NAME;
use rackforge_plugin_api::{PluginManifest, WebSurfaceKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::MissedTickBehavior;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

const DEFAULT_PORT: u16 = 8787;
const WEB_CONTROL_SOCKET_NAME: &str = "web-control.sock";
const WEB_AUTH_FILE_NAME: &str = "web-auth.json";
const SESSION_COOKIE_NAME: &str = "rackforge_session";
const PAIRING_LIFETIME_SECONDS: u64 = 120;
const MAX_PAIRING_ATTEMPTS: u8 = 5;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum WebAccess {
    #[default]
    Local,
    Lan,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct WebConfig {
    enabled: bool,
    access: WebAccess,
    port: u16,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            access: WebAccess::Local,
            port: DEFAULT_PORT,
        }
    }
}

impl WebConfig {
    fn validate(&self) -> Result<()> {
        if self.port < 1024 {
            bail!("web port must be in 1024..=65535");
        }
        Ok(())
    }

    fn address(&self) -> SocketAddr {
        let ip = match self.access {
            WebAccess::Local => IpAddr::V4(Ipv4Addr::LOCALHOST),
            WebAccess::Lan => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        };
        SocketAddr::new(ip, self.port)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct RackForgeConfig {
    web: WebConfig,
}

#[derive(Clone)]
struct AppState {
    control_socket: PathBuf,
    public_config: Arc<RwLock<WebConfig>>,
    plugins: PluginWebRegistry,
    auth: Arc<AuthManager>,
}

#[derive(Clone, Debug, Serialize)]
struct PublicWebSurface {
    kind: WebSurfaceKind,
    entry_url: String,
}

#[derive(Clone, Debug, Serialize)]
struct PublicPluginWeb {
    plugin_id: String,
    plugin_name: String,
    version: String,
    api_version: u16,
    surfaces: Vec<PublicWebSurface>,
}

#[derive(Clone)]
struct PluginWebPackage {
    root: PathBuf,
    public: PublicPluginWeb,
}

#[derive(Clone, Default)]
struct PluginWebRegistry {
    packages: BTreeMap<String, PluginWebPackage>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct AuthStore {
    session_hashes: Vec<String>,
    pending: Option<PairingChallenge>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PairingChallenge {
    code_hash: String,
    expires_at: u64,
    remaining_attempts: u8,
}

struct AuthManager {
    path: PathBuf,
    store: Mutex<AuthStore>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PairRequest {
    code: String,
}

impl AuthManager {
    fn load(path: PathBuf) -> Result<Self> {
        let store = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .with_context(|| format!("parsing RackForge web auth {}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => AuthStore::default(),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("reading RackForge web auth {}", path.display()));
            }
        };
        Ok(Self {
            path,
            store: Mutex::new(store),
        })
    }

    fn begin_pairing(&self) -> Result<String> {
        let mut random = [0_u8; 4];
        getrandom::fill(&mut random).context("generating pairing code")?;
        let code = 100_000 + u32::from_le_bytes(random) % 900_000;
        let code = format!("{code:06}");
        let mut store = self.store.lock().expect("web auth mutex poisoned");
        store.pending = Some(PairingChallenge {
            code_hash: hash_secret(&code),
            expires_at: unix_now().saturating_add(PAIRING_LIFETIME_SECONDS),
            remaining_attempts: MAX_PAIRING_ATTEMPTS,
        });
        self.persist(&store)?;
        Ok(code)
    }

    fn pair(&self, code: &str) -> Result<String> {
        if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
            bail!("pairing code is invalid or expired");
        }
        let mut store = self.store.lock().expect("web auth mutex poisoned");
        let now = unix_now();
        let Some(challenge) = store.pending.as_mut() else {
            bail!("pairing code is invalid or expired");
        };
        if challenge.expires_at < now || challenge.remaining_attempts == 0 {
            store.pending = None;
            self.persist(&store)?;
            bail!("pairing code is invalid or expired");
        }
        if !constant_time_eq(challenge.code_hash.as_bytes(), hash_secret(code).as_bytes()) {
            challenge.remaining_attempts = challenge.remaining_attempts.saturating_sub(1);
            if challenge.remaining_attempts == 0 {
                store.pending = None;
            }
            self.persist(&store)?;
            bail!("pairing code is invalid or expired");
        }

        let mut token = [0_u8; 32];
        getrandom::fill(&mut token).context("generating browser session")?;
        let token = hex_encode(&token);
        let token_hash = hash_secret(&token);
        if !store.session_hashes.contains(&token_hash) {
            store.session_hashes.push(token_hash);
        }
        store.pending = None;
        self.persist(&store)?;
        Ok(token)
    }

    fn is_authorized(&self, token: &str) -> bool {
        let candidate = hash_secret(token);
        self.store
            .lock()
            .expect("web auth mutex poisoned")
            .session_hashes
            .iter()
            .any(|stored| constant_time_eq(stored.as_bytes(), candidate.as_bytes()))
    }

    fn pairing_active(&self) -> bool {
        self.store
            .lock()
            .expect("web auth mutex poisoned")
            .pending
            .as_ref()
            .is_some_and(|challenge| {
                challenge.expires_at >= unix_now() && challenge.remaining_attempts > 0
            })
    }

    fn persist(&self, store: &AuthStore) -> Result<()> {
        let parent = self.path.parent().context("web auth path has no parent")?;
        fs::create_dir_all(parent)?;
        let temporary = self.path.with_extension("json.new");
        let bytes = serde_json::to_vec_pretty(store).context("encoding RackForge web auth")?;
        fs::write(&temporary, bytes).with_context(|| format!("writing {}", temporary.display()))?;
        fs::rename(&temporary, &self.path)
            .with_context(|| format!("installing {}", self.path.display()))
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn hash_secret(secret: &str) -> String {
    hex_encode(&Sha256::digest(secret.as_bytes()))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[tokio::main]
async fn main() -> Result<()> {
    let root = rackforge_root();
    let config_path = root.join("config").join("rackforge.toml");
    let config = load_config(&config_path)?;
    config.web.validate()?;
    let shared_config = Arc::new(RwLock::new(config.web.clone()));
    let auth = Arc::new(AuthManager::load(
        root.join("state").join(WEB_AUTH_FILE_NAME),
    )?);
    let _system_control = start_system_control(
        &root.join("state").join(WEB_CONTROL_SOCKET_NAME),
        config_path.clone(),
        Arc::clone(&shared_config),
        Arc::clone(&auth),
    )?;

    if !config.web.enabled {
        println!("RACKFORGE_WEB_DISABLED config={}", config_path.display());
        shutdown_signal().await;
        return Ok(());
    }

    let web_root = env::var_os("RACKFORGE_WEB_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("web"));
    let index = web_root.join("index.html");
    if !index.is_file() {
        bail!("RackForge SPA is missing at {}", index.display());
    }

    let state = Arc::new(AppState {
        control_socket: root.join("state").join(CONTROL_SOCKET_NAME),
        public_config: shared_config,
        plugins: PluginWebRegistry::scan(&root.join("plugins"))?,
        auth,
    });
    let static_files = ServeDir::new(&web_root).fallback(ServeFile::new(index));
    let app = Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/auth/status", get(auth_status))
        .route("/api/v1/auth/pair", post(pair_browser))
        .route("/api/v1/config", get(public_config))
        .route("/api/v1/plugins", get(plugin_web_catalog))
        .route("/api/v1/plugins/{plugin_id}", get(plugin_web_descriptor))
        .route("/ws/v1/session", get(session_socket))
        .route("/plugin-assets/{plugin_id}/{*asset}", get(plugin_web_asset))
        .fallback_service(static_files)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let address = config.web.address();
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("binding RackForge web server to {address}"))?;
    println!(
        "RACKFORGE_WEB_READY address=http://{} access={:?}",
        address, config.web.access
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serving RackForge web application")
}

fn rackforge_root() -> PathBuf {
    env::var_os("RACKFORGE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/kalex/rackforge"))
}

fn load_config(path: &Path) -> Result<RackForgeConfig> {
    match fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text)
            .with_context(|| format!("parsing RackForge config {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(RackForgeConfig::default())
        }
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

struct SystemControlSocket {
    path: PathBuf,
}

impl Drop for SystemControlSocket {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(unix)]
fn start_system_control(
    path: &Path,
    config_path: PathBuf,
    config: Arc<RwLock<WebConfig>>,
    auth: Arc<AuthManager>,
) -> Result<SystemControlSocket> {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::net::UnixListener;

    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_socket() {
            bail!(
                "refusing to replace non-socket RackForge web control path {}",
                path.display()
            );
        }
        fs::remove_file(path)
            .with_context(|| format!("removing stale web control socket {}", path.display()))?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating web state directory {}", parent.display()))?;
    }
    let listener = UnixListener::bind(path)
        .with_context(|| format!("binding web control socket {}", path.display()))?;
    let control_path = path.to_owned();
    std::thread::Builder::new()
        .name("rackforge-web-control".into())
        .spawn(move || {
            for connection in listener.incoming() {
                let Ok(mut stream) = connection else {
                    continue;
                };
                let mut request = String::new();
                let read = BufReader::new(&stream).take(4096).read_line(&mut request);
                let mut restart = false;
                let response = match read {
                    Ok(_) => match serde_json::from_str::<Value>(&request) {
                        Ok(request) => match request.get("op").and_then(Value::as_str) {
                            Some("status") => json!({
                                "status": "ok",
                                "config": config.read().expect("web config lock poisoned").clone(),
                                "lan_ip": local_lan_ipv4().map(|address| address.to_string()),
                                "pairing_available": matches!(
                                    config.read().expect("web config lock poisoned").access,
                                    WebAccess::Lan
                                )
                            }),
                            Some("begin_pairing")
                                if matches!(
                                    config.read().expect("web config lock poisoned").access,
                                    WebAccess::Lan
                                ) =>
                            {
                                match auth.begin_pairing() {
                                    Ok(code) => json!({
                                        "status": "ok",
                                        "pairing_code": code,
                                        "expires_in": PAIRING_LIFETIME_SECONDS
                                    }),
                                    Err(error) => json!({
                                        "status": "error",
                                        "message": error.to_string()
                                    }),
                                }
                            }
                            Some("set") => {
                                let mut next =
                                    config.read().expect("web config lock poisoned").clone();
                                let field = request.get("field").and_then(Value::as_str);
                                let update = match field {
                                    Some("enabled") => request
                                        .get("value")
                                        .and_then(Value::as_bool)
                                        .map(|value| next.enabled = value)
                                        .ok_or_else(|| "enabled requires a boolean".to_owned()),
                                    Some("access") => {
                                        match request.get("value").and_then(Value::as_str) {
                                            Some("local") => {
                                                next.access = WebAccess::Local;
                                                Ok(())
                                            }
                                            Some("lan") => {
                                                next.access = WebAccess::Lan;
                                                Ok(())
                                            }
                                            _ => Err("access requires local or lan".into()),
                                        }
                                    }
                                    Some("port") => request
                                        .get("value")
                                        .and_then(Value::as_u64)
                                        .and_then(|value| u16::try_from(value).ok())
                                        .map(|value| next.port = value)
                                        .ok_or_else(|| "port requires a valid integer".to_owned()),
                                    _ => Err("unsupported web setting".into()),
                                };
                                match update
                                    .and_then(|_| {
                                        next.validate().map_err(|error| error.to_string())
                                    })
                                    .and_then(|_| {
                                        persist_web_config(&config_path, &next)
                                            .map_err(|error| error.to_string())
                                    }) {
                                    Ok(()) => {
                                        *config.write().expect("web config lock poisoned") =
                                            next.clone();
                                        restart = true;
                                        json!({
                                            "status": "ok",
                                            "config": next,
                                            "restart_required": true
                                        })
                                    }
                                    Err(message) => json!({
                                        "status": "error",
                                        "message": message
                                    }),
                                }
                            }
                            _ => json!({
                                "status": "error",
                                "message": "unsupported web control request"
                            }),
                        },
                        Err(error) => json!({
                            "status": "error",
                            "message": format!("invalid web control JSON: {error}")
                        }),
                    },
                    Err(error) => json!({
                        "status": "error",
                        "message": format!("could not read web control request: {error}")
                    }),
                };
                let _ = serde_json::to_writer(&mut stream, &response);
                let _ = stream.write_all(b"\n");
                let _ = stream.flush();
                if restart {
                    std::thread::spawn(|| {
                        std::thread::sleep(Duration::from_millis(150));
                        std::process::exit(75);
                    });
                }
            }
        })
        .context("starting RackForge web control thread")?;
    Ok(SystemControlSocket { path: control_path })
}

#[cfg(not(unix))]
fn start_system_control(
    path: &Path,
    _config_path: PathBuf,
    _config: Arc<RwLock<WebConfig>>,
    _auth: Arc<AuthManager>,
) -> Result<SystemControlSocket> {
    Ok(SystemControlSocket {
        path: path.to_owned(),
    })
}

fn persist_web_config(path: &Path, web: &WebConfig) -> Result<()> {
    let mut root = match fs::read_to_string(path) {
        Ok(text) => toml::from_str::<toml::Table>(&text)
            .with_context(|| format!("parsing {}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    root.insert(
        "web".into(),
        toml::Value::try_from(web).context("encoding web configuration")?,
    );
    let text = toml::to_string_pretty(&root).context("formatting RackForge configuration")?;
    let parent = path.parent().context("RackForge config has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("toml.new");
    fs::write(&temporary, text).with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("installing {}", path.display()))
}

fn local_lan_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(address) if !address.is_loopback() && !address.is_unspecified() => Some(address),
        _ => None,
    }
}

impl PluginWebRegistry {
    fn scan(plugins_root: &Path) -> Result<Self> {
        let mut packages = BTreeMap::new();
        let entries = match fs::read_dir(plugins_root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self { packages });
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("scanning plugins at {}", plugins_root.display()));
            }
        };

        for entry in entries {
            let entry = entry.context("reading plugin directory entry")?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let root = entry.path();
            let manifest_path = root.join("rackforge-plugin.toml");
            if !manifest_path.is_file() {
                continue;
            }
            match Self::load_package(&root, &manifest_path) {
                Ok(Some(package)) => {
                    if packages
                        .insert(package.public.plugin_id.clone(), package)
                        .is_some()
                    {
                        bail!(
                            "more than one installed plugin declares the same web plugin identifier"
                        );
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!(
                        "RACKFORGE_WEB_PLUGIN_IGNORED manifest={} error={error:#}",
                        manifest_path.display()
                    );
                }
            }
        }
        Ok(Self { packages })
    }

    fn load_package(root: &Path, manifest_path: &Path) -> Result<Option<PluginWebPackage>> {
        let text = fs::read_to_string(manifest_path)
            .with_context(|| format!("reading {}", manifest_path.display()))?;
        let manifest: PluginManifest = toml::from_str(&text)
            .with_context(|| format!("parsing {}", manifest_path.display()))?;
        manifest.validate()?;
        let Some(web_ui) = &manifest.web_ui else {
            return Ok(None);
        };
        let root = fs::canonicalize(root)
            .with_context(|| format!("resolving plugin root {}", root.display()))?;
        let mut surfaces = Vec::with_capacity(web_ui.surfaces.len());
        for surface in &web_ui.surfaces {
            let entry = fs::canonicalize(root.join(&surface.entry))
                .with_context(|| format!("resolving web entry {:?}", surface.entry))?;
            if !entry.is_file() || !entry.starts_with(&root) {
                bail!("web entry escapes its plugin package: {:?}", surface.entry);
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
        Ok(Some(PluginWebPackage {
            root,
            public: PublicPluginWeb {
                plugin_id: manifest.id,
                plugin_name: manifest.name,
                version: manifest.version,
                api_version: web_ui.api_version,
                surfaces,
            },
        }))
    }
}

async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "core_connected": state.control_socket.exists(),
        "schema_version": 1
    }))
}

async fn auth_status(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Json<Value> {
    let requires_pairing = access_requires_auth(&state) && !request_is_authorized(&state, &headers);
    Json(json!({
        "status": "ok",
        "requires_pairing": requires_pairing,
        "paired": !requires_pairing,
        "pairing_active": state.auth.pairing_active()
    }))
}

async fn pair_browser(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<PairRequest>,
) -> Response {
    if !valid_same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !access_requires_auth(&state) {
        return Json(json!({"status":"ok","paired":true})).into_response();
    }
    match state.auth.pair(request.code.trim()) {
        Ok(token) => {
            let mut response = Json(json!({"status":"ok","paired":true})).into_response();
            let cookie = format!(
                "{SESSION_COOKIE_NAME}={token}; Path=/; Max-Age=31536000; HttpOnly; SameSite=Strict"
            );
            response.headers_mut().insert(
                SET_COOKIE,
                HeaderValue::from_str(&cookie).expect("session cookie is valid ASCII"),
            );
            response
        }
        Err(_) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "status":"error",
                "message":"The pairing code is invalid or expired."
            })),
        )
            .into_response(),
    }
}

async fn public_config(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Json<WebConfig>, StatusCode> {
    require_authorized(&state, &headers)?;
    Ok(Json(
        state
            .public_config
            .read()
            .expect("web config lock poisoned")
            .clone(),
    ))
}

async fn plugin_web_catalog(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PublicPluginWeb>>, StatusCode> {
    require_authorized(&state, &headers)?;
    Ok(Json(
        state
            .plugins
            .packages
            .values()
            .map(|package| package.public.clone())
            .collect(),
    ))
}

async fn plugin_web_descriptor(
    AxumPath(plugin_id): AxumPath<String>,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Json<PublicPluginWeb>, StatusCode> {
    require_authorized(&state, &headers)?;
    state
        .plugins
        .packages
        .get(&plugin_id)
        .map(|package| Json(package.public.clone()))
        .ok_or(StatusCode::NOT_FOUND)
}

async fn plugin_web_asset(
    AxumPath((plugin_id, asset)): AxumPath<(String, String)>,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(package) = state.plugins.packages.get(&plugin_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let relative = Path::new(&asset);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let requested = match tokio::fs::canonicalize(package.root.join(relative)).await {
        Ok(path) if path.starts_with(&package.root) && path.is_file() => path,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let bytes = match tokio::fs::read(&requested).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let mime = mime_guess::from_path(&requested).first_or_octet_stream();
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref())
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if mime == mime_guess::mime::TEXT_HTML {
        response.headers_mut().insert(
            CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                 img-src 'self' data:; font-src 'self'; connect-src 'none'; \
                 media-src 'none'; frame-ancestors 'self'; base-uri 'none'; form-action 'none'",
            ),
        );
    }
    response
}

async fn session_socket(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    ws.on_upgrade(move |socket| handle_session_socket(socket, state))
        .into_response()
}

fn access_requires_auth(state: &AppState) -> bool {
    state
        .public_config
        .read()
        .expect("web config lock poisoned")
        .access
        == WebAccess::Lan
}

fn require_authorized(state: &AppState, headers: &HeaderMap) -> Result<(), StatusCode> {
    if request_is_authorized(state, headers) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn request_is_authorized(state: &AppState, headers: &HeaderMap) -> bool {
    if !access_requires_auth(state) {
        return true;
    }
    cookie_value(headers, SESSION_COOKIE_NAME).is_some_and(|token| state.auth.is_authorized(token))
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|pair| {
            let (candidate, value) = pair.trim().split_once('=')?;
            (candidate == name).then_some(value)
        })
}

fn valid_same_origin(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(HOST).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    origin == format!("http://{host}") || origin == format!("https://{host}")
}

async fn handle_session_socket(socket: axum::extract::ws::WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut ticker = tokio::time::interval(Duration::from_millis(250));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_revision = None;

    if let Ok(snapshot) = core_request(&state.control_socket, &json!({"op":"snapshot"})).await {
        last_revision = snapshot
            .pointer("/snapshot/revision")
            .and_then(Value::as_u64);
        let _ = sender
            .send(Message::Text(snapshot.to_string().into()))
            .await;
    }

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let Some(revision) = last_revision else { continue };
                let events = json!({"op":"events", "after_revision": revision});
                match core_request(&state.control_socket, &events).await {
                    Ok(response) => {
                        let current = response.get("current_revision").and_then(Value::as_u64);
                        if current.is_some_and(|value| value != revision) {
                            if let Ok(snapshot) = core_request(&state.control_socket, &json!({"op":"snapshot"})).await {
                                last_revision = snapshot.pointer("/snapshot/revision").and_then(Value::as_u64);
                                if sender.send(Message::Text(snapshot.to_string().into())).await.is_err() { break; }
                            }
                        }
                    }
                    Err(error) => {
                        let message = json!({"status":"gateway_error","message":error.to_string()});
                        if sender.send(Message::Text(message.to_string().into())).await.is_err() { break; }
                    }
                }
            }
            incoming = receiver.next() => {
                let Some(Ok(message)) = incoming else { break };
                match message {
                    Message::Text(text) => {
                        let request: Value = match serde_json::from_str(&text) {
                            Ok(value) => value,
                            Err(error) => {
                                let response = json!({"status":"gateway_error","message":format!("invalid JSON: {error}")});
                                let _ = sender.send(Message::Text(response.to_string().into())).await;
                                continue;
                            }
                        };
                        match core_request(&state.control_socket, &request).await {
                            Ok(response) => {
                                if sender.send(Message::Text(response.to_string().into())).await.is_err() { break; }
                                if matches!(
                                    request.get("op").and_then(Value::as_str),
                                    Some("dispatch" | "load_plugin_preset")
                                ) {
                                    if let Ok(snapshot) = core_request(&state.control_socket, &json!({"op":"snapshot"})).await {
                                        last_revision = snapshot.pointer("/snapshot/revision").and_then(Value::as_u64);
                                        if sender.send(Message::Text(snapshot.to_string().into())).await.is_err() { break; }
                                    }
                                }
                            }
                            Err(error) => {
                                let response = json!({"status":"gateway_error","message":error.to_string()});
                                let _ = sender.send(Message::Text(response.to_string().into())).await;
                            }
                        }
                    }
                    Message::Close(_) => break,
                    Message::Ping(bytes) => {
                        if sender.send(Message::Pong(bytes)).await.is_err() { break; }
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn core_request(socket: &Path, request: &Value) -> Result<Value> {
    let socket = socket.to_owned();
    let request = request.clone();
    tokio::task::spawn_blocking(move || blocking_core_request(&socket, &request))
        .await
        .context("joining Core control request")?
}

#[cfg(unix)]
fn blocking_core_request(socket: &Path, request: &Value) -> Result<Value> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("connecting to Core at {}", socket.display()))?;
    serde_json::to_writer(&mut stream, request).context("encoding Core request")?;
    stream
        .write_all(b"\n")
        .context("terminating Core request")?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .context("reading Core response")?;
    serde_json::from_str(&line).context("decoding Core response")
}

#[cfg(not(unix))]
fn blocking_core_request(_socket: &Path, _request: &Value) -> Result<Value> {
    bail!("the current RackForge Core transport is only available on Unix")
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate =
            signal(SignalKind::terminate()).expect("installing SIGTERM handler for rackforge-web");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_enabled_but_local_only() {
        let config = RackForgeConfig::default();
        assert!(config.web.enabled);
        assert!(matches!(config.web.access, WebAccess::Local));
        assert_eq!(config.web.address(), "127.0.0.1:8787".parse().unwrap());
    }

    #[test]
    fn privileged_ports_are_rejected() {
        let config = WebConfig {
            port: 80,
            ..WebConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn lan_configuration_is_valid_when_authentication_is_enabled() {
        let config = WebConfig {
            access: WebAccess::Lan,
            ..WebConfig::default()
        };
        assert!(config.validate().is_ok());
        assert_eq!(config.address(), "0.0.0.0:8787".parse().unwrap());
    }

    #[test]
    fn secret_comparison_checks_full_values() {
        assert!(constant_time_eq(b"abcdef", b"abcdef"));
        assert!(!constant_time_eq(b"abcdef", b"abcdeg"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
