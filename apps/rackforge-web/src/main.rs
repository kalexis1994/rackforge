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
use rackforge_repository::{
    InstallationRecord, InstalledPackage, RepositoryFile, RepositoryIndex, RepositoryPlugin,
    fetch_repository, install_archive, repository_platform_key,
};
use rackforge_resource_api::{
    BindResourceRequest, BrowseGrantRequest, ListGrantsRequest, LoadGrantedResourceRequest,
    ResourceBrowser, ResourceEntryKind, ResourceError,
};
use rackforge_resource_host::NativeResourceBrowser;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
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
/// Digits an access PIN has.
const PIN_DIGITS: usize = 4;
/// Rounds the PIN is stretched over before it is stored.
const PIN_ROUNDS: u32 = 100_000;
/// How long after start-up an unclaimed device will accept a chosen PIN.
///
/// Bounded rather than open. A device that lets anyone on the network claim it
/// at any moment is a device that eventually gets claimed by somebody else.
const ENROLMENT_WINDOW_SECONDS: u64 = 15 * 60;
/// Wrong PINs allowed before the waiting starts. People mistype.
const FREE_ATTEMPTS: u32 = 5;
const LOCKOUT_STEP_SECONDS: u64 = 5;
const LOCKOUT_CAP_SECONDS: u64 = 15 * 60;

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
    auth: Arc<AuthManager>,
    repository_config_path: PathBuf,
    repositories: Arc<RwLock<RepositoryFile>>,
    plugins_root: PathBuf,
    plugin_store_root: PathBuf,
    resource_browser: Arc<NativeResourceBrowser>,
}

#[derive(Clone, Debug, Serialize)]
struct RepositoryCatalogResponse {
    repositories: Vec<RepositoryCatalogEntry>,
}

#[derive(Clone, Debug, Serialize)]
struct RepositoryCatalogEntry {
    repository_id: String,
    name: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog: Option<StoreRepositoryCatalog>,
}

#[derive(Clone, Debug, Serialize)]
struct StoreRepositoryCatalog {
    schema_version: u32,
    repository_id: String,
    name: String,
    generated_at: String,
    plugins: Vec<StorePluginStatus>,
}

#[derive(Clone, Debug, Serialize)]
struct StorePluginStatus {
    #[serde(flatten)]
    plugin: RepositoryPlugin,
    installed: bool,
    installed_versions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest_version: Option<String>,
    update_available: bool,
}

#[derive(Clone, Debug, Default)]
struct InstalledPlugin {
    versions: BTreeSet<String>,
    active_version: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallPluginRequest {
    repository_id: String,
    plugin_id: String,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct InstallPluginResponse {
    plugin_id: String,
    version: String,
    path: String,
    already_installed: bool,
    activation_required: bool,
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
    active: bool,
    api_version: u16,
    surfaces: Vec<PublicWebSurface>,
    resources: Vec<rackforge_plugin_api::ResourceRequirement>,
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
    /// The access PIN, salted and stretched. Absent until somebody sets one.
    pin: Option<StoredPin>,
    /// Consecutive wrong PINs, which decide how long the next wait is.
    failures: u32,
    /// Unix second before which no PIN will be accepted at all.
    locked_until: u64,
    /// The half-finished pairing an older version may have left behind.
    ///
    /// Read and thrown away. This struct refuses fields it does not know, so
    /// without somewhere for this to land a device that had ever started
    /// pairing would refuse to boot after the update — the service would fail
    /// to parse its own state file and restart forever. It is never written
    /// back, so the first save after an upgrade removes it for good.
    #[serde(default, skip_serializing)]
    pending: Option<serde_json::Value>,
}

/// A PIN as it is kept on disk.
///
/// Salted so the same PIN on two machines does not store the same hash, and
/// stretched so recovering it from the file is not instant. Four digits is ten
/// thousand possibilities, which a plain hash gives up in the time it takes to
/// write the loop.
///
/// This is the second line of defence and not the first. Anyone reading this
/// file is already on the machine.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredPin {
    salt: String,
    hash: String,
    rounds: u32,
}

struct AuthManager {
    path: PathBuf,
    store: Mutex<AuthStore>,
    /// When this process came up, which is what bounds the enrolment window.
    started_at: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnlockRequest {
    pin: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetPinRequest {
    pin: String,
    /// Required once a PIN exists. Absent only during first-run enrolment.
    #[serde(default)]
    current_pin: Option<String>,
}

/// What may be done about the PIN right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PinState {
    /// No PIN yet and the window is open: whoever reaches the page may claim
    /// the device by choosing one.
    Enrolling,
    /// No PIN and the window has closed. Nothing on the network can set one
    /// now; it has to be done from the machine itself.
    Unclaimed,
    /// A PIN exists, and it is what gets a browser in.
    Set,
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
            started_at: unix_now(),
        })
    }

    fn pin_state(&self) -> PinState {
        let store = self.store.lock().expect("web auth mutex poisoned");
        if store.pin.is_some() {
            return PinState::Set;
        }
        drop(store);
        if self.enrolment_open() {
            PinState::Enrolling
        } else {
            PinState::Unclaimed
        }
    }

    fn enrolment_open(&self) -> bool {
        unix_now() < self.started_at.saturating_add(ENROLMENT_WINDOW_SECONDS)
    }

    /// Seconds before another PIN may be tried, or zero.
    fn locked_for(&self) -> u64 {
        let store = self.store.lock().expect("web auth mutex poisoned");
        store.locked_until.saturating_sub(unix_now())
    }

    /// Trades a correct PIN for a browser session.
    ///
    /// Four digits are only worth anything because guessing is slowed down.
    /// The first few misses are free, because people mistype, and after that
    /// every further miss doubles the wait up to a quarter of an hour. Ten
    /// thousand possibilities stop being a minute of work and become weeks.
    fn unlock(&self, pin: &str) -> Result<String> {
        let mut store = self.store.lock().expect("web auth mutex poisoned");
        let now = unix_now();
        if now < store.locked_until {
            bail!("too many attempts; wait before trying again");
        }
        let Some(stored) = store.pin.clone() else {
            bail!("no PIN has been set on this device");
        };
        if !verify_pin(&stored, pin) {
            store.failures = store.failures.saturating_add(1);
            store.locked_until = now.saturating_add(lockout_seconds(store.failures));
            self.persist(&store)?;
            bail!("that PIN is not correct");
        }
        store.failures = 0;
        store.locked_until = 0;
        let token = Self::issue(&mut store);
        self.persist(&store)?;
        Ok(token)
    }

    /// Sets or replaces the PIN, returning a session for the caller.
    ///
    /// Replacing one needs the old one even from an already authorised
    /// browser: a session left open on a borrowed laptop should not be enough
    /// to take the device over. Every other session is dropped, so changing
    /// the PIN is also how somebody is put out.
    fn set_pin(&self, pin: &str, current: Option<&str>) -> Result<String> {
        if pin.len() != PIN_DIGITS || !pin.bytes().all(|byte| byte.is_ascii_digit()) {
            bail!("a PIN is {PIN_DIGITS} digits");
        }
        let open = self.enrolment_open();
        let mut store = self.store.lock().expect("web auth mutex poisoned");
        match store.pin.clone() {
            Some(stored) => {
                let now = unix_now();
                if now < store.locked_until {
                    bail!("too many attempts; wait before trying again");
                }
                let Some(current) = current else {
                    bail!("changing the PIN needs the current one");
                };
                if !verify_pin(&stored, current) {
                    store.failures = store.failures.saturating_add(1);
                    store.locked_until = now.saturating_add(lockout_seconds(store.failures));
                    self.persist(&store)?;
                    bail!("that PIN is not correct");
                }
            }
            None if !open => {
                bail!("the enrolment window has closed; set a PIN from the machine itself")
            }
            None => {}
        }
        store.pin = Some(build_pin(pin)?);
        store.failures = 0;
        store.locked_until = 0;
        // Everything issued under the previous PIN stops working.
        store.session_hashes.clear();
        let token = Self::issue(&mut store);
        self.persist(&store)?;
        Ok(token)
    }

    fn issue(store: &mut AuthStore) -> String {
        let mut token = [0_u8; 32];
        getrandom::fill(&mut token).expect("generating browser session");
        let token = hex_encode(&token);
        store.session_hashes.push(hash_secret(&token));
        token
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

/// How long the next attempt waits after `failures` wrong PINs.
fn lockout_seconds(failures: u32) -> u64 {
    let over = failures.saturating_sub(FREE_ATTEMPTS);
    if over == 0 {
        return 0;
    }
    LOCKOUT_STEP_SECONDS
        .saturating_mul(1_u64 << over.min(12))
        .min(LOCKOUT_CAP_SECONDS)
}

fn build_pin(pin: &str) -> Result<StoredPin> {
    let mut salt = [0_u8; 16];
    getrandom::fill(&mut salt).context("generating PIN salt")?;
    let salt = hex_encode(&salt);
    Ok(StoredPin {
        hash: stretch(&salt, pin, PIN_ROUNDS),
        salt,
        rounds: PIN_ROUNDS,
    })
}

fn verify_pin(stored: &StoredPin, candidate: &str) -> bool {
    let hashed = stretch(&stored.salt, candidate, stored.rounds);
    constant_time_eq(stored.hash.as_bytes(), hashed.as_bytes())
}

/// Hashes `secret` against `salt` enough times to make guessing expensive.
fn stretch(salt: &str, secret: &str, rounds: u32) -> String {
    let mut digest = Sha256::digest(format!("{salt}:{secret}").as_bytes());
    for _ in 1..rounds.max(1) {
        digest = Sha256::digest(digest);
    }
    hex_encode(&digest)
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
    let repository_config_path = root.join("config").join("repositories.toml");
    let repositories = load_repository_config(&repository_config_path)?;
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
        auth,
        repository_config_path,
        repositories: Arc::new(RwLock::new(repositories)),
        plugins_root: root.join("plugins"),
        plugin_store_root: root.join("plugin-store"),
        resource_browser: Arc::new(NativeResourceBrowser::platform_defaults_persistent(
            root.join("state/resource-grants.json"),
        )?),
    });
    let static_files = ServeDir::new(&web_root).fallback(ServeFile::new(index));
    let app = Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/auth/status", get(auth_status))
        .route("/api/v1/auth/unlock", post(unlock_browser))
        .route("/api/v1/auth/pin", post(set_pin))
        .route("/api/v1/config", get(public_config))
        .route("/api/v1/plugins", get(plugin_web_catalog))
        .route("/api/v1/plugins/{plugin_id}", get(plugin_web_descriptor))
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
        .route(
            "/api/v1/repositories",
            get(repository_config).put(replace_repository_config),
        )
        .route("/api/v1/store/catalog", get(repository_catalog))
        .route("/api/v1/store/install", post(install_store_plugin))
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
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("rackforge")
        })
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

fn load_repository_config(path: &Path) -> Result<RepositoryFile> {
    match fs::read(path) {
        Ok(bytes) => RepositoryFile::parse_toml(&bytes)
            .map_err(anyhow::Error::from)
            .with_context(|| format!("parsing repository config {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RepositoryFile {
            schema_version: 1,
            repositories: Vec::new(),
        }),
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
                                // What a controller can usefully say about
                                // access now: whether a PIN exists yet, and
                                // whether one can still be chosen over the
                                // network. There is no code to show, because
                                // there is no longer a screen in the loop.
                                "pin_state": auth.pin_state(),
                                "pin_digits": PIN_DIGITS,
                            }),
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

fn persist_repository_config(path: &Path, repositories: &RepositoryFile) -> Result<()> {
    repositories
        .validate()
        .map_err(anyhow::Error::from)
        .context("validating repository configuration")?;
    let text =
        toml::to_string_pretty(repositories).context("formatting repository configuration")?;
    let parent = path.parent().context("repository config has no parent")?;
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
    fn scan(plugins_root: &Path, plugin_store_root: &Path) -> Result<Self> {
        let mut packages = BTreeMap::new();
        Self::scan_directory(plugins_root, true, &mut packages)?;

        let store_packages = plugin_store_root.join("packages");
        let plugin_entries = match fs::read_dir(&store_packages) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self { packages });
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("scanning plugins at {}", store_packages.display()));
            }
        };

        for plugin_entry in plugin_entries {
            let plugin_entry = plugin_entry.context("reading stored plugin directory entry")?;
            if !plugin_entry.file_type()?.is_dir() {
                continue;
            }
            Self::scan_directory(&plugin_entry.path(), false, &mut packages)?;
        }
        Ok(Self { packages })
    }

    fn scan_directory(
        directory: &Path,
        active: bool,
        packages: &mut BTreeMap<String, PluginWebPackage>,
    ) -> Result<()> {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("scanning plugins at {}", directory.display()));
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
            match Self::load_package(&root, &manifest_path, active) {
                Ok(package) => {
                    let id = package.public.plugin_id.clone();
                    let replace = match packages.get(&id) {
                        None => true,
                        Some(current) if current.public.active => false,
                        Some(_) if package.public.active => true,
                        Some(current) => {
                            let current_version = Version::parse(&current.public.version).ok();
                            let candidate_version = Version::parse(&package.public.version).ok();
                            candidate_version > current_version
                        }
                    };
                    if replace {
                        packages.insert(id, package);
                    }
                }
                Err(error) => {
                    eprintln!(
                        "RACKFORGE_WEB_PLUGIN_IGNORED manifest={} error={error:#}",
                        manifest_path.display()
                    );
                }
            }
        }
        Ok(())
    }

    fn load_package(root: &Path, manifest_path: &Path, active: bool) -> Result<PluginWebPackage> {
        let text = fs::read_to_string(manifest_path)
            .with_context(|| format!("reading {}", manifest_path.display()))?;
        let manifest: PluginManifest = toml::from_str(&text)
            .with_context(|| format!("parsing {}", manifest_path.display()))?;
        manifest.validate()?;
        let root = fs::canonicalize(root)
            .with_context(|| format!("resolving plugin root {}", root.display()))?;
        let mut surfaces = Vec::new();
        if let Some(web_ui) = &manifest.web_ui {
            surfaces.reserve(web_ui.surfaces.len());
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
        }
        Ok(PluginWebPackage {
            root,
            public: PublicPluginWeb {
                plugin_id: manifest.id,
                plugin_name: manifest.name,
                version: manifest.version,
                active,
                api_version: manifest
                    .web_ui
                    .as_ref()
                    .map_or(0, |web_ui| web_ui.api_version),
                surfaces,
                resources: manifest.resources,
            },
        })
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
    let requires_pin = access_requires_auth(&state) && !request_is_authorized(&state, &headers);
    Json(json!({
        "status": "ok",
        // Whether getting in is something this host decides at all. A server
        // reachable across a network needs an answer; an application window on
        // somebody's own machine does not, and the interface should not offer
        // to change something that has no effect there.
        "pin_managed": true,
        "requires_pin": requires_pin,
        "unlocked": !requires_pin,
        "pin_state": state.auth.pin_state(),
        "pin_digits": PIN_DIGITS,
        "locked_for": state.auth.locked_for(),
    }))
}

/// Hands out a session to whoever knows the PIN.
async fn unlock_browser(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<UnlockRequest>,
) -> Response {
    if !valid_same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !access_requires_auth(&state) {
        return Json(json!({"status":"ok","unlocked":true})).into_response();
    }
    match state.auth.unlock(request.pin.trim()) {
        Ok(token) => session_response(json!({"status":"ok","unlocked":true}), &token),
        Err(error) => auth_refusal(&state, &error),
    }
}

/// Chooses the PIN, or replaces it for somebody who already knows it.
///
/// Deliberately reachable without a session: the first run has nobody holding
/// one, and refusing the request would leave a device nobody can ever get
/// into. What guards it instead is the enrolment window for a new device and
/// the current PIN for one already claimed.
async fn set_pin(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<SetPinRequest>,
) -> Response {
    if !valid_same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let current = request.current_pin.as_deref().map(str::trim);
    match state.auth.set_pin(request.pin.trim(), current) {
        Ok(token) => session_response(json!({"status":"ok","unlocked":true}), &token),
        Err(error) => auth_refusal(&state, &error),
    }
}

/// A reply carrying the session cookie a browser is identified by.
fn session_response(body: Value, token: &str) -> Response {
    let mut response = Json(body).into_response();
    let cookie = format!(
        "{SESSION_COOKIE_NAME}={token}; Path=/; Max-Age=31536000; HttpOnly; SameSite=Strict"
    );
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&cookie).expect("session cookie is valid ASCII"),
    );
    response
}

/// Says no, and says how long the caller has to wait before asking again.
///
/// The message is the one the manager produced. It distinguishes a wrong PIN
/// from a closed window from a lockout, and a player standing in front of a
/// machine that will not let them in deserves to know which.
fn auth_refusal(state: &AppState, error: &anyhow::Error) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "status": "error",
            "message": error.to_string(),
            "locked_for": state.auth.locked_for(),
        })),
    )
        .into_response()
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

async fn repository_config(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Json<RepositoryFile>, StatusCode> {
    require_authorized(&state, &headers)?;
    Ok(Json(
        state
            .repositories
            .read()
            .expect("repository config lock poisoned")
            .clone(),
    ))
}

async fn replace_repository_config(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(repositories): Json<RepositoryFile>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Err(error) = repositories.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": error.to_string()})),
        )
            .into_response();
    }
    let path = state.repository_config_path.clone();
    let candidate = repositories.clone();
    match tokio::task::spawn_blocking(move || persist_repository_config(&path, &candidate)).await {
        Ok(Ok(())) => {
            *state
                .repositories
                .write()
                .expect("repository config lock poisoned") = repositories.clone();
            Json(json!({"status": "ok", "config": repositories})).into_response()
        }
        Ok(Err(error)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": error.to_string()})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": error.to_string()})),
        )
            .into_response(),
    }
}

async fn repository_catalog(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let repositories = state
        .repositories
        .read()
        .expect("repository config lock poisoned")
        .repositories
        .clone();
    let plugins_root = state.plugins_root.clone();
    let plugin_store_root = state.plugin_store_root.clone();
    match tokio::task::spawn_blocking(move || {
        let installed = scan_installed_plugins(&plugins_root, &plugin_store_root);
        repositories
            .into_iter()
            .map(|repository| {
                if !repository.enabled {
                    return RepositoryCatalogEntry {
                        repository_id: repository.id,
                        name: repository.name,
                        status: "disabled",
                        error: None,
                        catalog: None,
                    };
                }
                match fetch_repository(&repository) {
                    Ok(verified) => RepositoryCatalogEntry {
                        repository_id: repository.id,
                        name: repository.name,
                        status: "available",
                        error: None,
                        catalog: Some(enrich_repository_catalog(verified.index, &installed)),
                    },
                    Err(error) => RepositoryCatalogEntry {
                        repository_id: repository.id,
                        name: repository.name,
                        status: "error",
                        error: Some(error.to_string()),
                        catalog: None,
                    },
                }
            })
            .collect()
    })
    .await
    {
        Ok(repositories) => Json(RepositoryCatalogResponse { repositories }).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": error.to_string()})),
        )
            .into_response(),
    }
}

fn enrich_repository_catalog(
    catalog: RepositoryIndex,
    installed: &BTreeMap<String, InstalledPlugin>,
) -> StoreRepositoryCatalog {
    let plugins = catalog
        .plugins
        .into_iter()
        .map(|plugin| {
            let latest_version = plugin
                .releases
                .iter()
                .filter_map(|release| Version::parse(&release.version).ok())
                .max()
                .map(|version| version.to_string());
            let installation = installed.get(&plugin.id);
            let installed_versions = installation
                .map(|entry| entry.versions.iter().cloned().collect())
                .unwrap_or_default();
            let active_version = installation.and_then(|entry| entry.active_version.clone());
            let is_installed = installation.is_some_and(|entry| !entry.versions.is_empty());
            let update_available = latest_version.as_ref().is_some_and(|latest| {
                is_installed && installation.is_some_and(|entry| !entry.versions.contains(latest))
            });
            StorePluginStatus {
                plugin,
                installed: is_installed,
                installed_versions,
                active_version,
                latest_version,
                update_available,
            }
        })
        .collect();
    StoreRepositoryCatalog {
        schema_version: catalog.schema_version,
        repository_id: catalog.repository_id,
        name: catalog.name,
        generated_at: catalog.generated_at,
        plugins,
    }
}

fn scan_installed_plugins(
    active_root: &Path,
    store_root: &Path,
) -> BTreeMap<String, InstalledPlugin> {
    let mut installed = BTreeMap::new();
    if let Ok(entries) = fs::read_dir(active_root) {
        for entry in entries.flatten() {
            let manifest_path = entry.path().join("rackforge-plugin.toml");
            let Ok(text) = fs::read_to_string(manifest_path) else {
                continue;
            };
            let Ok(manifest) = toml::from_str::<PluginManifest>(&text) else {
                continue;
            };
            if manifest.validate().is_err() {
                continue;
            }
            let plugin = installed
                .entry(manifest.id)
                .or_insert_with(InstalledPlugin::default);
            plugin.versions.insert(manifest.version.clone());
            plugin.active_version = Some(manifest.version);
        }
    }

    let records_root = store_root.join("records");
    if let Ok(plugin_directories) = fs::read_dir(records_root) {
        for plugin_directory in plugin_directories.flatten() {
            let Ok(records) = fs::read_dir(plugin_directory.path()) else {
                continue;
            };
            for record in records.flatten() {
                let Ok(bytes) = fs::read(record.path()) else {
                    continue;
                };
                let Ok(record) = serde_json::from_slice::<InstallationRecord>(&bytes) else {
                    continue;
                };
                installed
                    .entry(record.plugin_id)
                    .or_insert_with(InstalledPlugin::default)
                    .versions
                    .insert(record.version);
            }
        }
    }
    installed
}

async fn install_store_plugin(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<InstallPluginRequest>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let repository = state
        .repositories
        .read()
        .expect("repository config lock poisoned")
        .repositories
        .iter()
        .find(|repository| repository.id == request.repository_id && repository.enabled)
        .cloned();
    let Some(repository) = repository else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"status": "error", "message": "repository is missing or disabled"})),
        )
            .into_response();
    };
    let store_root = state.plugin_store_root.clone();
    match tokio::task::spawn_blocking(move || {
        let verified = fetch_repository(&repository)?;
        let platform = repository_platform_key()?;
        let selected = verified.select(&request.plugin_id, request.version.as_deref(), platform)?;
        let bytes = verified.download(&selected)?;
        install_archive(store_root, &selected, &bytes)
    })
    .await
    {
        Ok(Ok(installed)) => Json(install_response(installed)).into_response(),
        Ok(Err(error)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": error.to_string()})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": error.to_string()})),
        )
            .into_response(),
    }
}

fn install_response(installed: InstalledPackage) -> InstallPluginResponse {
    InstallPluginResponse {
        plugin_id: installed.record.plugin_id,
        version: installed.record.version,
        path: installed.path.display().to_string(),
        already_installed: installed.already_installed,
        activation_required: true,
    }
}

async fn plugin_web_catalog(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PublicPluginWeb>>, StatusCode> {
    require_authorized(&state, &headers)?;
    let plugins = PluginWebRegistry::scan(&state.plugins_root, &state.plugin_store_root)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        plugins
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
    PluginWebRegistry::scan(&state.plugins_root, &state.plugin_store_root)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .packages
        .get(&plugin_id)
        .map(|package| Json(package.public.clone()))
        .ok_or(StatusCode::NOT_FOUND)
}

async fn resource_mounts(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.resource_browser.mounts() {
        Ok(mounts) => Json(mounts).into_response(),
        Err(error) => resource_error(error),
    }
}

async fn resource_mount_root(
    AxumPath(mount_id): AxumPath<String>,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.resource_browser.mount_root(&mount_id) {
        Ok(entry) => Json(entry).into_response(),
        Err(error) => resource_error(error),
    }
}

async fn resource_entries(
    AxumPath(parent_id): AxumPath<String>,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.resource_browser.entries(&parent_id) {
        Ok(entries) => Json(entries).into_response(),
        Err(error) => resource_error(error),
    }
}

async fn bind_resource(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<BindResourceRequest>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let registry = match PluginWebRegistry::scan(&state.plugins_root, &state.plugin_store_root) {
        Ok(registry) => registry,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Some(package) = registry.packages.get(&request.plugin_id) else {
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
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<ListGrantsRequest>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let installed = PluginWebRegistry::scan(&state.plugins_root, &state.plugin_store_root)
        .is_ok_and(|registry| registry.packages.contains_key(&request.plugin_id));
    if !installed {
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
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<BrowseGrantRequest>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let installed = PluginWebRegistry::scan(&state.plugins_root, &state.plugin_store_root)
        .is_ok_and(|registry| registry.packages.contains_key(&request.plugin_id));
    if !installed {
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
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<LoadGrantedResourceRequest>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let registry = match PluginWebRegistry::scan(&state.plugins_root, &state.plugin_store_root) {
        Ok(registry) => registry,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Some(package) = registry.packages.get(&request.plugin_id) else {
        return resource_error(ResourceError::InvalidRequest(
            "plugin is not installed".into(),
        ));
    };
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
    let control = json!({
        "op": "load_plugin_resource",
        "plugin_id": request.plugin_id,
        "instance_id": request.instance_id,
        "resource_id": request.target_resource_id,
        "path": path,
    });
    match core_request(&state.control_socket, &control).await {
        Ok(response)
            if response.get("status").and_then(Value::as_str) == Some("plugin_resource_loaded") =>
        {
            Json(json!({"status":"ok"})).into_response()
        }
        Ok(response) => {
            let message = response
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Core rejected the plugin resource")
                .to_owned();
            resource_error(ResourceError::Backend(message))
        }
        Err(error) => resource_error(ResourceError::Backend(error.to_string())),
    }
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

async fn plugin_web_asset(
    AxumPath((plugin_id, asset)): AxumPath<(String, String)>,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let plugins = match PluginWebRegistry::scan(&state.plugins_root, &state.plugin_store_root) {
        Ok(plugins) => plugins,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Some(package) = plugins.packages.get(&plugin_id) else {
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
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SERIAL: AtomicU64 = AtomicU64::new(0);

    /// A manager over a scratch file, with no PIN yet.
    fn fresh_auth() -> (AuthManager, PathBuf) {
        let serial = TEST_SERIAL.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rackforge-auth-{}-{serial}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        (AuthManager::load(path.clone()).unwrap(), path)
    }

    #[test]
    fn an_unclaimed_device_lets_the_first_arrival_choose_a_pin() {
        let (auth, path) = fresh_auth();
        assert_eq!(auth.pin_state(), PinState::Enrolling);
        let token = auth.set_pin("4271", None).unwrap();
        assert!(auth.is_authorized(&token));
        assert_eq!(auth.pin_state(), PinState::Set);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn the_enrolment_window_does_not_stay_open_forever() {
        // A device that would accept a PIN from anyone at any time is a device
        // that eventually gets claimed by somebody who is not its owner.
        let (mut auth, path) = fresh_auth();
        auth.started_at = unix_now().saturating_sub(ENROLMENT_WINDOW_SECONDS + 1);
        assert_eq!(auth.pin_state(), PinState::Unclaimed);
        let error = auth.set_pin("4271", None).unwrap_err().to_string();
        assert!(error.contains("enrolment window"), "{error}");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_claimed_device_is_opened_only_by_its_pin() {
        let (auth, path) = fresh_auth();
        auth.set_pin("4271", None).unwrap();
        assert!(auth.unlock("0000").is_err());
        let token = auth.unlock("4271").unwrap();
        assert!(auth.is_authorized(&token));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn guessing_gets_slower_and_then_very_slow() {
        // Four digits is ten thousand possibilities. Without this it is a
        // minute of somebody's time; with it the tenth wrong guess already
        // costs more than the first nine together.
        let (auth, path) = fresh_auth();
        auth.set_pin("4271", None).unwrap();
        for _ in 0..FREE_ATTEMPTS {
            assert!(auth.unlock("0000").is_err());
        }
        assert_eq!(auth.locked_for(), 0, "the first few misses are free");
        assert!(auth.unlock("0000").is_err());
        let first = auth.locked_for();
        assert!(first > 0, "a wait starts once the free tries are gone");

        assert_eq!(lockout_seconds(FREE_ATTEMPTS), 0);
        assert!(lockout_seconds(FREE_ATTEMPTS + 2) > lockout_seconds(FREE_ATTEMPTS + 1));
        assert_eq!(lockout_seconds(FREE_ATTEMPTS + 60), LOCKOUT_CAP_SECONDS);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_correct_pin_clears_the_waiting() {
        let (auth, path) = fresh_auth();
        auth.set_pin("4271", None).unwrap();
        for _ in 0..FREE_ATTEMPTS {
            assert!(auth.unlock("0000").is_err());
        }
        auth.unlock("4271").unwrap();
        assert_eq!(auth.locked_for(), 0);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn changing_the_pin_needs_the_old_one_and_ends_other_sessions() {
        let (auth, path) = fresh_auth();
        let first = auth.set_pin("4271", None).unwrap();
        assert!(auth.is_authorized(&first));

        assert!(auth.set_pin("9999", None).is_err(), "no PIN, no change");
        assert!(auth.set_pin("9999", Some("0000")).is_err(), "wrong PIN");
        assert!(
            auth.is_authorized(&first),
            "a failed change changes nothing"
        );

        let second = auth.set_pin("9999", Some("4271")).unwrap();
        assert!(!auth.is_authorized(&first), "the old session is over");
        assert!(auth.is_authorized(&second));
        assert!(auth.unlock("9999").is_ok());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_pin_is_stored_salted_and_stretched() {
        // Ten thousand possibilities against a bare hash is a lookup table
        // somebody builds once. The salt makes the table per-machine and the
        // rounds make building it cost something.
        let (auth, path) = fresh_auth();
        auth.set_pin("4271", None).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("4271"), "the PIN itself is not on disk");
        let stored: AuthStore = serde_json::from_str(&text).unwrap();
        let pin = stored.pin.unwrap();
        assert_eq!(pin.rounds, PIN_ROUNDS);
        assert_ne!(pin.hash, hash_secret("4271"), "not a plain hash");
        assert_ne!(
            build_pin("4271").unwrap().hash,
            pin.hash,
            "salted per store"
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_device_that_had_started_pairing_still_boots() {
        // The state file from a version that used pairing carries a field this
        // one has never heard of, and the struct refuses unknown fields. With
        // nowhere for it to land the service cannot parse its own state and
        // restarts forever, which is a device bricked by an update.
        let serial = TEST_SERIAL.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rackforge-auth-legacy-{}-{serial}.json",
            std::process::id()
        ));
        fs::write(
            &path,
            r#"{
              "session_hashes": ["abc"],
              "pending": {
                "code_hash": "def",
                "expires_at": 1,
                "remaining_attempts": 5
              }
            }"#,
        )
        .unwrap();

        let auth = AuthManager::load(path.clone()).unwrap();
        assert_eq!(auth.pin_state(), PinState::Enrolling);
        auth.set_pin("4271", None).unwrap();

        // And the first save is what finally clears it out.
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("pending"), "{text}");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_pin_has_to_be_four_digits() {
        let (auth, path) = fresh_auth();
        for bad in ["", "123", "12345", "abcd", "12 4"] {
            assert!(auth.set_pin(bad, None).is_err(), "{bad:?} was accepted");
        }
        assert!(auth.set_pin("0000", None).is_ok());
        let _ = fs::remove_file(path);
    }

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

    #[test]
    fn active_plugin_identity_marks_the_catalog_plugin_as_installed() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-web-installed-test-{}-{}",
            std::process::id(),
            TEST_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let active = root.join("plugins").join("rf-dls");
        let store = root.join("plugin-store");
        fs::create_dir_all(&active).unwrap();
        fs::write(
            active.join("rackforge-plugin.toml"),
            r#"schema_version = 1
id = "org.rackforge.rf-dls"
name = "RF-DLS"
vendor = "RackForge"
version = "0.1.0"
kind = "instrument"
state_version = 1
capabilities = ["audio_output"]
ui_layouts = ["little@1"]

[api]
major = 1
minor = 5

[binaries]
linux-aarch64 = "lib/rf-dls.so"
"#,
        )
        .unwrap();
        let inventory = scan_installed_plugins(&root.join("plugins"), &store);
        let installed = inventory.get("org.rackforge.rf-dls").unwrap();
        assert!(installed.versions.contains("0.1.0"));
        assert_eq!(installed.active_version.as_deref(), Some("0.1.0"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installed_native_plugin_without_web_ui_is_discoverable() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-web-registry-test-{}-{}",
            std::process::id(),
            TEST_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let plugins = root.join("plugins");
        let package = root.join("plugin-store/packages/org.rackforge.rf-kr106/0.1.0");
        fs::create_dir_all(&plugins).unwrap();
        fs::create_dir_all(package.join("lib")).unwrap();
        fs::write(package.join("lib/librackforge_rf_kr106.so"), []).unwrap();
        fs::write(
            package.join("rackforge-plugin.toml"),
            r#"schema_version = 1
id = "org.rackforge.rf-kr106"
name = "RF-KR106"
vendor = "RackForge Community"
version = "0.1.0"
kind = "instrument"
state_version = 1
capabilities = ["audio_output"]
ui_layouts = ["little@1"]

[api]
major = 1
minor = 5

[binaries]
linux-aarch64 = "lib/librackforge_rf_kr106.so"
"#,
        )
        .unwrap();

        let registry = PluginWebRegistry::scan(&plugins, &root.join("plugin-store")).unwrap();
        let discovered = registry.packages.get("org.rackforge.rf-kr106").unwrap();
        assert_eq!(discovered.public.plugin_name, "RF-KR106");
        assert_eq!(discovered.public.version, "0.1.0");
        assert!(!discovered.public.active);
        assert!(discovered.public.surfaces.is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
