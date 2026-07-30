use crate::{AddonStorage, PluginPackage};
use anyhow::{Context, Result, anyhow, bail};
use libloading::{Library, Symbol};
use rackforge_plugin_api::abi::{
    ABI_VERSION, ENTRY_SYMBOL_V1, HostApiV1, LOG_LEVEL_DEBUG, LOG_LEVEL_ERROR, LOG_LEVEL_INFO,
    LOG_LEVEL_TRACE, LOG_LEVEL_WARN, MidiEventV1, PROGRAM_EXTENSION_ENTRY_SYMBOL_V1,
    ParameterEventV1, PluginApiV1, PluginEntryFnV1, ProcessBlockV1, ProgramExchangeJsonFnV1,
    ProgramExtensionApiV1, ProgramExtensionEntryFnV1, STATUS_INTERNAL_ERROR,
    STATUS_INVALID_ARGUMENT, STATUS_OK, SURFACE_EXTENSION_ENTRY_SYMBOL_V1, SurfaceExtensionApiV1,
    SurfaceExtensionEntryFnV1, copy_to_host_buffer, is_compatible, is_program_extension_compatible,
    is_surface_extension_compatible,
};
use rackforge_plugin_api::{
    Capability, ParameterSchema, PluginManifest, PreparedProgram, PresetCatalog, ProgramDocument,
    ProgramEditRequest, RuntimeDescriptor, SurfaceActivationRequest, SurfaceActivationResponse,
};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Mutex;

const MAX_METADATA_BYTES: usize = 1024 * 1024;

pub struct LoadedPlugin {
    manifest: PluginManifest,
    descriptor: RuntimeDescriptor,
    parameters: ParameterSchema,
    presets: PresetCatalog,
    api: &'static PluginApiV1,
    program_extension: Option<&'static ProgramExtensionApiV1>,
    surface_extension: Option<&'static SurfaceExtensionApiV1>,
    _host_context: Box<HostContext>,
    host_api: Box<HostApiV1>,
    _library: Library,
}

impl LoadedPlugin {
    /// Loads and validates a native plugin package.
    ///
    /// # Safety
    ///
    /// The selected dynamic library executes native code in the host process.
    /// Callers must only load trusted RackForge packages.
    pub unsafe fn load(
        package: &PluginPackage,
        binary_override: Option<&Path>,
        resource_overrides: &BTreeMap<String, PathBuf>,
        data_root: Option<&Path>,
    ) -> Result<Self> {
        let resources = package.resolve_resources(resource_overrides)?;
        let resource_paths = resources
            .into_iter()
            .map(|(id, path)| {
                let text = path
                    .to_str()
                    .with_context(|| format!("resource path is not UTF-8: {}", path.display()))?;
                Ok((id, text.as_bytes().to_vec()))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let addon_data_path = data_root
            .map(|root| {
                let directory = AddonStorage::new(root).ensure_addon(&package.manifest().id)?;
                let text = directory.root.to_str().with_context(|| {
                    format!("addon data path is not UTF-8: {}", directory.root.display())
                })?;
                Ok::<_, anyhow::Error>(text.as_bytes().to_vec())
            })
            .transpose()?;
        let binary = match binary_override {
            Some(path) => path.to_path_buf(),
            None => package.binary_path()?,
        };
        // SAFETY: loading native code is the explicit responsibility of this
        // function's caller.
        let library = unsafe { Library::new(&binary) }
            .with_context(|| format!("loading {}", binary.display()))?;
        // SAFETY: the exported symbol is part of the RackForge ABI contract.
        let entry: Symbol<PluginEntryFnV1> =
            unsafe { library.get(ENTRY_SYMBOL_V1) }.with_context(|| {
                format!(
                    "{} does not export rackforge_plugin_entry_v1",
                    binary.display()
                )
            })?;
        // SAFETY: the entry function is required to return a process-lifetime
        // static table owned by the loaded library.
        let api_pointer = unsafe { entry() };
        let api =
            unsafe { api_pointer.as_ref() }.context("plugin entry returned a null API table")?;
        if api.struct_size < size_of::<PluginApiV1>() as u32 {
            bail!("plugin API table is smaller than ABI v1");
        }
        if !is_compatible(api.api_version) {
            bail!(
                "plugin ABI {:#010x} is incompatible with host ABI {ABI_VERSION:#010x}",
                api.api_version
            );
        }
        let program_extension = match unsafe {
            library.get::<ProgramExtensionEntryFnV1>(PROGRAM_EXTENSION_ENTRY_SYMBOL_V1)
        } {
            Ok(entry) => {
                let pointer = unsafe { entry() };
                let extension = unsafe { pointer.as_ref() }
                    .context("program extension entry returned a null API table")?;
                if extension.struct_size < size_of::<ProgramExtensionApiV1>() as u32 {
                    bail!("program extension API table is truncated");
                }
                if !is_program_extension_compatible(extension.api_version) {
                    bail!(
                        "program extension ABI {:#010x} is incompatible",
                        extension.api_version
                    );
                }
                Some(extension)
            }
            Err(_) => None,
        };
        let surface_extension = match unsafe {
            library.get::<SurfaceExtensionEntryFnV1>(SURFACE_EXTENSION_ENTRY_SYMBOL_V1)
        } {
            Ok(entry) => {
                // SAFETY: the optional symbol follows the same static-table
                // lifetime contract as the primary plugin entry.
                let pointer = unsafe { entry() };
                let extension = unsafe { pointer.as_ref() }
                    .context("surface extension entry returned a null API table")?;
                if extension.struct_size < size_of::<SurfaceExtensionApiV1>() as u32 {
                    bail!("surface extension API table is truncated");
                }
                if !is_surface_extension_compatible(extension.api_version) {
                    bail!(
                        "surface extension ABI {:#010x} is incompatible",
                        extension.api_version
                    );
                }
                Some(extension)
            }
            Err(_) => None,
        };

        let descriptor_bytes =
            read_plugin_bytes(api.runtime_descriptor_json, "runtime descriptor")?;
        let descriptor: RuntimeDescriptor =
            serde_json::from_slice(&descriptor_bytes).context("parsing runtime descriptor")?;
        descriptor
            .validate_against(package.manifest())
            .context("validating runtime descriptor")?;

        let parameter_bytes = read_plugin_bytes(api.parameter_schema_json, "parameter schema")?;
        let parameters: ParameterSchema =
            serde_json::from_slice(&parameter_bytes).context("parsing parameter schema")?;
        parameters
            .validate()
            .context("validating parameter schema")?;

        let preset_bytes = read_plugin_bytes(api.preset_catalog_json, "preset catalog")?;
        let presets: PresetCatalog =
            serde_json::from_slice(&preset_bytes).context("parsing preset catalog")?;
        presets.validate().context("validating preset catalog")?;
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

        let mut host_context = Box::new(HostContext {
            resource_paths,
            addon_data_path,
            dynamic_presets: Mutex::new(None),
        });
        let context = host_context.as_mut() as *mut HostContext as *mut c_void;
        let host_api = Box::new(HostApiV1::new(
            context,
            Some(host_log),
            Some(host_get_resource_path),
            Some(host_get_addon_data_path),
            Some(host_publish_preset_catalog),
        ));
        Ok(Self {
            manifest: package.manifest().clone(),
            descriptor,
            parameters,
            presets,
            api,
            program_extension,
            surface_extension,
            _host_context: host_context,
            host_api,
            _library: library,
        })
    }

    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn descriptor(&self) -> &RuntimeDescriptor {
        &self.descriptor
    }

    pub fn parameters(&self) -> &ParameterSchema {
        &self.parameters
    }

    pub fn presets(&self) -> &PresetCatalog {
        &self.presets
    }

    pub fn create_instance(&self) -> Result<PluginInstance<'_>> {
        // SAFETY: host_api is boxed for a stable address and remains alive for
        // at least as long as the returned borrowing instance.
        let handle = unsafe { (self.api.create)(self.host_api.as_ref()) };
        if handle.is_null() {
            bail!("plugin failed to create an instance");
        }
        Ok(PluginInstance {
            plugin: self,
            handle,
            active: false,
        })
    }
}

struct HostContext {
    resource_paths: BTreeMap<String, Vec<u8>>,
    addon_data_path: Option<Vec<u8>>,
    dynamic_presets: Mutex<Option<PresetCatalog>>,
}

pub struct PluginInstance<'plugin> {
    plugin: &'plugin LoadedPlugin,
    handle: *mut c_void,
    active: bool,
}

impl PluginInstance<'_> {
    pub fn supports_program_editing(&self) -> bool {
        self.plugin.program_extension.is_some()
    }

    pub fn activate_surface(
        &mut self,
        request: &SurfaceActivationRequest,
    ) -> Result<SurfaceActivationResponse> {
        request
            .validate()
            .context("validating surface activation request")?;
        let Some(extension) = self.plugin.surface_extension else {
            return Ok(SurfaceActivationResponse::focus(None));
        };
        let source =
            serde_json::to_vec(request).context("serializing surface activation request")?;
        let bytes = exchange_program_json(
            extension.activate,
            self.handle,
            &source,
            "activate addon surface",
        )?;
        let response: SurfaceActivationResponse =
            serde_json::from_slice(&bytes).context("parsing surface activation response")?;
        response
            .validate()
            .context("validating surface activation response")?;
        Ok(response)
    }

    pub fn begin_program_edit(&mut self, request: &ProgramEditRequest) -> Result<PreparedProgram> {
        request
            .validate()
            .context("validating program edit request")?;
        let extension = self
            .plugin
            .program_extension
            .context("addon does not expose the program editing extension")?;
        let source = serde_json::to_vec(request).context("serializing program edit request")?;
        let bytes = exchange_program_json(
            extension.begin_edit,
            self.handle,
            &source,
            "begin program edit",
        )?;
        let prepared: PreparedProgram =
            serde_json::from_slice(&bytes).context("parsing prepared program")?;
        prepared.validate().context("validating prepared program")?;
        Ok(prepared)
    }

    pub fn prepare_program_save(&mut self, document: &ProgramDocument) -> Result<PreparedProgram> {
        document.validate().context("validating program draft")?;
        let extension = self
            .plugin
            .program_extension
            .context("addon does not expose the program editing extension")?;
        let source = serde_json::to_vec(document).context("serializing program draft")?;
        let bytes = exchange_program_json(
            extension.prepare_save,
            self.handle,
            &source,
            "prepare program save",
        )?;
        let prepared: PreparedProgram =
            serde_json::from_slice(&bytes).context("parsing prepared program")?;
        prepared.validate().context("validating prepared program")?;
        Ok(prepared)
    }

    pub fn install_program(&mut self, prepared: &PreparedProgram) -> Result<()> {
        prepared.validate().context("validating program install")?;
        let extension = self
            .plugin
            .program_extension
            .context("addon does not expose the program editing extension")?;
        let source = serde_json::to_vec(prepared).context("serializing program install")?;
        let status = unsafe { (extension.install)(self.handle, source.as_ptr(), source.len()) };
        check_status(status, "install_program")
    }

    pub fn preset_catalog(&self) -> Result<PresetCatalog> {
        let dynamic = self
            .plugin
            ._host_context
            .dynamic_presets
            .lock()
            .map_err(|_| anyhow!("dynamic preset catalog lock is poisoned"))?;
        Ok(dynamic
            .as_ref()
            .cloned()
            .unwrap_or_else(|| self.plugin.presets.clone()))
    }

    pub fn activate(
        &mut self,
        sample_rate: f64,
        maximum_frames: u32,
        input_channels: u32,
        output_channels: u32,
    ) -> Result<()> {
        if !sample_rate.is_finite() || sample_rate <= 0.0 || maximum_frames == 0 {
            bail!("invalid activation configuration");
        }
        let status = unsafe {
            (self.plugin.api.activate)(
                self.handle,
                sample_rate,
                maximum_frames,
                input_channels,
                output_channels,
            )
        };
        check_status(status, "activate")?;
        self.active = true;
        Ok(())
    }

    pub fn deactivate(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        let status = unsafe { (self.plugin.api.deactivate)(self.handle) };
        check_status(status, "deactivate")?;
        self.active = false;
        Ok(())
    }

    pub fn reset(&mut self) -> Result<()> {
        let status = unsafe { (self.plugin.api.reset)(self.handle) };
        check_status(status, "reset")
    }

    pub fn set_parameter(&mut self, index: u32, value: f64) -> Result<()> {
        if !value.is_finite() {
            bail!("parameter value must be finite");
        }
        let status = unsafe { (self.plugin.api.set_parameter)(self.handle, index, value) };
        check_status(status, "set_parameter")
    }

    pub fn get_parameter(&self, index: u32) -> Result<f64> {
        let mut value = 0.0;
        let status = unsafe { (self.plugin.api.get_parameter)(self.handle, index, &mut value) };
        check_status(status, "get_parameter")?;
        if !value.is_finite() {
            bail!("plugin returned a non-finite parameter value");
        }
        Ok(value)
    }

    pub fn save_state(&self) -> Result<Vec<u8>> {
        let required = unsafe { (self.plugin.api.save_state)(self.handle, ptr::null_mut(), 0) };
        if required > MAX_METADATA_BYTES {
            bail!("plugin state is unreasonably large: {required} bytes");
        }
        let mut bytes = vec![0_u8; required];
        let reported =
            unsafe { (self.plugin.api.save_state)(self.handle, bytes.as_mut_ptr(), bytes.len()) };
        if reported != required {
            bail!("plugin state changed size while being saved");
        }
        Ok(bytes)
    }

    pub fn load_state(&mut self, bytes: &[u8]) -> Result<()> {
        let status =
            unsafe { (self.plugin.api.load_state)(self.handle, bytes.as_ptr(), bytes.len()) };
        check_status(status, "load_state")
    }

    pub fn load_preset(&mut self, preset_id: &str) -> Result<()> {
        let dynamic = self.preset_catalog()?;
        let declared = dynamic
            .presets
            .iter()
            .chain(self.plugin.presets.presets.iter())
            .any(|preset| preset.id == preset_id);
        if !declared {
            bail!("plugin does not declare preset {preset_id:?}");
        }
        let status = unsafe {
            (self.plugin.api.load_preset)(self.handle, preset_id.as_ptr(), preset_id.len())
        };
        check_status(status, "load_preset")
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
        if !self.active {
            bail!("plugin instance is not active");
        }
        if frames == 0 {
            bail!("process block must contain at least one frame");
        }
        let midi_event_count: u32 = midi_events
            .len()
            .try_into()
            .context("too many MIDI events in process block")?;
        let parameter_event_count: u32 = parameter_events
            .len()
            .try_into()
            .context("too many parameter events in process block")?;
        let required_input = frames as usize * input_channels as usize;
        let required_output = frames as usize * output_channels as usize;
        if input.len() != required_input {
            bail!(
                "input has {} samples, expected {required_input}",
                input.len()
            );
        }
        if output.len() != required_output {
            bail!(
                "output has {} samples, expected {required_output}",
                output.len()
            );
        }
        if midi_events.iter().any(|event| event.frame >= frames)
            || parameter_events.iter().any(|event| event.frame >= frames)
        {
            bail!("event frame lies outside the process block");
        }

        let block = ProcessBlockV1 {
            struct_size: size_of::<ProcessBlockV1>() as u32,
            frames,
            input_channels,
            output_channels,
            input_interleaved: if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
            output_interleaved: if output.is_empty() {
                ptr::null_mut()
            } else {
                output.as_mut_ptr()
            },
            midi_events: if midi_events.is_empty() {
                ptr::null()
            } else {
                midi_events.as_ptr()
            },
            midi_event_count,
            parameter_events: if parameter_events.is_empty() {
                ptr::null()
            } else {
                parameter_events.as_ptr()
            },
            parameter_event_count,
        };
        let status = unsafe { (self.plugin.api.process)(self.handle, &block) };
        check_status(status, "process")
    }
}

impl Drop for PluginInstance<'_> {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                (self.plugin.api.deactivate)(self.handle);
            }
        }
        unsafe {
            (self.plugin.api.destroy)(self.handle);
        }
    }
}

fn read_plugin_bytes(
    writer: unsafe extern "C" fn(*mut u8, usize) -> usize,
    label: &str,
) -> Result<Vec<u8>> {
    let required = unsafe { writer(ptr::null_mut(), 0) };
    if required == 0 {
        bail!("plugin returned empty {label}");
    }
    if required > MAX_METADATA_BYTES {
        bail!("plugin {label} is unreasonably large: {required} bytes");
    }
    let mut bytes = vec![0_u8; required];
    let reported = unsafe { writer(bytes.as_mut_ptr(), bytes.len()) };
    if reported != required {
        bail!("plugin {label} changed size while being read");
    }
    Ok(bytes)
}

fn exchange_program_json(
    exchange: ProgramExchangeJsonFnV1,
    instance: *mut c_void,
    source: &[u8],
    label: &str,
) -> Result<Vec<u8>> {
    let required = unsafe { exchange(instance, source.as_ptr(), source.len(), ptr::null_mut(), 0) };
    if required == 0 {
        bail!("addon returned an empty response for {label}");
    }
    if required > MAX_METADATA_BYTES {
        bail!("addon response for {label} is unreasonably large: {required} bytes");
    }
    let mut bytes = vec![0_u8; required];
    let reported = unsafe {
        exchange(
            instance,
            source.as_ptr(),
            source.len(),
            bytes.as_mut_ptr(),
            bytes.len(),
        )
    };
    if reported != required {
        bail!("addon response for {label} changed size while being read");
    }
    Ok(bytes)
}

fn check_status(status: i32, operation: &str) -> Result<()> {
    if status == STATUS_OK {
        Ok(())
    } else {
        Err(anyhow!("{operation} failed with plugin status {status}"))
    }
}

unsafe extern "C" fn host_log(_context: *mut c_void, level: u32, text: *const u8, length: usize) {
    if text.is_null() || length == 0 {
        return;
    }
    // SAFETY: the callback ABI requires a readable buffer for the call.
    let bytes = unsafe { std::slice::from_raw_parts(text, length) };
    let message = String::from_utf8_lossy(bytes);
    let label = match level {
        LOG_LEVEL_TRACE => "trace",
        LOG_LEVEL_DEBUG => "debug",
        LOG_LEVEL_INFO => "info",
        LOG_LEVEL_WARN => "warn",
        LOG_LEVEL_ERROR => "error",
        _ => "unknown",
    };
    eprintln!("[plugin {label}] {message}");
}

unsafe extern "C" fn host_get_resource_path(
    context: *mut c_void,
    resource_id: *const u8,
    resource_id_length: usize,
    destination: *mut u8,
    capacity: usize,
) -> usize {
    if context.is_null() || resource_id.is_null() || resource_id_length == 0 {
        return 0;
    }
    // SAFETY: the callback contract provides a valid HostContext and readable
    // resource identifier for the duration of this call.
    let Some(host) = (unsafe { context.cast::<HostContext>().as_ref() }) else {
        return 0;
    };
    let id_bytes = unsafe { std::slice::from_raw_parts(resource_id, resource_id_length) };
    let Ok(id) = std::str::from_utf8(id_bytes) else {
        return 0;
    };
    let Some(path) = host.resource_paths.get(id) else {
        return 0;
    };
    // SAFETY: the host caller owns and describes the destination buffer.
    unsafe { copy_to_host_buffer(path, destination, capacity) }
}

unsafe extern "C" fn host_get_addon_data_path(
    context: *mut c_void,
    destination: *mut u8,
    capacity: usize,
) -> usize {
    if context.is_null() {
        return 0;
    }
    // SAFETY: the callback contract provides a valid HostContext for the
    // duration of this call.
    let Some(host) = (unsafe { context.cast::<HostContext>().as_ref() }) else {
        return 0;
    };
    let Some(path) = host.addon_data_path.as_deref() else {
        return 0;
    };
    // SAFETY: the plugin owns and describes the destination buffer.
    unsafe { copy_to_host_buffer(path, destination, capacity) }
}

unsafe extern "C" fn host_publish_preset_catalog(
    context: *mut c_void,
    source: *const u8,
    length: usize,
) -> i32 {
    if context.is_null() || source.is_null() || length == 0 || length > MAX_METADATA_BYTES {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: the callback contract provides a valid HostContext and readable
    // source bytes for this call.
    let Some(host) = (unsafe { context.cast::<HostContext>().as_ref() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let bytes = unsafe { std::slice::from_raw_parts(source, length) };
    let Ok(catalog) = serde_json::from_slice::<PresetCatalog>(bytes) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if catalog.validate().is_err() {
        return STATUS_INVALID_ARGUMENT;
    }
    let Ok(mut dynamic) = host.dynamic_presets.lock() else {
        return STATUS_INTERNAL_ERROR;
    };
    *dynamic = Some(catalog);
    STATUS_OK
}
