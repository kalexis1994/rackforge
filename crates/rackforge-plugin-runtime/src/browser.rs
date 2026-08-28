//! Browser-backed host for RackForge `wasm-v1` processors.
//!
//! A page already contains a WebAssembly engine, so the browser host does not
//! ship a second one. The shell asks its JavaScript embedder to compile and
//! instantiate the component, then drives the very same exported ABI that the
//! native backend drives with Wasmtime. Portable plugins may not import host
//! functions, so nothing but linear memory crosses the boundary.
//!
//! Two guarantees the native backend gets from Wasmtime cannot be reproduced
//! here, and callers must not assume them:
//!
//! * there is no fuel metering, so a component that loops forever blocks the
//!   audio callback instead of trapping. [`PortableInstance::last_realtime_fuel_consumed`]
//!   always reports `0`.
//! * memory growth is observed rather than prevented: the declared maximum is
//!   enforced after the fact, when the next call inspects linear memory.
//!
//! The embedder is expected to reject components that declare imports, which is
//! the check the native backend performs in [`PortableEngine::compile`].

use crate::shared::{
    MAX_PARALLEL_UNITS, PARALLEL_ABI_VERSION_V1, PARALLEL_PLAN_ENTRY_BYTES,
    PARALLEL_PLAN_HEADER_BYTES, PROGRAM_EDIT_KNOWN_CAPABILITIES, ParallelPlanEntry, byte_range,
    check_status, checked_samples, memory_range, ranges_overlap, read_f32,
    validate_parallel_plan, validate_realtime_events, write_f32, write_midi, write_parameters,
};
use crate::ParallelLayout;
use crate::{ABI_VERSION_V1, ABI_VERSION_V1_1, MidiEvent, ParameterEvent, RuntimeLimits};
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::rc::Rc;

/// Exported functions the embedder calls by index rather than by name, so a
/// real-time block does not pay for string lookups.
pub mod export {
    pub const ABI_VERSION: i32 = 0;
    pub const INPUT_PTR: i32 = 1;
    pub const OUTPUT_PTR: i32 = 2;
    pub const MIDI_PTR: i32 = 3;
    pub const PARAMETER_PTR: i32 = 4;
    pub const TRANSFER_PTR: i32 = 5;
    pub const EXCHANGE_INPUT_PTR: i32 = 6;
    pub const CAPACITY_INPUT_SAMPLES: i32 = 7;
    pub const CAPACITY_OUTPUT_SAMPLES: i32 = 8;
    pub const CAPACITY_MIDI_EVENTS: i32 = 9;
    pub const CAPACITY_PARAMETER_EVENTS: i32 = 10;
    pub const CAPACITY_TRANSFER_BYTES: i32 = 11;
    pub const INITIALIZE: i32 = 12;
    pub const PREPARE: i32 = 13;
    pub const SET_PARAMETER: i32 = 14;
    pub const GET_PARAMETER: i32 = 15;
    pub const RESET: i32 = 16;
    pub const RESOURCE_BEGIN: i32 = 17;
    pub const RESOURCE_WRITE: i32 = 18;
    pub const RESOURCE_END: i32 = 19;
    pub const PRESET_CATALOG: i32 = 20;
    pub const LOAD_PRESET: i32 = 21;
    pub const SAVE_STATE: i32 = 22;
    pub const LOAD_STATE: i32 = 23;
    pub const PROCESS: i32 = 24;
    pub const PROGRAM_EDITING_CAPABILITIES: i32 = 25;
    pub const PROGRAM_BEGIN_EDIT: i32 = 26;
    pub const PROGRAM_PREPARE_SAVE: i32 = 27;
    pub const PROGRAM_INSTALL: i32 = 28;
    pub const PROGRAM_PREVIEW: i32 = 29;
    pub const PROGRAM_EDITOR_VIEW: i32 = 30;
    pub const PROGRAM_APPLY_EDIT: i32 = 31;
    pub const PARALLEL_ABI_VERSION: i32 = 32;
    pub const PARALLEL_MAX_UNITS: i32 = 33;
    pub const PARALLEL_DISPATCH_STRIDE: i32 = 34;
    pub const PARALLEL_DISPATCH_PTR: i32 = 35;
    pub const PARALLEL_PLAN_PTR: i32 = 36;
    pub const PARALLEL_MIX_PTR: i32 = 37;
    pub const PARALLEL_SHARED_PTR: i32 = 38;
    pub const PARALLEL_SHARED_CAPACITY: i32 = 39;
    pub const PARALLEL_BEGIN_BLOCK: i32 = 40;
    pub const PARALLEL_RENDER_UNIT: i32 = 41;
    pub const PARALLEL_END_BLOCK: i32 = 42;
}

/// Raw imports the embedding page must supply.
///
/// Every call reports failure the same way: the shim returns a defined value
/// and records a message that [`take_error`] drains. A trap inside the guest is
/// therefore an ordinary `Err` in Rust rather than an aborted shell.
pub mod host {
    #[link(wasm_import_module = "rackforge_plugin_host")]
    unsafe extern "C" {
        pub fn rf_compile(bytes: *const u8, length: usize) -> i32;
        pub fn rf_module_release(module: i32);
        pub fn rf_instantiate(module: i32) -> i32;
        pub fn rf_instance_release(instance: i32);
        pub fn rf_memory_size(instance: i32) -> i32;
        pub fn rf_memory_read(instance: i32, offset: i32, destination: *mut u8, length: i32)
        -> i32;
        pub fn rf_memory_write(instance: i32, offset: i32, source: *const u8, length: i32) -> i32;
        pub fn rf_export_present(instance: i32, export: i32) -> i32;
        pub fn rf_call_0(instance: i32, export: i32) -> i32;
        pub fn rf_call_1(instance: i32, export: i32, argument: i32) -> i32;
        pub fn rf_call_f64(instance: i32, export: i32, argument: i32) -> f64;
        pub fn rf_call_set_parameter(instance: i32, index: i32, value: f64) -> i32;
        pub fn rf_call_prepare(
            instance: i32,
            sample_rate: f64,
            maximum_frames: i32,
            input_channels: i32,
            output_channels: i32,
        ) -> i32;
        pub fn rf_call_resource_begin(instance: i32, id_length: i32, total_bytes: i64) -> i32;
        pub fn rf_call_resource_write(instance: i32, offset: i64, length: i32) -> i32;
        pub fn rf_call_process(
            instance: i32,
            frames: i32,
            input_channels: i32,
            output_channels: i32,
            midi_events: i32,
            parameter_events: i32,
        ) -> i32;
        /// Copies the pending error message out and clears it. Returns the
        /// number of bytes written, or `0` when the last call succeeded.
        pub fn rf_take_error(destination: *mut u8, capacity: i32) -> i32;
        pub fn rf_call_2(instance: i32, export: i32, a: i32, b: i32) -> i32;
        pub fn rf_call_5(instance: i32, export: i32, a: i32, b: i32, c: i32, d: i32, e: i32)
        -> i32;
        // The browser render pool: parallel_render_v1 units on Web Workers,
        // coordinated over a SharedArrayBuffer the page attaches. Every one of
        // these is a cheap no-op until a pool exists.
        pub fn rf_par_ready() -> i32;
        #[allow(clippy::too_many_arguments)]
        pub fn rf_par_request(
            module: i32,
            max_units: i32,
            dispatch_stride: i32,
            mix_slot_samples: i32,
            shared_capacity: i32,
            sample_rate: i32,
            maximum_frames: i32,
            input_channels: i32,
            output_channels: i32,
        );
        pub fn rf_par_begin(frames: i32, active: i32, shared_len: i32, shared_ptr: *const u8)
        -> i32;
        pub fn rf_par_entry(slot: i32, unit: i32, payload_len: i32, payload_ptr: *const u8)
        -> i32;
        pub fn rf_par_commit() -> i32;
        pub fn rf_par_collect(budget_micros: i32) -> i32;
        pub fn rf_par_mix_read(unit: i32, destination: *mut u8, length_samples: i32) -> i32;
        pub fn rf_par_unit_mask() -> i32;
        pub fn rf_par_misses() -> i32;
    }
}

const MAX_HOST_ERROR_BYTES: usize = 512;

/// Drains the embedder's pending error, if the previous call recorded one.
fn take_error() -> Option<String> {
    let mut buffer = [0_u8; MAX_HOST_ERROR_BYTES];
    // SAFETY: the embedder writes at most `capacity` bytes into the buffer.
    let written = unsafe { host::rf_take_error(buffer.as_mut_ptr(), buffer.len() as i32) };
    if written <= 0 {
        return None;
    }
    let written = (written as usize).min(buffer.len());
    Some(String::from_utf8_lossy(&buffer[..written]).into_owned())
}

/// Turns a pending embedder error into a Rust error, so a guest trap never
/// looks like a successful call that returned a strange value.
fn checked<T>(value: T, operation: &str) -> Result<T> {
    match take_error() {
        None => Ok(value),
        Some(message) => bail!("portable plugin {operation} failed: {message}"),
    }
}

struct ModuleHandle(i32);

impl Drop for ModuleHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was produced by `rf_compile` and is released once.
        unsafe { host::rf_module_release(self.0) };
    }
}

struct InstanceHandle(i32);

impl Drop for InstanceHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was produced by `rf_instantiate` and is released
        // once.
        unsafe { host::rf_instance_release(self.0) };
    }
}

pub struct PortableEngine {
    limits: RuntimeLimits,
}

impl PortableEngine {
    pub fn new(limits: RuntimeLimits) -> Result<Self> {
        Ok(Self { limits })
    }

    /// Accepted for parity with the native backend. The browser caches compiled
    /// modules itself, so RackForge never owns a code cache directory here.
    pub fn with_cache(limits: RuntimeLimits, _directory: impl AsRef<Path>) -> Result<Self> {
        Self::new(limits)
    }

    pub fn compile(&self, bytes: &[u8]) -> Result<PortableModule> {
        // SAFETY: the embedder reads `length` bytes and copies them out before
        // returning.
        let handle = unsafe { host::rf_compile(bytes.as_ptr(), bytes.len()) };
        checked(handle, "compile")?;
        if handle < 0 {
            bail!("compiling RackForge WebAssembly component failed");
        }
        Ok(PortableModule {
            handle: Rc::new(ModuleHandle(handle)),
            limits: self.limits,
        })
    }
}

pub struct PortableModule {
    handle: Rc<ModuleHandle>,
    limits: RuntimeLimits,
}

impl PortableModule {
    pub fn instantiate(&self) -> Result<PortableInstance> {
        // SAFETY: the handle is a live module produced by `rf_compile`.
        let handle = unsafe { host::rf_instantiate(self.handle.0) };
        checked(handle, "instantiate")?;
        if handle < 0 {
            bail!("instantiating RackForge WebAssembly plugin failed");
        }
        let raw = RawInstance {
            handle: InstanceHandle(handle),
            maximum_memory_bytes: self.limits.maximum_memory_bytes,
        };

        let version = raw.call_0(export::ABI_VERSION, "abi_version")?;
        if !(ABI_VERSION_V1_1..=ABI_VERSION_V1).contains(&version) {
            bail!("unsupported wasm-v1 ABI version {version:#010x}");
        }

        let input_offset = raw.call_0(export::INPUT_PTR, "input_ptr")?;
        let output_offset = raw.call_0(export::OUTPUT_PTR, "output_ptr")?;
        let midi_offset = raw.call_0(export::MIDI_PTR, "midi_ptr")?;
        let parameter_offset = raw.call_0(export::PARAMETER_PTR, "parameter_ptr")?;
        let transfer_offset = raw.call_0(export::TRANSFER_PTR, "transfer_ptr")?;

        let input_capacity =
            raw.call_0(export::CAPACITY_INPUT_SAMPLES, "capacity_input_samples")?;
        let output_capacity =
            raw.call_0(export::CAPACITY_OUTPUT_SAMPLES, "capacity_output_samples")?;
        if input_capacity < 0 || output_capacity < 0 {
            bail!("wasm-v1 plugin reported an invalid audio buffer capacity");
        }
        let midi_capacity = raw.call_0(export::CAPACITY_MIDI_EVENTS, "capacity_midi_events")?;
        if midi_capacity <= 0 {
            bail!("wasm-v1 plugin reported an invalid MIDI event capacity");
        }
        let parameter_capacity = raw.call_0(
            export::CAPACITY_PARAMETER_EVENTS,
            "capacity_parameter_events",
        )?;
        if parameter_capacity <= 0 {
            bail!("wasm-v1 plugin reported an invalid parameter event capacity");
        }
        let transfer_capacity =
            raw.call_0(export::CAPACITY_TRANSFER_BYTES, "capacity_transfer_bytes")?;
        if transfer_capacity <= 0 {
            bail!("wasm-v1 plugin reported an invalid transfer capacity");
        }

        check_status(raw.call_0(export::INITIALIZE, "initialize")?, "initialize")?;

        let program_api = if raw.export_present(export::PROGRAM_EDITING_CAPABILITIES) {
            let capabilities = raw.call_0(
                export::PROGRAM_EDITING_CAPABILITIES,
                "program_editing_capabilities",
            )?;
            if capabilities < 0 {
                bail!("portable plugin returned invalid program-editing capabilities");
            }
            let capabilities = capabilities as u32;
            if capabilities & !PROGRAM_EDIT_KNOWN_CAPABILITIES != 0
                || capabilities != 0 && capabilities & crate::shared::PROGRAM_EDIT_BASIC == 0
            {
                bail!(
                    "portable plugin returned unsupported program-editing capabilities {capabilities:#x}"
                );
            }
            if capabilities == 0 {
                None
            } else {
                if !raw.export_present(export::EXCHANGE_INPUT_PTR) {
                    bail!("wasm-v1 plugin is missing export rackforge_exchange_input_ptr");
                }
                let exchange_input_offset =
                    raw.call_0(export::EXCHANGE_INPUT_PTR, "exchange_input_ptr")?;
                let memory_size = raw.memory_size()?;
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
                for (capability, export, name) in [
                    (
                        crate::shared::PROGRAM_EDIT_BASIC,
                        export::PROGRAM_BEGIN_EDIT,
                        "rackforge_program_begin_edit",
                    ),
                    (
                        crate::shared::PROGRAM_EDIT_BASIC,
                        export::PROGRAM_PREPARE_SAVE,
                        "rackforge_program_prepare_save",
                    ),
                    (
                        crate::shared::PROGRAM_EDIT_BASIC,
                        export::PROGRAM_INSTALL,
                        "rackforge_program_install",
                    ),
                    (
                        crate::shared::PROGRAM_EDIT_PREVIEW,
                        export::PROGRAM_PREVIEW,
                        "rackforge_program_preview",
                    ),
                    (
                        crate::shared::PROGRAM_EDIT_DECLARATIVE,
                        export::PROGRAM_EDITOR_VIEW,
                        "rackforge_program_editor_view",
                    ),
                    (
                        crate::shared::PROGRAM_EDIT_DECLARATIVE,
                        export::PROGRAM_APPLY_EDIT,
                        "rackforge_program_apply_edit",
                    ),
                ] {
                    if capabilities & capability != 0 && !raw.export_present(export) {
                        bail!("wasm-v1 plugin is missing export {name}");
                    }
                }
                Some(PortableProgramApi {
                    capabilities,
                    exchange_input_offset,
                })
            }
        } else {
            None
        };

        // The same discovery and validation the native backend performs: the
        // extension is only trusted when every export exists and every number
        // it reports respects the shared contract limits.
        let parallel_api = if raw.export_present(export::PARALLEL_ABI_VERSION) {
            let version = raw.call_0(export::PARALLEL_ABI_VERSION, "parallel_abi_version")?;
            if version != PARALLEL_ABI_VERSION_V1 {
                bail!("unsupported parallel-render ABI version {version:#010x}");
            }
            let max_units = raw.call_0(export::PARALLEL_MAX_UNITS, "parallel_max_units")?;
            if max_units < 1 || max_units as usize > MAX_PARALLEL_UNITS {
                bail!("parallel-render max_units {max_units} is outside 1..={MAX_PARALLEL_UNITS}");
            }
            let max_units = max_units as usize;
            let dispatch_stride =
                raw.call_0(export::PARALLEL_DISPATCH_STRIDE, "parallel_dispatch_stride")?;
            if dispatch_stride <= 0 || !(dispatch_stride as usize).is_multiple_of(8) {
                bail!("parallel-render dispatch stride must be a positive multiple of 8");
            }
            let dispatch_stride = dispatch_stride as usize;
            let dispatch_offset =
                raw.call_0(export::PARALLEL_DISPATCH_PTR, "parallel_dispatch_ptr")?;
            let plan_offset = raw.call_0(export::PARALLEL_PLAN_PTR, "parallel_plan_ptr")?;
            let mix_offset = raw.call_0(export::PARALLEL_MIX_PTR, "parallel_mix_ptr")?;
            let shared_offset = raw.call_0(export::PARALLEL_SHARED_PTR, "parallel_shared_ptr")?;
            let shared_capacity =
                raw.call_0(export::PARALLEL_SHARED_CAPACITY, "parallel_shared_capacity")?;
            if shared_capacity <= 0 || !(shared_capacity as usize).is_multiple_of(8) {
                bail!("parallel-render shared capacity must be a positive multiple of 8");
            }
            let shared_capacity = shared_capacity as usize;
            let memory_size = raw.memory_size()?;
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
            for (index, name) in [
                (export::PARALLEL_BEGIN_BLOCK, "rackforge_parallel_begin_block"),
                (export::PARALLEL_RENDER_UNIT, "rackforge_parallel_render_unit"),
                (export::PARALLEL_END_BLOCK, "rackforge_parallel_end_block"),
            ] {
                if !raw.export_present(index) {
                    bail!("wasm-v1 plugin is missing export {name}");
                }
            }
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
            })
        } else {
            None
        };

        Ok(PortableInstance {
            raw,
            input_offset,
            output_offset,
            midi_offset,
            parameter_offset,
            transfer_offset,
            capacity_input_samples: input_capacity as usize,
            capacity_output_samples: output_capacity as usize,
            capacity_midi_events: midi_capacity as usize,
            capacity_parameter_events: parameter_capacity as usize,
            capacity_transfer_bytes: transfer_capacity as usize,
            program_api,
            parallel_api,
            prepared_sample_rate: 0.0,
            prepared_input_channels: 0,
            prepared_output_channels: 0,
            maximum_frames: 0,
            scratch: Vec::new(),
            _module: Rc::clone(&self.handle),
        })
    }
}

struct PortableProgramApi {
    capabilities: u32,
    exchange_input_offset: i32,
}

/// Geometry and coordinator offsets of the parallel-render extension, the
/// browser twin of the native backend's api struct. The begin/render/end
/// functions are addressed through the shared export table instead of typed
/// funcs, and unit rendering happens in Web Workers the page owns.
struct PortableParallelApi {
    layout: ParallelLayout,
    dispatch_offset: i32,
    plan_offset: i32,
    mix_offset: i32,
    shared_offset: i32,
}

/// The thin, unchecked half of the boundary: one instance handle plus the
/// bounds the embedder cannot enforce on its own.
struct RawInstance {
    handle: InstanceHandle,
    maximum_memory_bytes: usize,
}

impl RawInstance {
    fn memory_size(&self) -> Result<usize> {
        // SAFETY: the handle is live for as long as `self`.
        let size = unsafe { host::rf_memory_size(self.handle.0) };
        checked(size, "memory_size")?;
        if size < 0 {
            bail!("portable plugin does not export memory");
        }
        let size = size as usize;
        if size > self.maximum_memory_bytes {
            bail!(
                "portable plugin grew linear memory to {size} bytes, past its {} byte limit",
                self.maximum_memory_bytes
            );
        }
        Ok(size)
    }

    fn export_present(&self, export: i32) -> bool {
        // SAFETY: the handle is live for as long as `self`.
        let present = unsafe { host::rf_export_present(self.handle.0, export) };
        let _ = take_error();
        present != 0
    }

    fn call_0(&self, export: i32, operation: &str) -> Result<i32> {
        // SAFETY: the handle is live and the export index is one of the
        // constants the embedder maps to a plugin export.
        let value = unsafe { host::rf_call_0(self.handle.0, export) };
        checked(value, operation)
    }

    fn call_1(&self, export: i32, argument: i32, operation: &str) -> Result<i32> {
        // SAFETY: as `call_0`.
        let value = unsafe { host::rf_call_1(self.handle.0, export, argument) };
        checked(value, operation)
    }

    fn read(&self, offset: i32, length: usize) -> Result<Vec<u8>> {
        byte_range(offset, length, 1, 1, self.memory_size()?)?;
        let mut buffer = vec![0_u8; length];
        // SAFETY: the destination holds `length` bytes and the source range was
        // just bounds-checked against linear memory.
        let written = unsafe {
            host::rf_memory_read(self.handle.0, offset, buffer.as_mut_ptr(), length as i32)
        };
        checked(written, "memory_read")?;
        if written < 0 || written as usize != length {
            bail!("portable plugin memory read was truncated");
        }
        Ok(buffer)
    }

    fn write(&self, offset: i32, bytes: &[u8]) -> Result<()> {
        byte_range(offset, bytes.len(), 1, 1, self.memory_size()?)?;
        // SAFETY: the source holds `len` bytes and the destination range was
        // just bounds-checked against linear memory.
        let written = unsafe {
            host::rf_memory_write(self.handle.0, offset, bytes.as_ptr(), bytes.len() as i32)
        };
        checked(written, "memory_write")?;
        if written < 0 || written as usize != bytes.len() {
            bail!("portable plugin memory write was truncated");
        }
        Ok(())
    }
}

pub struct PortableInstance {
    raw: RawInstance,
    input_offset: i32,
    output_offset: i32,
    midi_offset: i32,
    parameter_offset: i32,
    transfer_offset: i32,
    capacity_input_samples: usize,
    capacity_output_samples: usize,
    capacity_midi_events: usize,
    capacity_parameter_events: usize,
    capacity_transfer_bytes: usize,
    program_api: Option<PortableProgramApi>,
    parallel_api: Option<PortableParallelApi>,
    prepared_sample_rate: f64,
    prepared_input_channels: u32,
    prepared_output_channels: u32,
    maximum_frames: u32,
    /// Staging buffer reused by every real-time block so the audio callback
    /// does not allocate.
    scratch: Vec<u8>,
    /// Keeps the compiled module alive for as long as one of its instances is.
    _module: Rc<ModuleHandle>,
}

impl PortableInstance {
    /// The live module handle, used to name this component to the render
    /// pool: the page keeps the compiled bytes under the same handle.
    fn module_handle(&self) -> i32 {
        self._module.0
    }

    /// Rejected in the browser: the page has no filesystem to read from, so
    /// resources arrive through [`PortableInstance::load_resource`].
    pub fn load_resource_file(&mut self, _id: &str, path: impl AsRef<Path>) -> Result<()> {
        bail!(
            "the browser host cannot read portable resource {} from a filesystem",
            path.as_ref().display()
        )
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
        // SAFETY: the handle is live for as long as `self`.
        let status = unsafe {
            host::rf_call_prepare(
                self.raw.handle.0,
                sample_rate,
                maximum_frames as i32,
                input_channels as i32,
                output_channels as i32,
            )
        };
        check_status(checked(status, "prepare")?, "prepare")?;
        self.prepared_sample_rate = sample_rate;
        self.prepared_input_channels = input_channels;
        self.prepared_output_channels = output_channels;
        self.maximum_frames = maximum_frames;
        if let Some(api) = &self.parallel_api {
            // Fire-and-forget: the page builds workers off the audio thread
            // and attaches whenever they are ready. Until then — and on any
            // page that is not cross-origin isolated, ever — every block
            // simply takes the sequential path below.
            // SAFETY: plain integers describing the request.
            unsafe {
                host::rf_par_request(
                    self.module_handle(),
                    api.layout.max_units as i32,
                    api.layout.dispatch_stride as i32,
                    api.layout.mix_slot_samples as i32,
                    api.layout.shared_capacity as i32,
                    sample_rate as i32,
                    maximum_frames as i32,
                    input_channels as i32,
                    output_channels as i32,
                );
            }
        }
        Ok(())
    }

    pub fn set_parameter(&mut self, index: u32, value: f64) -> Result<()> {
        // SAFETY: the handle is live for as long as `self`.
        let status = unsafe { host::rf_call_set_parameter(self.raw.handle.0, index as i32, value) };
        check_status(checked(status, "set_parameter")?, "set_parameter")
    }

    pub fn get_parameter(&mut self, index: u32) -> Result<f64> {
        // SAFETY: the handle is live for as long as `self`.
        let value =
            unsafe { host::rf_call_f64(self.raw.handle.0, export::GET_PARAMETER, index as i32) };
        let value = checked(value, "get_parameter")?;
        if !value.is_finite() {
            bail!("portable plugin does not expose parameter {index}");
        }
        Ok(value)
    }

    pub fn reset(&mut self) -> Result<()> {
        check_status(self.raw.call_0(export::RESET, "reset")?, "reset")
    }

    /// Delivers one already-authorized package resource without exposing the
    /// page's storage to the guest.
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
        // SAFETY: the handle is live for as long as `self`.
        let status = unsafe {
            host::rf_call_resource_begin(self.raw.handle.0, id.len() as i32, total_bytes)
        };
        check_status(checked(status, "resource_begin")?, "resource_begin")
    }

    fn write_resource(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.capacity_transfer_bytes {
            bail!("resource chunk does not fit the portable transfer buffer");
        }
        let offset = i64::try_from(offset).context("resource offset is too large")?;
        self.write_transfer(bytes)?;
        // SAFETY: the handle is live for as long as `self`.
        let status =
            unsafe { host::rf_call_resource_write(self.raw.handle.0, offset, bytes.len() as i32) };
        check_status(checked(status, "resource_write")?, "resource_write")
    }

    fn end_resource(&mut self) -> Result<()> {
        check_status(
            self.raw.call_0(export::RESOURCE_END, "resource_end")?,
            "resource_end",
        )
    }

    /// Returns an optional instance-specific preset catalog produced after
    /// resource delivery. Components built with an older SDK simply fall back
    /// to their package metadata.
    pub fn preset_catalog(&mut self) -> Result<Option<Vec<u8>>> {
        if !self.raw.export_present(export::PRESET_CATALOG) {
            return Ok(None);
        }
        let length = self.raw.call_0(export::PRESET_CATALOG, "preset_catalog")?;
        if length == 0 {
            return Ok(None);
        }
        if length < 0 || length as usize > self.capacity_transfer_bytes {
            bail!("portable plugin returned an invalid preset catalog length {length}");
        }
        Ok(Some(self.read_transfer(length as usize)?))
    }

    pub fn load_preset(&mut self, id: &str) -> Result<()> {
        if id.is_empty() {
            bail!("preset id must not be empty");
        }
        self.write_transfer(id.as_bytes())?;
        let status = self
            .raw
            .call_1(export::LOAD_PRESET, id.len() as i32, "load_preset")?;
        check_status(status, "load_preset")
    }

    pub fn save_state(&mut self) -> Result<Vec<u8>> {
        let length = self.raw.call_0(export::SAVE_STATE, "save_state")?;
        if length < 0 || length as usize > self.capacity_transfer_bytes {
            bail!("portable plugin returned an invalid state length {length}");
        }
        self.read_transfer(length as usize)
    }

    pub fn load_state(&mut self, state: &[u8]) -> Result<()> {
        self.write_transfer(state)?;
        let status = self
            .raw
            .call_1(export::LOAD_STATE, state.len() as i32, "load_state")?;
        check_status(status, "load_state")
    }

    /// The browser backend never schedules units: the page has one audio
    /// rendering thread, so the classic `rackforge_process` export of the
    /// very same component is the sequential fallback the extension
    /// guarantees. Reporting `None` keeps callers on that path.
    pub fn parallel_layout(&self) -> Option<crate::ParallelLayout> {
        None
    }

    pub fn supports_program_editing(&self) -> bool {
        self.program_editing_capabilities() != 0
    }

    pub fn program_editing_capabilities(&self) -> u32 {
        self.program_api.as_ref().map_or(0, |api| api.capabilities)
    }

    pub fn begin_program_edit(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        self.require_program_api()?;
        self.exchange_program_bytes(export::PROGRAM_BEGIN_EDIT, request, "begin_program_edit")
    }

    pub fn prepare_program_save(&mut self, document: &[u8]) -> Result<Vec<u8>> {
        self.require_program_api()?;
        self.exchange_program_bytes(
            export::PROGRAM_PREPARE_SAVE,
            document,
            "prepare_program_save",
        )
    }

    pub fn install_program(&mut self, prepared: &[u8]) -> Result<()> {
        self.require_program_api()?;
        self.call_program_install(export::PROGRAM_INSTALL, prepared, "install_program")
    }

    pub fn preview_program(&mut self, prepared: &[u8]) -> Result<bool> {
        let capabilities = self.require_program_api()?;
        if capabilities & crate::shared::PROGRAM_EDIT_PREVIEW == 0 {
            return Ok(false);
        }
        self.call_program_install(export::PROGRAM_PREVIEW, prepared, "preview_program")?;
        Ok(true)
    }

    pub fn program_editor_view(&mut self, document: &[u8]) -> Result<Vec<u8>> {
        let capabilities = self.require_program_api()?;
        if capabilities & crate::shared::PROGRAM_EDIT_DECLARATIVE == 0 {
            bail!("portable plugin does not expose a declarative program editor");
        }
        self.exchange_program_bytes(export::PROGRAM_EDITOR_VIEW, document, "program_editor_view")
    }

    pub fn apply_program_edit(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        let capabilities = self.require_program_api()?;
        if capabilities & crate::shared::PROGRAM_EDIT_DECLARATIVE == 0 {
            bail!("portable plugin does not expose declarative program edits");
        }
        self.exchange_program_bytes(export::PROGRAM_APPLY_EDIT, request, "apply_program_edit")
    }

    fn require_program_api(&self) -> Result<u32> {
        self.program_api
            .as_ref()
            .map(|api| api.capabilities)
            .context("portable plugin does not expose program editing")
    }

    fn exchange_program_bytes(
        &mut self,
        export: i32,
        source: &[u8],
        operation: &str,
    ) -> Result<Vec<u8>> {
        self.write_program_input(source)?;
        let source_length = i32::try_from(source.len()).context("program payload is too large")?;
        let length = self.raw.call_1(export, source_length, operation)?;
        if length < 0 || length as usize > self.capacity_transfer_bytes {
            bail!("portable plugin {operation} returned invalid length {length}");
        }
        self.read_transfer(length as usize)
    }

    fn call_program_install(&mut self, export: i32, source: &[u8], operation: &str) -> Result<()> {
        self.write_program_input(source)?;
        let source_length = i32::try_from(source.len()).context("program payload is too large")?;
        let status = self.raw.call_1(export, source_length, operation)?;
        check_status(status, operation)
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
        self.raw.write(offset, bytes)
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

        let memory_size = self.raw.memory_size()?;
        let input_range = memory_range(self.input_offset, input_samples, memory_size)?;
        let output_range = memory_range(self.output_offset, output_samples, memory_size)?;
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

        self.stage(input_range.len(), |scratch| {
            write_f32(scratch, 0..input_samples * size_of::<f32>(), input)
        });
        self.raw.write(self.input_offset, &self.scratch)?;
        if !midi.is_empty() {
            self.stage(midi_range.len(), |scratch| {
                write_midi(scratch, 0..midi.len() * size_of::<u64>(), midi)
            });
            self.raw.write(self.midi_offset, &self.scratch)?;
        }
        if !parameters.is_empty() {
            self.stage(parameter_range.len(), |scratch| {
                write_parameters(scratch, 0..parameters.len() * 16, parameters)
            });
            self.raw.write(self.parameter_offset, &self.scratch)?;
        }

        // The extension's guarantee is that the classic `rackforge_process`
        // export of the very same component is the sequential fallback, so
        // the parallel path is taken only while a worker pool is actually
        // standing by. Everything serial about the block happens on this
        // thread either way.
        if self.parallel_api.is_some() {
            // SAFETY: no arguments; reports pool readiness.
            let workers = unsafe { host::rf_par_ready() };
            if workers > 0 {
                return self.process_parallel_block(frames, midi.len(), parameters.len(), output);
            }
        }

        // SAFETY: the handle is live for as long as `self`.
        let status = unsafe {
            host::rf_call_process(
                self.raw.handle.0,
                frames as i32,
                self.prepared_input_channels as i32,
                self.prepared_output_channels as i32,
                midi.len() as i32,
                parameters.len() as i32,
            )
        };
        check_status(checked(status, "process")?, "process")?;

        let rendered = self.raw.read(self.output_offset, output_range.len())?;
        read_f32(&rendered, 0..output_range.len(), output);
        Ok(())
    }

    /// One block through the parallel-render extension, with the units fanned
    /// out to the page-owned Web Worker pool.
    ///
    /// The order is the native coordinator's: begin_block serially, publish
    /// the shared payload and the per-unit dispatch payloads, let the pool
    /// render, deposit every mix slot in unit order, end_block serially. A
    /// unit the pool failed to deliver inside the budget contributes silence
    /// for this block — never a stall, and never incoherent unit state, since
    /// unit state advances only inside its own worker instance.
    fn process_parallel_block(
        &mut self,
        frames: u32,
        midi_events: usize,
        parameter_events: usize,
        output: &mut [f32],
    ) -> Result<()> {
        let api = self
            .parallel_api
            .as_ref()
            .context("portable plugin does not expose parallel render")?;
        let layout = api.layout;
        let plan_offset = api.plan_offset;
        let dispatch_offset = api.dispatch_offset;
        let mix_offset = api.mix_offset;
        let shared_offset = api.shared_offset;

        // Serial pre-stage on the coordinator. Events were already staged by
        // the caller; begin_block consumes them and writes the plan.
        // SAFETY: the handle is live for as long as `self`.
        let active = unsafe {
            host::rf_call_5(
                self.raw.handle.0,
                export::PARALLEL_BEGIN_BLOCK,
                frames as i32,
                self.prepared_input_channels as i32,
                self.prepared_output_channels as i32,
                midi_events as i32,
                parameter_events as i32,
            )
        };
        let active = checked(active, "parallel_begin_block")?;
        if active < 0 {
            bail!("portable plugin begin_block failed with status {active}");
        }
        let active = active as usize;
        if active > layout.max_units {
            bail!("portable plugin announced {active} units beyond max_units");
        }

        let plan_bytes = self.raw.read(
            plan_offset,
            PARALLEL_PLAN_HEADER_BYTES + active * PARALLEL_PLAN_ENTRY_BYTES,
        )?;
        let shared_bytes =
            u32::from_le_bytes(plan_bytes[0..4].try_into().expect("plan header")) as usize;
        if shared_bytes > layout.shared_capacity {
            bail!(
                "portable plugin announced {shared_bytes} shared bytes beyond capacity {}",
                layout.shared_capacity
            );
        }
        let mut plan = [ParallelPlanEntry::default(); MAX_PARALLEL_UNITS];
        for (entry, chunk) in plan[..active]
            .iter_mut()
            .zip(plan_bytes[PARALLEL_PLAN_HEADER_BYTES..].chunks_exact(8))
        {
            entry.unit = u32::from_le_bytes(chunk[0..4].try_into().expect("plan entry unit"));
            entry.payload_bytes =
                u32::from_le_bytes(chunk[4..8].try_into().expect("plan entry payload"));
        }
        validate_parallel_plan(&plan[..active], layout.max_units, layout.dispatch_stride)?;

        // Publish the block to the pool. The shared payload and each dispatch
        // payload cross coordinator -> host scratch -> shared buffer; both
        // hops are plain copies of small, bounded regions.
        let shared = self.raw.read(shared_offset, shared_bytes)?;
        // SAFETY: `shared` stays alive across the call, which copies from it.
        let begun = unsafe {
            host::rf_par_begin(
                frames as i32,
                active as i32,
                shared_bytes as i32,
                shared.as_ptr(),
            )
        };
        if begun != 0 {
            bail!("the render pool rejected the block header");
        }
        for (slot, entry) in plan[..active].iter().enumerate() {
            let payload = self.raw.read(
                dispatch_offset + (entry.unit as usize * layout.dispatch_stride) as i32,
                entry.payload_bytes as usize,
            )?;
            // SAFETY: `payload` stays alive across the call, which copies.
            let staged = unsafe {
                host::rf_par_entry(
                    slot as i32,
                    entry.unit as i32,
                    entry.payload_bytes as i32,
                    payload.as_ptr(),
                )
            };
            if staged != 0 {
                bail!("the render pool rejected a dispatch payload");
            }
        }
        // SAFETY: no arguments; wakes the workers.
        if unsafe { host::rf_par_commit() } != 0 {
            bail!("the render pool rejected the commit");
        }

        // Three quarters of the block's real-time budget: enough for the pool
        // to be genuinely late before anything is given up, while the audio
        // thread keeps a margin for end_block and the master chain.
        let budget_micros = if self.prepared_sample_rate > 0.0 {
            ((frames as f64 / self.prepared_sample_rate) * 750_000.0) as i32
        } else {
            2_000
        };
        // SAFETY: plain integer budget; spins on the shared buffer.
        let _done = unsafe { host::rf_par_collect(budget_micros) };
        // SAFETY: no arguments; reads the completion bitmask.
        let finished_mask = unsafe { host::rf_par_unit_mask() } as u32;

        let mix_samples = checked_samples(frames, self.prepared_output_channels)?;
        for entry in &plan[..active] {
            let slot_offset =
                mix_offset + (entry.unit as usize * layout.mix_slot_samples * 4) as i32;
            self.stage(mix_samples * 4, |scratch| scratch.fill(0));
            if finished_mask & (1 << entry.unit) != 0 {
                // SAFETY: scratch is host memory sized for the copy.
                let read = unsafe {
                    host::rf_par_mix_read(
                        entry.unit as i32,
                        self.scratch.as_mut_ptr(),
                        mix_samples as i32,
                    )
                };
                if read != 0 {
                    bail!("the render pool could not deliver a mix slot");
                }
            }
            // A missed unit keeps the zero fill: silence for one block.
            let staged = std::mem::take(&mut self.scratch);
            self.raw.write(slot_offset, &staged)?;
            self.scratch = staged;
        }

        // Serial post-stage: the deterministic combine, in slot order fixed
        // by unit index regardless of which worker finished first.
        // SAFETY: the handle is live for as long as `self`.
        let status = unsafe {
            host::rf_call_2(
                self.raw.handle.0,
                export::PARALLEL_END_BLOCK,
                frames as i32,
                self.prepared_output_channels as i32,
            )
        };
        check_status(checked(status, "parallel_end_block")?, "parallel_end_block")?;

        let rendered = self.raw.read(self.output_offset, mix_samples * 4)?;
        read_f32(&rendered, 0..mix_samples * 4, output);
        Ok(())
    }

    /// Always `0`: the browser engine does not meter guest execution, so no
    /// fuel figure would be truthful.
    pub const fn last_realtime_fuel_consumed(&self) -> u64 {
        0
    }

    /// Fills the reusable staging buffer, growing it only when a block needs
    /// more room than the previous one did.
    fn stage(&mut self, length: usize, fill: impl FnOnce(&mut [u8])) {
        if self.scratch.len() < length {
            self.scratch.resize(length, 0);
        }
        self.scratch.truncate(length);
        fill(&mut self.scratch);
    }

    fn write_transfer(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.capacity_transfer_bytes {
            bail!("control payload exceeds plugin transfer capacity");
        }
        self.raw.write(self.transfer_offset, bytes)
    }

    fn read_transfer(&self, length: usize) -> Result<Vec<u8>> {
        if length > self.capacity_transfer_bytes {
            bail!("control payload exceeds plugin transfer capacity");
        }
        self.raw.read(self.transfer_offset, length)
    }
}
