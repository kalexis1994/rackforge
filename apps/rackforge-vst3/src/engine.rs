use anyhow::{Context, Result, bail};
use rackforge_core::{LoadedPlugin, PluginInstance, PluginPackage};
use rackforge_plugin_api::{
    ParameterSchema, PluginBranding, PluginKind, ResourceRequirement, WebSurfaceKind,
    abi::{MidiEventV1, ParameterEventV1},
};
use rackforge_repository::install_local_archive;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::OnceLock,
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
static BUNDLED_MODELS: OnceLock<Vec<VstPluginModel>> = OnceLock::new();

pub struct RackForgeEngine {
    instance: PluginInstance<'static>,
    interleaved: Vec<f32>,
    events: Vec<MidiEventV1>,
    parameter_events: Vec<ParameterEventV1>,
    maximum_frames: usize,
}

impl RackForgeEngine {
    pub fn open_plugin(plugin_id: &str, sample_rate: f64, maximum_frames: usize) -> Result<Self> {
        let package_root = package_root_for_id(plugin_id)?;
        Self::open_package(&package_root, sample_rate, maximum_frames)
    }

    fn open_package(package_root: &Path, sample_rate: f64, maximum_frames: usize) -> Result<Self> {
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
        let runtime: &'static LoadedPlugin = Box::leak(Box::new(runtime));
        let mut instance = runtime.create_instance()?;
        let catalog = instance.preset_catalog()?;
        if let Some(preset) = catalog.presets.first() {
            instance.load_preset(&preset.id)?;
        }
        instance.activate(sample_rate, maximum_frames as u32, 0, 2)?;

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
        incoming: impl IntoIterator<Item = MidiEventV1>,
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
        self.events.sort_unstable_by_key(|event| event.frame);
        self.parameter_events.clear();
        for event in parameters.into_iter().take(MAX_REALTIME_EVENTS) {
            self.parameter_events.push(event);
        }
        self.parameter_events
            .sort_unstable_by_key(|event| event.frame);
        let output = &mut self.interleaved[..frames * 2];
        output.fill(0.0);
        self.instance.process_interleaved(
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
    if let Some(models) = BUNDLED_MODELS.get() {
        return Ok(models.clone());
    }
    let models = bundled_package_roots()?
        .iter()
        .cloned()
        .map(load_plugin_model_at)
        .collect::<Result<Vec<_>>>()?;
    let _ = BUNDLED_MODELS.set(models);
    Ok(BUNDLED_MODELS
        .get()
        .expect("bundled VST3 models initialized")
        .clone())
}

fn load_plugin_model_at(package_root: PathBuf) -> Result<VstPluginModel> {
    let root = rackforge_root()?;
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
    let runtime: &'static LoadedPlugin = Box::leak(Box::new(runtime));
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

fn bundled_package_roots() -> Result<&'static Vec<PathBuf>> {
    if let Some(paths) = BUNDLED_PACKAGES.get() {
        return Ok(paths);
    }
    let root = rackforge_root()?;
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
    for (name, bytes) in [
        ("Concert Grand", BUNDLED_PLUGIN),
        ("RF-106", BUNDLED_RF106_PLUGIN),
    ] {
        if let Some(bytes) = bytes {
            installed.push(
                install_local_archive(&store, bytes)
                    .with_context(|| format!("installing bundled VST3 plugin {name}"))?
                    .path,
            );
        }
    }
    let _ = BUNDLED_PACKAGES.set(installed);
    Ok(BUNDLED_PACKAGES
        .get()
        .expect("bundled VST3 packages initialized"))
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

    #[test]
    fn missing_store_has_no_fallback_instrument() {
        let root = env::temp_dir().join(format!("rackforge-vst3-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("packages")).unwrap();
        assert_eq!(newest_installed_instrument(&root).unwrap(), None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn configured_bundle_exposes_both_official_instruments() {
        if BUNDLED_PLUGIN.is_none() || BUNDLED_RF106_PLUGIN.is_none() {
            return;
        }
        let ids = load_bundled_plugin_models()
            .unwrap()
            .into_iter()
            .map(|model| model.plugin_id)
            .collect::<Vec<_>>();
        assert_eq!(ids, ["org.rackforge.concert-grand", "org.rackforge.rf-106"]);
    }

    #[test]
    fn bundled_rf106_preserves_sound_banks_for_its_web_contract() {
        if BUNDLED_RF106_PLUGIN.is_none() {
            return;
        }
        let model = load_bundled_plugin_models()
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
