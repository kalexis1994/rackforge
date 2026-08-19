use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path as AxumPath, Query, State, WebSocketUpgrade, ws::Message},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{
            CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, COOKIE, HOST, ORIGIN, SET_COOKIE,
        },
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use rackforge_control_api::CONTROL_SOCKET_NAME;
use rackforge_plugin_api::{PluginManifest, WebSurfaceKind};
use rackforge_repository::{
    InstalledPackage, LocalPackageInspection, MAX_PACKAGE_BYTES, PluginUserDataRemovalOptions,
    RepositoryError, cleanup_uninstall_tombstones, inspect_local_archive, install_local_archive,
    remove_plugin_user_data, uninstall_plugin,
};
use rackforge_resource_api::{
    BindResourceRequest, BindSelectionRequest, BrowseGrantRequest, ListGrantsRequest,
    LoadGrantedResourceRequest, MAX_CLIENT_UPLOAD_BYTES, ResourceBrowser, ResourceEntryKind,
    ResourceError, SelectHostEntryRequest,
};
use rackforge_resource_host::NativeResourceBrowser;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncWriteExt;
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
static CLIENT_UPLOAD_SERIAL: AtomicU64 = AtomicU64::new(1);
static PLUGIN_ACTIVATION_SERIAL: AtomicU64 = AtomicU64::new(1);

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
    controllers_root: PathBuf,
    public_config: Arc<RwLock<WebConfig>>,
    auth: Arc<AuthManager>,
    plugins_root: PathBuf,
    plugin_store_root: PathBuf,
    data_root: PathBuf,
    resource_browser: Arc<NativeResourceBrowser>,
    resource_upload_root: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallSelectedPluginRequest {
    selection_id: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UninstallPluginRequest {
    delete_presets: bool,
    delete_plugin_data: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadResourceQuery {
    name: String,
}

#[derive(Clone, Debug, Serialize)]
struct InstallPluginResponse {
    plugin_id: String,
    version: String,
    already_installed: bool,
    activation_required: bool,
}

#[derive(Clone, Debug, Serialize)]
struct PublicWebSurface {
    kind: WebSurfaceKind,
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
        controllers_root: root.join("controllers"),
        control_socket: root.join("state").join(CONTROL_SOCKET_NAME),
        public_config: shared_config,
        auth,
        plugins_root: root.join("plugins"),
        plugin_store_root: root.join("plugin-store"),
        data_root: root.join("data"),
        resource_browser: Arc::new(NativeResourceBrowser::platform_defaults_persistent(
            root.join("state/resource-grants.json"),
        )?),
        resource_upload_root: root.join("state/resource-uploads"),
    });
    let static_files = ServeDir::new(&web_root).fallback(ServeFile::new(index));
    let app = Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/auth/status", get(auth_status))
        .route("/api/v1/auth/unlock", post(unlock_browser))
        .route("/api/v1/auth/pin", post(set_pin))
        .route("/api/v1/config", get(public_config))
        .route("/api/v1/plugins", get(plugin_web_catalog))
        .route("/api/v1/controllers", get(controller_catalog))
        .route(
            "/api/v1/controllers/{controller_id}/settings",
            axum::routing::put(apply_controller_settings),
        )
        .route(
            "/api/v1/plugins/{plugin_id}",
            get(plugin_web_descriptor).delete(uninstall_managed_plugin),
        )
        .route(
            "/api/v1/plugins/{plugin_id}/activate",
            post(activate_plugin),
        )
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
        .route(
            "/api/v1/resources/uploads",
            post(upload_client_resource)
                .layer(DefaultBodyLimit::max(MAX_CLIENT_UPLOAD_BYTES as usize)),
        )
        .route("/api/v1/resources/selections", post(select_host_resource))
        .route(
            "/api/v1/resources/selections/release",
            post(release_resource_selection),
        )
        .route("/api/v1/plugins/inspect", post(inspect_selected_plugin))
        .route("/api/v1/plugins/install", post(install_selected_plugin))
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
        let _ = cleanup_uninstall_tombstones(plugin_store_root);
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
        rackforge_plugin_api::validate_branding_assets(&manifest, &root)?;
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
                        "/plugin-assets/{}/{}?v={}",
                        manifest.id,
                        surface.entry.replace('\\', "/"),
                        manifest.version,
                    ),
                });
            }
        }
        let branding = manifest
            .branding
            .as_ref()
            .map(|branding| PublicPluginBranding {
                icon_url: plugin_asset_url(&manifest.id, &branding.icon, &manifest.version),
                banner_url: plugin_asset_url(&manifest.id, &branding.banner, &manifest.version),
                splash_url: plugin_asset_url(&manifest.id, &branding.splash, &manifest.version),
                background_color: branding.background_color.clone(),
                accent_color: branding.accent_color.clone(),
            });
        Ok(PluginWebPackage {
            root,
            public: PublicPluginWeb {
                plugin_id: manifest.id,
                plugin_name: manifest.name,
                version: manifest.version,
                active,
                // Raspberry Pi packages may still live in the legacy
                // `plugins/` directory. They are host-managed installations
                // too; the uninstall endpoint handles both layouts safely.
                managed: true,
                api_version: manifest
                    .web_ui
                    .as_ref()
                    .map_or(0, |web_ui| web_ui.api_version),
                branding,
                surfaces,
                resources: manifest.resources,
            },
        })
    }
}

fn plugin_asset_url(plugin_id: &str, asset: &str, version: &str) -> String {
    format!(
        "/plugin-assets/{plugin_id}/{}?v={version}",
        asset.replace('\\', "/")
    )
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

fn install_response(installed: InstalledPackage) -> InstallPluginResponse {
    InstallPluginResponse {
        plugin_id: installed.record.plugin_id,
        version: installed.record.version,
        already_installed: installed.already_installed,
        activation_required: true,
    }
}

fn controller_settings_path(state: &AppState, controller_id: &str) -> PathBuf {
    state
        .controllers_root
        .join("state")
        .join(controller_id)
        .join("settings.toml")
}

fn read_controller_settings(
    state: &AppState,
    controller_id: &str,
) -> std::collections::BTreeMap<String, String> {
    std::fs::read_to_string(controller_settings_path(state, controller_id))
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

/// The installed controller packages, in the same shape every platform's
/// host serves: the shared UI's Plugin Manager and the /controllers/{id}
/// panel run unchanged against it.
async fn controller_catalog(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_authorized(&state, &headers)?;
    let installed = rackforge_controller_package::PackageStore::new(&state.controllers_root)
        .list()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let controllers: Vec<serde_json::Value> = installed
        .iter()
        .map(|controller| {
            let manifest = controller.package.manifest();
            let stored = read_controller_settings(&state, &controller.record.id);
            let settings: Vec<serde_json::Value> = manifest
                .settings
                .iter()
                .map(|setting| {
                    serde_json::json!({
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
            serde_json::json!({
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
    Ok(Json(
        serde_json::json!({"status": "ok", "controllers": controllers}),
    ))
}

#[derive(Debug, Deserialize)]
struct ControllerSettingsRequest {
    values: std::collections::BTreeMap<String, String>,
}

/// Validates against the package's settings schema and persists to the
/// store; the running driver watches the file and applies within a second.
async fn apply_controller_settings(
    axum::extract::Path(controller_id): axum::extract::Path<String>,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<ControllerSettingsRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_authorized(&state, &headers)?;
    let installed = rackforge_controller_package::PackageStore::new(&state.controllers_root)
        .resolve(&controller_id)
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let manifest = installed.package.manifest();
    let mut values = read_controller_settings(&state, &controller_id);
    for (id, value) in &request.values {
        let setting = manifest
            .settings
            .iter()
            .find(|setting| &setting.id == id)
            .ok_or(StatusCode::BAD_REQUEST)?;
        setting
            .validate_value(value)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        values.insert(id.clone(), value.clone());
    }
    let path = controller_settings_path(&state, &controller_id);
    let write = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = values
            .iter()
            .map(|(key, value)| format!("{key} = {value:?}
"))
            .collect::<String>();
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, body)?;
        std::fs::rename(&temporary, &path)?;
        Ok(())
    })();
    write.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"status": "ok", "values": values})))
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

async fn activate_plugin(
    AxumPath(plugin_id): AxumPath<String>,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let registry = match PluginWebRegistry::scan(&state.plugins_root, &state.plugin_store_root) {
        Ok(registry) => registry,
        Err(error) => return resource_error(ResourceError::Backend(error.to_string())),
    };
    if !registry.packages.contains_key(&plugin_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"status":"error", "message":"Plugin is not installed."})),
        )
            .into_response();
    }
    // Installing the first plugin recreates audio.toml. The systemd path unit
    // then starts Core, which may need several seconds to validate resources
    // and open the audio interface. Activation is one operation from the UI's
    // perspective, so wait for that supervised startup instead of exposing a
    // transient missing-socket error to the user.
    let startup_deadline = std::time::Instant::now() + Duration::from_secs(20);
    let snapshot = loop {
        match core_request(&state.control_socket, &json!({"op":"snapshot"})).await {
            Ok(snapshot) => break snapshot,
            Err(error) if std::time::Instant::now() < startup_deadline => {
                tokio::time::sleep(Duration::from_millis(250)).await;
                let _ = error;
            }
            Err(error) => return resource_error(ResourceError::Backend(error.to_string())),
        }
    };
    let instance_id = match plugin_instance_id_from_snapshot(&snapshot, &plugin_id) {
        Some(instance_id) => instance_id.to_owned(),
        None => {
            let root = state
                .plugin_store_root
                .parent()
                .unwrap_or_else(|| Path::new("."));
            if let Err(message) = request_audio_runtime_restart(root) {
                return (
                    StatusCode::CONFLICT,
                    Json(json!({"status":"error", "message":message})),
                )
                    .into_response();
            }
            let deadline = std::time::Instant::now() + Duration::from_secs(45);
            let mut loaded = None;
            while std::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if let Ok(snapshot) =
                    core_request(&state.control_socket, &json!({"op":"snapshot"})).await
                    && let Some(instance_id) =
                        plugin_instance_id_from_snapshot(&snapshot, &plugin_id)
                {
                    loaded = Some(instance_id.to_owned());
                    break;
                }
            }
            let Some(instance_id) = loaded else {
                return (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "status":"error",
                        "message":"The audio runtime restarted, but Core could not load this plugin. The package may be incompatible with the installed RackForge runtime."
                    })),
                )
                    .into_response();
            };
            instance_id
        }
    };
    for command in [
        json!({"type":"set_active_mode", "mode":"play"}),
        json!({"type":"select_plugin", "instance_id":instance_id}),
    ] {
        let request = json!({
            "op":"dispatch",
            "envelope":{
                "schema_version":rackforge_control_api::SESSION_SCHEMA_VERSION,
                "client_id":"web.plugin-install",
                "command_id":PLUGIN_ACTIVATION_SERIAL.fetch_add(1, Ordering::Relaxed),
                "command":command,
            }
        });
        match core_request(&state.control_socket, &request).await {
            Ok(response)
                if response.get("status").and_then(Value::as_str) == Some("command_applied") => {}
            Ok(response) => {
                let message = response
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Core rejected plugin activation");
                return (
                    StatusCode::CONFLICT,
                    Json(json!({"status":"error", "message":message})),
                )
                    .into_response();
            }
            Err(error) => return resource_error(ResourceError::Backend(error.to_string())),
        }
    }
    Json(json!({"status":"active", "plugin_id":plugin_id})).into_response()
}

fn plugin_instance_id_from_snapshot<'a>(snapshot: &'a Value, plugin_id: &str) -> Option<&'a str> {
    snapshot
        .pointer("/snapshot/instances")?
        .as_array()?
        .iter()
        .find(|instance| instance.get("plugin_id").and_then(Value::as_str) == Some(plugin_id))?
        .get("instance_id")?
        .as_str()
}

fn running_audio_engine_pid(root: &Path) -> Option<u32> {
    fs::read_to_string(root.join("state/audio-engine.lock"))
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|pid| {
            fs::read_link(format!("/proc/{pid}/exe"))
                .ok()
                .and_then(|path| path.file_name().map(|name| name.to_owned()))
                .is_some_and(|name| name == "rackforge-core")
        })
}

fn request_audio_runtime_restart(root: &Path) -> Result<(), String> {
    let pid = running_audio_engine_pid(root).ok_or_else(|| {
        "The plugin is installed, but the Raspberry Pi audio runtime is not running.".to_owned()
    })?;
    let status = std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .map_err(|error| format!("Could not restart the Raspberry Pi audio runtime: {error}"))?;
    if !status.success() {
        return Err("Could not restart the Raspberry Pi audio runtime.".into());
    }
    Ok(())
}

fn replace_startup_plugin(audio_config_path: &Path, package_root: &Path) -> Result<Vec<u8>> {
    let previous = fs::read(audio_config_path)
        .with_context(|| format!("reading {}", audio_config_path.display()))?;
    write_startup_plugin(&previous, audio_config_path, package_root)?;
    Ok(previous)
}

fn write_startup_plugin(
    source: &[u8],
    audio_config_path: &Path,
    package_root: &Path,
) -> Result<()> {
    let mut document = toml::from_str::<toml::Value>(
        std::str::from_utf8(source).context("audio configuration is not UTF-8")?,
    )
    .context("parsing audio configuration")?;
    let table = document
        .as_table_mut()
        .context("audio configuration root is not a TOML table")?;
    table.insert(
        "package".into(),
        toml::Value::String(package_root.to_string_lossy().into_owned()),
    );
    // A preset belongs to the old instrument and must not be applied to the
    // fallback package after Core restarts.
    table.remove("preset");
    let temporary = audio_config_path.with_extension("toml.plugin-uninstall-new");
    fs::write(&temporary, toml::to_string_pretty(&document)?)
        .with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, audio_config_path)
        .with_context(|| format!("replacing {}", audio_config_path.display()))?;
    Ok(())
}

fn ensure_startup_plugin(root: &Path, package_root: &Path) -> Result<bool> {
    let active = root.join("config/audio.toml");
    if active.is_file() {
        return Ok(false);
    }
    let inactive = root.join("config/audio.toml.inactive");
    let example = root.join("config/audio.toml.example");
    let source_path = if inactive.is_file() {
        inactive
    } else {
        example
    };
    let source = fs::read(&source_path).with_context(|| {
        format!(
            "reading inactive audio configuration {}",
            source_path.display()
        )
    })?;
    write_startup_plugin(&source, &active, package_root)?;
    Ok(true)
}

fn deactivate_startup_plugin(audio_config_path: &Path) -> Result<Vec<u8>> {
    let previous = fs::read(audio_config_path)
        .with_context(|| format!("reading {}", audio_config_path.display()))?;
    let inactive = audio_config_path.with_extension("toml.inactive");
    let temporary = audio_config_path.with_extension("toml.inactive-new");
    fs::write(&temporary, &previous).with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, &inactive)
        .with_context(|| format!("replacing {}", inactive.display()))?;
    fs::remove_file(audio_config_path)
        .with_context(|| format!("deactivating {}", audio_config_path.display()))?;
    Ok(previous)
}

fn restore_startup_plugin(audio_config_path: &Path, previous: &[u8]) -> Result<()> {
    let temporary = audio_config_path.with_extension("toml.plugin-uninstall-rollback");
    fs::write(&temporary, previous).with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, audio_config_path)
        .with_context(|| format!("restoring {}", audio_config_path.display()))
}

fn uninstall_legacy_plugin(
    plugins_root: &Path,
    store_root: &Path,
    plugin_id: &str,
) -> Result<(usize, bool)> {
    let plugins_root = fs::canonicalize(plugins_root)
        .with_context(|| format!("resolving {}", plugins_root.display()))?;
    let trash_root = store_root.join(".uninstall");
    fs::create_dir_all(&trash_root)
        .with_context(|| format!("creating {}", trash_root.display()))?;
    let trash_root = fs::canonicalize(&trash_root)
        .with_context(|| format!("resolving {}", trash_root.display()))?;
    let serial = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut moved = Vec::<(PathBuf, PathBuf)>::new();

    for entry in fs::read_dir(&plugins_root)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("rackforge-plugin.toml");
        if !manifest_path.is_file() {
            continue;
        }
        let manifest: PluginManifest = match fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
        {
            Some(manifest) => manifest,
            None => continue,
        };
        if manifest.validate().is_err() || manifest.id != plugin_id {
            continue;
        }
        let source = fs::canonicalize(entry.path())?;
        if source == plugins_root || !source.starts_with(&plugins_root) {
            bail!("legacy plugin path escapes the RackForge plugin directory");
        }
        let destination = trash_root.join(format!(
            "legacy-{}-{serial}-{}",
            std::process::id(),
            moved.len()
        ));
        if let Err(error) = fs::rename(&source, &destination) {
            for (original, tombstone) in moved.iter().rev() {
                let _ = fs::rename(tombstone, original);
            }
            return Err(error).context("moving legacy plugin out of discovery");
        }
        moved.push((source, destination));
    }

    let removed = moved.len();
    let cleanup_pending = moved
        .into_iter()
        .any(|(_, tombstone)| fs::remove_dir_all(tombstone).is_err());
    Ok((removed, cleanup_pending))
}

async fn uninstall_managed_plugin(
    AxumPath(plugin_id): AxumPath<String>,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    request: Option<Json<UninstallPluginRequest>>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let request = request.map(|Json(request)| request).unwrap_or_default();
    let registry = match PluginWebRegistry::scan(&state.plugins_root, &state.plugin_store_root) {
        Ok(registry) => registry,
        Err(error) => return resource_error(ResourceError::Backend(error.to_string())),
    };
    if !registry.packages.contains_key(&plugin_id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = state
        .plugin_store_root
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let configured_primary = fs::read_to_string(root.join("config/audio.toml"))
        .ok()
        .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
        .and_then(|document| document.get("package")?.as_str().map(PathBuf::from))
        .and_then(|path| fs::canonicalize(path).ok());
    let configured_primary_plugin_id = configured_primary
        .as_ref()
        .and_then(|path| fs::read_to_string(path.join("rackforge-plugin.toml")).ok())
        .and_then(|text| toml::from_str::<PluginManifest>(&text).ok())
        .map(|manifest| manifest.id);
    let audio_config_path = root.join("config/audio.toml");
    let fallback = registry
        .packages
        .values()
        .find(|candidate| candidate.public.plugin_id != plugin_id)
        .map(|candidate| candidate.root.clone());
    let mut startup_config_deactivated = false;
    let previous_audio_config = if configured_primary_plugin_id.as_deref() == Some(&plugin_id) {
        let update = if let Some(fallback) = fallback {
            replace_startup_plugin(&audio_config_path, &fallback)
        } else {
            startup_config_deactivated = true;
            deactivate_startup_plugin(&audio_config_path)
        };
        match update {
            Ok(previous) => Some(previous),
            Err(error) => return resource_error(ResourceError::Backend(error.to_string())),
        }
    } else {
        None
    };

    let engine_was_running = running_audio_engine_pid(root).is_some();
    let store_root = state.plugin_store_root.clone();
    let legacy_plugins_root = state.plugins_root.clone();
    let uninstall_id = plugin_id.clone();
    let removal = tokio::task::spawn_blocking(move || -> Result<(usize, bool)> {
        let managed = match uninstall_plugin(&store_root, &uninstall_id) {
            Ok(removed) => Some(removed),
            Err(RepositoryError::PluginNotFound(_)) => None,
            Err(error) => return Err(error.into()),
        };
        let legacy = uninstall_legacy_plugin(&legacy_plugins_root, &store_root, &uninstall_id)?;
        let removed_versions = managed
            .as_ref()
            .map_or(0, |removed| removed.removed_versions)
            .saturating_add(legacy.0);
        let cleanup_pending = managed.is_some_and(|removed| removed.cleanup_pending) || legacy.1;
        if removed_versions == 0 {
            bail!("plugin package is no longer installed");
        }
        Ok((removed_versions, cleanup_pending))
    })
    .await;
    let (removed_versions, cleanup_pending) = match removal {
        Ok(Ok(removed)) => removed,
        Ok(Err(error)) => {
            if let Some(previous) = previous_audio_config.as_deref() {
                let _ = restore_startup_plugin(&audio_config_path, previous);
                if startup_config_deactivated {
                    let _ = fs::remove_file(audio_config_path.with_extension("toml.inactive"));
                }
            }
            return (
                StatusCode::CONFLICT,
                Json(json!({"status":"error", "message":error.to_string()})),
            )
                .into_response();
        }
        Err(error) => return resource_error(ResourceError::Backend(error.to_string())),
    };

    let _ = core_request(
        &state.control_socket,
        &json!({
            "op":"dispatch",
            "envelope":{
                "schema_version":rackforge_control_api::SESSION_SCHEMA_VERSION,
                "client_id":"web.plugin-uninstall",
                "command_id":PLUGIN_ACTIVATION_SERIAL.fetch_add(1, Ordering::Relaxed),
                "command":{"type":"emergency_stop"}
            }
        }),
    )
    .await;

    let restart_requested = engine_was_running && request_audio_runtime_restart(root).is_ok();
    let cleanup_data_root = state.data_root.clone();
    let cleanup_plugin_id = plugin_id.clone();
    let user_data_cleanup = tokio::task::spawn_blocking(move || {
        remove_plugin_user_data(
            cleanup_data_root,
            &cleanup_plugin_id,
            PluginUserDataRemovalOptions {
                presets: request.delete_presets,
                plugin_data: request.delete_plugin_data,
            },
        )
    })
    .await;
    let (presets_deleted, plugin_data_deleted, mut user_data_cleanup_warning) =
        match user_data_cleanup {
            Ok(Ok(_)) => (request.delete_presets, request.delete_plugin_data, None),
            Ok(Err(error)) => (false, false, Some(error.to_string())),
            Err(error) => (false, false, Some(error.to_string())),
        };
    if plugin_data_deleted
        && let Err(error) = state.resource_browser.release_plugin_grants(&plugin_id)
    {
        user_data_cleanup_warning = Some(format!(
            "private data was deleted, but resource grants could not be revoked: {error}"
        ));
    }
    Json(json!({
        "status":"uninstalled",
        "plugin_id":plugin_id,
        "removed_versions":removed_versions,
        "cleanup_pending":cleanup_pending,
        "restart_requested":restart_requested,
        "engine_idle":startup_config_deactivated,
        "user_data_preserved":!presets_deleted && !plugin_data_deleted,
        "presets_deleted":presets_deleted,
        "plugin_data_deleted":plugin_data_deleted,
        "user_data_cleanup_warning":user_data_cleanup_warning
    }))
    .into_response()
}

/// Streams a file selected on the browser's device into host-owned storage and
/// returns a short-lived opaque handle. Consumers never see the staging path.
async fn upload_client_resource(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Query(query): Query<UploadResourceQuery>,
    body: Body,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
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

/// Turns an entry chosen in RackForge's host browser into the same generic
/// handle produced by a client upload.
async fn select_host_resource(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<SelectHostEntryRequest>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.resource_browser.select_host_entry(&request.entry_id) {
        Ok(selection) => Json(selection).into_response(),
        Err(error) => resource_error(error),
    }
}

async fn release_resource_selection(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<InstallSelectedPluginRequest>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state
        .resource_browser
        .release_selection(request.selection_id.trim())
    {
        Ok(()) | Err(ResourceError::UnknownHandle) => {
            Json(json!({"status": "released"})).into_response()
        }
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
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<InstallSelectedPluginRequest>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let selection_id = request.selection_id.trim().to_owned();
    if selection_id.is_empty() {
        return resource_error(ResourceError::InvalidRequest(
            "selection_id is required".into(),
        ));
    }
    let path = match state.resource_browser.resolve_selection_file(&selection_id) {
        Ok(path) => path,
        Err(error) => return resource_error(error),
    };
    if fs::metadata(&path).map_or(true, |metadata| metadata.len() > MAX_PACKAGE_BYTES) {
        return resource_error(ResourceError::InvalidRequest(
            "plugin package exceeds RackForge's 512 MB limit".into(),
        ));
    }
    let store_root = state.plugin_store_root.clone();
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

/// Plugin installation is one consumer of the generic resource-selection API.
/// The selection is released after this attempt whether validation succeeds or
/// fails; a client must explicitly select the file again to retry.
async fn install_selected_plugin(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<InstallSelectedPluginRequest>,
) -> Response {
    if require_authorized(&state, &headers).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let selection_id = request.selection_id.trim().to_owned();
    if selection_id.is_empty() {
        return resource_error(ResourceError::InvalidRequest(
            "selection_id is required".into(),
        ));
    }
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
    let store_root = state.plugin_store_root.clone();
    match tokio::task::spawn_blocking(move || {
        fs::read(selection.path())
            .map_err(anyhow::Error::from)
            .and_then(|bytes| install_local_archive(store_root, &bytes).map_err(Into::into))
    })
    .await
    {
        Ok(Ok(installed)) => {
            let root = state
                .plugin_store_root
                .parent()
                .unwrap_or_else(|| Path::new("."));
            if let Err(error) = ensure_startup_plugin(root, &installed.path) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "status":"error",
                        "message":format!("the package was installed, but its audio runtime could not be prepared: {error:#}")
                    })),
                )
                    .into_response();
            }
            Json(install_response(installed)).into_response()
        }
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

async fn bind_resource_selection(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<BindSelectionRequest>,
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

async fn resource_status(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<ListGrantsRequest>,
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
        request.entry_id.as_deref(),
    ) {
        Ok(path) => path,
        Err(error) => return resource_error(error),
    };
    let uploaded_grant = request
        .persist
        .then(|| (request.plugin_id.clone(), request.grant_id.clone()));
    let control = json!({
        "op": "load_plugin_resource",
        "plugin_id": request.plugin_id,
        "instance_id": request.instance_id,
        "resource_id": request.target_resource_id,
        "path": path,
        "persist": request.persist,
    });
    match core_request(&state.control_socket, &control).await {
        Ok(response)
            if response.get("status").and_then(Value::as_str) == Some("plugin_resource_loaded") =>
        {
            if let Some((plugin_id, grant_id)) = uploaded_grant {
                let _ = state
                    .resource_browser
                    .release_owned_grant(&plugin_id, &grant_id);
            }
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
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if mime == mime_guess::mime::TEXT_HTML {
        response.headers_mut().insert(
            CONTENT_SECURITY_POLICY,
            plugin_document_csp(&headers, &plugin_id),
        );
    }
    response
}

fn plugin_document_csp(headers: &HeaderMap, plugin_id: &str) -> HeaderValue {
    let connect_sources = headers
        .get(HOST)
        .and_then(|host| host.to_str().ok())
        .and_then(|host| host.parse::<axum::http::uri::Authority>().ok())
        .map(|authority| {
            let authority = authority.as_str();
            format!(
                "http://{authority}/plugin-assets/{plugin_id}/ \
                 https://{authority}/plugin-assets/{plugin_id}/"
            )
        })
        .unwrap_or_else(|| "'none'".to_owned());
    let policy = format!(
        "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; \
         style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; \
         connect-src {connect_sources}; media-src 'none'; frame-ancestors 'self'; \
         base-uri 'none'; form-action 'none'"
    );
    HeaderValue::from_str(&policy).unwrap_or_else(|_| {
        HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data:; font-src 'self'; connect-src 'none'; \
             media-src 'none'; frame-ancestors 'self'; base-uri 'none'; form-action 'none'",
        )
    })
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
    let mut virtual_midi_clients = std::collections::BTreeSet::<String>::new();
    let mut core_unavailable_since = None::<std::time::Instant>;
    let mut core_notice = None::<&'static str>;

    match core_request(&state.control_socket, &json!({"op":"snapshot"})).await {
        Ok(snapshot) => {
            last_revision = snapshot
                .pointer("/snapshot/revision")
                .and_then(Value::as_u64);
            let _ = sender
                .send(Message::Text(snapshot.to_string().into()))
                .await;
        }
        Err(error) => {
            let message = session_core_error(&state, &error);
            if message.get("status").and_then(Value::as_str) == Some("gateway_error") {
                core_unavailable_since = Some(std::time::Instant::now());
                core_notice = Some("core_restarting");
            }
            let message = if core_notice == Some("core_restarting") {
                json!({"status":"core_restarting"})
            } else {
                message
            };
            let _ = sender.send(Message::Text(message.to_string().into())).await;
        }
    }

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let Some(revision) = last_revision else {
                    match core_request(&state.control_socket, &json!({"op":"snapshot"})).await {
                        Ok(snapshot) => {
                            last_revision = snapshot.pointer("/snapshot/revision").and_then(Value::as_u64);
                            core_unavailable_since = None;
                            core_notice = None;
                            if sender.send(Message::Text(snapshot.to_string().into())).await.is_err() { break; }
                        }
                        Err(error) => {
                            let fallback = session_core_error(&state, &error);
                            let (notice, message) = if fallback.get("status").and_then(Value::as_str) == Some("host_idle") {
                                ("host_idle", fallback)
                            } else {
                                let since = core_unavailable_since.get_or_insert_with(std::time::Instant::now);
                                if since.elapsed() >= Duration::from_secs(10) {
                                    ("gateway_error", fallback)
                                } else {
                                    ("core_restarting", json!({"status":"core_restarting"}))
                                }
                            };
                            if core_notice != Some(notice) {
                                core_notice = Some(notice);
                                if sender.send(Message::Text(message.to_string().into())).await.is_err() { break; }
                            }
                        }
                    }
                    continue;
                };
                let events = json!({"op":"events", "after_revision": revision});
                match core_request(&state.control_socket, &events).await {
                    Ok(response) => {
                        core_unavailable_since = None;
                        core_notice = None;
                        let current = response.get("current_revision").and_then(Value::as_u64);
                        if current.is_some_and(|value| value != revision) {
                            if let Ok(snapshot) = core_request(&state.control_socket, &json!({"op":"snapshot"})).await {
                                last_revision = snapshot.pointer("/snapshot/revision").and_then(Value::as_u64);
                                if sender.send(Message::Text(snapshot.to_string().into())).await.is_err() { break; }
                            }
                        }
                    }
                    Err(_) => {
                        last_revision = None;
                        core_unavailable_since = Some(std::time::Instant::now());
                        if core_notice != Some("core_restarting") {
                            core_notice = Some("core_restarting");
                            let message = json!({"status":"core_restarting"});
                            if sender.send(Message::Text(message.to_string().into())).await.is_err() { break; }
                        }
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
                        match request.get("op").and_then(Value::as_str) {
                            Some("virtual_midi") => {
                                if let Some(client_id) = request.get("client_id").and_then(Value::as_str) {
                                    virtual_midi_clients.insert(client_id.to_owned());
                                }
                            }
                            Some("release_virtual_midi") => {
                                if let Some(client_id) = request.get("client_id").and_then(Value::as_str) {
                                    virtual_midi_clients.remove(client_id);
                                }
                            }
                            _ => {}
                        }
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
                                let response = session_core_error(&state, &error);
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
    for client_id in virtual_midi_clients {
        let _ = core_request(
            &state.control_socket,
            &json!({"op":"release_virtual_midi", "client_id":client_id}),
        )
        .await;
    }
}

fn session_core_error(state: &AppState, error: &anyhow::Error) -> Value {
    let no_plugins = PluginWebRegistry::scan(&state.plugins_root, &state.plugin_store_root)
        .is_ok_and(|registry| registry.packages.is_empty());
    if no_plugins {
        json!({
            "status": "host_idle",
            "reason": "no_plugin_installed"
        })
    } else {
        json!({"status":"gateway_error","message":error.to_string()})
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
    fn plugin_instance_lookup_uses_the_plugin_identity() {
        let snapshot = json!({
            "snapshot": {
                "instances": [
                    {"instance_id":"live.primary", "plugin_id":"org.rackforge.first"},
                    {"instance_id":"play.second", "plugin_id":"org.rackforge.second"}
                ]
            }
        });
        assert_eq!(
            plugin_instance_id_from_snapshot(&snapshot, "org.rackforge.second"),
            Some("play.second")
        );
        assert_eq!(
            plugin_instance_id_from_snapshot(&snapshot, "org.rackforge.missing"),
            None
        );
    }

    #[test]
    fn plugin_csp_allows_wasm_fetches_only_from_that_plugins_assets() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, HeaderValue::from_static("192.168.1.17:8787"));

        let header = plugin_document_csp(&headers, "org.rackforge.rf-kr106");
        let policy = header.to_str().unwrap();

        assert!(policy.contains("script-src 'self' 'wasm-unsafe-eval'"));
        assert!(policy.contains(
            "connect-src http://192.168.1.17:8787/plugin-assets/org.rackforge.rf-kr106/ \
             https://192.168.1.17:8787/plugin-assets/org.rackforge.rf-kr106/"
        ));
        assert!(!policy.contains("connect-src 'self'"));
        assert!(!policy.contains("'unsafe-eval'"));
    }

    #[test]
    fn plugin_csp_stays_closed_when_the_request_has_no_valid_authority() {
        let headers = HeaderMap::new();
        let header = plugin_document_csp(&headers, "org.rackforge.rf-kr106");
        let policy = header.to_str().unwrap();

        assert!(policy.contains("connect-src 'none'"));
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

    #[test]
    fn first_installed_plugin_reactivates_the_preserved_audio_configuration() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-web-first-plugin-test-{}-{}",
            std::process::id(),
            TEST_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let config = root.join("config");
        let package = root.join("plugin-store/packages/org.rackforge.test/1.0.0");
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(&package).unwrap();
        fs::write(
            config.join("audio.toml.inactive"),
            r#"schema_version = 2
package = "/old/plugin"
data_root = "/rackforge/data"
preset = "old.preset"
"#,
        )
        .unwrap();

        assert!(ensure_startup_plugin(&root, &package).unwrap());
        let active = fs::read_to_string(config.join("audio.toml")).unwrap();
        assert!(active.contains(package.to_string_lossy().as_ref()));
        assert!(!active.contains("old.preset"));
        assert!(config.join("audio.toml.inactive").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removing_the_last_plugin_leaves_an_inactive_audio_configuration() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-web-last-plugin-test-{}-{}",
            std::process::id(),
            TEST_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let config = root.join("config");
        let active = config.join("audio.toml");
        fs::create_dir_all(&config).unwrap();
        fs::write(&active, "schema_version = 2\npackage = \"/plugin\"\n").unwrap();

        let previous = deactivate_startup_plugin(&active).unwrap();
        assert!(!active.exists());
        assert_eq!(
            fs::read(config.join("audio.toml.inactive")).unwrap(),
            previous
        );
        fs::remove_dir_all(root).unwrap();
    }
}
