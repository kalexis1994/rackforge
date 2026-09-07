#![cfg(not(target_arch = "wasm32"))]

//! Expands the `midi2` arm of `export_parallel_processor!` natively and
//! drives its entries directly. An integration test, because the classic
//! wide probe already owns the one set of exports the lib's test binary
//! can carry.
//!
//! The arm gives a parallel component the wide-MIDI contract on its
//! composed `process` and a second pre-stage entry that takes the wide
//! count, and forwards the program editor to the coordinator. As with the
//! classic wide probe, the macro body is only checked where it is expanded,
//! so this probe reports what its coordinator was handed — into the
//! block-shared payload, where a unit and the composed block can read it
//! back — instead of making sound.

use rackforge_plugin_sdk::{
    BlockContext, MIDI_FAMILY_NOTE, MIDI2_FLAG_ORIGIN_7BIT, MIDI2_KIND_NOTE_ON, ParallelProcessor,
    PlanWriter, STATUS_INVALID_ARGUMENT, STATUS_OK, UnitContext, UnitMix,
};

#[derive(Default)]
struct Probe;

/// The report: narrow count, wide count, then the first wide event's
/// frame, kind, channel, index, flags, value and extra.
const REPORT_WORDS: usize = 9;

impl ParallelProcessor for Probe {
    type Unit = ();

    fn set_parameter(&mut self, _index: u32, _value: f64) -> bool {
        false
    }

    fn program_editing_capabilities(&self) -> u32 {
        7
    }

    fn begin_block(&mut self, context: &BlockContext<'_>, plan: &mut PlanWriter<'_>) {
        let mut report = [0.0_f32; REPORT_WORDS];
        report[0] = context.midi.len() as f32;
        report[1] = context.midi2.len() as f32;
        if let Some(event) = context.midi2.first() {
            report[2] = event.frame as f32;
            report[3] = event.kind as f32;
            report[4] = event.channel as f32;
            report[5] = event.index as f32;
            report[6] = event.flags as f32;
            report[7] = event.value as f32;
            report[8] = event.extra as f32;
        }
        let shared = plan.shared_buffer();
        for (word, value) in report.iter().enumerate() {
            shared[word * 4..][..4].copy_from_slice(&value.to_le_bytes());
        }
        assert!(plan.commit_shared(REPORT_WORDS * 4));
        assert!(plan.activate(0, &[]));
    }

    fn render_unit(
        _unit_index: u32,
        _unit: &mut Self::Unit,
        _payload: &[u8],
        context: &UnitContext<'_>,
        output: &mut [f32],
    ) {
        // A unit sees the report through the shared payload, as it would on
        // a worker instance.
        output.fill(0.0);
        for (word, chunk) in context.shared.as_chunks::<4>().0.iter().enumerate() {
            output[word] = f32::from_le_bytes(*chunk);
        }
    }

    fn end_block(
        &mut self,
        mix: &UnitMix<'_>,
        output: &mut [f32],
        _frames: u32,
        _output_channels: u32,
    ) {
        output.fill(0.0);
        for unit in mix.active_units() {
            for (target, sample) in output.iter_mut().zip(mix.slot(unit)) {
                *target += *sample;
            }
        }
    }
}

rackforge_plugin_sdk::export_parallel_processor!(
    Probe,
    max_units = 1,
    dispatch_stride = 8,
    shared_capacity = 64,
    max_frames = 64,
    max_input_channels = 0,
    max_output_channels = 2,
    max_midi_events = 8,
    max_parameter_events = 8,
    max_transfer_bytes = 64,
    midi2 = { max_events = 4, families = MIDI_FAMILY_NOTE }
);

fn shared_report() -> [f32; REPORT_WORDS] {
    let shared = unsafe { &(*core::ptr::addr_of!(RF_SHARED)).0 };
    let mut report = [0.0; REPORT_WORDS];
    for (word, value) in report.iter_mut().enumerate() {
        *value = f32::from_le_bytes(shared[word * 4..][..4].try_into().expect("four bytes"));
    }
    report
}

/// One test, because the generated statics are process-wide state.
#[test]
fn the_wide_pre_stage_hands_the_coordinator_what_the_host_wrote() {
    assert_eq!(rackforge_initialize(), STATUS_OK);
    assert_eq!(rackforge_prepare(48_000.0, 64, 0, 2), STATUS_OK);
    assert_eq!(rackforge_capacity_midi2_events(), 4);
    assert_eq!(rackforge_midi2_families(), MIDI_FAMILY_NOTE as i32);
    // The program editor reaches the coordinator through the wrapper.
    assert_eq!(rackforge_program_editing_capabilities(), 7);

    // One narrow controller and one wide note-on, packed the way the host
    // packs them.
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
    let expected = [
        1.0,
        1.0,
        5.0,
        MIDI2_KIND_NOTE_ON as f32,
        3.0,
        60.0,
        MIDI2_FLAG_ORIGIN_7BIT as f32,
        65535.0,
        7.0,
    ];

    // The wide pre-stage: one unit planned, the report in the shared payload.
    assert_eq!(rackforge_parallel_begin_block_v2(64, 0, 2, 1, 0, 1), 1);
    assert_eq!(shared_report(), expected);
    assert_eq!(
        unsafe { (*core::ptr::addr_of!(RF_PLAN))[0] },
        (REPORT_WORDS * 4) as u32
    );

    // The composed wide block reaches the same coordinator and a unit.
    assert_eq!(rackforge_process_v2(64, 0, 2, 1, 0, 1), STATUS_OK);
    let output = unsafe { *core::ptr::addr_of!(RF_OUTPUT) };
    assert_eq!(&output[..REPORT_WORDS], &expected);

    // The narrow pre-stage still exists and hands over no wide event.
    assert_eq!(rackforge_parallel_begin_block(64, 0, 2, 1, 0), 1);
    assert_eq!(&shared_report()[..2], &[1.0, 0.0]);

    // Over capacity, or outside the block: refused before the coordinator runs.
    assert_eq!(
        rackforge_parallel_begin_block_v2(64, 0, 2, 0, 0, 5),
        STATUS_INVALID_ARGUMENT
    );
    unsafe {
        (*core::ptr::addr_of_mut!(RF_MIDI2))[0] = 64;
    }
    assert_eq!(
        rackforge_parallel_begin_block_v2(64, 0, 2, 0, 0, 1),
        STATUS_INVALID_ARGUMENT
    );
}
