#![cfg(not(target_arch = "wasm32"))]

//! Drives one processor through both of `export_processor!`'s exports -- the
//! `wasm-v1` entries over the component's statics, and the native table over
//! its own instances -- and holds them to the same answers.
//!
//! A host runs a package's native build in place of its component only
//! because the two are the same plugin; this is where that is checked: the
//! same statuses for the same arguments, the same samples for the same
//! block, a state saved by one restored by the other, and instances that
//! share nothing. An integration test, because the exports are process-wide
//! symbols and the lib's own test binary already carries a set.

use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use rackforge_plugin_sdk::portable::native::{NATIVE_ABI_VERSION_V1, NativeApiV1, region};
use rackforge_plugin_sdk::{
    ABI_VERSION_V1, MidiEvent, ParameterEvent, Processor, STATUS_INVALID_ARGUMENT,
    STATUS_INVALID_STATE, STATUS_OK, STATUS_UNKNOWN_PARAMETER,
};

static DROPPED: AtomicUsize = AtomicUsize::new(0);

/// A gain, a latency, a preset and a state: enough of the contract to tell
/// two paths apart if they disagree anywhere.
struct Gain {
    gain: f64,
}

impl Default for Gain {
    fn default() -> Self {
        Self { gain: 0.5 }
    }
}

impl Drop for Gain {
    fn drop(&mut self) {
        DROPPED.fetch_add(1, Ordering::SeqCst);
    }
}

impl Processor for Gain {
    fn set_parameter(&mut self, index: u32, value: f64) -> bool {
        if index != 0 {
            return false;
        }
        self.gain = value;
        true
    }

    fn get_parameter(&self, index: u32) -> Option<f64> {
        (index == 0).then_some(self.gain)
    }

    fn latency_frames(&self) -> u32 {
        3
    }

    fn load_preset(&mut self, id: &str) -> bool {
        if id != "loud" {
            return false;
        }
        self.gain = 2.0;
        true
    }

    fn save_state(&self, destination: &mut [u8]) -> Option<usize> {
        destination
            .get_mut(..8)?
            .copy_from_slice(&self.gain.to_le_bytes());
        Some(8)
    }

    fn load_state(&mut self, state: &[u8]) -> bool {
        let Ok(bytes) = <[u8; 8]>::try_from(state) else {
            return false;
        };
        self.gain = f64::from_le_bytes(bytes);
        true
    }

    /// Input times the gain, automation applied at its frame, and each
    /// event's status byte added at its own frame.
    fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        midi: &[MidiEvent],
        parameters: &[ParameterEvent],
        frames: u32,
        input_channels: u32,
        output_channels: u32,
    ) {
        let mut gain = self.gain;
        for frame in 0..frames as usize {
            for event in parameters
                .iter()
                .filter(|event| event.frame as usize == frame)
            {
                if event.index == 0 {
                    gain = event.value;
                }
            }
            let marker: f32 = midi
                .iter()
                .filter(|event| event.frame as usize == frame)
                .map(|event| f32::from(event.data[0]))
                .sum();
            for channel in 0..output_channels as usize {
                let source = if input_channels == 0 {
                    0.0
                } else {
                    input[frame * input_channels as usize + channel % input_channels as usize]
                };
                output[frame * output_channels as usize + channel] =
                    (f64::from(source) * gain) as f32 + marker;
            }
        }
        self.gain = gain;
    }
}

rackforge_plugin_sdk::export_processor!(
    Gain,
    max_frames = 32,
    max_input_channels = 2,
    max_output_channels = 2,
    max_midi_events = 4,
    max_parameter_events = 4,
    max_transfer_bytes = 16
);

// Found by its symbol, as a host finds it in the library: the export itself
// has no name in Rust.
unsafe extern "C" {
    fn rackforge_portable_native_entry_v1() -> *const NativeApiV1;
}

fn table() -> &'static NativeApiV1 {
    unsafe { &*rackforge_portable_native_entry_v1() }
}

/// A native instance and its regions, as a host would hold them.
struct Native(*mut c_void);

impl Native {
    fn new() -> Self {
        let instance = unsafe { (table().create)() };
        assert!(!instance.is_null());
        Self(instance)
    }

    fn region<T>(&mut self, which: u32) -> &mut [T] {
        let mut capacity = 0;
        let pointer = unsafe { (table().region)(self.0, which, &mut capacity) };
        assert!(!pointer.is_null(), "region {which}");
        unsafe { core::slice::from_raw_parts_mut(pointer.cast::<T>(), capacity) }
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        unsafe { (table().destroy)(self.0) };
    }
}

/// The component's side: its statics, as a host sees its linear memory.
fn component_input() -> &'static mut [f32] {
    unsafe { &mut *core::ptr::addr_of_mut!(RF_INPUT) }
}
fn component_output() -> &'static [f32] {
    unsafe { &*core::ptr::addr_of!(RF_OUTPUT) }
}
fn component_midi() -> &'static mut [u64] {
    unsafe { &mut *core::ptr::addr_of_mut!(RF_MIDI) }
}
fn component_parameters() -> &'static mut [ParameterEvent] {
    unsafe { &mut *core::ptr::addr_of_mut!(RF_PARAMETERS) }
}
fn component_transfer() -> &'static mut [u8] {
    unsafe { &mut *core::ptr::addr_of_mut!(RF_TRANSFER) }
}

fn packed_midi(frame: u32, status: u8) -> u64 {
    u64::from(frame) | u64::from(status) << 32 | 60 << 40 | 100 << 48 | 3 << 56
}

/// One test, because the component's statics are process-wide state.
#[test]
fn the_native_table_answers_exactly_as_the_component_does() {
    let api = table();
    assert_eq!(api.struct_size as usize, size_of::<NativeApiV1>());
    assert_eq!(api.native_abi_version, NATIVE_ABI_VERSION_V1);
    assert_eq!(api.portable_abi_version, ABI_VERSION_V1);
    assert_eq!(api.midi2_families, 0);
    assert!(api.process_v2.is_none(), "no midi2 clause, no wide entry");

    let mut a = Native::new();
    let mut b = Native::new();

    // Regions: the declared capacities, nothing for a region this build
    // lacks or a number it does not know.
    assert_eq!(a.region::<f32>(region::INPUT).len(), 64);
    assert_eq!(a.region::<f32>(region::OUTPUT).len(), 64);
    assert_eq!(a.region::<u64>(region::MIDI).len(), 4);
    assert_eq!(a.region::<ParameterEvent>(region::PARAMETERS).len(), 4);
    assert_eq!(a.region::<u8>(region::TRANSFER).len(), 16);
    assert_eq!(a.region::<u8>(region::EXCHANGE_INPUT).len(), 16);
    for missing in [region::MIDI2, 99] {
        let mut capacity = 7;
        let pointer = unsafe { (api.region)(a.0, missing, &mut capacity) };
        assert!(pointer.is_null() && capacity == 0, "region {missing}");
    }

    // Before anything is made or prepared, the same refusals.
    assert_eq!(rackforge_process(8, 2, 2, 0, 0), STATUS_INVALID_STATE);
    assert_eq!(
        unsafe { (api.process)(a.0, 8, 2, 2, 0, 0) },
        STATUS_INVALID_STATE
    );
    assert_eq!(rackforge_set_parameter(0, 1.0), STATUS_INVALID_STATE);
    assert_eq!(
        unsafe { (api.set_parameter)(a.0, 0, 1.0) },
        STATUS_INVALID_STATE
    );
    assert!(rackforge_get_parameter(0).is_nan());
    assert!(unsafe { (api.get_parameter)(a.0, 0) }.is_nan());

    // `prepare`'s argument checks, then success on both sides.
    for (rate, frames, inputs, outputs) in [
        (0.0, 8, 2, 2),
        (f64::NAN, 8, 2, 2),
        (48_000.0, 0, 2, 2),
        (48_000.0, 33, 2, 2),
        (48_000.0, 8, 3, 2),
        (48_000.0, 8, 2, -1),
    ] {
        assert_eq!(
            rackforge_prepare(rate, frames, inputs, outputs),
            STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            unsafe { (api.prepare)(a.0, rate, frames, inputs, outputs) },
            STATUS_INVALID_ARGUMENT
        );
    }
    assert_eq!(rackforge_prepare(48_000.0, 32, 2, 2), STATUS_OK);
    assert_eq!(unsafe { (api.prepare)(a.0, 48_000.0, 32, 2, 2) }, STATUS_OK);
    assert_eq!(unsafe { (api.prepare)(b.0, 48_000.0, 32, 2, 2) }, STATUS_OK);

    assert_eq!(rackforge_latency_frames(), 3);
    assert_eq!(unsafe { (api.latency_frames)(a.0) }, 3);
    assert_eq!(rackforge_set_parameter(7, 1.0), STATUS_UNKNOWN_PARAMETER);
    assert_eq!(
        unsafe { (api.set_parameter)(a.0, 7, 1.0) },
        STATUS_UNKNOWN_PARAMETER
    );
    assert_eq!(rackforge_set_parameter(0, 0.75), STATUS_OK);
    assert_eq!(unsafe { (api.set_parameter)(a.0, 0, 0.75) }, STATUS_OK);

    // One block, written into each side's regions: the same samples back.
    let input: Vec<f32> = (0..64).map(|i| (i as f32 * 0.37).sin()).collect();
    let midi = [packed_midi(2, 0x90), packed_midi(9, 0x80)];
    let automation = [ParameterEvent {
        frame: 5,
        index: 0,
        value: 1.25,
    }];
    component_input().copy_from_slice(&input);
    component_midi()[..2].copy_from_slice(&midi);
    component_parameters()[..1].copy_from_slice(&automation);
    a.region::<f32>(region::INPUT).copy_from_slice(&input);
    a.region::<u64>(region::MIDI)[..2].copy_from_slice(&midi);
    a.region::<ParameterEvent>(region::PARAMETERS)[..1].copy_from_slice(&automation);
    assert_eq!(rackforge_process(32, 2, 2, 2, 1), STATUS_OK);
    assert_eq!(unsafe { (api.process)(a.0, 32, 2, 2, 2, 1) }, STATUS_OK);
    let native: Vec<u32> = a
        .region::<f32>(region::OUTPUT)
        .iter()
        .map(|x| x.to_bits())
        .collect();
    let component: Vec<u32> = component_output().iter().map(|x| x.to_bits()).collect();
    assert_eq!(native, component);
    assert_eq!(component_output()[4], 144.0 + input[4] * 0.75);

    // A block's shape is checked against the regions, the same way on both
    // sides.
    for (frames, inputs, outputs, events, points) in [
        (0, 2, 2, 0, 0),
        (33, 2, 2, 0, 0),
        (8, 9, 2, 0, 0),
        (8, 2, 9, 0, 0),
        (8, -1, 2, 0, 0),
        (8, 2, 2, 5, 0),
        (8, 2, 2, 0, 5),
    ] {
        assert_eq!(
            rackforge_process(frames, inputs, outputs, events, points),
            STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            unsafe { (api.process)(a.0, frames, inputs, outputs, events, points) },
            STATUS_INVALID_ARGUMENT
        );
    }
    // An event past the block is refused before the processor runs.
    a.region::<u64>(region::MIDI)[0] = packed_midi(8, 0x90);
    assert_eq!(
        unsafe { (api.process)(a.0, 8, 2, 2, 1, 0) },
        STATUS_INVALID_ARGUMENT
    );

    // The instances share nothing: B never saw A's parameter or automation.
    assert_eq!(unsafe { (api.get_parameter)(a.0, 0) }, 1.25);
    assert_eq!(unsafe { (api.get_parameter)(b.0, 0) }, 0.5);
    assert_eq!(rackforge_get_parameter(0), 1.25);

    // A state saved by the component restores into a native instance.
    assert_eq!(rackforge_save_state(), 8);
    let saved = component_transfer()[..8].to_vec();
    b.region::<u8>(region::TRANSFER)[..8].copy_from_slice(&saved);
    assert_eq!(unsafe { (api.load_state)(b.0, 8) }, STATUS_OK);
    assert_eq!(unsafe { (api.get_parameter)(b.0, 0) }, 1.25);
    assert_eq!(unsafe { (api.save_state)(b.0) }, 8);
    assert_eq!(&b.region::<u8>(region::TRANSFER)[..8], &saved[..]);
    assert_eq!(rackforge_load_state(17), STATUS_INVALID_ARGUMENT);
    assert_eq!(
        unsafe { (api.load_state)(b.0, 17) },
        STATUS_INVALID_ARGUMENT
    );
    assert_eq!(rackforge_load_state(3), STATUS_INVALID_STATE);
    assert_eq!(unsafe { (api.load_state)(b.0, 3) }, STATUS_INVALID_STATE);

    // Presets by id through the transfer region.
    component_transfer()[..4].copy_from_slice(b"loud");
    b.region::<u8>(region::TRANSFER)[..4].copy_from_slice(b"loud");
    assert_eq!(rackforge_load_preset(4), STATUS_OK);
    assert_eq!(unsafe { (api.load_preset)(b.0, 4) }, STATUS_OK);
    assert_eq!(unsafe { (api.get_parameter)(b.0, 0) }, 2.0);
    assert_eq!(rackforge_load_preset(3), STATUS_INVALID_STATE);
    assert_eq!(unsafe { (api.load_preset)(b.0, 3) }, STATUS_INVALID_STATE);
    assert_eq!(rackforge_load_preset(0), STATUS_INVALID_ARGUMENT);
    assert_eq!(
        unsafe { (api.load_preset)(b.0, 0) },
        STATUS_INVALID_ARGUMENT
    );

    // No program editor: the same answer from both.
    assert_eq!(rackforge_program_editing_capabilities(), 0);
    assert_eq!(unsafe { (api.program_editing_capabilities)(b.0) }, 0);

    // Destroying an instance drops its processor, and only its own.
    let before = DROPPED.load(Ordering::SeqCst);
    drop(a);
    assert_eq!(DROPPED.load(Ordering::SeqCst), before + 1);
    assert_eq!(unsafe { (api.get_parameter)(b.0, 0) }, 2.0);
    drop(b);
    assert_eq!(DROPPED.load(Ordering::SeqCst), before + 2);
    unsafe { (api.destroy)(core::ptr::null_mut()) };
}
