use anyhow::{Context, Result, bail};
use rackforge_core::{LoadedPlugin, PluginInstance, PluginPackage};
use rackforge_plugin_api::{
    ParameterSchema, PluginKind, WebSurfaceKind,
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

static BUNDLED_PACKAGE: OnceLock<PathBuf> = OnceLock::new();

pub struct RackForgeEngine {
    instance: PluginInstance<'static>,
    interleaved: Vec<f32>,
    events: Vec<MidiEventV1>,
    parameter_events: Vec<ParameterEventV1>,
    maximum_frames: usize,
}

impl RackForgeEngine {
    pub fn open(sample_rate: f64, maximum_frames: usize) -> Result<Self> {
        if !sample_rate.is_finite() || sample_rate <= 0.0 || maximum_frames == 0 {
            bail!("invalid VST3 processing setup");
        }
        let root = rackforge_root()?;
        let data = root.join("data");
        let package_root = active_package_root()?;
        let package = PluginPackage::open(&package_root)?;
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
    pub description: String,
    pub accent_color: String,
    pub play_entry: String,
    pub schema: ParameterSchema,
    pub initial_values: Vec<VstParameterValue>,
    pub preset_values: BTreeMap<String, Vec<VstParameterValue>>,
}

pub fn load_plugin_model() -> Result<VstPluginModel> {
    let root = rackforge_root()?;
    let package_root = active_package_root()?;
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

    let runtime =
        unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), Some(&root.join("data"))) }
            .context("loading the VST3 UI model")?;
    let runtime: &'static LoadedPlugin = Box::leak(Box::new(runtime));
    let mut instance = runtime
        .create_instance()
        .context("creating the VST3 UI model")?;
    let catalog = instance.preset_catalog()?;
    if let Some(preset) = catalog.presets.first() {
        instance.load_preset(&preset.id)?;
    }
    let initial_values = parameter_values(&mut instance, &schema)?;
    let mut preset_values = BTreeMap::new();
    for preset in &catalog.presets {
        instance.load_preset(&preset.id)?;
        preset_values.insert(preset.id.clone(), parameter_values(&mut instance, &schema)?);
    }

    Ok(VstPluginModel {
        package_root,
        plugin_id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone().unwrap_or_default(),
        accent_color: manifest
            .branding
            .as_ref()
            .and_then(|branding| branding.accent_color.clone())
            .unwrap_or_else(|| "#5de1f3".to_owned()),
        play_entry,
        schema,
        initial_values,
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

fn active_package_root() -> Result<PathBuf> {
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
    if let Some(bytes) = BUNDLED_PLUGIN {
        if let Some(path) = BUNDLED_PACKAGE.get() {
            return Ok(path.clone());
        }
        let installed = install_local_archive(&store, bytes)
            .context("installing the VST3 bundled instrument")?
            .path;
        let _ = BUNDLED_PACKAGE.set(installed.clone());
        return Ok(BUNDLED_PACKAGE.get().cloned().unwrap_or(installed));
    }
    newest_installed_instrument(&store)?
        .context("RackForge VST3 has no bundled or installed instrument")
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
}
