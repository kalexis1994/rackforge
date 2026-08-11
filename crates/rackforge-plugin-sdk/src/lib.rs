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
                match processor.write_preset_catalog(destination) {
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
