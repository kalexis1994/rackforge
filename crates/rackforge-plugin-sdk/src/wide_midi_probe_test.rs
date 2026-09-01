//! Expands the `midi2` arm of `export_processor!` natively and drives its
//! entry directly.
//!
//! A `macro_rules!` body is checked only where it is expanded, and no shipped
//! component has adopted the wide-MIDI contract yet, so without this the arm
//! would be unproven text. The probe processor reports what it was handed
//! instead of making sound, and the test writes the two words per event the
//! host writes, at the layout the host ABI documents, then reads them back
//! through the generated entry.

use crate::{
    MIDI_FAMILY_NOTE, MIDI2_FLAG_ORIGIN_7BIT, MIDI2_KIND_NOTE_ON, MidiEvent, MidiEvent2,
    ParameterEvent, Processor, STATUS_INVALID_ARGUMENT, STATUS_INVALID_STATE, STATUS_OK,
};

#[derive(Default)]
struct Probe;

impl Processor for Probe {
    fn set_parameter(&mut self, _index: u32, _value: f64) -> bool {
        false
    }

    /// The narrow entry leaves a marker no wide block can produce.
    fn process(
        &mut self,
        _input: &[f32],
        output: &mut [f32],
        _midi: &[MidiEvent],
        _parameters: &[ParameterEvent],
        _frames: u32,
        _input_channels: u32,
        _output_channels: u32,
    ) {
        output.fill(-1.0);
    }

    fn process_wide(
        &mut self,
        _input: &[f32],
        output: &mut [f32],
        midi: &[MidiEvent],
        midi2: &[MidiEvent2],
        _parameters: &[ParameterEvent],
        _frames: u32,
        _input_channels: u32,
        _output_channels: u32,
    ) {
        output.fill(0.0);
        output[0] = midi.len() as f32;
        output[1] = midi2.len() as f32;
        if let Some(event) = midi2.first() {
            output[2] = event.frame as f32;
            output[3] = event.kind as f32;
            output[4] = event.channel as f32;
            output[5] = event.index as f32;
            output[6] = event.flags as f32;
            output[7] = event.value as f32;
            output[8] = event.extra as f32;
        }
        if let Some(event) = midi.first() {
            output[9] = event.data[0] as f32;
        }
    }
}

crate::export_processor!(
    Probe,
    max_frames = 64,
    max_input_channels = 0,
    max_output_channels = 2,
    max_midi_events = 8,
    max_parameter_events = 8,
    max_transfer_bytes = 64,
    midi2 = { max_events = 4, families = MIDI_FAMILY_NOTE }
);

/// One test, because the generated statics are process-wide state.
#[test]
fn the_wide_entry_hands_over_exactly_what_the_host_wrote() {
    // Like the narrow entry, it refuses to run before `prepare`.
    assert_eq!(
        rackforge_process_v2(64, 0, 2, 0, 0, 0),
        STATUS_INVALID_STATE
    );
    assert_eq!(rackforge_initialize(), STATUS_OK);
    assert_eq!(rackforge_prepare(48_000.0, 64, 0, 2), STATUS_OK);
    assert_eq!(rackforge_capacity_midi2_events(), 4);
    assert_eq!(rackforge_midi2_families(), MIDI_FAMILY_NOTE as i32);

    // One narrow controller and one wide note-on, packed the way the host
    // packs them: frame in the low word, then kind/channel/index/flags one
    // byte each; value and extra in the second word.
    let narrow: u64 = 0xB0u64 << 32 | 1u64 << 40 | 2u64 << 48 | 3u64 << 56;
    let head: u64 = 5
        | (MIDI2_KIND_NOTE_ON as u64) << 32
        | 3u64 << 40
        | 60u64 << 48
        | (MIDI2_FLAG_ORIGIN_7BIT as u64) << 56;
    let tail: u64 = 0xFFFF | 7u64 << 32;
    unsafe {
        (*core::ptr::addr_of_mut!(RF_MIDI))[0] = narrow;
        let wide = &mut *core::ptr::addr_of_mut!(RF_MIDI2);
        wide[0] = head;
        wide[1] = tail;
    }
    assert_eq!(rackforge_process_v2(64, 0, 2, 1, 0, 1), STATUS_OK);
    let output = unsafe { *core::ptr::addr_of!(RF_OUTPUT) };
    assert_eq!(
        &output[..10],
        &[
            1.0,
            1.0,
            5.0,
            MIDI2_KIND_NOTE_ON as f32,
            3.0,
            60.0,
            MIDI2_FLAG_ORIGIN_7BIT as f32,
            65535.0,
            7.0,
            0xB0 as f32,
        ]
    );

    // Over capacity, or outside the block: refused before the processor runs.
    assert_eq!(
        rackforge_process_v2(64, 0, 2, 0, 0, 5),
        STATUS_INVALID_ARGUMENT
    );
    unsafe {
        (*core::ptr::addr_of_mut!(RF_MIDI2))[0] = 64;
    }
    assert_eq!(
        rackforge_process_v2(64, 0, 2, 0, 0, 1),
        STATUS_INVALID_ARGUMENT
    );

    // The narrow entry is still exported and still reaches `process`.
    assert_eq!(rackforge_process(64, 0, 2, 0, 0), STATUS_OK);
    assert_eq!(unsafe { (*core::ptr::addr_of!(RF_OUTPUT))[0] }, -1.0);
}
