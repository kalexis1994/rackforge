use anyhow::{Context, Result, bail};
use rackforge_core::{LoadedPlugin, PluginInstance, PluginPackage, midi2::Midi2Event};
use rackforge_plugin_api::{
    ParameterSchema, PluginBranding, PluginKind, ResourceRequirement, WebSurfaceKind,
    abi::ParameterEventV1,
};
use rackforge_repository::install_local_archive_replacing;
use serde::Deserialize;
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

static BUNDLED_PACKAGES: OnceLock<Vec<PathBuf>> = OnceLock::new();
static BUNDLED_PACKAGES_INIT: Mutex<()> = Mutex::new(());
static BUNDLED_MODELS: OnceLock<Vec<VstPluginModel>> = OnceLock::new();
static BUNDLED_MODELS_INIT: Mutex<()> = Mutex::new(());

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

pub fn load_bundled_plugin_models() -> Result<Vec<VstPluginModel>> {
    get_or_try_init_cloned(&BUNDLED_MODELS, &BUNDLED_MODELS_INIT, || {
        let root = rackforge_root()?;
        bundled_package_roots()?
            .into_iter()
            .map(|package_root| load_plugin_model_at(&root, package_root))
            .collect::<Result<Vec<_>>>()
    })
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

fn bundled_package_roots() -> Result<Vec<PathBuf>> {
    get_or_try_init_cloned(&BUNDLED_PACKAGES, &BUNDLED_PACKAGES_INIT, || {
        install_bundled_packages_at(&rackforge_root()?)
    })
}

fn install_bundled_packages_at(root: &Path) -> Result<Vec<PathBuf>> {
    let store = root.join("plugin-store");
    let data = root.join("data");
    for path in [
        store.join("packages"),
        store.join("records"),
        data.join("plugins"),
    ] {
        fs::create_dir_all(&path).with_context(|| format!("creating {}", path.display()))?;
    }
    let mut installed = Vec::new();
    let carried = std::iter::once(("Concert Grand", BUNDLED_PLUGIN)).chain(
        BUNDLED_OFFICIAL_PLUGINS
            .iter()
            .map(|(name, bytes)| (*name, Some(*bytes))),
    );
    for (name, bytes) in carried {
        if let Some(bytes) = bytes {
            // The release is the authority on what a bundled version holds, the
            // same rule the desktop and the platform installers follow. Without
            // it a store left holding an older build of the same version made
            // every install fail, and a plug-in with no instruments has no
            // model, no view and nothing to draw: the editor opened black.
            installed.push(
                install_local_archive_replacing(&store, bytes)
                    .with_context(|| format!("installing bundled VST3 plugin {name}"))?
                    .path,
            );
        }
    }
    Ok(installed)
}

#[cfg(test)]
fn load_bundled_plugin_models_at(root: &Path) -> Result<Vec<VstPluginModel>> {
    install_bundled_packages_at(root)?
        .into_iter()
        .map(|package_root| load_plugin_model_at(root, package_root))
        .collect()
}

fn package_root_for_id(plugin_id: &str) -> Result<PathBuf> {
    if let Some(path) = bundled_package_roots()?.iter().find(|path| {
        PluginPackage::open(path).is_ok_and(|package| package.manifest().id == plugin_id)
    }) {
        return Ok(path.clone());
    }
    let store = rackforge_root()?.join("plugin-store");
    let packages = store.join("packages").join(plugin_id);
    let mut candidates = Vec::new();
    for version in fs::read_dir(packages).into_iter().flatten().flatten() {
        let root = version.path();
        if PluginPackage::open(&root).is_ok() {
            candidates.push(root);
        }
    }
    candidates.sort();
    candidates
        .pop()
        .with_context(|| format!("RackForge VST3 has no installed plugin {plugin_id}"))
}

impl Drop for RackForgeEngine {
    fn drop(&mut self) {
        let _ = self.instance.deactivate();
        LIVE_INSTANCES.fetch_sub(1, Ordering::SeqCst);
    }
}

fn rackforge_root() -> Result<PathBuf> {
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
fn newest_installed_instrument(store: &Path) -> Result<Option<PathBuf>> {
    let packages = store.join("packages");
    let mut candidates = Vec::new();
    for plugin in fs::read_dir(packages).into_iter().flatten().flatten() {
        for version in fs::read_dir(plugin.path()).into_iter().flatten().flatten() {
            let root = version.path();
            if let Ok(package) = PluginPackage::open(&root)
                && package.manifest().kind == PluginKind::Instrument
            {
                candidates.push(root);
            }
        }
    }
    candidates.sort();
    Ok(candidates.pop())
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

    #[test]
    fn missing_store_has_no_fallback_instrument() {
        let root = TestRoot::create();
        fs::create_dir_all(root.path().join("packages")).unwrap();
        assert_eq!(newest_installed_instrument(root.path()).unwrap(), None);
    }

    /// Every instrument the build carried must reach the store: the plug-in
    /// ships the same official set the desktop does, not one chosen name.
    #[test]
    fn a_configured_bundle_exposes_every_instrument_it_carries() {
        if BUNDLED_PLUGIN.is_none() || BUNDLED_OFFICIAL_PLUGINS.is_empty() {
            return;
        }
        let root = TestRoot::create();
        let ids = load_bundled_plugin_models_at(root.path())
            .unwrap()
            .into_iter()
            .map(|model| model.plugin_id)
            .collect::<Vec<_>>();
        assert!(ids.contains(&"org.rackforge.concert-grand".to_owned()));
        assert_eq!(
            ids.len(),
            BUNDLED_OFFICIAL_PLUGINS.len() + 1,
            "an instrument was carried but never installed: {ids:?}"
        );
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
        let installed = install_bundled_packages_at(root.path()).expect("first install");
        let version_root = installed
            .first()
            .expect("the install placed a package")
            .clone();
        let version = version_root
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a version directory")
            .to_owned();
        let plugin_id = version_root
            .parent()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .expect("a plugin directory")
            .to_owned();

        // The record is what the conflict is decided against, so this is where
        // an older build of the same version shows: same identity, different
        // bytes. Planting a file inside the package would not reproduce it,
        // because nothing reads the directory back.
        let record = root
            .path()
            .join("plugin-store/records")
            .join(&plugin_id)
            .join(format!("{version}.json"));
        let text = fs::read_to_string(&record).expect("reading the installation record");
        let key = "\"artifact_sha256\": \"";
        let start = text.find(key).expect("the record carries a digest") + key.len();
        let end = start + 64;
        let drifted = format!("{}{}{}", &text[..start], "0".repeat(64), &text[end..]);
        fs::write(&record, drifted).expect("planting the drift");

        install_bundled_packages_at(root.path())
            .expect("a release must be able to correct a version that drifted");
        assert!(
            !load_bundled_plugin_models_at(root.path())
                .expect("models after replacement")
                .is_empty(),
            "the plug-in was left with no instrument to show"
        );
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
        let model = load_bundled_plugin_models_at(root.path())
            .unwrap()
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
