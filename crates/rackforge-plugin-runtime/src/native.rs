//! Wasmtime-backed host for RackForge `wasm-v1` processors.
//!
//! This is the backend used by every platform that runs RackForge as a native
//! process. The browser backend in [`crate::browser`] implements the same
//! public surface on top of the engine already present in the page.

use crate::shared::{
    PARALLEL_PLAN_ENTRY_BYTES, PARALLEL_PLAN_HEADER_BYTES, PROGRAM_EDIT_BASIC,
    PROGRAM_EDIT_DECLARATIVE, PROGRAM_EDIT_KNOWN_CAPABILITIES, PROGRAM_EDIT_PREVIEW, byte_range,
    check_status, checked_samples, memory_range, ranges_overlap, read_f32, validate_parallel_plan,
    validate_realtime_events, write_f32, write_midi, write_midi2, write_parameters,
};
use crate::{
    ABI_VERSION_V1, ABI_VERSION_V1_1, MAX_PARALLEL_UNITS, MidiEvent, MidiEvent2,
    PARALLEL_ABI_VERSION_V1, ParallelBlockPlan, ParallelLayout, ParallelPlanEntry, ParameterEvent,
    RuntimeLimits,
};
use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use wasmtime::{
    Cache, CacheConfig, Config, Engine, Instance, Memory, Module, OptLevel, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc,
};

struct HostState {
    limits: StoreLimits,
}

pub struct PortableEngine {
    engine: Engine,
    limits: RuntimeLimits,
}

impl PortableEngine {
    pub fn new(limits: RuntimeLimits) -> Result<Self> {
        Self::configured(limits, None)
    }

    /// Creates a compiler whose derived native code is cached below a
    /// RackForge-owned directory. The cache is disposable and never belongs in
    /// the plugin package.
    pub fn with_cache(limits: RuntimeLimits, directory: impl AsRef<Path>) -> Result<Self> {
        let directory = directory.as_ref();
        std::fs::create_dir_all(directory)
            .with_context(|| format!("creating portable code cache {}", directory.display()))?;
        let directory = std::fs::canonicalize(directory)
            .with_context(|| format!("resolving portable code cache {}", directory.display()))?;
        Self::configured(limits, Some(&directory))
    }

    fn configured(limits: RuntimeLimits, cache_directory: Option<&Path>) -> Result<Self> {
        let mut config = Config::new();
        config.cranelift_opt_level(OptLevel::Speed);
        config.consume_fuel(true);
        config.wasm_multi_memory(false);
        config.wasm_memory64(false);
        if let Some(directory) = cache_directory {
            let mut cache_config = CacheConfig::new();
            cache_config.with_directory(directory);
            config.cache(Some(Cache::new(cache_config).map_err(|error| {
                anyhow::anyhow!("creating RackForge portable code cache: {error}")
            })?));
        }
        Ok(Self {
            engine: Engine::new(&config).map_err(|error| {
                anyhow::anyhow!("creating RackForge WebAssembly engine: {error}")
            })?,
            limits,
        })
    }

    pub fn compile(&self, bytes: &[u8]) -> Result<PortableModule> {
        let module = Module::from_binary(&self.engine, bytes).map_err(|error| {
            anyhow::anyhow!("compiling RackForge WebAssembly component: {error}")
        })?;
        if let Some(import) = module.imports().next() {
            bail!(
                "wasm-v1 modules may not import host functions (found {}::{})",
                import.module(),
                import.name()
            );
        }
        Ok(PortableModule {
            module,
            limits: self.limits,
        })
    }
}

/// Takes Wasmtime's process-wide trap handlers back out of the process.
///
/// On Windows, Wasmtime installs a vectored exception handler and a vectored
/// continue handler the first time an `Engine` is created. They are
/// PROCESS-GLOBAL and point at code inside whichever DLL created them, and
/// an ordinary `Drop` of an `Engine` never removes them -- only
/// `Engine::unload_process_handlers` does, and it demands to hold the last
/// `Engine` handle in the whole process. A plug-in DLL that is unloaded
/// without doing this leaves a dangling function pointer at the head of the
/// host's exception chain, and the next exception anywhere in the host --
/// even a benign first-chance one it uses internally -- jumps into unmapped
/// memory. FL Studio 2025 died exactly that way on 2026-09-01, faulting in
/// `RackForge.vst3_unloaded` at Wasmtime's `exception_handler` symbol.
///
/// This consumes every module a host still holds, keeps one `Engine` handle
/// from the first, drops the rest so that handle is the last, and unloads.
/// Wasmtime asserts its preconditions with panics; a panic inside a DLL's
/// exit hook would abort the host, so the call is caught and reported
/// instead, and on failure the process is left as it was -- handlers in
/// place, which is the pre-existing leak, never a use-after-free.
///
/// # Safety
///
/// Wasmtime's contract, which the caller inherits: no other `Engine` may
/// exist anywhere in the process (every `Store` and `Module` is an `Engine`
/// clone, so all of them must be gone), and no thread that has run
/// WebAssembly through this copy of Wasmtime may run it again. A VST3
/// module's `ExitDll` satisfies both by the host's own contract: components
/// are released and audio is stopped before the module is unloaded.
pub unsafe fn unload_process_handlers(modules: Vec<PortableModule>) -> Result<(), String> {
    let Some(first) = modules.first() else {
        return Ok(());
    };
    let engine = first.module.engine().clone();
    drop(modules);
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        // SAFETY: forwarded from the caller, see above.
        unsafe { engine.unload_process_handlers() }
    }));
    attempt.map_err(|panic| {
        panic
            .downcast_ref::<&str>()
            .map(|message| (*message).to_owned())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "another Engine handle was still alive".to_owned())
    })
}

pub struct PortableModule {
    module: Module,
    limits: RuntimeLimits,
}

impl PortableModule {
    pub fn instantiate(&self) -> Result<PortableInstance> {
        let store_limits = StoreLimitsBuilder::new()
            .memory_size(self.limits.maximum_memory_bytes)
            .memories(1)
            .instances(1)
            .build();
        let mut store = Store::new(
            self.module.engine(),
            HostState {
                limits: store_limits,
            },
        );
        store.limiter(|state| &mut state.limits);
        store.set_fuel(self.limits.control_fuel_per_call)?;
        let instance = Instance::new(&mut store, &self.module, &[]).map_err(|error| {
            anyhow::anyhow!("instantiating RackForge WebAssembly plugin: {error}")
        })?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .context("wasm-v1 plugin does not export memory")?;
        let abi_version = typed::<(), i32>(&instance, &mut store, "rackforge_abi_version")?;
        let version = abi_version.call(&mut store, ())?;
        if !(ABI_VERSION_V1_1..=ABI_VERSION_V1).contains(&version) {
            bail!("unsupported wasm-v1 ABI version {version:#010x}");
        }
        let input_ptr = typed::<(), i32>(&instance, &mut store, "rackforge_input_ptr")?;
        let output_ptr = typed::<(), i32>(&instance, &mut store, "rackforge_output_ptr")?;
        let midi_ptr = typed::<(), i32>(&instance, &mut store, "rackforge_midi_ptr")?;
        let parameter_ptr = typed::<(), i32>(&instance, &mut store, "rackforge_parameter_ptr")?;
        let transfer_ptr = typed::<(), i32>(&instance, &mut store, "rackforge_transfer_ptr")?;
        let input_capacity =
            typed::<(), i32>(&instance, &mut store, "rackforge_capacity_input_samples")?
                .call(&mut store, ())?;
        let output_capacity =
            typed::<(), i32>(&instance, &mut store, "rackforge_capacity_output_samples")?
                .call(&mut store, ())?;
        if input_capacity < 0 || output_capacity < 0 {
            bail!("wasm-v1 plugin reported an invalid audio buffer capacity");
        }
        let midi_capacity =
            typed::<(), i32>(&instance, &mut store, "rackforge_capacity_midi_events")?
                .call(&mut store, ())?;
        if midi_capacity <= 0 {
            bail!("wasm-v1 plugin reported an invalid MIDI event capacity");
        }
        let parameter_capacity =
            typed::<(), i32>(&instance, &mut store, "rackforge_capacity_parameter_events")?
                .call(&mut store, ())?;
        if parameter_capacity <= 0 {
            bail!("wasm-v1 plugin reported an invalid parameter event capacity");
        }
        let transfer_capacity =
            typed::<(), i32>(&instance, &mut store, "rackforge_capacity_transfer_bytes")?
                .call(&mut store, ())?;
        if transfer_capacity <= 0 {
            bail!("wasm-v1 plugin reported an invalid transfer capacity");
        }
        let input_offset = input_ptr.call(&mut store, ())?;
        let output_offset = output_ptr.call(&mut store, ())?;
        let midi_offset = midi_ptr.call(&mut store, ())?;
        // Optional, like the program API: a component that wants MIDI at 2.0
        // widths exports a second region, its capacity, the families it
        // wants there, and a process entry that takes the extra count.
        let midi2 = match (
            optional_typed::<(), i32>(&instance, &mut store, "rackforge_midi2_ptr")?,
            optional_typed::<(), i32>(&instance, &mut store, "rackforge_capacity_midi2_events")?,
            optional_typed::<(), i32>(&instance, &mut store, "rackforge_midi2_families")?,
            optional_typed::<(i32, i32, i32, i32, i32, i32), i32>(
                &instance,
                &mut store,
                "rackforge_process_v2",
            )?,
        ) {
            (Some(ptr), Some(capacity), Some(families), Some(process_v2)) => {
                let capacity = capacity.call(&mut store, ())?;
                if capacity <= 0 {
                    bail!("component reported a non-positive wide-MIDI capacity");
                }
                Some((
                    ptr.call(&mut store, ())?,
                    capacity as usize,
                    families.call(&mut store, ())? as u32,
                    process_v2,
                ))
            }
            (None, None, None, None) => None,
            _ => bail!("component exports only part of the wide-MIDI contract"),
        };
        let parameter_offset = parameter_ptr.call(&mut store, ())?;
        let transfer_offset = transfer_ptr.call(&mut store, ())?;
        let initialize = typed::<(), i32>(&instance, &mut store, "rackforge_initialize")?;
        check_status(initialize.call(&mut store, ())?, "initialize")?;
        let program_api = match optional_typed::<(), i32>(
            &instance,
            &mut store,
            "rackforge_program_editing_capabilities",
        )? {
            None => None,
            Some(capabilities) => {
                store.set_fuel(self.limits.control_fuel_per_call)?;
                let capabilities = capabilities.call(&mut store, ())?;
                if capabilities < 0 {
                    bail!("portable plugin returned invalid program-editing capabilities");
                }
                let capabilities = capabilities as u32;
                if capabilities & !PROGRAM_EDIT_KNOWN_CAPABILITIES != 0
                    || capabilities != 0 && capabilities & PROGRAM_EDIT_BASIC == 0
                {
                    bail!(
                        "portable plugin returned unsupported program-editing capabilities {capabilities:#x}"
                    );
                }
                if capabilities == 0 {
                    None
                } else {
                    let exchange_input_ptr =
                        typed::<(), i32>(&instance, &mut store, "rackforge_exchange_input_ptr")?;
                    let exchange_input_offset = exchange_input_ptr.call(&mut store, ())?;
                    let memory_size = memory.data_size(&store);
                    let transfer_range = byte_range(
                        transfer_offset,
                        transfer_capacity as usize,
                        1,
                        1,
                        memory_size,
                    )?;
                    let exchange_input_range = byte_range(
                        exchange_input_offset,
                        transfer_capacity as usize,
                        1,
                        1,
                        memory_size,
                    )?;
                    if ranges_overlap(&transfer_range, &exchange_input_range) {
                        bail!("portable plugin program input and output buffers must not overlap");
                    }
                    Some(PortableProgramApi {
                        capabilities,
                        exchange_input_offset,
                        begin_edit: typed(&instance, &mut store, "rackforge_program_begin_edit")?,
                        prepare_save: typed(
                            &instance,
                            &mut store,
                            "rackforge_program_prepare_save",
                        )?,
                        install: typed(&instance, &mut store, "rackforge_program_install")?,
                        preview: if capabilities & PROGRAM_EDIT_PREVIEW != 0 {
                            Some(typed(&instance, &mut store, "rackforge_program_preview")?)
                        } else {
                            None
                        },
                        editor_view: if capabilities & PROGRAM_EDIT_DECLARATIVE != 0 {
                            Some(typed(
                                &instance,
                                &mut store,
                                "rackforge_program_editor_view",
                            )?)
                        } else {
                            None
                        },
                        apply_edit: if capabilities & PROGRAM_EDIT_DECLARATIVE != 0 {
                            Some(typed(
                                &instance,
                                &mut store,
                                "rackforge_program_apply_edit",
                            )?)
                        } else {
                            None
                        },
                    })
                }
            }
        };
        let parallel_api = match optional_typed::<(), i32>(
            &instance,
            &mut store,
            "rackforge_parallel_abi_version",
        )? {
            None => None,
            Some(version_function) => {
                store.set_fuel(self.limits.control_fuel_per_call)?;
                let version = version_function.call(&mut store, ())?;
                if version != PARALLEL_ABI_VERSION_V1 {
                    bail!("unsupported parallel-render ABI version {version:#010x}");
                }
                let max_units =
                    typed::<(), i32>(&instance, &mut store, "rackforge_parallel_max_units")?
                        .call(&mut store, ())?;
                if max_units < 1 || max_units as usize > MAX_PARALLEL_UNITS {
                    bail!(
                        "parallel-render max_units {max_units} is outside 1..={MAX_PARALLEL_UNITS}"
                    );
                }
                let max_units = max_units as usize;
                let dispatch_stride =
                    typed::<(), i32>(&instance, &mut store, "rackforge_parallel_dispatch_stride")?
                        .call(&mut store, ())?;
                if dispatch_stride <= 0 || !(dispatch_stride as usize).is_multiple_of(8) {
                    bail!("parallel-render dispatch stride must be a positive multiple of 8");
                }
                let dispatch_stride = dispatch_stride as usize;
                let dispatch_offset =
                    typed::<(), i32>(&instance, &mut store, "rackforge_parallel_dispatch_ptr")?
                        .call(&mut store, ())?;
                let plan_offset =
                    typed::<(), i32>(&instance, &mut store, "rackforge_parallel_plan_ptr")?
                        .call(&mut store, ())?;
                let mix_offset =
                    typed::<(), i32>(&instance, &mut store, "rackforge_parallel_mix_ptr")?
                        .call(&mut store, ())?;
                let shared_offset =
                    typed::<(), i32>(&instance, &mut store, "rackforge_parallel_shared_ptr")?
                        .call(&mut store, ())?;
                let shared_capacity =
                    typed::<(), i32>(&instance, &mut store, "rackforge_parallel_shared_capacity")?
                        .call(&mut store, ())?;
                if shared_capacity <= 0 || !(shared_capacity as usize).is_multiple_of(8) {
                    bail!("parallel-render shared capacity must be a positive multiple of 8");
                }
                let shared_capacity = shared_capacity as usize;
                let memory_size = memory.data_size(&store);
                byte_range(
                    dispatch_offset,
                    max_units
                        .checked_mul(dispatch_stride)
                        .context("parallel-render dispatch region overflow")?,
                    1,
                    8,
                    memory_size,
                )?;
                byte_range(shared_offset, shared_capacity, 1, 8, memory_size)?;
                byte_range(
                    plan_offset,
                    PARALLEL_PLAN_HEADER_BYTES + max_units * PARALLEL_PLAN_ENTRY_BYTES,
                    1,
                    4,
                    memory_size,
                )?;
                byte_range(
                    mix_offset,
                    max_units
                        .checked_mul(output_capacity as usize)
                        .context("parallel-render mix region overflow")?,
                    size_of::<f32>(),
                    align_of::<f32>(),
                    memory_size,
                )?;
                Some(PortableParallelApi {
                    layout: ParallelLayout {
                        max_units,
                        dispatch_stride,
                        mix_slot_samples: output_capacity as usize,
                        shared_capacity,
                    },
                    dispatch_offset,
                    plan_offset,
                    mix_offset,
                    shared_offset,
                    begin_block: typed(&instance, &mut store, "rackforge_parallel_begin_block")?,
                    render_unit: typed(&instance, &mut store, "rackforge_parallel_render_unit")?,
                    end_block: typed(&instance, &mut store, "rackforge_parallel_end_block")?,
                })
            }
        };
        let prepare = typed(&instance, &mut store, "rackforge_prepare")?;
        let set_parameter = typed(&instance, &mut store, "rackforge_set_parameter")?;
        let get_parameter = typed(&instance, &mut store, "rackforge_get_parameter")?;
        let reset = typed(&instance, &mut store, "rackforge_reset")?;
        let resource_begin = typed(&instance, &mut store, "rackforge_resource_begin")?;
        let resource_write = typed(&instance, &mut store, "rackforge_resource_write")?;
        let resource_end = typed(&instance, &mut store, "rackforge_resource_end")?;
        let preset_catalog = optional_typed(&instance, &mut store, "rackforge_preset_catalog")?;
        let load_preset = typed(&instance, &mut store, "rackforge_load_preset")?;
        let save_state = typed(&instance, &mut store, "rackforge_save_state")?;
        let load_state = typed(&instance, &mut store, "rackforge_load_state")?;
        let process = typed(&instance, &mut store, "rackforge_process")?;
        Ok(PortableInstance {
            store,
            memory,
            input_offset,
            output_offset,
            midi_offset,
            parameter_offset,
            transfer_offset,
            capacity_input_samples: input_capacity as usize,
            capacity_output_samples: output_capacity as usize,
            midi2_offset: midi2.as_ref().map(|(offset, ..)| *offset),
            capacity_midi2_events: midi2.as_ref().map_or(0, |(_, capacity, ..)| *capacity),
            midi2_families: midi2.as_ref().map_or(0, |(_, _, families, _)| *families),
            process_v2: midi2.map(|(_, _, _, process_v2)| process_v2),
            capacity_midi_events: midi_capacity as usize,
            capacity_parameter_events: parameter_capacity as usize,
            capacity_transfer_bytes: transfer_capacity as usize,
            prepare,
            set_parameter,
            get_parameter,
            reset,
            resource_begin,
            resource_write,
            resource_end,
            preset_catalog,
            load_preset,
            save_state,
            load_state,
            program_api,
            parallel_api,
            process,
            fuel_per_call: self.limits.fuel_per_call,
            control_fuel_per_call: self.limits.control_fuel_per_call,
            last_realtime_fuel_consumed: 0,
            prepared_input_channels: 0,
            prepared_output_channels: 0,
            maximum_frames: 0,
        })
    }
}

struct PortableParallelApi {
    layout: ParallelLayout,
    dispatch_offset: i32,
    plan_offset: i32,
    mix_offset: i32,
    shared_offset: i32,
    begin_block: TypedFunc<(i32, i32, i32, i32, i32), i32>,
    render_unit: TypedFunc<(i32, i32, i32, i32, i32), i32>,
    end_block: TypedFunc<(i32, i32), i32>,
}

struct PortableProgramApi {
    capabilities: u32,
    exchange_input_offset: i32,
    begin_edit: TypedFunc<i32, i32>,
    prepare_save: TypedFunc<i32, i32>,
    install: TypedFunc<i32, i32>,
    preview: Option<TypedFunc<i32, i32>>,
    editor_view: Option<TypedFunc<i32, i32>>,
    apply_edit: Option<TypedFunc<i32, i32>>,
}

/// The wide-MIDI block entry: `rackforge_process` plus the wide event count.
type ProcessV2Fn = TypedFunc<(i32, i32, i32, i32, i32, i32), i32>;

pub struct PortableInstance {
    store: Store<HostState>,
    memory: Memory,
    input_offset: i32,
    output_offset: i32,
    midi_offset: i32,
    /// The wide-MIDI region and entry, present only for a component that
    /// exported them. See `MidiExtensionApiV1` for the contract.
    midi2_offset: Option<i32>,
    capacity_midi2_events: usize,
    midi2_families: u32,
    process_v2: Option<ProcessV2Fn>,
    parameter_offset: i32,
    transfer_offset: i32,
    capacity_input_samples: usize,
    capacity_output_samples: usize,
    capacity_midi_events: usize,
    capacity_parameter_events: usize,
    capacity_transfer_bytes: usize,
    prepare: TypedFunc<(f64, i32, i32, i32), i32>,
    set_parameter: TypedFunc<(i32, f64), i32>,
    get_parameter: TypedFunc<i32, f64>,
    reset: TypedFunc<(), i32>,
    resource_begin: TypedFunc<(i32, i64), i32>,
    resource_write: TypedFunc<(i64, i32), i32>,
    resource_end: TypedFunc<(), i32>,
    preset_catalog: Option<TypedFunc<(), i32>>,
    load_preset: TypedFunc<i32, i32>,
    save_state: TypedFunc<(), i32>,
    load_state: TypedFunc<i32, i32>,
    program_api: Option<PortableProgramApi>,
    parallel_api: Option<PortableParallelApi>,
    process: TypedFunc<(i32, i32, i32, i32, i32), i32>,
    fuel_per_call: u64,
    control_fuel_per_call: u64,
    last_realtime_fuel_consumed: u64,
    prepared_input_channels: u32,
    prepared_output_channels: u32,
    maximum_frames: u32,
}

impl PortableInstance {
    pub fn load_resource_file(&mut self, id: &str, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let file = File::open(path)
            .with_context(|| format!("opening portable resource {}", path.display()))?;
        let total_bytes = file
            .metadata()
            .with_context(|| format!("reading portable resource metadata {}", path.display()))?
            .len();
        self.begin_resource(id, total_bytes)?;
        let mut reader = BufReader::new(file);
        let mut chunk = vec![0_u8; self.capacity_transfer_bytes.min(64 * 1024)];
        let mut offset = 0_u64;
        loop {
            let read = reader
                .read(&mut chunk)
                .with_context(|| format!("reading portable resource {}", path.display()))?;
            if read == 0 {
                break;
            }
            self.write_resource(offset, &chunk[..read])?;
            offset += read as u64;
        }
        if offset != total_bytes {
            bail!("portable resource changed while it was being delivered");
        }
        self.end_resource()
    }

    pub fn prepare(
        &mut self,
        sample_rate: f64,
        maximum_frames: u32,
        input_channels: u32,
        output_channels: u32,
    ) -> Result<()> {
        let input_samples = checked_samples(maximum_frames, input_channels)?;
        let output_samples = checked_samples(maximum_frames, output_channels)?;
        if input_samples > self.capacity_input_samples {
            bail!(
                "plugin input capacity {} samples is smaller than requested {input_samples}",
                self.capacity_input_samples
            );
        }
        if output_samples > self.capacity_output_samples {
            bail!(
                "plugin output capacity {} samples is smaller than requested {output_samples}",
                self.capacity_output_samples
            );
        }
        self.reset_control_fuel()?;
        check_status(
            self.prepare.call(
                &mut self.store,
                (
                    sample_rate,
                    maximum_frames as i32,
                    input_channels as i32,
                    output_channels as i32,
                ),
            )?,
            "prepare",
        )?;
        self.prepared_input_channels = input_channels;
        self.prepared_output_channels = output_channels;
        self.maximum_frames = maximum_frames;
        Ok(())
    }

    pub fn set_parameter(&mut self, index: u32, value: f64) -> Result<()> {
        self.reset_control_fuel()?;
        check_status(
            self.set_parameter
                .call(&mut self.store, (index as i32, value))?,
            "set_parameter",
        )
    }

    pub fn get_parameter(&mut self, index: u32) -> Result<f64> {
        self.reset_control_fuel()?;
        let value = self.get_parameter.call(&mut self.store, index as i32)?;
        if !value.is_finite() {
            bail!("portable plugin does not expose parameter {index}");
        }
        Ok(value)
    }

    pub fn reset(&mut self) -> Result<()> {
        self.reset_control_fuel()?;
        check_status(self.reset.call(&mut self.store, ())?, "reset")
    }

    /// Delivers one already-authorized package resource without exposing the
    /// filesystem to the guest. This method belongs on the control thread.
    pub fn load_resource(&mut self, id: &str, bytes: &[u8]) -> Result<()> {
        self.begin_resource(id, bytes.len() as u64)?;
        for (chunk_index, chunk) in bytes.chunks(self.capacity_transfer_bytes).enumerate() {
            let offset = chunk_index
                .checked_mul(self.capacity_transfer_bytes)
                .context("resource offset overflow")? as u64;
            self.write_resource(offset, chunk)?;
        }
        self.end_resource()
    }

    fn begin_resource(&mut self, id: &str, total_bytes: u64) -> Result<()> {
        if id.is_empty() || id.len() > self.capacity_transfer_bytes {
            bail!("resource id does not fit the portable transfer buffer");
        }
        let total_bytes = i64::try_from(total_bytes).context("resource is too large")?;
        self.write_transfer(id.as_bytes())?;
        self.reset_control_fuel()?;
        check_status(
            self.resource_begin
                .call(&mut self.store, (id.len() as i32, total_bytes))?,
            "resource_begin",
        )
    }

    fn write_resource(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.capacity_transfer_bytes {
            bail!("resource chunk does not fit the portable transfer buffer");
        }
        let offset = i64::try_from(offset).context("resource offset is too large")?;
        self.write_transfer(bytes)?;
        self.reset_control_fuel()?;
        check_status(
            self.resource_write
                .call(&mut self.store, (offset, bytes.len() as i32))?,
            "resource_write",
        )
    }

    fn end_resource(&mut self) -> Result<()> {
        self.reset_control_fuel()?;
        check_status(self.resource_end.call(&mut self.store, ())?, "resource_end")
    }

    /// Returns an optional instance-specific preset catalog produced after
    /// resource delivery. Components built with an older SDK simply fall back
    /// to their package metadata.
    pub fn preset_catalog(&mut self) -> Result<Option<Vec<u8>>> {
        let Some(function) = self.preset_catalog.clone() else {
            return Ok(None);
        };
        self.reset_control_fuel()?;
        let length = function.call(&mut self.store, ())?;
        if length == 0 {
            return Ok(None);
        }
        if length < 0 || length as usize > self.capacity_transfer_bytes {
            bail!("portable plugin returned an invalid preset catalog length {length}");
        }
        Ok(Some(self.read_transfer(length as usize)?.to_vec()))
    }

    pub fn load_preset(&mut self, id: &str) -> Result<()> {
        if id.is_empty() {
            bail!("preset id must not be empty");
        }
        self.write_transfer(id.as_bytes())?;
        self.reset_control_fuel()?;
        check_status(
            self.load_preset.call(&mut self.store, id.len() as i32)?,
            "load_preset",
        )
    }

    pub fn save_state(&mut self) -> Result<Vec<u8>> {
        self.reset_control_fuel()?;
        let length = self.save_state.call(&mut self.store, ())?;
        if length < 0 || length as usize > self.capacity_transfer_bytes {
            bail!("portable plugin returned an invalid state length {length}");
        }
        Ok(self.read_transfer(length as usize)?.to_vec())
    }

    pub fn load_state(&mut self, state: &[u8]) -> Result<()> {
        self.write_transfer(state)?;
        self.reset_control_fuel()?;
        check_status(
            self.load_state.call(&mut self.store, state.len() as i32)?,
            "load_state",
        )
    }

    pub fn supports_program_editing(&self) -> bool {
        self.program_editing_capabilities() != 0
    }

    pub fn program_editing_capabilities(&self) -> u32 {
        self.program_api.as_ref().map_or(0, |api| api.capabilities)
    }

    pub fn begin_program_edit(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        let function = self
            .program_api
            .as_ref()
            .context("portable plugin does not expose program editing")?
            .begin_edit
            .clone();
        self.exchange_program_bytes(function, request, "begin_program_edit")
    }

    pub fn prepare_program_save(&mut self, document: &[u8]) -> Result<Vec<u8>> {
        let function = self
            .program_api
            .as_ref()
            .context("portable plugin does not expose program editing")?
            .prepare_save
            .clone();
        self.exchange_program_bytes(function, document, "prepare_program_save")
    }

    pub fn install_program(&mut self, prepared: &[u8]) -> Result<()> {
        let function = self
            .program_api
            .as_ref()
            .context("portable plugin does not expose program editing")?
            .install
            .clone();
        self.call_program_install(function, prepared, "install_program")
    }

    pub fn preview_program(&mut self, prepared: &[u8]) -> Result<bool> {
        let Some(function) = self
            .program_api
            .as_ref()
            .context("portable plugin does not expose program editing")?
            .preview
            .clone()
        else {
            return Ok(false);
        };
        self.call_program_install(function, prepared, "preview_program")?;
        Ok(true)
    }

    pub fn program_editor_view(&mut self, document: &[u8]) -> Result<Vec<u8>> {
        let function = self
            .program_api
            .as_ref()
            .context("portable plugin does not expose program editing")?
            .editor_view
            .clone()
            .context("portable plugin does not expose a declarative program editor")?;
        self.exchange_program_bytes(function, document, "program_editor_view")
    }

    pub fn apply_program_edit(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        let function = self
            .program_api
            .as_ref()
            .context("portable plugin does not expose program editing")?
            .apply_edit
            .clone()
            .context("portable plugin does not expose declarative program edits")?;
        self.exchange_program_bytes(function, request, "apply_program_edit")
    }

    fn exchange_program_bytes(
        &mut self,
        function: TypedFunc<i32, i32>,
        source: &[u8],
        operation: &str,
    ) -> Result<Vec<u8>> {
        self.write_program_input(source)?;
        self.reset_control_fuel()?;
        let source_length = i32::try_from(source.len()).context("program payload is too large")?;
        let length = function.call(&mut self.store, source_length)?;
        if length < 0 || length as usize > self.capacity_transfer_bytes {
            bail!("portable plugin {operation} returned invalid length {length}");
        }
        Ok(self.read_transfer(length as usize)?.to_vec())
    }

    fn call_program_install(
        &mut self,
        function: TypedFunc<i32, i32>,
        source: &[u8],
        operation: &str,
    ) -> Result<()> {
        self.write_program_input(source)?;
        self.reset_control_fuel()?;
        let source_length = i32::try_from(source.len()).context("program payload is too large")?;
        check_status(function.call(&mut self.store, source_length)?, operation)
    }

    fn write_program_input(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.capacity_transfer_bytes {
            bail!("program payload exceeds plugin transfer capacity");
        }
        let offset = self
            .program_api
            .as_ref()
            .context("portable plugin does not expose program editing")?
            .exchange_input_offset;
        let range = byte_range(
            offset,
            bytes.len(),
            1,
            1,
            self.memory.data_size(&self.store),
        )?;
        self.memory.data_mut(&mut self.store)[range].copy_from_slice(bytes);
        Ok(())
    }

    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
    ) -> Result<()> {
        self.process_interleaved_with_events(input, output, frames, &[], &[])
    }

    pub fn process_interleaved_with_midi(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
        midi: &[MidiEvent],
    ) -> Result<()> {
        self.process_interleaved_with_events(input, output, frames, midi, &[])
    }

    pub fn process_interleaved_with_events(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
        midi: &[MidiEvent],
        parameters: &[ParameterEvent],
    ) -> Result<()> {
        self.process_interleaved_with_midi2(input, output, frames, midi, parameters, &[])
    }

    /// The `MIDI_FAMILY_*` bits the component asked to receive wide; zero
    /// when it does not export the wide-MIDI contract.
    pub fn midi2_families(&self) -> u32 {
        self.midi2_families
    }

    /// Runs one block with `midi2` delivered at MIDI 2.0 widths. A component
    /// that exports the wide-MIDI contract is entered through
    /// `rackforge_process_v2` on every block, wide events or not; one that
    /// does not is entered through `rackforge_process` and may only ever be
    /// handed an empty `midi2`.
    pub fn process_interleaved_with_midi2(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
        midi: &[MidiEvent],
        parameters: &[ParameterEvent],
        midi2: &[MidiEvent2],
    ) -> Result<()> {
        if frames == 0 || frames > self.maximum_frames {
            bail!("portable plugin is not prepared for this audio block");
        }
        let input_samples = checked_samples(frames, self.prepared_input_channels)?;
        let output_samples = checked_samples(frames, self.prepared_output_channels)?;
        if input.len() != input_samples || output.len() != output_samples {
            bail!("audio buffer length does not match prepared input/output channels");
        }
        validate_realtime_events(
            frames,
            midi,
            parameters,
            self.capacity_midi_events,
            self.capacity_parameter_events,
        )?;
        if midi2.len() > self.capacity_midi2_events {
            bail!("wide MIDI event count exceeds plugin capacity");
        }
        if midi2.iter().any(|event| event.frame >= frames) {
            bail!("wide MIDI event is outside the audio block");
        }
        let input_range = memory_range(
            self.input_offset,
            input_samples,
            self.memory.data_size(&self.store),
        )?;
        let output_range = memory_range(
            self.output_offset,
            output_samples,
            self.memory.data_size(&self.store),
        )?;
        let midi_range = byte_range(
            self.midi_offset,
            midi.len(),
            size_of::<u64>(),
            align_of::<u64>(),
            self.memory.data_size(&self.store),
        )?;
        let parameter_range = byte_range(
            self.parameter_offset,
            parameters.len(),
            16,
            align_of::<u64>(),
            self.memory.data_size(&self.store),
        )?;
        let midi2_range = match self.midi2_offset {
            Some(offset) if !midi2.is_empty() => Some(byte_range(
                offset,
                midi2.len(),
                16,
                align_of::<u64>(),
                self.memory.data_size(&self.store),
            )?),
            _ => None,
        };
        write_f32(self.memory.data_mut(&mut self.store), input_range, input);
        write_midi(self.memory.data_mut(&mut self.store), midi_range, midi);
        if let Some(range) = midi2_range {
            write_midi2(self.memory.data_mut(&mut self.store), range, midi2);
        }
        write_parameters(
            self.memory.data_mut(&mut self.store),
            parameter_range,
            parameters,
        );
        self.reset_realtime_fuel()?;
        let result = match &self.process_v2 {
            Some(process_v2) => process_v2.call(
                &mut self.store,
                (
                    frames as i32,
                    self.prepared_input_channels as i32,
                    self.prepared_output_channels as i32,
                    midi.len() as i32,
                    parameters.len() as i32,
                    midi2.len() as i32,
                ),
            ),
            None => self.process.call(
                &mut self.store,
                (
                    frames as i32,
                    self.prepared_input_channels as i32,
                    self.prepared_output_channels as i32,
                    midi.len() as i32,
                    parameters.len() as i32,
                ),
            ),
        };
        self.last_realtime_fuel_consumed = self
            .fuel_per_call
            .saturating_sub(self.store.get_fuel().unwrap_or(0));
        check_status(result?, "process")?;
        read_f32(self.memory.data(&self.store), output_range, output);
        Ok(())
    }

    /// Returns the parallel-render geometry when the component exports the
    /// optional extension, `None` for classic single-unit components.
    pub fn parallel_layout(&self) -> Option<ParallelLayout> {
        self.parallel_api.as_ref().map(|api| api.layout)
    }

    /// Runs the serial pre-stage of one block on a coordinator instance: MIDI,
    /// sample-accurate automation, voice allocation and every other piece of
    /// global state. Fills `plan` with the units that are ready to render and
    /// returns how many entries are valid.
    pub fn parallel_begin_block(
        &mut self,
        input: &[f32],
        frames: u32,
        midi: &[MidiEvent],
        parameters: &[ParameterEvent],
        plan: &mut [ParallelPlanEntry],
    ) -> Result<ParallelBlockPlan> {
        let api = self
            .parallel_api
            .as_ref()
            .context("portable plugin does not expose parallel render")?;
        let layout = api.layout;
        let plan_offset = api.plan_offset;
        let begin_block = api.begin_block.clone();
        if plan.len() < layout.max_units {
            bail!("parallel plan buffer is smaller than max_units");
        }
        if frames == 0 || frames > self.maximum_frames {
            bail!("portable plugin is not prepared for this audio block");
        }
        let input_samples = checked_samples(frames, self.prepared_input_channels)?;
        if input.len() != input_samples {
            bail!("audio buffer length does not match prepared input channels");
        }
        validate_realtime_events(
            frames,
            midi,
            parameters,
            self.capacity_midi_events,
            self.capacity_parameter_events,
        )?;
        let memory_size = self.memory.data_size(&self.store);
        let input_range = memory_range(self.input_offset, input_samples, memory_size)?;
        let midi_range = byte_range(
            self.midi_offset,
            midi.len(),
            size_of::<u64>(),
            align_of::<u64>(),
            memory_size,
        )?;
        let parameter_range = byte_range(
            self.parameter_offset,
            parameters.len(),
            16,
            align_of::<u64>(),
            memory_size,
        )?;
        write_f32(self.memory.data_mut(&mut self.store), input_range, input);
        write_midi(self.memory.data_mut(&mut self.store), midi_range, midi);
        write_parameters(
            self.memory.data_mut(&mut self.store),
            parameter_range,
            parameters,
        );
        self.reset_realtime_fuel()?;
        let result = begin_block.call(
            &mut self.store,
            (
                frames as i32,
                self.prepared_input_channels as i32,
                self.prepared_output_channels as i32,
                midi.len() as i32,
                parameters.len() as i32,
            ),
        );
        self.last_realtime_fuel_consumed = self
            .fuel_per_call
            .saturating_sub(self.store.get_fuel().unwrap_or(0));
        let active = result?;
        if active < 0 {
            bail!("portable plugin begin_block failed with status {active}");
        }
        let active = active as usize;
        if active > layout.max_units {
            bail!("portable plugin announced {active} units beyond max_units");
        }
        let plan_range = byte_range(
            plan_offset,
            PARALLEL_PLAN_HEADER_BYTES + active * PARALLEL_PLAN_ENTRY_BYTES,
            1,
            4,
            self.memory.data_size(&self.store),
        )?;
        let memory = self.memory.data(&self.store);
        let plan_bytes = &memory[plan_range];
        let shared_bytes =
            u32::from_le_bytes(plan_bytes[0..4].try_into().expect("plan header")) as usize;
        if shared_bytes > layout.shared_capacity {
            bail!(
                "portable plugin announced {shared_bytes} shared bytes beyond capacity {}",
                layout.shared_capacity
            );
        }
        for (entry, chunk) in plan[..active]
            .iter_mut()
            .zip(plan_bytes[PARALLEL_PLAN_HEADER_BYTES..].as_chunks::<8>().0)
        {
            entry.unit = u32::from_le_bytes(chunk[0..4].try_into().expect("plan entry unit"));
            entry.payload_bytes =
                u32::from_le_bytes(chunk[4..8].try_into().expect("plan entry payload"));
        }
        validate_parallel_plan(&plan[..active], layout.max_units, layout.dispatch_stride)?;
        Ok(ParallelBlockPlan {
            active_units: active,
            shared_bytes,
        })
    }

    /// Copies the block-shared payload the coordinator produced into a
    /// host-owned buffer sized by the plan header.
    pub fn parallel_read_shared(&self, shared: &mut [u8]) -> Result<()> {
        let api = self
            .parallel_api
            .as_ref()
            .context("portable plugin does not expose parallel render")?;
        if shared.len() > api.layout.shared_capacity {
            bail!("shared payload exceeds the declared capacity");
        }
        let range = byte_range(
            api.shared_offset,
            shared.len(),
            1,
            1,
            self.memory.data_size(&self.store),
        )?;
        shared.copy_from_slice(&self.memory.data(&self.store)[range]);
        Ok(())
    }

    /// Writes the block-shared payload into a worker instance before its
    /// `parallel_render_unit` calls.
    pub fn parallel_write_shared(&mut self, shared: &[u8]) -> Result<()> {
        let api = self
            .parallel_api
            .as_ref()
            .context("portable plugin does not expose parallel render")?;
        if shared.len() > api.layout.shared_capacity {
            bail!("shared payload exceeds the declared capacity");
        }
        let range = byte_range(
            api.shared_offset,
            shared.len(),
            1,
            1,
            self.memory.data_size(&self.store),
        )?;
        self.memory.data_mut(&mut self.store)[range].copy_from_slice(shared);
        Ok(())
    }

    /// Copies the dispatch payload a coordinator produced for `unit` into a
    /// host-owned buffer sized by the plan entry.
    pub fn parallel_read_dispatch(&self, unit: u32, payload: &mut [u8]) -> Result<()> {
        let api = self
            .parallel_api
            .as_ref()
            .context("portable plugin does not expose parallel render")?;
        let range = self.parallel_dispatch_range(api, unit, payload.len())?;
        payload.copy_from_slice(&self.memory.data(&self.store)[range]);
        Ok(())
    }

    /// Writes the dispatch payload for `unit` into a worker instance before
    /// its `parallel_render_unit` call.
    pub fn parallel_write_dispatch(&mut self, unit: u32, payload: &[u8]) -> Result<()> {
        let api = self
            .parallel_api
            .as_ref()
            .context("portable plugin does not expose parallel render")?;
        let range = self.parallel_dispatch_range(api, unit, payload.len())?;
        self.memory.data_mut(&mut self.store)[range].copy_from_slice(payload);
        Ok(())
    }

    fn parallel_dispatch_range(
        &self,
        api: &PortableParallelApi,
        unit: u32,
        payload_bytes: usize,
    ) -> Result<std::ops::Range<usize>> {
        if unit as usize >= api.layout.max_units {
            bail!("parallel dispatch unit {unit} is beyond max_units");
        }
        if payload_bytes > api.layout.dispatch_stride {
            bail!("parallel dispatch payload exceeds the declared stride");
        }
        let offset =
            i64::from(api.dispatch_offset) + unit as i64 * api.layout.dispatch_stride as i64;
        let offset = i32::try_from(offset).context("parallel dispatch offset overflow")?;
        byte_range(
            offset,
            payload_bytes,
            1,
            1,
            self.memory.data_size(&self.store),
        )
    }

    /// Renders one independent unit inside a worker instance. The dispatch
    /// payload must already be in place; the unit's audio is copied into
    /// `output` afterwards.
    pub fn parallel_render_unit(
        &mut self,
        unit: u32,
        payload_bytes: usize,
        shared_bytes: usize,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
    ) -> Result<()> {
        let api = self
            .parallel_api
            .as_ref()
            .context("portable plugin does not expose parallel render")?;
        if unit as usize >= api.layout.max_units {
            bail!("parallel render unit {unit} is beyond max_units");
        }
        if payload_bytes > api.layout.dispatch_stride {
            bail!("parallel dispatch payload exceeds the declared stride");
        }
        if shared_bytes > api.layout.shared_capacity {
            bail!("shared payload exceeds the declared capacity");
        }
        let render_unit = api.render_unit.clone();
        if frames == 0 || frames > self.maximum_frames {
            bail!("portable plugin is not prepared for this audio block");
        }
        let input_samples = checked_samples(frames, self.prepared_input_channels)?;
        let output_samples = checked_samples(frames, self.prepared_output_channels)?;
        if input.len() != input_samples || output.len() != output_samples {
            bail!("audio buffer length does not match prepared input/output channels");
        }
        let memory_size = self.memory.data_size(&self.store);
        let input_range = memory_range(self.input_offset, input_samples, memory_size)?;
        let output_range = memory_range(self.output_offset, output_samples, memory_size)?;
        write_f32(self.memory.data_mut(&mut self.store), input_range, input);
        self.reset_realtime_fuel()?;
        let result = render_unit.call(
            &mut self.store,
            (
                unit as i32,
                payload_bytes as i32,
                shared_bytes as i32,
                frames as i32,
                self.prepared_output_channels as i32,
            ),
        );
        self.last_realtime_fuel_consumed = self
            .fuel_per_call
            .saturating_sub(self.store.get_fuel().unwrap_or(0));
        check_status(result?, "parallel_render_unit")?;
        read_f32(self.memory.data(&self.store), output_range, output);
        Ok(())
    }

    /// Deposits one finished unit's audio into the coordinator's mix region.
    /// Slot order is fixed by the unit index, so the combine in `end_block`
    /// is deterministic no matter which worker finished first.
    pub fn parallel_write_mix_slot(&mut self, unit: u32, samples: &[f32]) -> Result<()> {
        let api = self
            .parallel_api
            .as_ref()
            .context("portable plugin does not expose parallel render")?;
        if unit as usize >= api.layout.max_units {
            bail!("parallel mix unit {unit} is beyond max_units");
        }
        if samples.len() > api.layout.mix_slot_samples {
            bail!("parallel mix slot cannot hold this block");
        }
        let offset = i64::from(api.mix_offset)
            + unit as i64 * api.layout.mix_slot_samples as i64 * size_of::<f32>() as i64;
        let offset = i32::try_from(offset).context("parallel mix offset overflow")?;
        let range = memory_range(offset, samples.len(), self.memory.data_size(&self.store))?;
        write_f32(self.memory.data_mut(&mut self.store), range, samples);
        Ok(())
    }

    /// Runs the serial post-stage on the coordinator: combines the deposited
    /// unit slots in unit order and applies global stages, writing the final
    /// block into `output`.
    pub fn parallel_end_block(&mut self, output: &mut [f32], frames: u32) -> Result<()> {
        let api = self
            .parallel_api
            .as_ref()
            .context("portable plugin does not expose parallel render")?;
        let end_block = api.end_block.clone();
        if frames == 0 || frames > self.maximum_frames {
            bail!("portable plugin is not prepared for this audio block");
        }
        let output_samples = checked_samples(frames, self.prepared_output_channels)?;
        if output.len() != output_samples {
            bail!("audio buffer length does not match prepared output channels");
        }
        let output_range = memory_range(
            self.output_offset,
            output_samples,
            self.memory.data_size(&self.store),
        )?;
        self.reset_realtime_fuel()?;
        let result = end_block.call(
            &mut self.store,
            (frames as i32, self.prepared_output_channels as i32),
        );
        self.last_realtime_fuel_consumed = self
            .fuel_per_call
            .saturating_sub(self.store.get_fuel().unwrap_or(0));
        check_status(result?, "parallel_end_block")?;
        read_f32(self.memory.data(&self.store), output_range, output);
        Ok(())
    }

    /// Returns the Wasmtime fuel consumed by the most recent real-time call.
    /// This is diagnostic telemetry and does not alter the next call's budget.
    pub const fn last_realtime_fuel_consumed(&self) -> u64 {
        self.last_realtime_fuel_consumed
    }

    fn reset_realtime_fuel(&mut self) -> Result<()> {
        self.store.set_fuel(self.fuel_per_call)?;
        Ok(())
    }

    fn reset_control_fuel(&mut self) -> Result<()> {
        self.store.set_fuel(self.control_fuel_per_call)?;
        Ok(())
    }

    fn write_transfer(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.capacity_transfer_bytes {
            bail!("control payload exceeds plugin transfer capacity");
        }
        let range = byte_range(
            self.transfer_offset,
            bytes.len(),
            1,
            1,
            self.memory.data_size(&self.store),
        )?;
        self.memory.data_mut(&mut self.store)[range].copy_from_slice(bytes);
        Ok(())
    }

    fn read_transfer(&self, length: usize) -> Result<&[u8]> {
        let range = byte_range(
            self.transfer_offset,
            length,
            1,
            1,
            self.memory.data_size(&self.store),
        )?;
        Ok(&self.memory.data(&self.store)[range])
    }
}

fn typed<Params, Results>(
    instance: &Instance,
    store: &mut Store<HostState>,
    name: &str,
) -> Result<TypedFunc<Params, Results>>
where
    Params: wasmtime::WasmParams,
    Results: wasmtime::WasmResults,
{
    instance
        .get_typed_func(store, name)
        .map_err(|error| anyhow::anyhow!("wasm-v1 plugin is missing export {name}: {error}"))
}

fn optional_typed<Params, Results>(
    instance: &Instance,
    store: &mut Store<HostState>,
    name: &str,
) -> Result<Option<TypedFunc<Params, Results>>>
where
    Params: wasmtime::WasmParams,
    Results: wasmtime::WasmResults,
{
    let Some(function) = instance.get_func(&mut *store, name) else {
        return Ok(None);
    };
    function.typed(&mut *store).map(Some).map_err(|error| {
        anyhow::anyhow!("wasm-v1 plugin export {name} has the wrong type: {error}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAIN: &str = r#"
        (module
          (memory (export "memory") 1)
          (global $gain (mut f32) (f32.const 1))
          (func (export "rackforge_abi_version") (result i32) i32.const 65537)
          (func (export "rackforge_input_ptr") (result i32) i32.const 0)
          (func (export "rackforge_output_ptr") (result i32) i32.const 1024)
          (func (export "rackforge_capacity_input_samples") (result i32) i32.const 256)
          (func (export "rackforge_capacity_output_samples") (result i32) i32.const 256)
          (func (export "rackforge_midi_ptr") (result i32) i32.const 4096)
          (func (export "rackforge_capacity_midi_events") (result i32) i32.const 64)
          (func (export "rackforge_parameter_ptr") (result i32) i32.const 5120)
          (func (export "rackforge_capacity_parameter_events") (result i32) i32.const 64)
          (func (export "rackforge_transfer_ptr") (result i32) i32.const 8192)
          (func (export "rackforge_capacity_transfer_bytes") (result i32) i32.const 1024)
          (func (export "rackforge_initialize") (result i32) i32.const 0)
          (func (export "rackforge_prepare") (param f64 i32 i32 i32) (result i32) i32.const 0)
          (func (export "rackforge_set_parameter") (param $index i32) (param $value f64) (result i32)
            local.get $value f32.demote_f64 global.set $gain i32.const 0)
          (func (export "rackforge_get_parameter") (param $index i32) (result f64)
            global.get $gain f64.promote_f32)
          (func (export "rackforge_reset") (result i32) i32.const 0)
          (func (export "rackforge_resource_begin") (param i32 i64) (result i32) i32.const -3)
          (func (export "rackforge_resource_write") (param i64 i32) (result i32) i32.const -3)
          (func (export "rackforge_resource_end") (result i32) i32.const -3)
          (func (export "rackforge_load_preset") (param i32) (result i32) i32.const -3)
          (func (export "rackforge_save_state") (result i32) i32.const -3)
          (func (export "rackforge_load_state") (param i32) (result i32) i32.const -3)
          (func (export "rackforge_process") (param $frames i32) (param $input_channels i32) (param $output_channels i32) (param $midi i32) (param $parameters i32) (result i32)
            (local $i i32) (local $count i32)
            local.get $parameters i32.const 0 i32.gt_s
            if
              i32.const 5128 f64.load f32.demote_f64 global.set $gain
            end
            local.get $frames local.get $output_channels i32.mul local.set $count
            (block $done
              (loop $copy
                local.get $i local.get $count i32.ge_u br_if $done
                local.get $i i32.const 4 i32.mul i32.const 1024 i32.add
                local.get $i i32.const 4 i32.mul f32.load global.get $gain f32.mul f32.store
                local.get $i i32.const 1 i32.add local.set $i
                br $copy))
            local.get $midi i32.const 0 i32.gt_s
            if
              i32.const 1024
              i32.const 4100 i32.load8_u f32.convert_i32_u f32.store
            end
            i32.const 0))
    "#;

    fn editable_gain() -> String {
        GAIN.replace(
            "          (func (export \"rackforge_abi_version\") (result i32) i32.const 65537)",
            "          (func (export \"rackforge_abi_version\") (result i32) i32.const 65538)",
        )
        .replace(
            "          (func (export \"rackforge_load_preset\")",
            r#"          (func (export "rackforge_exchange_input_ptr") (result i32) i32.const 12288)
          (func (export "rackforge_program_editing_capabilities") (result i32) i32.const 7)
          (func $exchange (param $length i32) (result i32)
            i32.const 8192 i32.const 12288 local.get $length memory.copy local.get $length)
          (func (export "rackforge_program_begin_edit") (param i32) (result i32)
            local.get 0 call $exchange)
          (func (export "rackforge_program_prepare_save") (param i32) (result i32)
            local.get 0 call $exchange)
          (func (export "rackforge_program_install") (param i32) (result i32) i32.const 0)
          (func (export "rackforge_program_preview") (param i32) (result i32) i32.const 0)
          (func (export "rackforge_program_editor_view") (param i32) (result i32)
            local.get 0 call $exchange)
          (func (export "rackforge_program_apply_edit") (param i32) (result i32)
            local.get 0 call $exchange)
          (func (export "rackforge_load_preset")"#,
        )
    }

    /// A miniature parallel instrument: a block-rate LFO advanced once per
    /// block in `begin_block`, four voice units with persistent per-unit
    /// phase counters, and a final stage that halves the deterministic sum.
    /// Every sample value is an integer-valued f32 so comparisons are exact.
    const PARALLEL_SYNTH: &str = r#"
        (module
          (memory (export "memory") 2)
          (global $lfo (mut f32) (f32.const 0))
          (global $active (mut i32) (i32.const 3))
          (global $last_active (mut i32) (i32.const 0))
          (global $fail (mut i32) (i32.const -1))
          (func (export "rackforge_abi_version") (result i32) i32.const 65538)
          (func (export "rackforge_input_ptr") (result i32) i32.const 0)
          (func (export "rackforge_output_ptr") (result i32) i32.const 1024)
          (func (export "rackforge_capacity_input_samples") (result i32) i32.const 256)
          (func (export "rackforge_capacity_output_samples") (result i32) i32.const 256)
          (func (export "rackforge_midi_ptr") (result i32) i32.const 4096)
          (func (export "rackforge_capacity_midi_events") (result i32) i32.const 64)
          (func (export "rackforge_parameter_ptr") (result i32) i32.const 5120)
          (func (export "rackforge_capacity_parameter_events") (result i32) i32.const 64)
          (func (export "rackforge_transfer_ptr") (result i32) i32.const 8192)
          (func (export "rackforge_capacity_transfer_bytes") (result i32) i32.const 1024)
          (func (export "rackforge_parallel_abi_version") (result i32) i32.const 65536)
          (func (export "rackforge_parallel_max_units") (result i32) i32.const 4)
          (func (export "rackforge_parallel_dispatch_stride") (result i32) i32.const 16)
          (func (export "rackforge_parallel_plan_ptr") (result i32) i32.const 12288)
          (func (export "rackforge_parallel_dispatch_ptr") (result i32) i32.const 12352)
          (func (export "rackforge_parallel_mix_ptr") (result i32) i32.const 12544)
          (func (export "rackforge_parallel_shared_ptr") (result i32) i32.const 16704)
          (func (export "rackforge_parallel_shared_capacity") (result i32) i32.const 256)
          (func (export "rackforge_initialize") (result i32) i32.const 0)
          (func (export "rackforge_prepare") (param f64 i32 i32 i32) (result i32) i32.const 0)
          (func (export "rackforge_set_parameter") (param $index i32) (param $value f64) (result i32)
            local.get $index i32.const 0 i32.eq
            if
              local.get $value i32.trunc_f64_s global.set $active
              i32.const 0 return
            end
            local.get $index i32.const 1 i32.eq
            if
              local.get $value i32.trunc_f64_s global.set $fail
              i32.const 0 return
            end
            i32.const -3)
          (func (export "rackforge_get_parameter") (param $index i32) (result f64)
            local.get $index i32.const 0 i32.eq
            if (result f64)
              global.get $active f64.convert_i32_s
            else
              global.get $fail f64.convert_i32_s
            end)
          (func (export "rackforge_reset") (result i32)
            f32.const 0 global.set $lfo
            i32.const 16640 i64.const 0 i64.store
            i32.const 16648 i64.const 0 i64.store
            i32.const 0)
          (func (export "rackforge_resource_begin") (param i32 i64) (result i32) i32.const -3)
          (func (export "rackforge_resource_write") (param i64 i32) (result i32) i32.const -3)
          (func (export "rackforge_resource_end") (result i32) i32.const -3)
          (func (export "rackforge_load_preset") (param i32) (result i32) i32.const -3)
          (func (export "rackforge_save_state") (result i32)
            i32.const 8192 global.get $active i32.store
            i32.const 4)
          (func (export "rackforge_load_state") (param $length i32) (result i32)
            local.get $length i32.const 4 i32.ne
            if i32.const -1 return end
            i32.const 8192 i32.load global.set $active
            i32.const 0)
          (func $plan_unit (param $i i32) (result i32) local.get $i)
          (func $begin (param $frames i32) (param $midi i32) (param $parameters i32) (result i32)
            (local $count i32) (local $i i32)
            global.get $lfo f32.const 1 f32.add global.set $lfo
            local.get $parameters i32.const 0 i32.gt_s
            if
              i32.const 5128 f64.load i32.trunc_f64_s global.set $active
            end
            local.get $midi i32.const 0 i32.gt_s
            if (result i32)
              i32.const 4
            else
              global.get $active
            end
            local.set $count
            local.get $count global.set $last_active
            ;; plan header {shared_bytes, reserved} then the entries
            i32.const 12288 i32.const 8 i32.store
            i32.const 12292 i32.const 0 i32.store
            ;; the block-shared payload: the LFO value every unit reads
            i32.const 16704 global.get $lfo f32.store
            (block $done
              (loop $units
                local.get $i local.get $count i32.ge_s br_if $done
                i32.const 12296 local.get $i i32.const 8 i32.mul i32.add
                local.get $i call $plan_unit i32.store
                i32.const 12300 local.get $i i32.const 8 i32.mul i32.add
                i32.const 8 i32.store
                ;; dispatch payload {lfo, scale}
                i32.const 12352 local.get $i i32.const 16 i32.mul i32.add
                global.get $lfo f32.store
                i32.const 12356 local.get $i i32.const 16 i32.mul i32.add
                local.get $i i32.const 1 i32.add f32.convert_i32_s f32.store
                local.get $i i32.const 1 i32.add local.set $i
                br $units))
            local.get $count)
          (func $render (param $unit i32) (param $payload i32) (param $shared i32) (param $frames i32) (param $channels i32) (result i32)
            (local $lfo f32) (local $scale f32) (local $phase f32)
            (local $k i32) (local $count i32) (local $sample f32)
            local.get $unit global.get $fail i32.eq
            if unreachable end
            ;; the block-shared payload proves host transport into workers
            i32.const 16704 f32.load local.set $lfo
            i32.const 12356 local.get $unit i32.const 16 i32.mul i32.add f32.load local.set $scale
            i32.const 16640 local.get $unit i32.const 4 i32.mul i32.add
            i32.const 16640 local.get $unit i32.const 4 i32.mul i32.add f32.load
            f32.const 1 f32.add local.tee $phase
            f32.store
            local.get $lfo f32.const 1000 f32.mul
            local.get $scale f32.const 100 f32.mul f32.add
            local.get $phase f32.add local.set $sample
            local.get $frames local.get $channels i32.mul local.set $count
            (block $done
              (loop $fill
                local.get $k local.get $count i32.ge_s br_if $done
                i32.const 1024 local.get $k i32.const 4 i32.mul i32.add
                local.get $sample f32.store
                local.get $k i32.const 1 i32.add local.set $k
                br $fill))
            i32.const 0)
          (func $end (param $frames i32) (param $channels i32) (result i32)
            (local $k i32) (local $count i32) (local $sum f32) (local $u i32)
            local.get $frames local.get $channels i32.mul local.set $count
            (block $done
              (loop $frames_loop
                local.get $k local.get $count i32.ge_s br_if $done
                f32.const 0 local.set $sum
                i32.const 0 local.set $u
                (block $mixed
                  (loop $mix
                    local.get $u global.get $last_active i32.ge_s br_if $mixed
                    local.get $sum
                    i32.const 12544
                    local.get $u i32.const 1024 i32.mul i32.add
                    local.get $k i32.const 4 i32.mul i32.add
                    f32.load f32.add local.set $sum
                    local.get $u i32.const 1 i32.add local.set $u
                    br $mix))
                i32.const 1024 local.get $k i32.const 4 i32.mul i32.add
                local.get $sum f32.const 0.5 f32.mul global.get $lfo f32.add
                f32.store
                local.get $k i32.const 1 i32.add local.set $k
                br $frames_loop))
            i32.const 0)
          (func (export "rackforge_parallel_begin_block") (param $frames i32) (param $in i32) (param $out i32) (param $midi i32) (param $parameters i32) (result i32)
            local.get $frames local.get $midi local.get $parameters call $begin)
          (func (export "rackforge_parallel_render_unit") (param $unit i32) (param $payload i32) (param $shared i32) (param $frames i32) (param $channels i32) (result i32)
            local.get $unit local.get $payload local.get $shared local.get $frames local.get $channels call $render)
          (func (export "rackforge_parallel_end_block") (param $frames i32) (param $channels i32) (result i32)
            local.get $frames local.get $channels call $end)
          (func (export "rackforge_process") (param $frames i32) (param $in i32) (param $out i32) (param $midi i32) (param $parameters i32) (result i32)
            (local $count i32) (local $u i32) (local $status i32)
            local.get $frames local.get $midi local.get $parameters call $begin
            local.set $count
            (block $done
              (loop $units
                local.get $u local.get $count i32.ge_s br_if $done
                local.get $u i32.const 8 i32.const 8 local.get $frames local.get $out call $render
                local.tee $status i32.const 0 i32.ne
                if local.get $status return end
                ;; deposit the unit's audio in its deterministic mix slot
                i32.const 12544 local.get $u i32.const 1024 i32.mul i32.add
                i32.const 1024
                local.get $frames local.get $out i32.mul i32.const 4 i32.mul
                memory.copy
                local.get $u i32.const 1 i32.add local.set $u
                br $units))
            local.get $frames local.get $out call $end)
        )
    "#;

    /// The worked example in `docs/PLUGIN_ABI.md` is compiled and run here
    /// rather than trusted.
    ///
    /// That document is the whole contract for a plugin author outside this
    /// repository, and its example is the shortest complete statement of it.
    /// Reading the WAT out of the Markdown means the two cannot drift: an ABI
    /// change that invalidates the example fails this test, and the fix is to
    /// correct the document.
    #[test]
    fn the_documented_minimal_plugin_loads_and_renders() {
        const SPEC: &str = include_str!("../../../docs/PLUGIN_ABI.md");
        let source = documented_wat_module(SPEC);
        let bytes = wat::parse_str(&source).expect("the documented example must assemble");
        let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
        let module = engine
            .compile(&bytes)
            .expect("the documented example must load as a wasm-v1 plugin");
        let mut instance = module.instantiate().unwrap();
        instance.prepare(48_000.0, 64, 2, 2).unwrap();
        instance.set_parameter(0, 0.5).unwrap();
        let input = [1.0, -1.0, 0.25, -0.25];
        let mut output = [0.0; 4];
        instance
            .process_interleaved(&input, &mut output, 2)
            .unwrap();
        assert_eq!(
            output,
            [0.5, -0.5, 0.125, -0.125],
            "the documented gain must halve its input"
        );
    }

    /// The one fenced `wat` block in the specification that is a whole module;
    /// the others are single signatures quoted in prose.
    fn documented_wat_module(specification: &str) -> String {
        let mut blocks = specification.split("```wat");
        blocks.next();
        for block in blocks {
            let body = block.split("```").next().unwrap_or_default();
            if body.contains("(module") {
                return body.to_owned();
            }
        }
        panic!("docs/PLUGIN_ABI.md no longer contains a complete wat module");
    }

    #[test]
    fn runs_one_portable_gain_module() {
        let bytes = wat::parse_str(GAIN).unwrap();
        let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
        let module = engine.compile(&bytes).unwrap();
        let mut instance = module.instantiate().unwrap();
        instance.prepare(48_000.0, 64, 2, 2).unwrap();
        instance.set_parameter(0, 0.5).unwrap();
        let input = [1.0, -1.0, 0.25, -0.25];
        let mut output = [0.0; 4];
        instance
            .process_interleaved(&input, &mut output, 2)
            .unwrap();
        assert_eq!(output, [0.5, -0.5, 0.125, -0.125]);
    }

    #[test]
    fn dynamic_preset_catalog_is_optional_and_uses_the_transfer_buffer() {
        let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
        let legacy = engine.compile(&wat::parse_str(GAIN).unwrap()).unwrap();
        assert_eq!(
            legacy.instantiate().unwrap().preset_catalog().unwrap(),
            None
        );

        let source = GAIN.replace(
            "          (func (export \"rackforge_load_preset\")",
            "          (data (i32.const 8192) \"catalog\")\n          (func (export \"rackforge_preset_catalog\") (result i32) i32.const 7)\n          (func (export \"rackforge_load_preset\")",
        );
        let module = engine.compile(&wat::parse_str(source).unwrap()).unwrap();
        let mut instance = module.instantiate().unwrap();
        assert_eq!(
            instance.preset_catalog().unwrap(),
            Some(b"catalog".to_vec())
        );
    }

    #[test]
    fn portable_program_editing_uses_separate_bounded_exchange_buffers() {
        let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
        let module = engine
            .compile(&wat::parse_str(editable_gain()).unwrap())
            .unwrap();
        let mut instance = module.instantiate().unwrap();
        assert!(instance.supports_program_editing());
        assert_eq!(instance.program_editing_capabilities(), 7);
        assert_eq!(instance.begin_program_edit(b"begin").unwrap(), b"begin");
        assert_eq!(
            instance.prepare_program_save(b"prepare").unwrap(),
            b"prepare"
        );
        instance.install_program(b"install").unwrap();
        assert!(instance.preview_program(b"preview").unwrap());
        assert_eq!(instance.program_editor_view(b"editor").unwrap(), b"editor");
        assert_eq!(instance.apply_program_edit(b"edit").unwrap(), b"edit");

        let oversized = vec![0_u8; 1_025];
        assert!(
            instance
                .begin_program_edit(&oversized)
                .unwrap_err()
                .to_string()
                .contains("exceeds")
        );
    }

    #[test]
    fn declared_program_editing_requires_the_complete_basic_contract() {
        let source = GAIN
            .replace("i32.const 65537", "i32.const 65538")
            .replace(
                "          (func (export \"rackforge_load_preset\")",
                "          (func (export \"rackforge_program_editing_capabilities\") (result i32) i32.const 1)\n          (func (export \"rackforge_load_preset\")",
            );
        let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
        let module = engine.compile(&wat::parse_str(source).unwrap()).unwrap();
        let error = match module.instantiate() {
            Ok(_) => panic!("incomplete program-editing ABI was accepted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("rackforge_exchange_input_ptr"));
    }

    #[test]
    fn rejects_overlapping_program_exchange_buffers() {
        let source = editable_gain().replace(
            "(func (export \"rackforge_exchange_input_ptr\") (result i32) i32.const 12288)",
            "(func (export \"rackforge_exchange_input_ptr\") (result i32) i32.const 8192)",
        );
        let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
        let module = engine.compile(&wat::parse_str(source).unwrap()).unwrap();
        let error = match module.instantiate() {
            Ok(_) => panic!("overlapping program exchange buffers were accepted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("must not overlap"));
    }

    #[test]
    fn rejects_ambient_host_imports() {
        let bytes =
            wat::parse_str(r#"(module (import "wasi_snapshot_preview1" "fd_write" (func)))"#)
                .unwrap();
        let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
        let error = match engine.compile(&bytes) {
            Ok(_) => panic!("ambient import was accepted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("may not import"));
    }

    #[test]
    fn delivers_sample_positioned_midi_without_host_imports() {
        let bytes = wat::parse_str(GAIN).unwrap();
        let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
        let module = engine.compile(&bytes).unwrap();
        let mut instance = module.instantiate().unwrap();
        instance.prepare(48_000.0, 64, 2, 2).unwrap();
        let input = [0.0; 4];
        let mut output = [0.0; 4];
        let note_on = MidiEvent::new(1, &[0x90, 60, 100]).unwrap();
        instance
            .process_interleaved_with_midi(&input, &mut output, 2, &[note_on])
            .unwrap();
        assert_eq!(output[0], 0x90 as f32);
    }

    #[test]
    fn delivers_parameter_events_without_host_imports() {
        let bytes = wat::parse_str(GAIN).unwrap();
        let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
        let module = engine.compile(&bytes).unwrap();
        let mut instance = module.instantiate().unwrap();
        instance.prepare(48_000.0, 64, 2, 2).unwrap();
        let input = [1.0; 4];
        let mut output = [0.0; 4];
        let event = ParameterEvent {
            frame: 0,
            index: 0,
            value: 0.25,
        };
        instance
            .process_interleaved_with_events(&input, &mut output, 2, &[], &[event])
            .unwrap();
        assert_eq!(output, [0.25; 4]);
    }

    fn parallel_engine() -> PortableEngine {
        PortableEngine::new(RuntimeLimits::default()).unwrap()
    }

    #[test]
    fn parallel_extension_reports_bounded_geometry() {
        let engine = parallel_engine();
        let module = engine
            .compile(&wat::parse_str(PARALLEL_SYNTH).unwrap())
            .unwrap();
        let instance = module.instantiate().unwrap();
        let layout = instance.parallel_layout().unwrap();
        assert_eq!(layout.max_units, 4);
        assert_eq!(layout.dispatch_stride, 16);
        assert_eq!(layout.mix_slot_samples, 256);

        let classic = engine.compile(&wat::parse_str(GAIN).unwrap()).unwrap();
        assert!(classic.instantiate().unwrap().parallel_layout().is_none());
    }

    #[test]
    fn parallel_path_matches_the_sequential_export_exactly() {
        let engine = parallel_engine();
        let module = engine
            .compile(&wat::parse_str(PARALLEL_SYNTH).unwrap())
            .unwrap();
        let mut sequential = module.instantiate().unwrap();
        let mut coordinator = module.instantiate().unwrap();
        let mut workers: Vec<_> = (0..4).map(|_| module.instantiate().unwrap()).collect();
        sequential.prepare(48_000.0, 64, 0, 2).unwrap();
        coordinator.prepare(48_000.0, 64, 0, 2).unwrap();
        for worker in &mut workers {
            worker.prepare(48_000.0, 64, 0, 2).unwrap();
        }

        let note_on = MidiEvent::new(1, &[0x90, 60, 100]).unwrap();
        let automation = ParameterEvent {
            frame: 0,
            index: 0,
            value: 2.0,
        };
        for block in 0..5 {
            let midi: &[MidiEvent] = if block == 2 {
                core::slice::from_ref(&note_on)
            } else {
                &[]
            };
            let parameters: &[ParameterEvent] = if block == 3 {
                core::slice::from_ref(&automation)
            } else {
                &[]
            };
            let mut expected = [0.0_f32; 128];
            sequential
                .process_interleaved_with_events(&[], &mut expected, 64, midi, parameters)
                .unwrap();

            let mut plan = [ParallelPlanEntry::default(); MAX_PARALLEL_UNITS];
            let block_plan = coordinator
                .parallel_begin_block(&[], 64, midi, parameters, &mut plan)
                .unwrap();
            let active = block_plan.active_units;
            let mut shared = [0_u8; 256];
            let shared = &mut shared[..block_plan.shared_bytes];
            coordinator.parallel_read_shared(shared).unwrap();
            // Units are rendered in reverse plan order on purpose: the mix
            // slots, not completion order, define the deterministic result.
            let mut payload = [0_u8; 16];
            let mut unit_output = [0.0_f32; 128];
            for entry in plan[..active].iter().rev() {
                let payload = &mut payload[..entry.payload_bytes as usize];
                coordinator
                    .parallel_read_dispatch(entry.unit, payload)
                    .unwrap();
                let worker = &mut workers[entry.unit as usize];
                worker.parallel_write_dispatch(entry.unit, payload).unwrap();
                worker.parallel_write_shared(shared).unwrap();
                worker
                    .parallel_render_unit(
                        entry.unit,
                        payload.len(),
                        shared.len(),
                        &[],
                        &mut unit_output,
                        64,
                    )
                    .unwrap();
                coordinator
                    .parallel_write_mix_slot(entry.unit, &unit_output)
                    .unwrap();
            }
            let mut produced = [0.0_f32; 128];
            coordinator.parallel_end_block(&mut produced, 64).unwrap();
            assert_eq!(expected, produced, "block {block} diverged");
            assert!(expected.iter().any(|sample| *sample != 0.0));
        }
    }

    #[test]
    fn a_failing_unit_traps_only_its_own_instance() {
        let engine = parallel_engine();
        let module = engine
            .compile(&wat::parse_str(PARALLEL_SYNTH).unwrap())
            .unwrap();
        let mut coordinator = module.instantiate().unwrap();
        let mut healthy = module.instantiate().unwrap();
        let mut failing = module.instantiate().unwrap();
        coordinator.prepare(48_000.0, 64, 0, 2).unwrap();
        healthy.prepare(48_000.0, 64, 0, 2).unwrap();
        failing.prepare(48_000.0, 64, 0, 2).unwrap();
        failing.set_parameter(1, 1.0).unwrap();

        let mut plan = [ParallelPlanEntry::default(); MAX_PARALLEL_UNITS];
        let block_plan = coordinator
            .parallel_begin_block(&[], 64, &[], &[], &mut plan)
            .unwrap();
        assert_eq!(block_plan.active_units, 3);
        let mut unit_output = [0.0_f32; 128];
        healthy
            .parallel_render_unit(0, 8, 8, &[], &mut unit_output, 64)
            .unwrap();
        let error = failing
            .parallel_render_unit(1, 8, 8, &[], &mut unit_output, 64)
            .unwrap_err();
        assert!(format!("{error:#}").contains("unreachable"));
        // The host silences the quarantined unit and the block still ends.
        coordinator.parallel_write_mix_slot(1, &[0.0; 128]).unwrap();
        let mut produced = [0.0_f32; 128];
        coordinator.parallel_end_block(&mut produced, 64).unwrap();
    }

    #[test]
    fn plan_units_beyond_max_are_rejected() {
        let source = PARALLEL_SYNTH.replace(
            "(func $plan_unit (param $i i32) (result i32) local.get $i)",
            "(func $plan_unit (param $i i32) (result i32) i32.const 9)",
        );
        let engine = parallel_engine();
        let module = engine.compile(&wat::parse_str(source).unwrap()).unwrap();
        let mut coordinator = module.instantiate().unwrap();
        coordinator.prepare(48_000.0, 64, 0, 2).unwrap();
        let mut plan = [ParallelPlanEntry::default(); MAX_PARALLEL_UNITS];
        let error = coordinator
            .parallel_begin_block(&[], 64, &[], &[], &mut plan)
            .unwrap_err();
        assert!(format!("{error:#}").contains("beyond max_units"));
    }

    #[test]
    fn plan_payloads_beyond_the_stride_are_rejected() {
        let source = PARALLEL_SYNTH.replace(
            "                i32.const 12300 local.get $i i32.const 8 i32.mul i32.add\n                i32.const 8 i32.store",
            "                i32.const 12300 local.get $i i32.const 8 i32.mul i32.add\n                i32.const 24 i32.store",
        );
        let engine = parallel_engine();
        let module = engine.compile(&wat::parse_str(&source).unwrap()).unwrap();
        let mut coordinator = module.instantiate().unwrap();
        coordinator.prepare(48_000.0, 64, 0, 2).unwrap();
        let mut plan = [ParallelPlanEntry::default(); MAX_PARALLEL_UNITS];
        let error = coordinator
            .parallel_begin_block(&[], 64, &[], &[], &mut plan)
            .unwrap_err();
        assert!(format!("{error:#}").contains("exceeds dispatch stride"));
    }

    #[test]
    fn duplicate_plan_units_are_rejected() {
        let source = PARALLEL_SYNTH.replace(
            "(func $plan_unit (param $i i32) (result i32) local.get $i)",
            "(func $plan_unit (param $i i32) (result i32) i32.const 0)",
        );
        let engine = parallel_engine();
        let module = engine.compile(&wat::parse_str(&source).unwrap()).unwrap();
        let mut coordinator = module.instantiate().unwrap();
        coordinator.prepare(48_000.0, 64, 0, 2).unwrap();
        let mut plan = [ParallelPlanEntry::default(); MAX_PARALLEL_UNITS];
        let error = coordinator
            .parallel_begin_block(&[], 64, &[], &[], &mut plan)
            .unwrap_err();
        assert!(format!("{error:#}").contains("strictly increasing"));
    }

    #[test]
    fn a_module_may_not_declare_more_units_than_the_host_bound() {
        let source = PARALLEL_SYNTH.replace(
            "(func (export \"rackforge_parallel_max_units\") (result i32) i32.const 4)",
            "(func (export \"rackforge_parallel_max_units\") (result i32) i32.const 32)",
        );
        let engine = parallel_engine();
        let module = engine.compile(&wat::parse_str(&source).unwrap()).unwrap();
        let error = match module.instantiate() {
            Ok(_) => panic!("oversized max_units was accepted"),
            Err(error) => error,
        };
        assert!(format!("{error:#}").contains("outside 1..="));
    }

    #[test]
    fn shared_payloads_beyond_capacity_are_rejected() {
        let source = PARALLEL_SYNTH.replace(
            "            i32.const 12288 i32.const 8 i32.store",
            "            i32.const 12288 i32.const 999 i32.store",
        );
        let engine = parallel_engine();
        let module = engine.compile(&wat::parse_str(&source).unwrap()).unwrap();
        let mut coordinator = module.instantiate().unwrap();
        coordinator.prepare(48_000.0, 64, 0, 2).unwrap();
        let mut plan = [ParallelPlanEntry::default(); MAX_PARALLEL_UNITS];
        let error = coordinator
            .parallel_begin_block(&[], 64, &[], &[], &mut plan)
            .unwrap_err();
        assert!(format!("{error:#}").contains("beyond capacity"));
    }

    #[test]
    fn an_empty_plan_still_runs_the_final_stage() {
        let engine = parallel_engine();
        let module = engine
            .compile(&wat::parse_str(PARALLEL_SYNTH).unwrap())
            .unwrap();
        let mut coordinator = module.instantiate().unwrap();
        coordinator.prepare(48_000.0, 64, 0, 2).unwrap();
        coordinator.set_parameter(0, 0.0).unwrap();
        let mut plan = [ParallelPlanEntry::default(); MAX_PARALLEL_UNITS];
        let block_plan = coordinator
            .parallel_begin_block(&[], 64, &[], &[], &mut plan)
            .unwrap();
        assert_eq!(block_plan.active_units, 0);
        let mut produced = [0.0_f32; 128];
        coordinator.parallel_end_block(&mut produced, 64).unwrap();
        // No units sounded, but the global stage (here: the LFO offset)
        // still shaped the block.
        assert!(produced.iter().all(|sample| *sample == 1.0));
    }

    #[test]
    fn fuel_trap_is_measured_and_control_calls_remain_recoverable() {
        let source = GAIN.replace(
            "            local.get $parameters i32.const 0 i32.gt_s",
            "            (loop $spin br $spin)\n            local.get $parameters i32.const 0 i32.gt_s",
        );
        let limits = RuntimeLimits {
            fuel_per_call: 10_000,
            ..RuntimeLimits::default()
        };
        let engine = PortableEngine::new(limits).unwrap();
        let module = engine.compile(&wat::parse_str(source).unwrap()).unwrap();
        let mut instance = module.instantiate().unwrap();
        instance.prepare(48_000.0, 64, 2, 2).unwrap();
        let input = [0.0; 4];
        let mut output = [0.0; 4];
        let error = instance
            .process_interleaved(&input, &mut output, 2)
            .unwrap_err();
        assert!(format!("{error:#}").contains("fuel"));
        assert_eq!(instance.last_realtime_fuel_consumed(), 10_000);
        instance.reset().unwrap();
    }
}
