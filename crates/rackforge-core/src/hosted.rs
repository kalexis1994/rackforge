use crate::{PluginPackage, PluginStorage, loader::NativeLoadedPlugin};
use anyhow::{Context, Result, bail};
use rackforge_plugin_api::abi::{MidiEventV1, ParameterEventV1};
use rackforge_plugin_api::{
    Capability, ParameterSchema, PluginManifest, PreparedProgram, PresetCatalog, ProgramDocument,
    ProgramEditRequest, ProgramEditorView, ProgramFieldEditRequest, ResourceKind,
    RuntimeDescriptor, SurfaceActivationRequest, SurfaceActivationResponse,
};
use rackforge_plugin_runtime::{
    MidiEvent, ParameterEvent, PortableEngine, PortableInstance as WasmInstance, PortableModule,
    RuntimeLimits,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_REALTIME_EVENTS: usize = 4096;
const MAX_STATIC_METADATA_BYTES: u64 = 1024 * 1024;

/// One validated RackForge plugin, independent of its execution backend.
pub struct LoadedPlugin {
    backend: LoadedBackend,
}

enum LoadedBackend {
    Native(NativeLoadedPlugin),
    Portable(PortableLoadedPlugin),
}

impl LoadedPlugin {
    /// Loads either the legacy trusted native ABI or the sandboxed portable
    /// ABI selected by the package manifest.
    ///
    /// # Safety
    ///
    /// Native packages execute code in the host process. Portable packages do
    /// not require caller trust, but share this entry point so callers cannot
    /// accidentally bypass the package's declared runtime.
    pub unsafe fn load(
        package: &PluginPackage,
        binary_override: Option<&Path>,
        resource_overrides: &BTreeMap<String, PathBuf>,
        data_root: Option<&Path>,
    ) -> Result<Self> {
        if package.manifest().portable_component().is_some() {
            if binary_override.is_some() {
                bail!("a binary override cannot replace a portable component");
            }
            Ok(Self {
                backend: LoadedBackend::Portable(PortableLoadedPlugin::load(
                    package,
                    resource_overrides,
                    data_root,
                )?),
            })
        } else {
            // SAFETY: forwarded from this function's native-package contract.
            Ok(Self {
                backend: LoadedBackend::Native(unsafe {
                    NativeLoadedPlugin::load(
                        package,
                        binary_override,
                        resource_overrides,
                        data_root,
                    )?
                }),
            })
        }
    }

    pub fn manifest(&self) -> &PluginManifest {
        match &self.backend {
            LoadedBackend::Native(plugin) => plugin.manifest(),
            LoadedBackend::Portable(plugin) => &plugin.manifest,
        }
    }

    pub fn descriptor(&self) -> &RuntimeDescriptor {
        match &self.backend {
            LoadedBackend::Native(plugin) => plugin.descriptor(),
            LoadedBackend::Portable(plugin) => &plugin.descriptor,
        }
    }

    pub fn parameters(&self) -> &ParameterSchema {
        match &self.backend {
            LoadedBackend::Native(plugin) => plugin.parameters(),
            LoadedBackend::Portable(plugin) => &plugin.parameters,
        }
    }

    pub fn presets(&self) -> &PresetCatalog {
        match &self.backend {
            LoadedBackend::Native(plugin) => plugin.presets(),
            LoadedBackend::Portable(plugin) => &plugin.presets,
        }
    }

    pub fn create_instance(&self) -> Result<PluginInstance<'_>> {
        let backend = match &self.backend {
            LoadedBackend::Native(plugin) => {
                PluginInstanceBackend::Native(plugin.create_instance()?)
            }
            LoadedBackend::Portable(plugin) => {
                PluginInstanceBackend::Portable(plugin.create_instance()?)
            }
        };
        Ok(PluginInstance { backend })
    }
}

pub struct PortableLoadedPlugin {
    manifest: PluginManifest,
    descriptor: RuntimeDescriptor,
    parameters: ParameterSchema,
    presets: PresetCatalog,
    resources: BTreeMap<String, PathBuf>,
    data_root: Option<PathBuf>,
    module: PortableModule,
}

impl PortableLoadedPlugin {
    fn load(
        package: &PluginPackage,
        resource_overrides: &BTreeMap<String, PathBuf>,
        data_root: Option<&Path>,
    ) -> Result<Self> {
        let component = package
            .manifest()
            .portable_component()
            .context("portable package has no component declaration")?;
        let descriptor: RuntimeDescriptor = read_json(
            &package.root().join(&component.runtime_descriptor),
            "runtime descriptor",
        )?;
        descriptor
            .validate_against(package.manifest())
            .context("validating portable runtime descriptor")?;
        let parameters: ParameterSchema = read_json(
            &package.root().join(&component.parameter_schema),
            "parameter schema",
        )?;
        parameters
            .validate()
            .context("validating portable parameter schema")?;
        let presets: PresetCatalog = read_json(
            &package.root().join(&component.preset_catalog),
            "preset catalog",
        )?;
        presets
            .validate()
            .context("validating portable preset catalog")?;
        let declares_presets = package
            .manifest()
            .capabilities
            .contains(&Capability::Presets);
        if declares_presets != !presets.presets.is_empty() {
            bail!(
                "preset capability and catalog disagree: capability={declares_presets} presets={}",
                presets.presets.len()
            );
        }

        let mut resources = BTreeMap::new();
        for (id, path) in package.resolve_resources(resource_overrides, data_root)? {
            let requirement = package
                .manifest()
                .resources
                .iter()
                .find(|resource| resource.id == id)
                .context("resolved undeclared resource")?;
            if requirement.kind == ResourceKind::Directory {
                bail!(
                    "portable resource {id:?} is a directory; wasm-v1 currently accepts files only"
                );
            }
            resources.insert(id, path);
        }

        let component_path = package
            .component_path()?
            .context("portable component path is unavailable")?;
        let component_bytes = fs::read(&component_path)
            .with_context(|| format!("reading {}", component_path.display()))?;
        let requested_memory = component
            .memory_limit_mib
            .unwrap_or(64)
            .checked_mul(1024 * 1024)
            .context("portable memory request overflow")? as usize;
        let limits = RuntimeLimits {
            maximum_memory_bytes: requested_memory,
            ..RuntimeLimits::default()
        };
        let engine = match data_root {
            Some(root) => PortableEngine::with_cache(limits, root.join(".cache/portable-code"))?,
            None => PortableEngine::new(limits)?,
        };
        let module = engine.compile(&component_bytes)?;
        Ok(Self {
            manifest: package.manifest().clone(),
            descriptor,
            parameters,
            presets,
            resources,
            data_root: data_root.map(Path::to_path_buf),
            module,
        })
    }

    fn create_instance(&self) -> Result<PortablePluginInstance> {
        let mut instance = self.module.instantiate()?;
        for (id, path) in &self.resources {
            instance
                .load_resource_file(id, path)
                .with_context(|| format!("delivering portable resource {id:?}"))?;
        }
        let saved_programs = match &self.data_root {
            Some(root) => PluginStorage::new(root)
                .list_programs(&self.manifest.id)
                .context("loading saved portable programs")?,
            None => Vec::new(),
        };
        for document in &saved_programs {
            let source =
                serde_json::to_vec(document).context("serializing saved portable program")?;
            let bytes = instance
                .prepare_program_save(&source)
                .with_context(|| format!("preparing saved program {:?}", document.id))?;
            let prepared: PreparedProgram = serde_json::from_slice(&bytes)
                .with_context(|| format!("parsing saved program {:?}", document.id))?;
            prepared
                .validate()
                .with_context(|| format!("validating saved program {:?}", document.id))?;
            let prepared_bytes = serde_json::to_vec(&prepared)
                .context("serializing prepared saved portable program")?;
            instance
                .install_program(&prepared_bytes)
                .with_context(|| format!("installing saved program {:?}", document.id))?;
        }
        let presets = match instance.preset_catalog()? {
            Some(bytes) => {
                let catalog: PresetCatalog = serde_json::from_slice(&bytes)
                    .context("parsing portable dynamic preset catalog")?;
                catalog
                    .validate()
                    .context("validating portable dynamic preset catalog")?;
                if !self.manifest.capabilities.contains(&Capability::Presets)
                    && !catalog.presets.is_empty()
                {
                    bail!("portable plugin published presets without declaring the capability");
                }
                catalog
            }
            None => self.presets.clone(),
        };
        Ok(PortablePluginInstance {
            instance,
            presets,
            allows_presets: self.manifest.capabilities.contains(&Capability::Presets),
            active: false,
            midi_scratch: Vec::with_capacity(MAX_REALTIME_EVENTS),
            parameter_scratch: Vec::with_capacity(MAX_REALTIME_EVENTS),
        })
    }
}

pub struct PluginInstance<'plugin> {
    backend: PluginInstanceBackend<'plugin>,
}

enum PluginInstanceBackend<'plugin> {
    Native(crate::loader::NativePluginInstance<'plugin>),
    Portable(PortablePluginInstance),
}

impl PluginInstance<'_> {
    pub fn load_resource_file(&mut self, id: &str, path: impl AsRef<Path>) -> Result<()> {
        match &mut self.backend {
            PluginInstanceBackend::Portable(instance) => instance
                .instance
                .load_resource_file(id, path)
                .with_context(|| format!("loading portable resource {id:?}")),
            PluginInstanceBackend::Native(_) => {
                bail!("native plugin resources are fixed when the plugin is loaded")
            }
        }
    }

    pub fn supports_program_editing(&self) -> bool {
        match &self.backend {
            PluginInstanceBackend::Native(instance) => instance.supports_program_editing(),
            PluginInstanceBackend::Portable(instance) => {
                instance.instance.supports_program_editing()
            }
        }
    }

    pub fn activate_surface(
        &mut self,
        request: &SurfaceActivationRequest,
    ) -> Result<SurfaceActivationResponse> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.activate_surface(request),
            PluginInstanceBackend::Portable(_) => {
                request
                    .validate()
                    .context("validating surface activation request")?;
                Ok(SurfaceActivationResponse::focus(None))
            }
        }
    }

    pub fn begin_program_edit(&mut self, request: &ProgramEditRequest) -> Result<PreparedProgram> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.begin_program_edit(request),
            PluginInstanceBackend::Portable(instance) => {
                request
                    .validate()
                    .context("validating program edit request")?;
                let source = serde_json::to_vec(request)
                    .context("serializing portable program edit request")?;
                let bytes = instance.instance.begin_program_edit(&source)?;
                let prepared: PreparedProgram =
                    serde_json::from_slice(&bytes).context("parsing portable prepared program")?;
                prepared
                    .validate()
                    .context("validating portable prepared program")?;
                Ok(prepared)
            }
        }
    }

    pub fn prepare_program_save(&mut self, document: &ProgramDocument) -> Result<PreparedProgram> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.prepare_program_save(document),
            PluginInstanceBackend::Portable(instance) => {
                document.validate().context("validating program draft")?;
                let source =
                    serde_json::to_vec(document).context("serializing portable program draft")?;
                let bytes = instance.instance.prepare_program_save(&source)?;
                let prepared: PreparedProgram =
                    serde_json::from_slice(&bytes).context("parsing portable prepared program")?;
                prepared
                    .validate()
                    .context("validating portable prepared program")?;
                Ok(prepared)
            }
        }
    }

    pub fn install_program(&mut self, prepared: &PreparedProgram) -> Result<()> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.install_program(prepared),
            PluginInstanceBackend::Portable(instance) => {
                prepared
                    .validate()
                    .context("validating portable program install")?;
                let source =
                    serde_json::to_vec(prepared).context("serializing portable program install")?;
                instance.instance.install_program(&source)?;
                instance.refresh_presets()
            }
        }
    }

    pub fn preview_program(&mut self, prepared: &PreparedProgram) -> Result<bool> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.preview_program(prepared),
            PluginInstanceBackend::Portable(instance) => {
                prepared
                    .validate()
                    .context("validating portable program preview")?;
                let source =
                    serde_json::to_vec(prepared).context("serializing portable program preview")?;
                instance.instance.preview_program(&source)
            }
        }
    }

    pub fn program_editor_view(&mut self, document: &ProgramDocument) -> Result<ProgramEditorView> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.program_editor_view(document),
            PluginInstanceBackend::Portable(instance) => {
                document
                    .validate()
                    .context("validating portable editor program")?;
                let source =
                    serde_json::to_vec(document).context("serializing portable editor program")?;
                let bytes = instance.instance.program_editor_view(&source)?;
                let view: ProgramEditorView = serde_json::from_slice(&bytes)
                    .context("parsing portable program editor view")?;
                view.validate()
                    .context("validating portable program editor view")?;
                Ok(view)
            }
        }
    }

    pub fn apply_program_edit(
        &mut self,
        request: &ProgramFieldEditRequest,
    ) -> Result<PreparedProgram> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.apply_program_edit(request),
            PluginInstanceBackend::Portable(instance) => {
                request
                    .validate()
                    .context("validating portable program field edit")?;
                let source = serde_json::to_vec(request)
                    .context("serializing portable program field edit")?;
                let bytes = instance.instance.apply_program_edit(&source)?;
                let prepared: PreparedProgram =
                    serde_json::from_slice(&bytes).context("parsing portable edited program")?;
                prepared
                    .validate()
                    .context("validating portable edited program")?;
                Ok(prepared)
            }
        }
    }

    pub fn preset_catalog(&self) -> Result<PresetCatalog> {
        match &self.backend {
            PluginInstanceBackend::Native(instance) => instance.preset_catalog(),
            PluginInstanceBackend::Portable(instance) => Ok(instance.presets.clone()),
        }
    }

    pub fn activate(
        &mut self,
        sample_rate: f64,
        maximum_frames: u32,
        input_channels: u32,
        output_channels: u32,
    ) -> Result<()> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => {
                instance.activate(sample_rate, maximum_frames, input_channels, output_channels)
            }
            PluginInstanceBackend::Portable(instance) => {
                instance.instance.prepare(
                    sample_rate,
                    maximum_frames,
                    input_channels,
                    output_channels,
                )?;
                instance.active = true;
                Ok(())
            }
        }
    }

    pub fn deactivate(&mut self) -> Result<()> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.deactivate(),
            PluginInstanceBackend::Portable(instance) => {
                if instance.active {
                    instance.instance.reset()?;
                    instance.active = false;
                }
                Ok(())
            }
        }
    }

    pub fn reset(&mut self) -> Result<()> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.reset(),
            PluginInstanceBackend::Portable(instance) => instance.instance.reset(),
        }
    }

    pub fn set_parameter(&mut self, index: u32, value: f64) -> Result<()> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.set_parameter(index, value),
            PluginInstanceBackend::Portable(instance) => {
                instance.instance.set_parameter(index, value)
            }
        }
    }

    pub fn get_parameter(&mut self, index: u32) -> Result<f64> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.get_parameter(index),
            PluginInstanceBackend::Portable(instance) => instance.instance.get_parameter(index),
        }
    }

    pub fn save_state(&mut self) -> Result<Vec<u8>> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.save_state(),
            PluginInstanceBackend::Portable(instance) => instance.instance.save_state(),
        }
    }

    pub fn load_state(&mut self, bytes: &[u8]) -> Result<()> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.load_state(bytes),
            PluginInstanceBackend::Portable(instance) => instance.instance.load_state(bytes),
        }
    }

    pub fn load_preset(&mut self, preset_id: &str) -> Result<()> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.load_preset(preset_id),
            PluginInstanceBackend::Portable(instance) => {
                if !instance
                    .presets
                    .presets
                    .iter()
                    .any(|preset| preset.id == preset_id)
                {
                    bail!("plugin does not declare preset {preset_id:?}");
                }
                instance.instance.load_preset(preset_id)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
        input_channels: u32,
        output_channels: u32,
        midi_events: &[MidiEventV1],
        parameter_events: &[ParameterEventV1],
    ) -> Result<()> {
        match &mut self.backend {
            PluginInstanceBackend::Native(instance) => instance.process_interleaved(
                input,
                output,
                frames,
                input_channels,
                output_channels,
                midi_events,
                parameter_events,
            ),
            PluginInstanceBackend::Portable(instance) => instance.process_interleaved(
                input,
                output,
                frames,
                input_channels,
                output_channels,
                midi_events,
                parameter_events,
            ),
        }
    }

    /// Reports the fuel consumed by the most recent portable process call.
    /// Native plugins are not metered by the WebAssembly sandbox.
    pub fn last_realtime_fuel_consumed(&self) -> Option<u64> {
        match &self.backend {
            PluginInstanceBackend::Native(_) => None,
            PluginInstanceBackend::Portable(instance) => {
                Some(instance.instance.last_realtime_fuel_consumed())
            }
        }
    }
}

pub struct PortablePluginInstance {
    instance: WasmInstance,
    presets: PresetCatalog,
    allows_presets: bool,
    active: bool,
    midi_scratch: Vec<MidiEvent>,
    parameter_scratch: Vec<ParameterEvent>,
}

impl PortablePluginInstance {
    fn refresh_presets(&mut self) -> Result<()> {
        let Some(bytes) = self.instance.preset_catalog()? else {
            return Ok(());
        };
        let catalog: PresetCatalog =
            serde_json::from_slice(&bytes).context("parsing refreshed portable preset catalog")?;
        catalog
            .validate()
            .context("validating refreshed portable preset catalog")?;
        if !self.allows_presets && !catalog.presets.is_empty() {
            bail!("portable plugin published presets without declaring the capability");
        }
        self.presets = catalog;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
        input_channels: u32,
        output_channels: u32,
        midi_events: &[MidiEventV1],
        parameter_events: &[ParameterEventV1],
    ) -> Result<()> {
        if !self.active {
            bail!("plugin instance is not active");
        }
        if midi_events.len() > MAX_REALTIME_EVENTS || parameter_events.len() > MAX_REALTIME_EVENTS {
            bail!("process block exceeds RackForge's real-time event limit");
        }
        self.midi_scratch.clear();
        for event in midi_events {
            let length = event.length as usize;
            if length == 0 || length > event.data.len() {
                bail!("MIDI event has an invalid length");
            }
            self.midi_scratch
                .push(MidiEvent::new(event.frame, &event.data[..length])?);
        }
        self.parameter_scratch.clear();
        self.parameter_scratch
            .extend(parameter_events.iter().map(|event| ParameterEvent {
                frame: event.frame,
                index: event.parameter_index,
                value: event.value,
            }));
        if input.len() != frames as usize * input_channels as usize
            || output.len() != frames as usize * output_channels as usize
        {
            bail!("audio buffers do not match the declared process block");
        }
        self.instance.process_interleaved_with_events(
            input,
            output,
            frames,
            &self.midi_scratch,
            &self.parameter_scratch,
        )
    }
}

impl Drop for PortablePluginInstance {
    fn drop(&mut self) {
        if self.active {
            let _ = self.instance.reset();
        }
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    let metadata = fs::metadata(path).with_context(|| format!("reading {label} metadata"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_STATIC_METADATA_BYTES {
        bail!("portable plugin {label} has an invalid size");
    }
    let bytes = fs::read(path).with_context(|| format!("reading {label} {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing portable {label}"))
}
