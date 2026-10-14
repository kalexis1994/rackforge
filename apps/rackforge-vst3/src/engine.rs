use anyhow::{Context, Result, bail};
use rackforge_core::DEFAULT_INSTRUMENT_ID;
use rackforge_core::{LoadedPlugin, PluginInstance, PluginPackage, midi2::Midi2Event};
use rackforge_plugin_api::{
    ParameterSchema, PluginBranding, PluginKind, ResourceRequirement, WebSurfaceKind,
    abi::ParameterEventV1,
};
use rackforge_repository::{BundledInstall, install_bundled_official_archive, plugin_is_enabled};
#[cfg(windows)]
use rackforge_resource_host::NativeResourceBrowser;
use serde::Deserialize;
#[cfg(windows)]
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

include!(concat!(env!("OUT_DIR"), "/bundled_plugin.rs"));

const MAX_REALTIME_EVENTS: usize = 4096;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootDocument {
    schema_version: u32,
    mode: String,
    root: PathBuf,
}

/// The storage browser, opened once for this process.
///
/// The processor and the controller are separate VST3 objects and neither can
/// see the other's state, so they must not each open their own: the grants
/// file would have two writers. One browser, and both read the same installs.
///
/// Note that an installed resource reaches the instrument through the
/// plug-in's DATA directory and not through the runtime's resource overrides.
/// Both exist; every host passes an empty override map, the desktop included.
/// What "installed" MEANS is a file at `data/plugins/<id>/<data_path>`, which
/// is what `resources/status` reports everywhere, and the plug-in reads it
/// from the data root it is handed when it is built. Installing through the
/// overrides would have given the VST3 a second, private mechanism for the
/// same thing, and a cartridge installed in the DAW would have been invisible
/// to the desktop app on the same machine.
#[cfg(windows)]
static RESOURCES: OnceLock<Option<Arc<NativeResourceBrowser>>> = OnceLock::new();

static CARRIED_INSTALLED: OnceLock<()> = OnceLock::new();
static CARRIED_INSTALLED_INIT: Mutex<()> = Mutex::new(());
static MODELS: OnceLock<Vec<VstPluginModel>> = OnceLock::new();
static MODELS_INIT: Mutex<()> = Mutex::new(());

/// Every plug-in runtime this module has loaded, kept so `ExitDll` can retire
/// them, and a count of the instances still borrowing them.
///
/// The runtimes used to be `Box::leak`ed, on the reasoning that a VST host may
/// keep factory objects beyond a component's lifetime and an unloaded module
/// would leave ABI pointers dangling. That reasoning is right about the
/// components and wrong about the DLL: Wasmtime's trap handlers are
/// process-global and must be removed BEFORE the DLL is unmapped, and a leaked
/// runtime can never do that. See `rackforge_plugin_runtime::unload_process_handlers`
/// for the crash it caused.
///
/// Handing out `&'static LoadedPlugin` from boxes owned by this registry is
/// sound under one invariant: a box is only dropped from `unload_runtimes`,
/// and only when `LIVE_INSTANCES` is zero, i.e. when no `PluginInstance`
/// borrows any of them. If a host ever calls `ExitDll` with an instance still
/// alive, the registry leaks rather than frees -- a crash on unload is bad, a
/// use-after-free is worse -- and says so in the diagnostic log.
/// A registered runtime. `LoadedPlugin` is not `Send` only because its
/// native backend carries raw host-API pointers; the VST3 loads the portable
/// (WebAssembly) backend, and either way a runtime here is touched by exactly
/// two parties -- the loader that registers it and `ExitDll` that retires it
/// -- both on host threads and both under the registry's lock. That is the
/// whole extent of the sharing this wrapper vouches for.
#[cfg_attr(not(windows), allow(dead_code))]
struct Registered(Box<LoadedPlugin>);
// SAFETY: see `Registered`; access is serialised by `RUNTIMES`' mutex and no
// reference escapes except the `&'static` handed out by `register_runtime`,
// whose lifetime is bounded by `LIVE_INSTANCES` as documented on `RUNTIMES`.
unsafe impl Send for Registered {}

static RUNTIMES: Mutex<Vec<Registered>> = Mutex::new(Vec::new());
static LIVE_INSTANCES: AtomicUsize = AtomicUsize::new(0);

/// Registers a freshly loaded runtime and returns the reference the rest of
/// this module needs. See `RUNTIMES` for why this is not a leak.
fn register_runtime(runtime: LoadedPlugin) -> &'static LoadedPlugin {
    let boxed = Box::new(runtime);
    // SAFETY: the box lives in RUNTIMES until `unload_runtimes`, which only
    // drops it once LIVE_INSTANCES is zero, and every borrower of this
    // reference is counted there.
    let reference: &'static LoadedPlugin =
        unsafe { &*(Box::as_ref(&boxed) as *const LoadedPlugin) };
    RUNTIMES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(Registered(boxed));
    reference
}

/// What `ExitDll` does with the runtimes: retire Wasmtime's process-wide
/// handlers if nothing still borrows a runtime, or leave everything in place
/// and say why. Returns what happened so the exit hook can log it and a test
/// can see it.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn unload_runtimes() -> UnloadOutcome {
    let live = LIVE_INSTANCES.load(Ordering::SeqCst);
    if live != 0 {
        return UnloadOutcome::LeftInPlace {
            live_instances: live,
        };
    }
    let runtimes: Vec<Registered> = std::mem::take(
        &mut *RUNTIMES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    );
    let count = runtimes.len();
    let modules: Vec<_> = runtimes
        .into_iter()
        .filter_map(|Registered(runtime)| runtime.into_portable_module())
        .collect();
    // SAFETY: LIVE_INSTANCES is zero so no PluginInstance -- and therefore no
    // Store -- exists; the modules just collected are the only remaining
    // Engine clones and are consumed by the call; and ExitDll runs after the
    // host has released every component and stopped audio.
    match unsafe { rackforge_core::unload_process_handlers(modules) } {
        Ok(()) => UnloadOutcome::Unloaded { runtimes: count },
        Err(reason) => UnloadOutcome::Failed { reason },
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, PartialEq, Eq)]
pub enum UnloadOutcome {
    Unloaded { runtimes: usize },
    LeftInPlace { live_instances: usize },
    Failed { reason: String },
}

pub struct RackForgeEngine {
    instance: PluginInstance<'static>,
    interleaved: Vec<f32>,
    events: Vec<Midi2Event>,
    parameter_events: Vec<ParameterEventV1>,
    maximum_frames: usize,
}

impl RackForgeEngine {
    pub fn open_plugin(plugin_id: &str, sample_rate: f64, maximum_frames: usize) -> Result<Self> {
        let package_root = package_root_for_id(plugin_id)?;
        Self::open_package(&package_root, sample_rate, maximum_frames)
    }

    fn open_package(package_root: &Path, sample_rate: f64, maximum_frames: usize) -> Result<Self> {
        let startup = rackforge_core::startup::StartupTimeline::new("vst3");
        if !sample_rate.is_finite() || sample_rate <= 0.0 || maximum_frames == 0 {
            bail!("invalid VST3 processing setup");
        }
        let root = rackforge_root()?;
        let data = root.join("data");
        let package = PluginPackage::open(package_root)?;
        if package.manifest().kind != PluginKind::Instrument {
            bail!("the VST3 package is not an instrument");
        }
        // RackForge runtimes are process-lifetime modules. A VST host may keep
        // factory objects beyond an individual component's lifetime, so an
        // unloaded module would leave ABI pointers dangling.
        let runtime = unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), Some(&data)) }?;
        let runtime = register_runtime(runtime);
        let mut instance = runtime.create_instance()?;
        LIVE_INSTANCES.fetch_add(1, Ordering::SeqCst);
        let catalog = instance.preset_catalog()?;
        if let Some(preset) = catalog.presets.first() {
            instance.load_preset(&preset.id)?;
        }
        instance.activate(sample_rate, maximum_frames as u32, 0, 2)?;
        startup.advance(rackforge_core::startup::StartupPhase::AudioReady)?;
        // A VST3 instance has no host-owned network or Web server on its
        // processing path. Its editor is created lazily by the DAW.
        startup.advance(rackforge_core::startup::StartupPhase::BackgroundReady)?;

        Ok(Self {
            instance,
            interleaved: vec![0.0; maximum_frames * 2],
            events: Vec::with_capacity(MAX_REALTIME_EVENTS),
            parameter_events: Vec::with_capacity(MAX_REALTIME_EVENTS),
            maximum_frames,
        })
    }

    pub fn process(
        &mut self,
        frames: usize,
        incoming: impl IntoIterator<Item = Midi2Event>,
        parameters: impl IntoIterator<Item = ParameterEventV1>,
        left: &mut [f32],
        right: &mut [f32],
        level: f32,
    ) -> Result<()> {
        if frames > self.maximum_frames || left.len() < frames || right.len() < frames {
            bail!("VST3 process block exceeds its negotiated buffers");
        }
        self.events.clear();
        for event in incoming.into_iter().take(MAX_REALTIME_EVENTS) {
            self.events.push(event);
        }
        // Stable: two events the host placed on one frame keep its order.
        self.events.sort_by_key(|event| event.frame);
        self.parameter_events.clear();
        for event in parameters.into_iter().take(MAX_REALTIME_EVENTS) {
            self.parameter_events.push(event);
        }
        self.parameter_events
            .sort_unstable_by_key(|event| event.frame);
        let output = &mut self.interleaved[..frames * 2];
        output.fill(0.0);
        self.instance.process_wide(
            &[],
            output,
            frames as u32,
            0,
            2,
            &self.events,
            &self.parameter_events,
        )?;
        for frame in 0..frames {
            left[frame] = output[frame * 2] * level;
            right[frame] = output[frame * 2 + 1] * level;
        }
        Ok(())
    }

    pub fn save_state(&mut self) -> Result<Vec<u8>> {
        self.instance.save_state()
    }

    pub fn load_state(&mut self, state: &[u8]) -> Result<()> {
        self.instance.load_state(state)
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct VstParameterValue {
    pub index: u32,
    pub value: f64,
}

#[derive(Clone, Debug)]
#[cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "these package metadata fields feed the Windows WebView UI"
    )
)]
pub struct VstPluginModel {
    pub package_root: PathBuf,
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub branding: Option<PluginBranding>,
    pub web_api_version: u16,
    pub play_entry: String,
    pub config_entry: Option<String>,
    pub resources: Vec<ResourceRequirement>,
    pub initial_sound_id: Option<String>,
    pub schema: ParameterSchema,
    pub initial_values: Vec<VstParameterValue>,
    pub preset_names: BTreeMap<String, String>,
    pub preset_banks: BTreeMap<String, Option<String>>,
    pub preset_values: BTreeMap<String, Vec<VstParameterValue>>,
}

/// The storage browser this host serves, or `None` if it could not be opened.
#[cfg(windows)]
pub(crate) fn resource_browser() -> Option<Arc<NativeResourceBrowser>> {
    RESOURCES.get_or_init(open_resource_browser).clone()
}

/// Grants outlive the DAW session, so they are kept in the RackForge root
/// beside the desktop's: one machine, one set of installs.
///
/// A root that cannot be written is not fatal. The host stops claiming it can
/// configure anything -- see `config_available` -- rather than offering a
/// surface whose first request would fail.
#[cfg(windows)]
fn open_resource_browser() -> Option<Arc<NativeResourceBrowser>> {
    let root = rackforge_root()
        .inspect_err(|error| {
            crate::diagnostic::write(format!("resource browser: no RackForge root: {error:#}"));
        })
        .ok()?;
    let grants = root.join("state/resource-grants.json");
    if let Some(parent) = grants.parent() {
        fs::create_dir_all(parent)
            .inspect_err(|error| {
                crate::diagnostic::write(format!(
                    "resource browser: cannot create {}: {error}",
                    parent.display()
                ));
            })
            .ok()?;
    }
    NativeResourceBrowser::platform_defaults_persistent(grants)
        .inspect_err(|error| {
            crate::diagnostic::write(format!("resource browser: {error}"));
        })
        .ok()
        .map(Arc::new)
}

/// The instruments the DAW can switch between: every instrument the player
/// has installed and enabled in RackForge, at its newest version, Concert
/// Grand first and the rest by name.
///
/// The list is the player's, not the DLL's. It used to be exactly what the
/// build carried, so an instrument installed from a file or from a plugin's
/// own release never reached the DAW, and one the player had uninstalled or
/// switched off was offered anyway. The packages this build carries are laid
/// down first, by the same official install every host uses, so a machine
/// that has never run RackForge still opens with them.
///
/// INSTRUMENTS. A VST3 instrument plug-in offers a list of things to play,
/// and installed packages are not all playable: the Standard edition
/// travels with RF-Comp, RF-EQ and RF-Limiter beside the instruments, and
/// `open_package` rightly refuses an effect. Unfiltered, the first effect
/// in the list failed `setActive` and the WHOLE plug-in failed to activate
/// -- Live reporting only "could not be activated (error: false)" while the
/// scan and the load had both succeeded. The effects belong to the chain a
/// PLAY surface builds, not to the list of instruments the DAW switches
/// between.
pub fn load_plugin_models() -> Result<Vec<VstPluginModel>> {
    get_or_try_init_cloned(&MODELS, &MODELS_INIT, || {
        let root = rackforge_root()?;
        ensure_carried_installed()?;
        Ok(models_from_roots(
            &root,
            instrument_roots(&root.join("plugin-store")),
        ))
    })
}

/// The models of these packages. A package that cannot be modelled is left
/// out and logged: one broken instrument must not take the others with it.
fn models_from_roots(root: &Path, packages: Vec<PathBuf>) -> Vec<VstPluginModel> {
    packages
        .into_iter()
        .filter_map(|package_root| {
            load_plugin_model_at(root, package_root.clone())
                .inspect_err(|error| {
                    crate::diagnostic::write(format!(
                        "plugin model {} left out: {error:#}",
                        package_root.display()
                    ));
                })
                .ok()
        })
        .collect()
}

/// The newest version of every enabled instrument in `store` that the VST3
/// can run -- a portable component, since it loads no native code -- with
/// Concert Grand first and the rest by name.
fn instrument_roots(store: &Path) -> Vec<PathBuf> {
    let mut instruments = Vec::new();
    for plugin in fs::read_dir(store.join("packages"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let Some(plugin_id) = plugin.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !plugin_is_enabled(store, &plugin_id).unwrap_or(false) {
            continue;
        }
        let Some(root) = newest_package_root(store, &plugin_id) else {
            continue;
        };
        let Ok(package) = PluginPackage::open(&root) else {
            continue;
        };
        let manifest = package.manifest();
        if manifest.kind != PluginKind::Instrument || manifest.portable_component().is_none() {
            continue;
        }
        instruments.push((
            manifest.id != DEFAULT_INSTRUMENT_ID,
            manifest.name.to_lowercase(),
            root,
        ));
    }
    instruments.sort();
    instruments.into_iter().map(|(_, _, root)| root).collect()
}

fn load_plugin_model_at(root: &Path, package_root: PathBuf) -> Result<VstPluginModel> {
    let package = PluginPackage::open(&package_root)?;
    let manifest = package.manifest();
    let component = manifest
        .portable_component()
        .context("the VST3 instrument has no portable component")?;
    let schema: ParameterSchema = serde_json::from_slice(
        &fs::read(package_root.join(&component.parameter_schema)).with_context(|| {
            format!(
                "reading VST3 parameter schema {}",
                component.parameter_schema
            )
        })?,
    )
    .context("parsing the VST3 parameter schema")?;
    schema.validate().context("validating VST3 parameters")?;
    let play_entry = manifest
        .web_ui
        .as_ref()
        .and_then(|web| {
            web.surfaces
                .iter()
                .find(|surface| surface.kind == WebSurfaceKind::Play)
        })
        .map(|surface| surface.entry.clone())
        .context("the VST3 instrument does not provide a PLAY web surface")?;
    let config_entry = manifest.web_ui.as_ref().and_then(|web| {
        web.surfaces
            .iter()
            .find(|surface| surface.kind == WebSurfaceKind::Config)
            .map(|surface| surface.entry.clone())
    });

    let runtime =
        unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), Some(&root.join("data"))) }
            .context("loading the VST3 UI model")?;
    let runtime = register_runtime(runtime);
    let mut instance = runtime
        .create_instance()
        .context("creating the VST3 UI model")?;
    let catalog = instance.preset_catalog()?;
    let initial_sound_id = catalog.presets.first().map(|preset| preset.id.clone());
    if let Some(preset) = catalog.presets.first() {
        instance.load_preset(&preset.id)?;
    }
    let initial_values = parameter_values(&mut instance, &schema)?;
    let mut preset_values = BTreeMap::new();
    let mut preset_names = BTreeMap::new();
    let mut preset_banks = BTreeMap::new();
    for preset in &catalog.presets {
        instance.load_preset(&preset.id)?;
        preset_names.insert(preset.id.clone(), preset.name.clone());
        preset_banks.insert(preset.id.clone(), preset.bank.clone());
        preset_values.insert(preset.id.clone(), parameter_values(&mut instance, &schema)?);
    }

    Ok(VstPluginModel {
        package_root,
        plugin_id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        branding: manifest.branding.clone(),
        web_api_version: manifest.web_ui.as_ref().map_or(0, |web| web.api_version),
        play_entry,
        config_entry,
        resources: manifest.resources.clone(),
        initial_sound_id,
        schema,
        initial_values,
        preset_names,
        preset_banks,
        preset_values,
    })
}

fn parameter_values(
    instance: &mut PluginInstance<'static>,
    schema: &ParameterSchema,
) -> Result<Vec<VstParameterValue>> {
    schema
        .parameters
        .iter()
        .map(|parameter| {
            Ok(VstParameterValue {
                index: parameter.index,
                value: instance.get_parameter(parameter.index)?,
            })
        })
        .collect()
}

fn get_or_try_init_cloned<T, F>(cache: &OnceLock<T>, gate: &Mutex<()>, initialize: F) -> Result<T>
where
    T: Clone,
    F: FnOnce() -> Result<T>,
{
    if let Some(value) = cache.get() {
        return Ok(value.clone());
    }
    let _guard = gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(value) = cache.get() {
        return Ok(value.clone());
    }
    let value = initialize()?;
    let _ = cache.set(value);
    Ok(cache.get().expect("guarded VST3 cache initialized").clone())
}

fn ensure_carried_installed() -> Result<()> {
    get_or_try_init_cloned(&CARRIED_INSTALLED, &CARRIED_INSTALLED_INIT, || {
        install_carried_packages_at(&rackforge_root()?);
        Ok(())
    })
}

/// Lays down every package this build carries through the store's official
/// install, the rule every host follows: the release corrects a same-version
/// copy with other bytes, a plugin the player uninstalled stays out, a
/// newcomer is enabled, and versions older than the two newest are pruned.
///
/// A package that fails is logged and skipped. Failing the lot left a plug-in
/// with no instruments -- no model, no view and nothing to draw: the editor
/// opened black.
fn install_carried_packages_at(root: &Path) {
    let store = root.join("plugin-store");
    for path in [
        store.join("packages"),
        store.join("records"),
        root.join("data").join("plugins"),
    ] {
        if let Err(error) = fs::create_dir_all(&path) {
            crate::diagnostic::write(format!("creating {}: {error}", path.display()));
        }
    }
    let carried = BUNDLED_PLUGIN
        .map(|bytes| ("Concert Grand", bytes))
        .into_iter()
        .chain(BUNDLED_OFFICIAL_PLUGINS.iter().copied());
    for (name, bytes) in carried {
        match install_bundled_official_archive(&store, bytes) {
            Ok(BundledInstall::Installed { .. }) => {}
            Ok(BundledInstall::RemovedByPlayer { plugin_id, .. }) => {
                crate::diagnostic::write(format!(
                    "carried {plugin_id} skipped: removed by the player"
                ));
            }
            Err(error) => {
                crate::diagnostic::write(format!("installing carried plugin {name}: {error}"));
            }
        }
    }
}

#[cfg(test)]
fn load_plugin_models_at(root: &Path) -> Vec<VstPluginModel> {
    install_carried_packages_at(root);
    models_from_roots(root, instrument_roots(&root.join("plugin-store")))
}

/// Where the engine opens `plugin_id`: the newest version the store holds,
/// as every other host plays it.
///
/// This used to prefer the copy the DLL carried over anything installed, so a
/// newer version from a plugin's own release never reached the DAW, and it
/// compared the rest as text, which ranks 0.9.0 above 0.10.0.
fn package_root_for_id(plugin_id: &str) -> Result<PathBuf> {
    // Carried packages are installed into the store before anything opens.
    ensure_carried_installed()?;
    newest_package_root(&rackforge_root()?.join("plugin-store"), plugin_id)
        .with_context(|| format!("RackForge VST3 has no installed plugin {plugin_id}"))
}

/// The highest semantic version of `plugin_id` installed in `store`.
fn newest_package_root(store: &Path, plugin_id: &str) -> Option<PathBuf> {
    fs::read_dir(store.join("packages").join(plugin_id))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|version| {
            let root = version.path();
            let package = PluginPackage::open(&root).ok()?;
            let version = semver::Version::parse(&package.manifest().version).ok()?;
            Some((version, root))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, root)| root)
}

impl Drop for RackForgeEngine {
    fn drop(&mut self) {
        let _ = self.instance.deactivate();
        LIVE_INSTANCES.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) fn rackforge_root() -> Result<PathBuf> {
    if let Some(explicit) = env::var_os("RACKFORGE_ROOT") {
        return Ok(PathBuf::from(explicit));
    }
    let standard = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or(env::current_dir()?)
        .join("RackForge");
    let bootstrap = standard.join("bootstrap.toml");
    if !bootstrap.is_file() {
        return Ok(standard);
    }
    let document: RootDocument = toml::from_str(&fs::read_to_string(&bootstrap)?)?;
    if document.schema_version != 1
        || !matches!(document.mode.as_str(), "standard" | "custom")
        || !document.root.is_absolute()
    {
        bail!(
            "invalid RackForge root bootstrap at {}",
            bootstrap.display()
        );
    }
    Ok(document.root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            Arc, Barrier,
            atomic::{AtomicU64, AtomicUsize, Ordering},
        },
        thread,
        time::Duration,
    };

    static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(0);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn create() -> Self {
            loop {
                let sequence = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
                let path = env::temp_dir().join(format!(
                    "rackforge-vst3-test-{}-{sequence}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("creating isolated VST3 test root: {error}"),
                }
            }
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn fallible_cache_initializes_once_under_contention() {
        const THREADS: usize = 8;
        let cache = Arc::new(OnceLock::new());
        let gate = Arc::new(Mutex::new(()));
        let barrier = Arc::new(Barrier::new(THREADS));
        let initializations = Arc::new(AtomicUsize::new(0));
        let handles = (0..THREADS)
            .map(|_| {
                let cache = Arc::clone(&cache);
                let gate = Arc::clone(&gate);
                let barrier = Arc::clone(&barrier);
                let initializations = Arc::clone(&initializations);
                thread::spawn(move || {
                    barrier.wait();
                    get_or_try_init_cloned(&cache, &gate, || {
                        initializations.fetch_add(1, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(20));
                        Ok(42_u32)
                    })
                    .unwrap()
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            assert_eq!(handle.join().unwrap(), 42);
        }
        assert_eq!(initializations.load(Ordering::SeqCst), 1);
    }

    /// A package directory in `store`, enough for `PluginPackage::open`.
    fn plant(store: &Path, id: &str, name: &str, version: &str, portable: bool) -> PathBuf {
        let package = store.join("packages").join(id).join(version);
        fs::create_dir_all(&package).unwrap();
        let code = if portable {
            "[component]\nabi = \"wasm-v1\"\npath = \"component.wasm\"\n\
             runtime_descriptor = \"metadata/runtime.json\"\n\
             parameter_schema = \"metadata/parameters.json\"\n\
             preset_catalog = \"metadata/presets.json\"\n"
        } else {
            "[binaries]\nlinux-aarch64 = \"lib/synth.so\"\n"
        };
        fs::write(
            package.join("rackforge-plugin.toml"),
            format!(
                "schema_version = 1\nid = \"{id}\"\nname = \"{name}\"\n\
                 vendor = \"RackForge Test\"\nversion = \"{version}\"\nkind = \"instrument\"\n\
                 state_version = 1\ncapabilities = [\"audio_output\", \"midi_input\"]\n\
                 ui_layouts = [\"little@1\"]\n\n[api]\nmajor = 1\nminor = 5\n\n{code}"
            ),
        )
        .unwrap();
        package
    }

    #[test]
    fn the_engine_opens_the_newest_semantic_version() {
        let root = TestRoot::create();
        let store = root.path().join("plugin-store");
        for version in ["0.9.0", "0.10.0", "0.2.0"] {
            plant(&store, "org.rackforge.synth", "Synth", version, false);
        }
        assert_eq!(
            newest_package_root(&store, "org.rackforge.synth"),
            Some(store.join("packages/org.rackforge.synth/0.10.0"))
        );
        assert_eq!(newest_package_root(&store, "org.rackforge.other"), None);
    }

    /// The DAW offers what the player has: every enabled instrument in the
    /// store at its newest version, whether a release carried it or not,
    /// and nothing they switched off or that the VST3 cannot run.
    #[test]
    fn the_instrument_list_is_the_players_store() {
        let root = TestRoot::create();
        let store = root.path().join("plugin-store");
        assert!(instrument_roots(&store).is_empty());

        plant(&store, "org.rackforge.zither", "Zither", "1.0.0", true);
        plant(&store, "org.rackforge.alto", "alto", "0.9.0", true);
        let newest_alto = plant(&store, "org.rackforge.alto", "alto", "0.10.0", true);
        let grand = plant(
            &store,
            DEFAULT_INSTRUMENT_ID,
            "RF - Concert Grand",
            "1.0.0",
            true,
        );
        plant(&store, "org.rackforge.native", "Native", "1.0.0", false);
        plant(&store, "org.rackforge.off", "Off", "1.0.0", true);
        rackforge_repository::set_plugin_enabled(&store, "org.rackforge.off", false).unwrap();

        assert_eq!(
            instrument_roots(&store),
            vec![
                grand,
                newest_alto,
                store.join("packages/org.rackforge.zither/1.0.0"),
            ]
        );
    }

    /// Every instrument the build carried reaches the store -- and NO
    /// effect does.
    ///
    /// This counted `BUNDLED_OFFICIAL_PLUGINS.len() + 1` and so asserted the
    /// bug: the Standard edition carries RF-Comp, RF-EQ and RF-Limiter
    /// beside the instruments, `open_package` refuses an effect, and
    /// `setActive` opens every model -- so the first effect failed and the
    /// whole plug-in failed to activate. Live said only "could not be
    /// activated (error: false)" after reporting the scan and the load
    /// successful. It survived because both guards below are true in CI,
    /// where nothing is carried: the test returned before asserting
    /// anything. It needs a build with a bundle to have teeth.
    #[test]
    fn a_configured_bundle_exposes_its_instruments_and_no_effect() {
        if BUNDLED_PLUGIN.is_none() || BUNDLED_OFFICIAL_PLUGINS.is_empty() {
            return;
        }
        let root = TestRoot::create();
        let ids = load_plugin_models_at(root.path())
            .into_iter()
            .map(|model| model.plugin_id)
            .collect::<Vec<_>>();
        assert_eq!(ids.first().map(String::as_str), Some(DEFAULT_INSTRUMENT_ID));
        let store = root.path().join("plugin-store");
        let mut instruments = 0usize;
        let mut effects = Vec::new();
        for plugin in fs::read_dir(store.join("packages")).unwrap().flatten() {
            let plugin_id = plugin.file_name().to_string_lossy().into_owned();
            let path = newest_package_root(&store, &plugin_id).expect("an installed version");
            let package = PluginPackage::open(&path).unwrap();
            if package.manifest().kind == PluginKind::Instrument {
                instruments += 1;
            } else {
                effects.push(package.manifest().id.clone());
            }
        }
        assert_eq!(
            ids.len(),
            instruments,
            "an instrument was carried but never offered: {ids:?}"
        );
        for effect in effects {
            assert!(
                !ids.contains(&effect),
                "the effect {effect} is offered as an instrument; setActive will refuse it"
            );
        }
    }

    /// A store left holding an older build of a version the release also
    /// carries used to fail the install, and a plug-in with no instruments has
    /// no model, no view and nothing to draw — the editor opened black. This
    /// is that store.
    #[test]
    fn a_drifted_bundled_version_is_replaced_rather_than_refused() {
        if BUNDLED_PLUGIN.is_none() {
            return;
        }
        let root = TestRoot::create();
        install_carried_packages_at(root.path());
        let store = root.path().join("plugin-store");
        let version_root =
            newest_package_root(&store, DEFAULT_INSTRUMENT_ID).expect("the install placed it");
        let version = version_root
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a version directory")
            .to_owned();

        // The record is what the conflict is decided against, so this is where
        // an older build of the same version shows: same identity, different
        // bytes. Planting a file inside the package would not reproduce it,
        // because nothing reads the directory back.
        let record = store
            .join("records")
            .join(DEFAULT_INSTRUMENT_ID)
            .join(format!("{version}.json"));
        let text = fs::read_to_string(&record).expect("reading the installation record");
        let key = "\"artifact_sha256\": \"";
        let start = text.find(key).expect("the record carries a digest") + key.len();
        let end = start + 64;
        let drifted = format!("{}{}{}", &text[..start], "0".repeat(64), &text[end..]);
        fs::write(&record, drifted).expect("planting the drift");

        assert!(
            load_plugin_models_at(root.path())
                .iter()
                .any(|model| model.plugin_id == DEFAULT_INSTRUMENT_ID),
            "a release must be able to correct a version that drifted"
        );
        let corrected = fs::read_to_string(&record).expect("reading the corrected record");
        assert!(!corrected.contains(&"0".repeat(64)));
    }

    /// An instrument the player uninstalled stays out of the DAW too: the
    /// VST3 used to install everything its DLL carried and offer it again.
    #[test]
    fn a_carried_instrument_the_player_removed_is_not_offered() {
        if BUNDLED_PLUGIN.is_none() {
            return;
        }
        let root = TestRoot::create();
        install_carried_packages_at(root.path());
        let store = root.path().join("plugin-store");
        rackforge_repository::uninstall_plugin(&store, DEFAULT_INSTRUMENT_ID).unwrap();
        assert!(
            !load_plugin_models_at(root.path())
                .iter()
                .any(|model| model.plugin_id == DEFAULT_INSTRUMENT_ID)
        );
        assert!(!store.join("packages").join(DEFAULT_INSTRUMENT_ID).exists());
    }

    #[test]
    fn bundled_rf106_preserves_sound_banks_for_its_web_contract() {
        if !BUNDLED_OFFICIAL_PLUGINS
            .iter()
            .any(|(name, _)| *name == "RF-106")
        {
            return;
        }
        let root = TestRoot::create();
        let model = load_plugin_models_at(root.path())
            .into_iter()
            .find(|model| model.plugin_id == "org.rackforge.rf-106")
            .expect("bundled RF-106 model");
        assert!(!model.preset_banks.is_empty());
        assert!(model.preset_names.keys().all(|id| {
            model
                .preset_banks
                .get(id)
                .and_then(Option::as_deref)
                .is_some_and(|bank| !bank.is_empty())
        }));
    }
}

#[cfg(test)]
mod unload_tests {
    use super::*;

    /// With nothing registered and nothing live there is nothing to unload,
    /// and the hook must say so without touching Wasmtime at all.
    #[test]
    fn an_empty_registry_unloads_nothing_and_does_not_panic() {
        LIVE_INSTANCES.store(0, Ordering::SeqCst);
        RUNTIMES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        assert_eq!(unload_runtimes(), UnloadOutcome::Unloaded { runtimes: 0 });
    }

    /// A host that unloads the module while a component is still alive is
    /// breaking its own contract; the only safe answer is to leave every
    /// runtime where it is and say why, never to free under a live borrower.
    #[test]
    fn a_live_instance_keeps_every_runtime_in_place() {
        LIVE_INSTANCES.store(1, Ordering::SeqCst);
        let outcome = unload_runtimes();
        LIVE_INSTANCES.store(0, Ordering::SeqCst);
        assert_eq!(outcome, UnloadOutcome::LeftInPlace { live_instances: 1 });
    }
}
