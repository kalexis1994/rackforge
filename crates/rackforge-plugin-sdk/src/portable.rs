//! The portable contract's operations, written once.
//!
//! [`export_processor!`](crate::export_processor) exports a processor twice
//! from the same source: as the `wasm-v1` component every RackForge host runs,
//! and, built for a native target, as the same contract behind a table of
//! functions a host may load in its place ([`native`]). Both call these
//! operations, so an argument a component refuses, a status it returns and
//! the events it decodes are the same on either side, bit for bit: what
//! differs is only where the buffers live -- a component's linear memory, one
//! of each per instance, or a native instance's own allocation.

use crate::{
    MidiEvent, MidiEvent2, PROGRAM_EDIT_BASIC, PROGRAM_EDIT_KNOWN_CAPABILITIES, ParameterEvent,
    Processor, STATUS_INVALID_ARGUMENT, STATUS_INVALID_STATE, STATUS_OK, STATUS_UNKNOWN_PARAMETER,
};
use core::mem::MaybeUninit;

/// The largest block, and the channel counts, an export was declared with.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_frames: usize,
    pub max_input_channels: usize,
    pub max_output_channels: usize,
}

/// One processor and what the contract knows of it: whether it has been
/// made, and whether its last `prepare` succeeded.
#[doc(hidden)]
pub struct Slot<P> {
    processor: MaybeUninit<P>,
    initialized: bool,
    prepared: bool,
}

impl<P> Default for Slot<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P> Slot<P> {
    pub const fn new() -> Self {
        Self {
            processor: MaybeUninit::uninit(),
            initialized: false,
            prepared: false,
        }
    }

    /// Writes an empty slot at `slot` without building one elsewhere first:
    /// a processor can be megabytes, more than a native host thread's stack.
    ///
    /// # Safety
    ///
    /// `slot` is valid for writes and holds no slot to drop.
    pub unsafe fn write_empty(slot: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*slot).initialized).write(false);
            core::ptr::addr_of_mut!((*slot).prepared).write(false);
        }
    }
}

impl<P> Drop for Slot<P> {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: written exactly once by `initialize`, and only then is
            // `initialized` set.
            unsafe { self.processor.assume_init_drop() };
        }
    }
}

/// A transfer length the contract accepts: within the buffer, never negative.
fn within(length: i32, capacity: usize) -> Option<usize> {
    usize::try_from(length)
        .ok()
        .filter(|length| *length <= capacity)
}

impl<P> Slot<P> {
    /// Whether the last `prepare` succeeded.
    pub fn is_prepared(&self) -> bool {
        self.prepared
    }

    /// The processor, once made: for the parallel export's own entries.
    pub fn processor_mut(&mut self) -> Option<&mut P> {
        // SAFETY: initialized exactly when `initialized` is set.
        self.initialized
            .then(|| unsafe { self.processor.assume_init_mut() })
    }
}

impl<P: Processor> Slot<P> {
    fn processor(&mut self) -> Option<&mut P> {
        // SAFETY: initialized exactly when `initialized` is set.
        self.initialized
            .then(|| unsafe { self.processor.assume_init_mut() })
    }

    fn processor_ref(&self) -> Option<&P> {
        // SAFETY: as above.
        self.initialized
            .then(|| unsafe { self.processor.assume_init_ref() })
    }

    /// Makes the processor once; later calls change nothing.
    pub fn initialize(&mut self) -> i32 {
        if !self.initialized {
            self.processor.write(P::default());
            self.initialized = true;
        }
        STATUS_OK
    }

    pub fn prepare(
        &mut self,
        limits: &Limits,
        sample_rate: f64,
        maximum_frames: i32,
        input_channels: i32,
        output_channels: i32,
    ) -> i32 {
        if !sample_rate.is_finite()
            || sample_rate <= 0.0
            || maximum_frames <= 0
            || input_channels < 0
            || output_channels < 0
            || maximum_frames as usize > limits.max_frames
            || input_channels as usize > limits.max_input_channels
            || output_channels as usize > limits.max_output_channels
        {
            return STATUS_INVALID_ARGUMENT;
        }
        if self.initialize() != STATUS_OK {
            return STATUS_INVALID_STATE;
        }
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        let prepared = processor.prepare(
            sample_rate,
            maximum_frames as u32,
            input_channels as u32,
            output_channels as u32,
        );
        self.prepared = prepared;
        if prepared {
            STATUS_OK
        } else {
            STATUS_INVALID_STATE
        }
    }

    pub fn set_parameter(&mut self, index: i32, value: f64) -> i32 {
        if index < 0 || !value.is_finite() {
            return STATUS_INVALID_ARGUMENT;
        }
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        if processor.set_parameter(index as u32, value) {
            STATUS_OK
        } else {
            STATUS_UNKNOWN_PARAMETER
        }
    }

    pub fn get_parameter(&self, index: i32) -> f64 {
        if index < 0 {
            return f64::NAN;
        }
        self.processor_ref()
            .and_then(|processor| processor.get_parameter(index as u32))
            .unwrap_or(f64::NAN)
    }

    pub fn set_realtime_budget(&mut self, fuel_per_call: i64) -> i32 {
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        if fuel_per_call < 0 {
            return STATUS_INVALID_ARGUMENT;
        }
        // 1 taken, 0 declined. Every processor exports this; only the ones
        // that answer 1 are ever asked again.
        i32::from(processor.set_realtime_budget(fuel_per_call as u64))
    }

    pub fn latency_frames(&self) -> i32 {
        let Some(processor) = self.processor_ref() else {
            return STATUS_INVALID_STATE;
        };
        i32::try_from(processor.latency_frames()).unwrap_or(STATUS_INVALID_STATE)
    }

    pub fn reset(&mut self) -> i32 {
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        processor.reset();
        STATUS_OK
    }

    pub fn resource_begin(&mut self, transfer: &[u8], id_length: i32, total_bytes: i64) -> i32 {
        if id_length <= 0 || id_length as usize > transfer.len() || total_bytes < 0 {
            return STATUS_INVALID_ARGUMENT;
        }
        if self.initialize() != STATUS_OK {
            return STATUS_INVALID_STATE;
        }
        let Ok(id) = core::str::from_utf8(&transfer[..id_length as usize]) else {
            return STATUS_INVALID_ARGUMENT;
        };
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        if processor.begin_resource(id, total_bytes as u64) {
            STATUS_OK
        } else {
            STATUS_INVALID_STATE
        }
    }

    pub fn resource_write(&mut self, transfer: &[u8], offset: i64, length: i32) -> i32 {
        if offset < 0 {
            return STATUS_INVALID_ARGUMENT;
        }
        let Some(length) = within(length, transfer.len()) else {
            return STATUS_INVALID_ARGUMENT;
        };
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        if processor.write_resource(offset as u64, &transfer[..length]) {
            STATUS_OK
        } else {
            STATUS_INVALID_STATE
        }
    }

    pub fn resource_end(&mut self) -> i32 {
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        if processor.end_resource() {
            STATUS_OK
        } else {
            STATUS_INVALID_STATE
        }
    }

    /// The instance's program catalog into `transfer`: its length, 0 to keep
    /// the package's static catalog, or a status.
    pub fn preset_catalog(&mut self, transfer: &mut [u8]) -> i32 {
        let capacity = transfer.len();
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        match processor.write_program_catalog(transfer) {
            Some(length) if length > 0 && length <= capacity => length as i32,
            Some(_) => STATUS_INVALID_STATE,
            None => 0,
        }
    }

    pub fn load_preset(&mut self, transfer: &[u8], length: i32) -> i32 {
        if length <= 0 {
            return STATUS_INVALID_ARGUMENT;
        }
        let Some(length) = within(length, transfer.len()) else {
            return STATUS_INVALID_ARGUMENT;
        };
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        let Ok(id) = core::str::from_utf8(&transfer[..length]) else {
            return STATUS_INVALID_ARGUMENT;
        };
        if processor.load_preset(id) {
            STATUS_OK
        } else {
            STATUS_INVALID_STATE
        }
    }

    pub fn save_state(&self, transfer: &mut [u8]) -> i32 {
        let capacity = transfer.len();
        let Some(processor) = self.processor_ref() else {
            return STATUS_INVALID_STATE;
        };
        match processor.save_state(transfer) {
            Some(length) if length <= capacity => length as i32,
            _ => STATUS_INVALID_STATE,
        }
    }

    pub fn load_state(&mut self, transfer: &[u8], length: i32) -> i32 {
        let Some(length) = within(length, transfer.len()) else {
            return STATUS_INVALID_ARGUMENT;
        };
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        if processor.load_state(&transfer[..length]) {
            STATUS_OK
        } else {
            STATUS_INVALID_STATE
        }
    }

    pub fn program_editing_capabilities(&mut self) -> i32 {
        if self.initialize() != STATUS_OK {
            return STATUS_INVALID_STATE;
        }
        let Some(processor) = self.processor_ref() else {
            return STATUS_INVALID_STATE;
        };
        let capabilities = processor.program_editing_capabilities();
        if capabilities & !PROGRAM_EDIT_KNOWN_CAPABILITIES != 0
            || capabilities != 0 && capabilities & PROGRAM_EDIT_BASIC == 0
            || capabilities > i32::MAX as u32
        {
            STATUS_INVALID_STATE
        } else {
            capabilities as i32
        }
    }

    /// A program document in `exchange`, its answer into `transfer`: 0 begins
    /// an edit, 1 prepares a save, 2 builds the editor's view, 3 applies an
    /// edit.
    pub fn program_exchange(
        &mut self,
        operation: u8,
        exchange: &[u8],
        transfer: &mut [u8],
        source_length: i32,
    ) -> i32 {
        let capacity = transfer.len();
        let Some(source_length) = within(source_length, capacity.min(exchange.len())) else {
            return STATUS_INVALID_ARGUMENT;
        };
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        let source = &exchange[..source_length];
        let result = match operation {
            0 => processor.begin_program_edit(source, transfer),
            1 => processor.prepare_program_save(source, transfer),
            2 => processor.program_editor_view(source, transfer),
            3 => processor.apply_program_edit(source, transfer),
            _ => return STATUS_INVALID_ARGUMENT,
        };
        match result {
            Some(length) if length <= capacity => length as i32,
            _ => STATUS_INVALID_STATE,
        }
    }

    /// A prepared program in `exchange`: 0 installs it, 1 previews it.
    pub fn program_install_operation(
        &mut self,
        operation: u8,
        exchange: &[u8],
        transfer_capacity: usize,
        source_length: i32,
    ) -> i32 {
        let Some(source_length) = within(source_length, transfer_capacity.min(exchange.len()))
        else {
            return STATUS_INVALID_ARGUMENT;
        };
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        let source = &exchange[..source_length];
        let accepted = match operation {
            0 => processor.install_program(source),
            1 => processor.preview_program(source),
            _ => return STATUS_INVALID_ARGUMENT,
        };
        if accepted {
            STATUS_OK
        } else {
            STATUS_INVALID_STATE
        }
    }

    /// One block: `input` and `output` are the whole regions, `midi` the
    /// packed events and `parameters` the automation, as the host wrote them.
    /// `MAX_MIDI` is the region's capacity in events.
    #[allow(clippy::too_many_arguments)]
    pub fn process<const MAX_MIDI: usize>(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        midi: &[u64],
        parameters: &[ParameterEvent],
        frames: i32,
        input_channels: i32,
        output_channels: i32,
        midi_event_count: i32,
        parameter_event_count: i32,
    ) -> i32 {
        let Some(block) = Block::new::<MAX_MIDI>(
            input.len(),
            output.len(),
            midi.len(),
            parameters.len(),
            frames,
            input_channels,
            output_channels,
            midi_event_count,
            parameter_event_count,
        ) else {
            return STATUS_INVALID_ARGUMENT;
        };
        if !self.prepared {
            return STATUS_INVALID_STATE;
        }
        let mut events = [MidiEvent {
            frame: 0,
            data: [0; 3],
            length: 1,
        }; MAX_MIDI];
        let Some(parameters) = block.decode(midi, parameters, &mut events) else {
            return STATUS_INVALID_ARGUMENT;
        };
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        processor.process(
            &input[..block.input_samples],
            &mut output[..block.output_samples],
            &events[..block.midi_events],
            parameters,
            block.frames,
            block.input_channels,
            block.output_channels,
        );
        STATUS_OK
    }

    /// [`Self::process`] with the wide events, two packed words each, in
    /// `midi2`; `MAX_MIDI2` is that region's capacity in events.
    #[allow(clippy::too_many_arguments)]
    pub fn process_v2<const MAX_MIDI: usize, const MAX_MIDI2: usize>(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        midi: &[u64],
        parameters: &[ParameterEvent],
        midi2: &[u64],
        frames: i32,
        input_channels: i32,
        output_channels: i32,
        midi_event_count: i32,
        parameter_event_count: i32,
        midi2_event_count: i32,
    ) -> i32 {
        let Some(block) = Block::new::<MAX_MIDI>(
            input.len(),
            output.len(),
            midi.len(),
            parameters.len(),
            frames,
            input_channels,
            output_channels,
            midi_event_count,
            parameter_event_count,
        ) else {
            return STATUS_INVALID_ARGUMENT;
        };
        let Some(midi2_events) = usize::try_from(midi2_event_count)
            .ok()
            .filter(|count| *count <= MAX_MIDI2 && 2 * count <= midi2.len())
        else {
            return STATUS_INVALID_ARGUMENT;
        };
        if !self.prepared {
            return STATUS_INVALID_STATE;
        }
        let mut events = [MidiEvent {
            frame: 0,
            data: [0; 3],
            length: 1,
        }; MAX_MIDI];
        let Some(parameters) = block.decode(midi, parameters, &mut events) else {
            return STATUS_INVALID_ARGUMENT;
        };
        let mut events2 = [MidiEvent2 {
            frame: 0,
            kind: 0,
            channel: 0,
            index: 0,
            flags: 0,
            value: 0,
            extra: 0,
        }; MAX_MIDI2];
        for (destination, packed) in events2
            .iter_mut()
            .zip(midi2[..2 * midi2_events].as_chunks::<2>().0)
        {
            *destination = MidiEvent2::from_packed(packed[0], packed[1]);
            if destination.frame >= block.frames {
                return STATUS_INVALID_ARGUMENT;
            }
        }
        let Some(processor) = self.processor() else {
            return STATUS_INVALID_STATE;
        };
        processor.process_wide(
            &input[..block.input_samples],
            &mut output[..block.output_samples],
            &events[..block.midi_events],
            &events2[..midi2_events],
            parameters,
            block.frames,
            block.input_channels,
            block.output_channels,
        );
        STATUS_OK
    }
}

/// One block's shape, checked against the regions it reads and writes.
struct Block {
    frames: u32,
    input_channels: u32,
    output_channels: u32,
    input_samples: usize,
    output_samples: usize,
    midi_events: usize,
    parameter_events: usize,
}

impl Block {
    #[allow(clippy::too_many_arguments)]
    fn new<const MAX_MIDI: usize>(
        input_capacity: usize,
        output_capacity: usize,
        midi_capacity: usize,
        parameter_capacity: usize,
        frames: i32,
        input_channels: i32,
        output_channels: i32,
        midi_event_count: i32,
        parameter_event_count: i32,
    ) -> Option<Self> {
        if frames <= 0
            || input_channels < 0
            || output_channels < 0
            || midi_event_count < 0
            || parameter_event_count < 0
        {
            return None;
        }
        let input_samples = (frames as usize).checked_mul(input_channels as usize)?;
        let output_samples = (frames as usize).checked_mul(output_channels as usize)?;
        let midi_events = midi_event_count as usize;
        let parameter_events = parameter_event_count as usize;
        if input_samples > input_capacity
            || output_samples > output_capacity
            || midi_events > MAX_MIDI.min(midi_capacity)
            || parameter_events > parameter_capacity
        {
            return None;
        }
        Some(Self {
            frames: frames as u32,
            input_channels: input_channels as u32,
            output_channels: output_channels as u32,
            input_samples,
            output_samples,
            midi_events,
            parameter_events,
        })
    }

    /// Unpacks the narrow events into `events` and checks every event and
    /// every automation point lies inside the block.
    fn decode<'a>(
        &self,
        midi: &[u64],
        parameters: &'a [ParameterEvent],
        events: &mut [MidiEvent],
    ) -> Option<&'a [ParameterEvent]> {
        for (destination, packed) in events.iter_mut().zip(&midi[..self.midi_events]) {
            *destination = MidiEvent::from_packed(*packed);
            if destination.frame >= self.frames || destination.length == 0 || destination.length > 3
            {
                return None;
            }
        }
        let parameters = &parameters[..self.parameter_events];
        if parameters
            .iter()
            .any(|event| event.frame >= self.frames || !event.value.is_finite())
        {
            return None;
        }
        Some(parameters)
    }
}

/// The portable contract behind a table of functions, for a host that runs a
/// plugin's native build in place of its component (`binaries` beside
/// `component` in the manifest).
///
/// One table per library, found at [`ENTRY_SYMBOL_V1`]. Every instance is
/// made by `create` and owns its processor and its regions, which a component
/// keeps in its own linear memory; every other entry is the `wasm-v1` export
/// of the same name, taking the instance first, with the same arguments, the
/// same results and the same statuses. A host drives either the same way.
pub mod native {
    use core::ffi::c_void;

    /// The symbol a native build exports, returning `*const NativeApiV1`.
    pub const ENTRY_SYMBOL_V1: &[u8] = b"rackforge_portable_native_entry_v1\0";
    /// The table's own version: the portable contract's is
    /// [`crate::ABI_VERSION_V1`], reported beside it.
    pub const NATIVE_ABI_VERSION_V1: u32 = 0x0001_0000;

    /// The regions a host writes and reads, by number: a new region is a new
    /// number, not a new field, so the table keeps its shape.
    pub mod region {
        /// Interleaved `f32` audio in.
        pub const INPUT: u32 = 0;
        /// Interleaved `f32` audio out.
        pub const OUTPUT: u32 = 1;
        /// Packed narrow MIDI events, one `u64` each.
        pub const MIDI: u32 = 2;
        /// `ParameterEvent`s.
        pub const PARAMETERS: u32 = 3;
        /// Bytes: resource chunks, preset ids, state, a program's answers.
        pub const TRANSFER: u32 = 4;
        /// Bytes: a program document going in.
        pub const EXCHANGE_INPUT: u32 = 5;
        /// Packed wide MIDI events, two `u64` each; absent without `midi2`.
        pub const MIDI2: u32 = 6;
        /// Bytes: each unit's dispatch payload, at the dispatch stride.
        /// This and the four below exist only in a parallel build.
        pub const DISPATCH: u32 = 7;
        /// `u32`s: the plan header (shared bytes, reserved), then each
        /// planned unit's index and payload length.
        pub const PLAN: u32 = 8;
        /// `f32`s: each unit's mix slot, `mix_slot_samples` apart.
        pub const MIX: u32 = 9;
        /// Bytes: the block-shared payload.
        pub const SHARED: u32 = 10;
        /// Bytes: each unit's report, at the report stride; absent when the
        /// plugin reports nothing.
        pub const REPORTS: u32 = 11;
    }

    pub type CreateFn = unsafe extern "C" fn() -> *mut c_void;
    pub type DestroyFn = unsafe extern "C" fn(instance: *mut c_void);
    /// A region's address, its capacity in elements written to `capacity`;
    /// null, with capacity 0, for a region this build does not have.
    pub type RegionFn =
        unsafe extern "C" fn(instance: *mut c_void, region: u32, capacity: *mut usize) -> *mut u8;
    pub type CallFn = unsafe extern "C" fn(instance: *mut c_void) -> i32;
    pub type LengthFn = unsafe extern "C" fn(instance: *mut c_void, length: i32) -> i32;
    pub type PrepareFn = unsafe extern "C" fn(
        instance: *mut c_void,
        sample_rate: f64,
        maximum_frames: i32,
        input_channels: i32,
        output_channels: i32,
    ) -> i32;
    pub type SetParameterFn =
        unsafe extern "C" fn(instance: *mut c_void, index: i32, value: f64) -> i32;
    pub type GetParameterFn = unsafe extern "C" fn(instance: *mut c_void, index: i32) -> f64;
    pub type BudgetFn = unsafe extern "C" fn(instance: *mut c_void, fuel_per_call: i64) -> i32;
    pub type ResourceBeginFn =
        unsafe extern "C" fn(instance: *mut c_void, id_length: i32, total_bytes: i64) -> i32;
    pub type ResourceWriteFn =
        unsafe extern "C" fn(instance: *mut c_void, offset: i64, length: i32) -> i32;
    pub type ProcessFn = unsafe extern "C" fn(
        instance: *mut c_void,
        frames: i32,
        input_channels: i32,
        output_channels: i32,
        midi_event_count: i32,
        parameter_event_count: i32,
    ) -> i32;
    pub type ProcessV2Fn = unsafe extern "C" fn(
        instance: *mut c_void,
        frames: i32,
        input_channels: i32,
        output_channels: i32,
        midi_event_count: i32,
        parameter_event_count: i32,
        midi2_event_count: i32,
    ) -> i32;

    /// A unit's render: `rackforge_parallel_render_unit`'s arguments.
    pub type RenderUnitFn = unsafe extern "C" fn(
        instance: *mut c_void,
        unit: i32,
        payload_bytes: i32,
        shared_bytes: i32,
        frames: i32,
        output_channels: i32,
    ) -> i32;
    /// The post-stage: `rackforge_parallel_end_block`'s arguments.
    pub type EndBlockFn =
        unsafe extern "C" fn(instance: *mut c_void, frames: i32, output_channels: i32) -> i32;

    /// The parallel-render extension of a build whose processor renders in
    /// units ([`crate::ParallelProcessor`]): the component's
    /// `rackforge_parallel_*` exports, each taking the instance first, and
    /// the geometry its constant exports report. The pre-stages return the
    /// planned unit count, or a status.
    #[repr(C)]
    pub struct NativeParallelApiV1 {
        pub struct_size: u32,
        pub parallel_abi_version: u32,
        pub max_units: u32,
        pub dispatch_stride: u32,
        /// Floats a unit writes per frame.
        pub unit_channels: u32,
        pub report_stride: u32,
        pub shared_capacity: u32,
        /// The distance between two units' slots in the mix region.
        pub mix_slot_samples: u32,
        pub begin_block: ProcessFn,
        /// Present exactly when the table's `process_v2` is.
        pub begin_block_v2: Option<ProcessV2Fn>,
        pub render_unit: RenderUnitFn,
        pub end_block: EndBlockFn,
    }

    /// The table. Fields are only ever appended; `struct_size` says how many a
    /// build has.
    #[repr(C)]
    pub struct NativeApiV1 {
        pub struct_size: u32,
        pub native_abi_version: u32,
        /// The portable contract's version, `rackforge_abi_version` in a
        /// component.
        pub portable_abi_version: u32,
        /// The wide-MIDI families, `rackforge_midi2_families`; 0, with no
        /// `process_v2`, for a processor that takes none.
        pub midi2_families: u32,
        pub create: CreateFn,
        pub destroy: DestroyFn,
        pub region: RegionFn,
        pub initialize: CallFn,
        pub prepare: PrepareFn,
        pub set_parameter: SetParameterFn,
        pub get_parameter: GetParameterFn,
        pub set_realtime_budget: BudgetFn,
        pub latency_frames: CallFn,
        pub reset: CallFn,
        pub resource_begin: ResourceBeginFn,
        pub resource_write: ResourceWriteFn,
        pub resource_end: CallFn,
        pub preset_catalog: CallFn,
        pub load_preset: LengthFn,
        pub save_state: CallFn,
        pub load_state: LengthFn,
        pub program_editing_capabilities: CallFn,
        pub program_begin_edit: LengthFn,
        pub program_prepare_save: LengthFn,
        pub program_install: LengthFn,
        pub program_preview: LengthFn,
        pub program_editor_view: LengthFn,
        pub program_apply_edit: LengthFn,
        pub process: ProcessFn,
        pub process_v2: Option<ProcessV2Fn>,
        /// The parallel-render extension, for a processor that renders in
        /// units; absent for every other.
        pub parallel: Option<&'static NativeParallelApiV1>,
    }

    pub type EntryFnV1 = unsafe extern "C" fn() -> *const NativeApiV1;
}
