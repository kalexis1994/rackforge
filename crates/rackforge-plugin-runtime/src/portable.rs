//! One portable processor, however it runs on this machine.
//!
//! A package's `wasm-v1` component runs everywhere, compiled by Wasmtime
//! ([`crate::native`]); a native build of the same processor, where the
//! package carries one for this platform and the caller trusts it, runs in
//! its place ([`crate::library`]). Both answer the same calls with the same
//! checks, so everything above -- resources, presets, state, programs, the
//! real-time path -- is written once against [`PortableModule`] and
//! [`PortableInstance`].
//!
//! What only the sandbox has, a native build answers as a component that
//! does not use it would: no fuel and no budget.

use crate::library::{LibraryInstance, LibraryModule};
use crate::native::{WasmInstance, WasmModule};
use crate::{
    MidiEvent, MidiEvent2, ParallelBlockPlan, ParallelLayout, ParallelPlanEntry, ParameterEvent,
};

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

pub struct PortableModule {
    backend: ModuleBackend,
}

enum ModuleBackend {
    Wasm(WasmModule),
    Library(Arc<LibraryModule>),
}

impl PortableModule {
    pub(crate) fn from_wasm(module: WasmModule) -> Self {
        Self {
            backend: ModuleBackend::Wasm(module),
        }
    }

    pub(crate) fn into_wasm(self) -> Option<WasmModule> {
        match self.backend {
            ModuleBackend::Wasm(module) => Some(module),
            ModuleBackend::Library(_) => None,
        }
    }

    /// Loads the native build of a portable processor from `path`: a library
    /// exporting the SDK's portable entry.
    ///
    /// # Safety
    ///
    /// The library's code runs in this process, unsandboxed, from the moment
    /// it is loaded: the caller vouches for it.
    pub unsafe fn load_native_build(path: impl AsRef<Path>) -> Result<Self> {
        let module = unsafe { LibraryModule::open(path.as_ref()) }?;
        Ok(Self {
            backend: ModuleBackend::Library(Arc::new(module)),
        })
    }

    /// A native build linked into this process, by its portable entry: for
    /// platforms that load no libraries at run time, and for tests.
    ///
    /// # Safety
    ///
    /// As [`Self::load_native_build`]; `entry` is a portable entry the SDK
    /// exported.
    pub unsafe fn from_native_entry(
        entry: rackforge_plugin_sdk::portable::native::EntryFnV1,
    ) -> Result<Self> {
        let module = unsafe { LibraryModule::from_entry(entry, None) }?;
        Ok(Self {
            backend: ModuleBackend::Library(Arc::new(module)),
        })
    }

    /// Whether this module runs a native build rather than the component.
    pub fn is_native_build(&self) -> bool {
        matches!(self.backend, ModuleBackend::Library(_))
    }

    pub fn instantiate(&self) -> Result<PortableInstance> {
        let backend = match &self.backend {
            ModuleBackend::Wasm(module) => InstanceBackend::Wasm(module.instantiate()?),
            ModuleBackend::Library(module) => {
                InstanceBackend::Library(LibraryInstance::new(Arc::clone(module))?)
            }
        };
        Ok(PortableInstance { backend })
    }
}

pub struct PortableInstance {
    backend: InstanceBackend,
}

// The Wasmtime instance is the larger by far, and stays inline: a box would
// put a load in front of every block for an instance that lives for minutes.
#[allow(clippy::large_enum_variant)]
enum InstanceBackend {
    Wasm(WasmInstance),
    Library(LibraryInstance),
}

/// The same call on whichever backend runs the instance.
macro_rules! either {
    ($backend:expr, $instance:ident => $call:expr) => {
        match $backend {
            InstanceBackend::Wasm($instance) => $call,
            InstanceBackend::Library($instance) => $call,
        }
    };
}

impl PortableInstance {
    /// Whether this instance runs a native build rather than the component.
    pub fn is_native_build(&self) -> bool {
        matches!(self.backend, InstanceBackend::Library(_))
    }

    pub fn load_resource_file(&mut self, id: &str, path: impl AsRef<Path>) -> Result<()> {
        either!(&mut self.backend, instance => instance.load_resource_file(id, path))
    }

    pub fn prepare(
        &mut self,
        sample_rate: f64,
        maximum_frames: u32,
        input_channels: u32,
        output_channels: u32,
    ) -> Result<()> {
        either!(&mut self.backend, instance => instance.prepare(
            sample_rate,
            maximum_frames,
            input_channels,
            output_channels,
        ))
    }

    pub fn set_parameter(&mut self, index: u32, value: f64) -> Result<()> {
        either!(&mut self.backend, instance => instance.set_parameter(index, value))
    }

    pub fn get_parameter(&mut self, index: u32) -> Result<f64> {
        either!(&mut self.backend, instance => instance.get_parameter(index))
    }

    pub fn latency_frames(&mut self) -> Result<u32> {
        either!(&mut self.backend, instance => instance.latency_frames())
    }

    /// Whether this plugin accepts a real-time budget at all. A budget is
    /// fuel, which only the sandbox counts.
    pub fn accepts_realtime_budget(&self) -> bool {
        match &self.backend {
            InstanceBackend::Wasm(instance) => instance.accepts_realtime_budget(),
            InstanceBackend::Library(_) => false,
        }
    }

    pub fn set_realtime_budget(&mut self, fuel: u64) -> Result<bool> {
        match &mut self.backend {
            InstanceBackend::Wasm(instance) => instance.set_realtime_budget(fuel),
            InstanceBackend::Library(_) => Ok(false),
        }
    }

    pub fn reset(&mut self) -> Result<()> {
        either!(&mut self.backend, instance => instance.reset())
    }

    pub fn load_resource(&mut self, id: &str, bytes: &[u8]) -> Result<()> {
        either!(&mut self.backend, instance => instance.load_resource(id, bytes))
    }

    pub fn preset_catalog(&mut self) -> Result<Option<Vec<u8>>> {
        either!(&mut self.backend, instance => instance.preset_catalog())
    }

    pub fn load_preset(&mut self, id: &str) -> Result<()> {
        either!(&mut self.backend, instance => instance.load_preset(id))
    }

    pub fn save_state(&mut self) -> Result<Vec<u8>> {
        either!(&mut self.backend, instance => instance.save_state())
    }

    pub fn load_state(&mut self, state: &[u8]) -> Result<()> {
        either!(&mut self.backend, instance => instance.load_state(state))
    }

    pub fn supports_program_editing(&self) -> bool {
        self.program_editing_capabilities() != 0
    }

    pub fn program_editing_capabilities(&self) -> u32 {
        either!(&self.backend, instance => instance.program_editing_capabilities())
    }

    pub fn begin_program_edit(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        either!(&mut self.backend, instance => instance.begin_program_edit(request))
    }

    pub fn prepare_program_save(&mut self, document: &[u8]) -> Result<Vec<u8>> {
        either!(&mut self.backend, instance => instance.prepare_program_save(document))
    }

    pub fn install_program(&mut self, prepared: &[u8]) -> Result<()> {
        either!(&mut self.backend, instance => instance.install_program(prepared))
    }

    pub fn preview_program(&mut self, prepared: &[u8]) -> Result<bool> {
        either!(&mut self.backend, instance => instance.preview_program(prepared))
    }

    pub fn program_editor_view(&mut self, document: &[u8]) -> Result<Vec<u8>> {
        either!(&mut self.backend, instance => instance.program_editor_view(document))
    }

    pub fn apply_program_edit(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        either!(&mut self.backend, instance => instance.apply_program_edit(request))
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

    /// The `MIDI_FAMILY_*` bits the processor asked to receive wide; zero
    /// when it does not take the wide-MIDI contract.
    pub fn midi2_families(&self) -> u32 {
        either!(&self.backend, instance => instance.midi2_families())
    }

    /// Runs one block with `midi2` delivered at MIDI 2.0 widths: through the
    /// wide entry on every block when the processor has one, through the
    /// narrow one, with an empty `midi2` only, when it does not.
    pub fn process_interleaved_with_midi2(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
        midi: &[MidiEvent],
        parameters: &[ParameterEvent],
        midi2: &[MidiEvent2],
    ) -> Result<()> {
        either!(&mut self.backend, instance => instance.process_interleaved_with_midi2(
            input, output, frames, midi, parameters, midi2,
        ))
    }

    /// The parallel-render geometry when the processor renders in units,
    /// `None` for a classic single-unit one.
    pub fn parallel_layout(&self) -> Option<ParallelLayout> {
        either!(&self.backend, instance => instance.parallel_layout())
    }

    pub fn parallel_begin_block(
        &mut self,
        input: &[f32],
        frames: u32,
        midi: &[MidiEvent],
        parameters: &[ParameterEvent],
        midi2: &[MidiEvent2],
        plan: &mut [ParallelPlanEntry],
    ) -> Result<ParallelBlockPlan> {
        either!(&mut self.backend, instance => instance.parallel_begin_block(
            input, frames, midi, parameters, midi2, plan,
        ))
    }

    pub fn parallel_read_shared(&self, shared: &mut [u8]) -> Result<()> {
        either!(&self.backend, instance => instance.parallel_read_shared(shared))
    }

    pub fn parallel_write_shared(&mut self, shared: &[u8]) -> Result<()> {
        either!(&mut self.backend, instance => instance.parallel_write_shared(shared))
    }

    pub fn parallel_read_dispatch(&self, unit: u32, payload: &mut [u8]) -> Result<()> {
        either!(&self.backend, instance => instance.parallel_read_dispatch(unit, payload))
    }

    pub fn parallel_write_dispatch(&mut self, unit: u32, payload: &[u8]) -> Result<()> {
        either!(&mut self.backend, instance => instance.parallel_write_dispatch(unit, payload))
    }

    pub fn parallel_render_unit(
        &mut self,
        unit: u32,
        payload_bytes: usize,
        shared_bytes: usize,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
    ) -> Result<()> {
        either!(&mut self.backend, instance => instance.parallel_render_unit(
            unit, payload_bytes, shared_bytes, input, output, frames,
        ))
    }

    pub fn parallel_read_report(&self, unit: u32, report: &mut [u8]) -> Result<()> {
        either!(&self.backend, instance => instance.parallel_read_report(unit, report))
    }

    pub fn parallel_write_report(&mut self, unit: u32, report: &[u8]) -> Result<()> {
        either!(&mut self.backend, instance => instance.parallel_write_report(unit, report))
    }

    pub fn parallel_write_mix_slot(&mut self, unit: u32, samples: &[f32]) -> Result<()> {
        either!(&mut self.backend, instance => instance.parallel_write_mix_slot(unit, samples))
    }

    pub fn parallel_end_block(&mut self, output: &mut [f32], frames: u32) -> Result<()> {
        either!(&mut self.backend, instance => instance.parallel_end_block(output, frames))
    }

    /// Whether this plugin is metered at all: never, for a native build.
    pub fn is_metered(&self) -> bool {
        match &self.backend {
            InstanceBackend::Wasm(instance) => instance.is_metered(),
            InstanceBackend::Library(_) => false,
        }
    }

    pub fn last_realtime_fuel_consumed(&self) -> u64 {
        match &self.backend {
            InstanceBackend::Wasm(instance) => instance.last_realtime_fuel_consumed(),
            InstanceBackend::Library(_) => 0,
        }
    }

    /// Adds fuel burned outside this instance to what it reports for the
    /// block (see the Wasmtime backend); nothing to add to a native build.
    pub fn add_realtime_fuel(&mut self, fuel: u64) {
        if let InstanceBackend::Wasm(instance) = &mut self.backend {
            instance.add_realtime_fuel(fuel);
        }
    }
}
