#![no_std]

//! Guest-side contract for `wasm-v1` RackForge processors.
//!
//! Plugin authors implement [`Processor`] and export it with
//! [`export_processor!`]. The SDK owns the raw WebAssembly ABI and its linear
//! memory buffers so plugin code does not handle pointers or host platforms.

pub const ABI_VERSION_V1_1: u32 = 0x0001_0001;
pub const ABI_VERSION_V1: u32 = 0x0001_0002;
pub const STATUS_OK: i32 = 0;
pub const STATUS_INVALID_ARGUMENT: i32 = -1;
pub const STATUS_UNKNOWN_PARAMETER: i32 = -2;
pub const STATUS_INVALID_STATE: i32 = -3;

/// The guest can begin, prepare and install individual program documents.
pub const PROGRAM_EDIT_BASIC: u32 = 1 << 0;
/// The guest can preview a prepared document without persisting it.
pub const PROGRAM_EDIT_PREVIEW: u32 = 1 << 1;
/// The guest exposes a declarative editor view and typed field mutations.
pub const PROGRAM_EDIT_DECLARATIVE: u32 = 1 << 2;
pub const PROGRAM_EDIT_KNOWN_CAPABILITIES: u32 =
    PROGRAM_EDIT_BASIC | PROGRAM_EDIT_PREVIEW | PROGRAM_EDIT_DECLARATIVE;

/// One MIDI 1.0 channel/system-common message delivered at an exact sample
/// offset inside the current audio block. SysEx uses the control plane and is
/// intentionally excluded from the real-time ABI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MidiEvent {
    pub frame: u32,
    pub data: [u8; 3],
    pub length: u8,
}

impl MidiEvent {
    pub const fn new(frame: u32, data: [u8; 3], length: u8) -> Option<Self> {
        if length == 0 || length > 3 {
            return None;
        }
        Some(Self {
            frame,
            data,
            length,
        })
    }

    #[doc(hidden)]
    pub const fn from_packed(value: u64) -> Self {
        Self {
            frame: value as u32,
            data: [
                (value >> 32) as u8,
                (value >> 40) as u8,
                (value >> 48) as u8,
            ],
            length: (value >> 56) as u8,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParameterEvent {
    pub frame: u32,
    pub index: u32,
    pub value: f64,
}

pub trait Processor: Default {
    fn prepare(
        &mut self,
        _sample_rate: f64,
        _maximum_frames: u32,
        _input_channels: u32,
        _output_channels: u32,
    ) -> bool {
        true
    }

    fn set_parameter(&mut self, index: u32, value: f64) -> bool;

    fn get_parameter(&self, _index: u32) -> Option<f64> {
        None
    }

    fn reset(&mut self) {}

    /// Starts delivery of one manifest-declared resource on the control thread.
    fn begin_resource(&mut self, _id: &str, _total_bytes: u64) -> bool {
        false
    }

    fn write_resource(&mut self, _offset: u64, _bytes: &[u8]) -> bool {
        false
    }

    fn end_resource(&mut self) -> bool {
        false
    }

    /// Writes an instance-specific preset catalog as RackForge Preset Catalog
    /// JSON after resources have been delivered. Returning `None` keeps the
    /// package's static catalog. This runs on the control thread.
    /// Publishes this instance's selectable PROGRAMS.
    ///
    /// Every plugin package must also ship a non-empty static program catalog.
    /// The historical ABI calls that document a `preset_catalog`; RackForge
    /// keeps that transport name compatible but presents these entries as
    /// PROGRAMS. Portable RackForge `.rfpreset` files are host-owned and are
    /// not returned here.
    fn write_program_catalog(&mut self, destination: &mut [u8]) -> Option<usize> {
        self.write_preset_catalog(destination)
    }

    /// Legacy spelling retained for source compatibility. New plugins should
    /// implement [`Self::write_program_catalog`].
    fn write_preset_catalog(&mut self, _destination: &mut [u8]) -> Option<usize> {
        None
    }

    fn load_preset(&mut self, _id: &str) -> bool {
        false
    }

    fn save_state(&self, _destination: &mut [u8]) -> Option<usize> {
        None
    }

    fn load_state(&mut self, _state: &[u8]) -> bool {
        false
    }

    /// Optional individual-program editing contract. Payloads use the same
    /// bounded JSON envelopes as native RackForge plugins, but the SDK keeps
    /// them as bytes so portable guests do not need a particular serializer.
    fn program_editing_capabilities(&self) -> u32 {
        0
    }

    fn begin_program_edit(&mut self, _request: &[u8], _destination: &mut [u8]) -> Option<usize> {
        None
    }

    fn prepare_program_save(&mut self, _document: &[u8], _destination: &mut [u8]) -> Option<usize> {
        None
    }

    fn install_program(&mut self, _prepared: &[u8]) -> bool {
        false
    }

    fn preview_program(&mut self, _prepared: &[u8]) -> bool {
        false
    }

    fn program_editor_view(&mut self, _document: &[u8], _destination: &mut [u8]) -> Option<usize> {
        None
    }

    fn apply_program_edit(&mut self, _request: &[u8], _destination: &mut [u8]) -> Option<usize> {
        None
    }

    // Keep the v1 source contract stable for existing portable plugins. Grouping
    // these arguments would be cleaner, but would force every plugin to migrate
    // even though the exported ABI is unchanged.
    #[allow(clippy::too_many_arguments)]
    fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        midi: &[MidiEvent],
        parameters: &[ParameterEvent],
        frames: u32,
        input_channels: u32,
        output_channels: u32,
    );
}

/// Version this SDK writes into `rackforge_parallel_abi_version`.
pub const PARALLEL_ABI_VERSION_V1: u32 = 0x0001_0000;

/// Everything the coordinator sees while preparing one block: audio input,
/// sample-positioned MIDI, sample-accurate automation and the block shape.
pub struct BlockContext<'a> {
    pub input: &'a [f32],
    pub midi: &'a [MidiEvent],
    pub parameters: &'a [ParameterEvent],
    pub frames: u32,
    pub input_channels: u32,
    pub output_channels: u32,
}

/// Collects the block plan inside `begin_block`: which units render this
/// block and the dispatch payload each receives. Units must be activated in
/// ascending order, which is also the deterministic combine order.
pub struct PlanWriter<'a> {
    plan: &'a mut [u32],
    dispatch: &'a mut [u8],
    shared: &'a mut [u8],
    shared_len: usize,
    stride: usize,
    max_units: usize,
    count: usize,
    last_unit: Option<u32>,
}

impl<'a> PlanWriter<'a> {
    #[doc(hidden)]
    pub fn new(
        plan: &'a mut [u32],
        dispatch: &'a mut [u8],
        shared: &'a mut [u8],
        stride: usize,
        max_units: usize,
    ) -> Self {
        Self {
            plan,
            dispatch,
            shared,
            shared_len: 0,
            stride,
            max_units,
            count: 0,
            last_unit: None,
        }
    }

    /// The block-shared payload buffer: one immutable byte region every
    /// unit receives alongside its own dispatch payload. Write the block's
    /// shared signals here — per-frame LFO or noise arrays, wheel and bend
    /// curves, automation segments — then call [`Self::commit_shared`].
    pub fn shared_buffer(&mut self) -> &mut [u8] {
        self.shared
    }

    /// Declares how many bytes of [`Self::shared_buffer`] this block uses.
    pub fn commit_shared(&mut self, length: usize) -> bool {
        if length > self.shared.len() {
            return false;
        }
        self.shared_len = length;
        true
    }

    pub fn shared_len(&self) -> usize {
        self.shared_len
    }

    /// Schedules `unit` for this block with its dispatch payload. Returns
    /// `false` (and schedules nothing) when the unit is out of range, out of
    /// ascending order, or the payload exceeds the declared stride.
    pub fn activate(&mut self, unit: u32, payload: &[u8]) -> bool {
        if unit as usize >= self.max_units
            || payload.len() > self.stride
            || self.count >= self.max_units
        {
            return false;
        }
        if self.last_unit.is_some_and(|previous| unit <= previous) {
            return false;
        }
        self.plan[self.count * 2] = unit;
        self.plan[self.count * 2 + 1] = payload.len() as u32;
        self.dispatch[unit as usize * self.stride..][..payload.len()].copy_from_slice(payload);
        self.count += 1;
        self.last_unit = Some(unit);
        true
    }

    pub fn activated(&self) -> usize {
        self.count
    }
}

/// The block shape a unit renders against, including the immutable
/// block-shared payload the coordinator committed for every unit.
pub struct UnitContext<'a> {
    pub input: &'a [f32],
    pub shared: &'a [u8],
    pub frames: u32,
    pub output_channels: u32,
}

/// Deterministic access to the finished unit blocks inside `end_block`.
/// Slots are addressed by unit index, never by completion order, and a unit
/// the host had to silence reads as zeros.
pub struct UnitMix<'a> {
    mix: &'a [f32],
    slot_samples: usize,
    plan: &'a [u32],
    count: usize,
    samples: usize,
}

impl<'a> UnitMix<'a> {
    #[doc(hidden)]
    pub fn new(
        mix: &'a [f32],
        slot_samples: usize,
        plan: &'a [u32],
        count: usize,
        samples: usize,
    ) -> Self {
        Self {
            mix,
            slot_samples,
            plan,
            count,
            samples,
        }
    }

    /// Units activated by this block's `begin_block`, in ascending order.
    pub fn active_units(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.count).map(|index| self.plan[index * 2])
    }

    /// This block's samples for one unit slot.
    pub fn slot(&self, unit: u32) -> &[f32] {
        &self.mix[unit as usize * self.slot_samples..][..self.samples]
    }
}

/// A processor whose block is split into a serial pre-stage, independent
/// units the host may render on any of its own threads, and a serial
/// post-stage. Export it with [`export_parallel_processor!`], which also
/// derives the classic `rackforge_process` as `begin_block` → every planned
/// unit in ascending order → `end_block`, so hosts without unit scheduling
/// (single core, browsers) produce identical audio from the same component.
///
/// The division of state is the whole contract:
///
/// * `Self` is *coordinator* state — MIDI parsing, voice allocation, LFOs,
///   noise seeds, global effects. It is only touched by `begin_block` /
///   `end_block` and the control plane.
/// * `Self::Unit` is per-unit state — oscillator phases, envelopes, filters.
///   [`Self::render_unit`] deliberately has no access to `&self`: everything
///   a unit needs each block must arrive in its dispatch payload. On a
///   multi-core host each unit lives in its own isolated instance, so any
///   value smuggled around the payload would simply not be there.
pub trait ParallelProcessor: Default {
    type Unit: Default;

    fn prepare(
        &mut self,
        _sample_rate: f64,
        _maximum_frames: u32,
        _input_channels: u32,
        _output_channels: u32,
    ) -> bool {
        true
    }

    fn set_parameter(&mut self, index: u32, value: f64) -> bool;

    fn get_parameter(&self, _index: u32) -> Option<f64> {
        None
    }

    /// Resets coordinator state. Unit state is reset separately through
    /// [`Self::reset_unit`] on every instance that holds it.
    fn reset(&mut self) {}

    fn reset_unit(unit: &mut Self::Unit) {
        *unit = Self::Unit::default();
    }

    fn begin_resource(&mut self, _id: &str, _total_bytes: u64) -> bool {
        false
    }

    fn write_resource(&mut self, _offset: u64, _bytes: &[u8]) -> bool {
        false
    }

    fn end_resource(&mut self) -> bool {
        false
    }

    fn write_program_catalog(&mut self, _destination: &mut [u8]) -> Option<usize> {
        None
    }

    fn load_preset(&mut self, _id: &str) -> bool {
        false
    }

    fn save_state(&self, _destination: &mut [u8]) -> Option<usize> {
        None
    }

    fn load_state(&mut self, _state: &[u8]) -> bool {
        false
    }

    /// Serial pre-stage: consume MIDI and automation, advance global state
    /// exactly once, decide the active units and write their payloads.
    fn begin_block(&mut self, context: &BlockContext<'_>, plan: &mut PlanWriter<'_>);

    /// Renders one unit from its persistent state and dispatch payload into
    /// `output` (`frames × output_channels` interleaved samples, fully
    /// overwritten). No `&self`: units must not read coordinator state.
    fn render_unit(
        unit_index: u32,
        unit: &mut Self::Unit,
        payload: &[u8],
        context: &UnitContext<'_>,
        output: &mut [f32],
    );

    /// Serial post-stage: combine the unit slots in ascending unit order and
    /// apply global stages. Iterate [`UnitMix::active_units`] — never the
    /// completion order, which the host does not even expose.
    fn end_block(
        &mut self,
        mix: &UnitMix<'_>,
        output: &mut [f32],
        frames: u32,
        output_channels: u32,
    );
}

#[macro_export]
macro_rules! export_processor {
    ($processor:ty, max_frames = $max_frames:expr, max_input_channels = $max_input_channels:expr, max_output_channels = $max_output_channels:expr, max_midi_events = $max_midi_events:expr, max_parameter_events = $max_parameter_events:expr, max_transfer_bytes = $max_transfer_bytes:expr) => {
        const RF_MAX_FRAMES: usize = $max_frames;
        const RF_MAX_INPUT_CHANNELS: usize = $max_input_channels;
        const RF_MAX_OUTPUT_CHANNELS: usize = $max_output_channels;
        const RF_MAX_INPUT_SAMPLES: usize = RF_MAX_FRAMES * RF_MAX_INPUT_CHANNELS;
        const RF_MAX_OUTPUT_SAMPLES: usize = RF_MAX_FRAMES * RF_MAX_OUTPUT_CHANNELS;
        const RF_MAX_MIDI_EVENTS: usize = $max_midi_events;
        const RF_MAX_PARAMETER_EVENTS: usize = $max_parameter_events;
        const RF_MAX_TRANSFER_BYTES: usize = $max_transfer_bytes;

        static mut RF_INPUT: [f32; RF_MAX_INPUT_SAMPLES] = [0.0; RF_MAX_INPUT_SAMPLES];
        static mut RF_OUTPUT: [f32; RF_MAX_OUTPUT_SAMPLES] = [0.0; RF_MAX_OUTPUT_SAMPLES];
        static mut RF_MIDI: [u64; RF_MAX_MIDI_EVENTS] = [0; RF_MAX_MIDI_EVENTS];
        static mut RF_PARAMETERS: [$crate::ParameterEvent; RF_MAX_PARAMETER_EVENTS] =
            [$crate::ParameterEvent {
                frame: 0,
                index: 0,
                value: 0.0,
            }; RF_MAX_PARAMETER_EVENTS];
        static mut RF_TRANSFER: [u8; RF_MAX_TRANSFER_BYTES] = [0; RF_MAX_TRANSFER_BYTES];
        static mut RF_EXCHANGE_INPUT: [u8; RF_MAX_TRANSFER_BYTES] = [0; RF_MAX_TRANSFER_BYTES];
        static mut RF_PROCESSOR: core::mem::MaybeUninit<$processor> =
            core::mem::MaybeUninit::uninit();
        static mut RF_INITIALIZED: bool = false;
        static mut RF_PREPARED: bool = false;

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_abi_version() -> i32 {
            $crate::ABI_VERSION_V1 as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_input_ptr() -> i32 {
            core::ptr::addr_of_mut!(RF_INPUT).cast::<f32>() as usize as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_output_ptr() -> i32 {
            core::ptr::addr_of_mut!(RF_OUTPUT).cast::<f32>() as usize as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_capacity_input_samples() -> i32 {
            RF_MAX_INPUT_SAMPLES as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_capacity_output_samples() -> i32 {
            RF_MAX_OUTPUT_SAMPLES as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_midi_ptr() -> i32 {
            core::ptr::addr_of_mut!(RF_MIDI).cast::<u64>() as usize as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_capacity_midi_events() -> i32 {
            RF_MAX_MIDI_EVENTS as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_parameter_ptr() -> i32 {
            core::ptr::addr_of_mut!(RF_PARAMETERS).cast::<$crate::ParameterEvent>() as usize as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_capacity_parameter_events() -> i32 {
            RF_MAX_PARAMETER_EVENTS as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_transfer_ptr() -> i32 {
            core::ptr::addr_of_mut!(RF_TRANSFER).cast::<u8>() as usize as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_exchange_input_ptr() -> i32 {
            core::ptr::addr_of_mut!(RF_EXCHANGE_INPUT).cast::<u8>() as usize as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_capacity_transfer_bytes() -> i32 {
            RF_MAX_TRANSFER_BYTES as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_initialize() -> i32 {
            unsafe {
                if !RF_INITIALIZED {
                    core::ptr::addr_of_mut!(RF_PROCESSOR)
                        .cast::<$processor>()
                        .write(<$processor as Default>::default());
                    RF_INITIALIZED = true;
                }
            }
            $crate::STATUS_OK
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_prepare(
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
                || maximum_frames as usize > RF_MAX_FRAMES
                || input_channels as usize > RF_MAX_INPUT_CHANNELS
                || output_channels as usize > RF_MAX_OUTPUT_CHANNELS
            {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            // SAFETY: each WebAssembly instance is single-threaded at this ABI
            // boundary and owns one isolated linear memory.
            unsafe {
                if rackforge_initialize() != $crate::STATUS_OK {
                    return $crate::STATUS_INVALID_STATE;
                }
                let processor = &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<$processor>();
                RF_PREPARED = processor.prepare(
                    sample_rate,
                    maximum_frames as u32,
                    input_channels as u32,
                    output_channels as u32,
                );
                if RF_PREPARED {
                    $crate::STATUS_OK
                } else {
                    $crate::STATUS_INVALID_STATE
                }
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_set_parameter(index: i32, value: f64) -> i32 {
            if index < 0 || !value.is_finite() {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            unsafe {
                if !RF_INITIALIZED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let processor = &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<$processor>();
                if processor.set_parameter(index as u32, value) {
                    $crate::STATUS_OK
                } else {
                    $crate::STATUS_UNKNOWN_PARAMETER
                }
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_get_parameter(index: i32) -> f64 {
            if index < 0 {
                return f64::NAN;
            }
            unsafe {
                if !RF_INITIALIZED {
                    return f64::NAN;
                }
                let processor = &*core::ptr::addr_of!(RF_PROCESSOR).cast::<$processor>();
                processor.get_parameter(index as u32).unwrap_or(f64::NAN)
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_reset() -> i32 {
            unsafe {
                if !RF_INITIALIZED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let processor = &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<$processor>();
                processor.reset();
                $crate::STATUS_OK
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_resource_begin(id_length: i32, total_bytes: i64) -> i32 {
            if id_length <= 0 || id_length as usize > RF_MAX_TRANSFER_BYTES || total_bytes < 0 {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            unsafe {
                if rackforge_initialize() != $crate::STATUS_OK {
                    return $crate::STATUS_INVALID_STATE;
                }
                let bytes = core::slice::from_raw_parts(
                    core::ptr::addr_of!(RF_TRANSFER).cast::<u8>(),
                    id_length as usize,
                );
                let Ok(id) = core::str::from_utf8(bytes) else {
                    return $crate::STATUS_INVALID_ARGUMENT;
                };
                let processor = &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<$processor>();
                if processor.begin_resource(id, total_bytes as u64) {
                    $crate::STATUS_OK
                } else {
                    $crate::STATUS_INVALID_STATE
                }
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_resource_write(offset: i64, length: i32) -> i32 {
            if offset < 0 || length < 0 || length as usize > RF_MAX_TRANSFER_BYTES {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            unsafe {
                if !RF_INITIALIZED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let bytes = core::slice::from_raw_parts(
                    core::ptr::addr_of!(RF_TRANSFER).cast::<u8>(),
                    length as usize,
                );
                let processor = &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<$processor>();
                if processor.write_resource(offset as u64, bytes) {
                    $crate::STATUS_OK
                } else {
                    $crate::STATUS_INVALID_STATE
                }
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_resource_end() -> i32 {
            unsafe {
                if !RF_INITIALIZED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let processor = &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<$processor>();
                if processor.end_resource() {
                    $crate::STATUS_OK
                } else {
                    $crate::STATUS_INVALID_STATE
                }
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_preset_catalog() -> i32 {
            unsafe {
                if !RF_INITIALIZED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let processor = &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<$processor>();
                let destination = core::slice::from_raw_parts_mut(
                    core::ptr::addr_of_mut!(RF_TRANSFER).cast::<u8>(),
                    RF_MAX_TRANSFER_BYTES,
                );
                match processor.write_program_catalog(destination) {
                    Some(length) if length > 0 && length <= RF_MAX_TRANSFER_BYTES => length as i32,
                    Some(_) => $crate::STATUS_INVALID_STATE,
                    None => 0,
                }
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_load_preset(length: i32) -> i32 {
            if length <= 0 || length as usize > RF_MAX_TRANSFER_BYTES {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            unsafe {
                if !RF_INITIALIZED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let bytes = core::slice::from_raw_parts(
                    core::ptr::addr_of!(RF_TRANSFER).cast::<u8>(),
                    length as usize,
                );
                let Ok(id) = core::str::from_utf8(bytes) else {
                    return $crate::STATUS_INVALID_ARGUMENT;
                };
                let processor = &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<$processor>();
                if processor.load_preset(id) {
                    $crate::STATUS_OK
                } else {
                    $crate::STATUS_INVALID_STATE
                }
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_save_state() -> i32 {
            unsafe {
                if !RF_INITIALIZED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let processor = &*core::ptr::addr_of!(RF_PROCESSOR).cast::<$processor>();
                let destination = core::slice::from_raw_parts_mut(
                    core::ptr::addr_of_mut!(RF_TRANSFER).cast::<u8>(),
                    RF_MAX_TRANSFER_BYTES,
                );
                match processor.save_state(destination) {
                    Some(length) if length <= RF_MAX_TRANSFER_BYTES => length as i32,
                    _ => $crate::STATUS_INVALID_STATE,
                }
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_load_state(length: i32) -> i32 {
            if length < 0 || length as usize > RF_MAX_TRANSFER_BYTES {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            unsafe {
                if !RF_INITIALIZED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let state = core::slice::from_raw_parts(
                    core::ptr::addr_of!(RF_TRANSFER).cast::<u8>(),
                    length as usize,
                );
                let processor = &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<$processor>();
                if processor.load_state(state) {
                    $crate::STATUS_OK
                } else {
                    $crate::STATUS_INVALID_STATE
                }
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_program_editing_capabilities() -> i32 {
            unsafe {
                if rackforge_initialize() != $crate::STATUS_OK {
                    return $crate::STATUS_INVALID_STATE;
                }
                let processor = &*core::ptr::addr_of!(RF_PROCESSOR).cast::<$processor>();
                let capabilities = processor.program_editing_capabilities();
                if capabilities & !$crate::PROGRAM_EDIT_KNOWN_CAPABILITIES != 0
                    || capabilities != 0 && capabilities & $crate::PROGRAM_EDIT_BASIC == 0
                    || capabilities > i32::MAX as u32
                {
                    $crate::STATUS_INVALID_STATE
                } else {
                    capabilities as i32
                }
            }
        }

        unsafe fn rackforge_program_exchange(operation: u8, source_length: i32) -> i32 {
            if source_length < 0 || source_length as usize > RF_MAX_TRANSFER_BYTES {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            unsafe {
                if !RF_INITIALIZED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let source = core::slice::from_raw_parts(
                    core::ptr::addr_of!(RF_EXCHANGE_INPUT).cast::<u8>(),
                    source_length as usize,
                );
                let destination = core::slice::from_raw_parts_mut(
                    core::ptr::addr_of_mut!(RF_TRANSFER).cast::<u8>(),
                    RF_MAX_TRANSFER_BYTES,
                );
                let processor = &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<$processor>();
                let result = match operation {
                    0 => processor.begin_program_edit(source, destination),
                    1 => processor.prepare_program_save(source, destination),
                    2 => processor.program_editor_view(source, destination),
                    3 => processor.apply_program_edit(source, destination),
                    _ => return $crate::STATUS_INVALID_ARGUMENT,
                };
                match result {
                    Some(length) if length <= RF_MAX_TRANSFER_BYTES => length as i32,
                    _ => $crate::STATUS_INVALID_STATE,
                }
            }
        }

        unsafe fn rackforge_program_install_operation(operation: u8, source_length: i32) -> i32 {
            if source_length < 0 || source_length as usize > RF_MAX_TRANSFER_BYTES {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            unsafe {
                if !RF_INITIALIZED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let source = core::slice::from_raw_parts(
                    core::ptr::addr_of!(RF_EXCHANGE_INPUT).cast::<u8>(),
                    source_length as usize,
                );
                let processor = &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<$processor>();
                let accepted = match operation {
                    0 => processor.install_program(source),
                    1 => processor.preview_program(source),
                    _ => return $crate::STATUS_INVALID_ARGUMENT,
                };
                if accepted {
                    $crate::STATUS_OK
                } else {
                    $crate::STATUS_INVALID_STATE
                }
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_program_begin_edit(source_length: i32) -> i32 {
            unsafe { rackforge_program_exchange(0, source_length) }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_program_prepare_save(source_length: i32) -> i32 {
            unsafe { rackforge_program_exchange(1, source_length) }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_program_install(source_length: i32) -> i32 {
            unsafe { rackforge_program_install_operation(0, source_length) }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_program_preview(source_length: i32) -> i32 {
            unsafe { rackforge_program_install_operation(1, source_length) }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_program_editor_view(source_length: i32) -> i32 {
            unsafe { rackforge_program_exchange(2, source_length) }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_program_apply_edit(source_length: i32) -> i32 {
            unsafe { rackforge_program_exchange(3, source_length) }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_process(
            frames: i32,
            input_channels: i32,
            output_channels: i32,
            midi_event_count: i32,
            parameter_event_count: i32,
        ) -> i32 {
            if frames <= 0
                || input_channels < 0
                || output_channels < 0
                || midi_event_count < 0
                || parameter_event_count < 0
            {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            let Some(input_samples) = (frames as usize).checked_mul(input_channels as usize) else {
                return $crate::STATUS_INVALID_ARGUMENT;
            };
            let Some(output_samples) = (frames as usize).checked_mul(output_channels as usize)
            else {
                return $crate::STATUS_INVALID_ARGUMENT;
            };
            if input_samples > RF_MAX_INPUT_SAMPLES
                || output_samples > RF_MAX_OUTPUT_SAMPLES
                || midi_event_count as usize > RF_MAX_MIDI_EVENTS
                || parameter_event_count as usize > RF_MAX_PARAMETER_EVENTS
            {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            unsafe {
                if !RF_PREPARED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let processor = &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<$processor>();
                let input = core::slice::from_raw_parts(
                    core::ptr::addr_of!(RF_INPUT).cast::<f32>(),
                    input_samples,
                );
                let output = core::slice::from_raw_parts_mut(
                    core::ptr::addr_of_mut!(RF_OUTPUT).cast::<f32>(),
                    output_samples,
                );
                let packed_midi = core::slice::from_raw_parts(
                    core::ptr::addr_of!(RF_MIDI).cast::<u64>(),
                    midi_event_count as usize,
                );
                let parameter_events = core::slice::from_raw_parts(
                    core::ptr::addr_of!(RF_PARAMETERS).cast::<$crate::ParameterEvent>(),
                    parameter_event_count as usize,
                );
                let mut events = [$crate::MidiEvent {
                    frame: 0,
                    data: [0; 3],
                    length: 1,
                }; RF_MAX_MIDI_EVENTS];
                for (destination, packed) in events.iter_mut().zip(packed_midi) {
                    *destination = $crate::MidiEvent::from_packed(*packed);
                    if destination.frame >= frames as u32
                        || destination.length == 0
                        || destination.length > 3
                    {
                        return $crate::STATUS_INVALID_ARGUMENT;
                    }
                }
                if parameter_events
                    .iter()
                    .any(|event| event.frame >= frames as u32 || !event.value.is_finite())
                {
                    return $crate::STATUS_INVALID_ARGUMENT;
                }
                processor.process(
                    input,
                    output,
                    &events[..midi_event_count as usize],
                    parameter_events,
                    frames as u32,
                    input_channels as u32,
                    output_channels as u32,
                );
                $crate::STATUS_OK
            }
        }
    };
}

/// Exports a [`ParallelProcessor`] as a complete `wasm-v1` component with
/// the optional `parallel_render_v1` extension.
///
/// The classic exports are derived from the same stage functions: the
/// generated `rackforge_process` runs `begin_block`, renders every planned
/// unit in ascending order into its mix slot, then runs `end_block`. A host
/// that schedules units performs exactly the same sequence across its own
/// worker instances, so both paths produce identical audio.
#[macro_export]
macro_rules! export_parallel_processor {
    ($processor:ty, max_units = $max_units:expr, dispatch_stride = $dispatch_stride:expr, shared_capacity = $shared_capacity:expr, max_frames = $max_frames:expr, max_input_channels = $max_input_channels:expr, max_output_channels = $max_output_channels:expr, max_midi_events = $max_midi_events:expr, max_parameter_events = $max_parameter_events:expr, max_transfer_bytes = $max_transfer_bytes:expr) => {
        use $crate::Processor as _;

        const RF_PARALLEL_MAX_UNITS: usize = $max_units;
        const RF_PARALLEL_DISPATCH_STRIDE: usize = $dispatch_stride;
        const RF_PARALLEL_SHARED_CAPACITY: usize = $shared_capacity;
        const RF_PARALLEL_MIX_SLOT_SAMPLES: usize = $max_frames * $max_output_channels;

        /// The host requires an 8-aligned dispatch region.
        #[repr(C, align(8))]
        pub struct RackForgeDispatchBuffer(
            [u8; RF_PARALLEL_MAX_UNITS * RF_PARALLEL_DISPATCH_STRIDE],
        );

        static mut RF_DISPATCH: RackForgeDispatchBuffer =
            RackForgeDispatchBuffer([0; RF_PARALLEL_MAX_UNITS * RF_PARALLEL_DISPATCH_STRIDE]);

        /// The host requires an 8-aligned shared region.
        #[repr(C, align(8))]
        pub struct RackForgeSharedBuffer([u8; RF_PARALLEL_SHARED_CAPACITY]);

        static mut RF_SHARED: RackForgeSharedBuffer =
            RackForgeSharedBuffer([0; RF_PARALLEL_SHARED_CAPACITY]);
        /// Header (shared_bytes, reserved) followed by the plan entries.
        static mut RF_PLAN: [u32; 2 + RF_PARALLEL_MAX_UNITS * 2] =
            [0; 2 + RF_PARALLEL_MAX_UNITS * 2];
        static mut RF_PLAN_COUNT: usize = 0;
        static mut RF_MIX: [f32; RF_PARALLEL_MAX_UNITS * RF_PARALLEL_MIX_SLOT_SAMPLES] =
            [0.0; RF_PARALLEL_MAX_UNITS * RF_PARALLEL_MIX_SLOT_SAMPLES];
        static mut RF_PARALLEL_INPUT_CHANNELS: u32 = 0;

        /// Owns the coordinator and, in single-instance hosts, every unit.
        /// In a scheduling host each instance is either used as the
        /// coordinator or entered only through one unit's render calls, so
        /// the states can never race.
        pub struct RackForgeParallelExport {
            inner: $processor,
            units: [<$processor as $crate::ParallelProcessor>::Unit; RF_PARALLEL_MAX_UNITS],
        }

        impl Default for RackForgeParallelExport {
            fn default() -> Self {
                Self {
                    inner: Default::default(),
                    units: core::array::from_fn(|_| Default::default()),
                }
            }
        }

        impl RackForgeParallelExport {
            /// Serial pre-stage over the shared plan/dispatch statics.
            /// Returns the number of planned units.
            fn rf_begin(
                &mut self,
                input: &[f32],
                midi: &[$crate::MidiEvent],
                parameters: &[$crate::ParameterEvent],
                frames: u32,
                input_channels: u32,
                output_channels: u32,
            ) -> usize {
                // SAFETY: every entry point into this component is
                // single-threaded; the statics are only reached from here.
                unsafe {
                    let plan_region = &mut *core::ptr::addr_of_mut!(RF_PLAN);
                    let plan = &mut plan_region[2..];
                    let dispatch = &mut (*core::ptr::addr_of_mut!(RF_DISPATCH)).0;
                    let shared = &mut (*core::ptr::addr_of_mut!(RF_SHARED)).0;
                    let mut writer = $crate::PlanWriter::new(
                        plan,
                        dispatch,
                        shared,
                        RF_PARALLEL_DISPATCH_STRIDE,
                        RF_PARALLEL_MAX_UNITS,
                    );
                    let context = $crate::BlockContext {
                        input,
                        midi,
                        parameters,
                        frames,
                        input_channels,
                        output_channels,
                    };
                    $crate::ParallelProcessor::begin_block(&mut self.inner, &context, &mut writer);
                    let count = writer.activated();
                    let shared_len = writer.shared_len();
                    let header = &mut *core::ptr::addr_of_mut!(RF_PLAN);
                    header[0] = shared_len as u32;
                    header[1] = 0;
                    RF_PLAN_COUNT = count;
                    count
                }
            }

            fn rf_end(&mut self, output: &mut [f32], frames: u32, output_channels: u32) {
                // SAFETY: single-threaded component, as above.
                unsafe {
                    let plan_region = &*core::ptr::addr_of!(RF_PLAN);
                    let mix = $crate::UnitMix::new(
                        &*core::ptr::addr_of!(RF_MIX),
                        RF_PARALLEL_MIX_SLOT_SAMPLES,
                        &plan_region[2..],
                        RF_PLAN_COUNT,
                        frames as usize * output_channels as usize,
                    );
                    $crate::ParallelProcessor::end_block(
                        &mut self.inner,
                        &mix,
                        output,
                        frames,
                        output_channels,
                    );
                }
            }
        }

        impl $crate::Processor for RackForgeParallelExport {
            fn prepare(
                &mut self,
                sample_rate: f64,
                maximum_frames: u32,
                input_channels: u32,
                output_channels: u32,
            ) -> bool {
                // SAFETY: single-threaded component.
                unsafe {
                    RF_PARALLEL_INPUT_CHANNELS = input_channels;
                }
                for unit in &mut self.units {
                    <$processor as $crate::ParallelProcessor>::reset_unit(unit);
                }
                $crate::ParallelProcessor::prepare(
                    &mut self.inner,
                    sample_rate,
                    maximum_frames,
                    input_channels,
                    output_channels,
                )
            }

            fn set_parameter(&mut self, index: u32, value: f64) -> bool {
                $crate::ParallelProcessor::set_parameter(&mut self.inner, index, value)
            }

            fn get_parameter(&self, index: u32) -> Option<f64> {
                $crate::ParallelProcessor::get_parameter(&self.inner, index)
            }

            fn reset(&mut self) {
                $crate::ParallelProcessor::reset(&mut self.inner);
                for unit in &mut self.units {
                    <$processor as $crate::ParallelProcessor>::reset_unit(unit);
                }
            }

            fn begin_resource(&mut self, id: &str, total_bytes: u64) -> bool {
                $crate::ParallelProcessor::begin_resource(&mut self.inner, id, total_bytes)
            }

            fn write_resource(&mut self, offset: u64, bytes: &[u8]) -> bool {
                $crate::ParallelProcessor::write_resource(&mut self.inner, offset, bytes)
            }

            fn end_resource(&mut self) -> bool {
                $crate::ParallelProcessor::end_resource(&mut self.inner)
            }

            fn write_program_catalog(&mut self, destination: &mut [u8]) -> Option<usize> {
                $crate::ParallelProcessor::write_program_catalog(&mut self.inner, destination)
            }

            fn load_preset(&mut self, id: &str) -> bool {
                $crate::ParallelProcessor::load_preset(&mut self.inner, id)
            }

            fn save_state(&self, destination: &mut [u8]) -> Option<usize> {
                $crate::ParallelProcessor::save_state(&self.inner, destination)
            }

            fn load_state(&mut self, state: &[u8]) -> bool {
                $crate::ParallelProcessor::load_state(&mut self.inner, state)
            }

            fn process(
                &mut self,
                input: &[f32],
                output: &mut [f32],
                midi: &[$crate::MidiEvent],
                parameters: &[$crate::ParameterEvent],
                frames: u32,
                input_channels: u32,
                output_channels: u32,
            ) {
                let count = self.rf_begin(
                    input,
                    midi,
                    parameters,
                    frames,
                    input_channels,
                    output_channels,
                );
                let samples = frames as usize * output_channels as usize;
                for index in 0..count {
                    // SAFETY: single-threaded component; the plan was just
                    // written by `rf_begin` and stays untouched until the
                    // next block.
                    let (unit, payload_len, shared_len) = unsafe {
                        let plan = &*core::ptr::addr_of!(RF_PLAN);
                        (
                            plan[2 + index * 2],
                            plan[2 + index * 2 + 1] as usize,
                            plan[0] as usize,
                        )
                    };
                    // SAFETY: as above; payload and mix regions are disjoint
                    // from `output`.
                    unsafe {
                        let dispatch = &(*core::ptr::addr_of!(RF_DISPATCH)).0;
                        let payload =
                            &dispatch[unit as usize * RF_PARALLEL_DISPATCH_STRIDE..][..payload_len];
                        let shared_region = &(*core::ptr::addr_of!(RF_SHARED)).0;
                        let shared = &shared_region[..shared_len];
                        let context = $crate::UnitContext {
                            input,
                            shared,
                            frames,
                            output_channels,
                        };
                        <$processor as $crate::ParallelProcessor>::render_unit(
                            unit,
                            &mut self.units[unit as usize],
                            payload,
                            &context,
                            &mut output[..samples],
                        );
                        let mix = &mut *core::ptr::addr_of_mut!(RF_MIX);
                        mix[unit as usize * RF_PARALLEL_MIX_SLOT_SAMPLES..][..samples]
                            .copy_from_slice(&output[..samples]);
                    }
                }
                self.rf_end(output, frames, output_channels);
            }
        }

        $crate::export_processor!(
            RackForgeParallelExport,
            max_frames = $max_frames,
            max_input_channels = $max_input_channels,
            max_output_channels = $max_output_channels,
            max_midi_events = $max_midi_events,
            max_parameter_events = $max_parameter_events,
            max_transfer_bytes = $max_transfer_bytes
        );

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_parallel_abi_version() -> i32 {
            $crate::PARALLEL_ABI_VERSION_V1 as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_parallel_max_units() -> i32 {
            RF_PARALLEL_MAX_UNITS as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_parallel_dispatch_stride() -> i32 {
            RF_PARALLEL_DISPATCH_STRIDE as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_parallel_dispatch_ptr() -> i32 {
            core::ptr::addr_of_mut!(RF_DISPATCH).cast::<u8>() as usize as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_parallel_plan_ptr() -> i32 {
            core::ptr::addr_of_mut!(RF_PLAN).cast::<u32>() as usize as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_parallel_mix_ptr() -> i32 {
            core::ptr::addr_of_mut!(RF_MIX).cast::<f32>() as usize as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_parallel_shared_ptr() -> i32 {
            core::ptr::addr_of_mut!(RF_SHARED).cast::<u8>() as usize as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_parallel_shared_capacity() -> i32 {
            RF_PARALLEL_SHARED_CAPACITY as i32
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_parallel_begin_block(
            frames: i32,
            input_channels: i32,
            output_channels: i32,
            midi_event_count: i32,
            parameter_event_count: i32,
        ) -> i32 {
            if frames <= 0
                || input_channels < 0
                || output_channels < 0
                || midi_event_count < 0
                || parameter_event_count < 0
            {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            let Some(input_samples) = (frames as usize).checked_mul(input_channels as usize) else {
                return $crate::STATUS_INVALID_ARGUMENT;
            };
            if input_samples > RF_MAX_INPUT_SAMPLES
                || frames as usize > RF_MAX_FRAMES
                || midi_event_count as usize > RF_MAX_MIDI_EVENTS
                || parameter_event_count as usize > RF_MAX_PARAMETER_EVENTS
            {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            // SAFETY: single-threaded component entry point over the same
            // statics as `rackforge_process`.
            unsafe {
                if !RF_PREPARED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let processor =
                    &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<RackForgeParallelExport>();
                let input = core::slice::from_raw_parts(
                    core::ptr::addr_of!(RF_INPUT).cast::<f32>(),
                    input_samples,
                );
                let packed_midi = core::slice::from_raw_parts(
                    core::ptr::addr_of!(RF_MIDI).cast::<u64>(),
                    midi_event_count as usize,
                );
                let parameter_events = core::slice::from_raw_parts(
                    core::ptr::addr_of!(RF_PARAMETERS).cast::<$crate::ParameterEvent>(),
                    parameter_event_count as usize,
                );
                let mut events = [$crate::MidiEvent {
                    frame: 0,
                    data: [0; 3],
                    length: 1,
                }; RF_MAX_MIDI_EVENTS];
                for (destination, packed) in events.iter_mut().zip(packed_midi) {
                    *destination = $crate::MidiEvent::from_packed(*packed);
                    if destination.frame >= frames as u32
                        || destination.length == 0
                        || destination.length > 3
                    {
                        return $crate::STATUS_INVALID_ARGUMENT;
                    }
                }
                if parameter_events
                    .iter()
                    .any(|event| event.frame >= frames as u32 || !event.value.is_finite())
                {
                    return $crate::STATUS_INVALID_ARGUMENT;
                }
                processor.rf_begin(
                    input,
                    &events[..midi_event_count as usize],
                    parameter_events,
                    frames as u32,
                    input_channels as u32,
                    output_channels as u32,
                ) as i32
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_parallel_render_unit(
            unit: i32,
            payload_bytes: i32,
            shared_bytes: i32,
            frames: i32,
            output_channels: i32,
        ) -> i32 {
            if unit < 0
                || unit as usize >= RF_PARALLEL_MAX_UNITS
                || payload_bytes < 0
                || payload_bytes as usize > RF_PARALLEL_DISPATCH_STRIDE
                || shared_bytes < 0
                || shared_bytes as usize > RF_PARALLEL_SHARED_CAPACITY
                || frames <= 0
                || frames as usize > RF_MAX_FRAMES
                || output_channels < 0
            {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            let Some(samples) = (frames as usize).checked_mul(output_channels as usize) else {
                return $crate::STATUS_INVALID_ARGUMENT;
            };
            if samples > RF_MAX_OUTPUT_SAMPLES {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            // SAFETY: single-threaded component entry point. In a scheduling
            // host this instance is a worker: only this unit's state is
            // touched, exactly as the trait contract promises.
            unsafe {
                if !RF_PREPARED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let processor =
                    &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<RackForgeParallelExport>();
                let input_samples =
                    (frames as usize).saturating_mul(RF_PARALLEL_INPUT_CHANNELS as usize);
                if input_samples > RF_MAX_INPUT_SAMPLES {
                    return $crate::STATUS_INVALID_ARGUMENT;
                }
                let input = core::slice::from_raw_parts(
                    core::ptr::addr_of!(RF_INPUT).cast::<f32>(),
                    input_samples,
                );
                let dispatch = &(*core::ptr::addr_of!(RF_DISPATCH)).0;
                let payload = &dispatch[unit as usize * RF_PARALLEL_DISPATCH_STRIDE..]
                    [..payload_bytes as usize];
                let shared_region = &(*core::ptr::addr_of!(RF_SHARED)).0;
                let shared = &shared_region[..shared_bytes as usize];
                let output = core::slice::from_raw_parts_mut(
                    core::ptr::addr_of_mut!(RF_OUTPUT).cast::<f32>(),
                    samples,
                );
                let context = $crate::UnitContext {
                    input,
                    shared,
                    frames: frames as u32,
                    output_channels: output_channels as u32,
                };
                <$processor as $crate::ParallelProcessor>::render_unit(
                    unit as u32,
                    &mut processor.units[unit as usize],
                    payload,
                    &context,
                    output,
                );
                $crate::STATUS_OK
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn rackforge_parallel_end_block(frames: i32, output_channels: i32) -> i32 {
            if frames <= 0 || frames as usize > RF_MAX_FRAMES || output_channels < 0 {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            let Some(samples) = (frames as usize).checked_mul(output_channels as usize) else {
                return $crate::STATUS_INVALID_ARGUMENT;
            };
            if samples > RF_MAX_OUTPUT_SAMPLES {
                return $crate::STATUS_INVALID_ARGUMENT;
            }
            // SAFETY: single-threaded component entry point.
            unsafe {
                if !RF_PREPARED {
                    return $crate::STATUS_INVALID_STATE;
                }
                let processor =
                    &mut *core::ptr::addr_of_mut!(RF_PROCESSOR).cast::<RackForgeParallelExport>();
                let output = core::slice::from_raw_parts_mut(
                    core::ptr::addr_of_mut!(RF_OUTPUT).cast::<f32>(),
                    samples,
                );
                processor.rf_end(output, frames as u32, output_channels as u32);
                $crate::STATUS_OK
            }
        }
    };
}
