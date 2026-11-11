#![cfg(not(target_arch = "wasm32"))]

//! A parallel processor's native build, held to its component three ways.
//!
//! The component's composed `rackforge_process`, the native table's composed
//! `process`, and the native parallel stages run the way a scheduling host
//! runs them -- the pre-stage on a coordinator instance, each planned unit on
//! a worker instance, the payloads, audio and reports carried across by
//! copying regions, the post-stage on the coordinator -- must all come out
//! the same bits. Units keep state across blocks, are wider than the
//! instrument's channels, and report back, so each of those crossings is
//! exercised.

use rackforge_plugin_sdk::portable::native::{NativeApiV1, region};
use rackforge_plugin_sdk::{
    BlockContext, PARALLEL_ABI_VERSION_V1, ParallelProcessor, PlanWriter, STATUS_INVALID_ARGUMENT,
    STATUS_INVALID_STATE, STATUS_OK, UnitContext, UnitMix,
};
use std::ffi::c_void;

const UNITS: usize = 3;
const FRAMES: i32 = 32;

#[derive(Default)]
struct Voices {
    next: u32,
    blocks: u32,
}

#[derive(Default)]
struct Voice {
    phase: f32,
    step: f32,
}

impl ParallelProcessor for Voices {
    type Unit = Voice;
    /// Wider than the instrument's two channels.
    const UNIT_CHANNELS: u32 = 3;
    const REPORT_BYTES: u32 = 4;

    fn set_parameter(&mut self, _index: u32, _value: f64) -> bool {
        false
    }

    /// Each note-on starts the next voice, its key and velocity the payload;
    /// the block count is the shared payload.
    fn begin_block(&mut self, context: &BlockContext<'_>, plan: &mut PlanWriter<'_>) {
        self.blocks += 1;
        plan.shared_buffer()[..4].copy_from_slice(&self.blocks.to_le_bytes());
        assert!(plan.commit_shared(4));
        // The plan is in ascending unit order: payloads first, then every
        // voice activated in turn, a new note's with its key and velocity.
        let mut payloads = [None; UNITS];
        for event in context
            .midi
            .iter()
            .filter(|event| event.data[0] & 0xf0 == 0x90)
        {
            let unit = (self.next as usize) % UNITS;
            self.next += 1;
            payloads[unit] = Some([event.data[1], event.data[2]]);
        }
        for (unit, payload) in payloads.iter().enumerate() {
            let payload: &[u8] = match payload {
                Some(payload) => payload,
                None => &[],
            };
            assert!(plan.activate(unit as u32, payload));
        }
    }

    fn render_unit(
        unit_index: u32,
        unit: &mut Self::Unit,
        payload: &[u8],
        context: &UnitContext<'_>,
        output: &mut [f32],
        report: &mut [u8],
    ) {
        if let [key, velocity] = payload {
            unit.step = f32::from(*key) / 997.0;
            unit.phase = f32::from(*velocity) / 127.0;
        }
        let blocks = u32::from_le_bytes(context.shared[..4].try_into().unwrap()) as f32;
        for frame in output.as_chunks_mut::<3>().0 {
            unit.phase = (unit.phase + unit.step).fract();
            frame[0] = unit.phase;
            frame[1] = unit.phase * 0.5 + unit_index as f32;
            frame[2] = blocks;
        }
        report.copy_from_slice(&unit.phase.to_le_bytes());
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
            let reported = f32::from_le_bytes(mix.report(unit)[..4].try_into().unwrap());
            for (frame, wide) in output
                .as_chunks_mut::<2>()
                .0
                .iter_mut()
                .zip(mix.slot(unit).as_chunks::<3>().0)
            {
                frame[0] += wide[0] * 0.3 + wide[2];
                frame[1] += wide[1] * 0.2 + reported;
            }
        }
    }
}

rackforge_plugin_sdk::export_parallel_processor!(
    Voices,
    max_units = UNITS,
    dispatch_stride = 8,
    shared_capacity = 16,
    max_frames = 64,
    max_input_channels = 0,
    max_output_channels = 2,
    max_midi_events = 4,
    max_parameter_events = 4,
    max_transfer_bytes = 16
);

unsafe extern "C" {
    fn rackforge_portable_native_entry_v1() -> *const NativeApiV1;
}

fn api() -> &'static NativeApiV1 {
    unsafe { &*rackforge_portable_native_entry_v1() }
}

struct Native(*mut c_void);

impl Native {
    fn prepared() -> Self {
        let instance = unsafe { (api().create)() };
        assert_eq!(unsafe { (api().initialize)(instance) }, STATUS_OK);
        assert_eq!(
            unsafe { (api().prepare)(instance, 48_000.0, 64, 0, 2) },
            STATUS_OK
        );
        Self(instance)
    }

    fn region<T>(&mut self, which: u32) -> &mut [T] {
        let mut capacity = 0;
        let at = unsafe { (api().region)(self.0, which, &mut capacity) };
        assert!(!at.is_null(), "region {which}");
        unsafe { std::slice::from_raw_parts_mut(at.cast::<T>(), capacity) }
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        unsafe { (api().destroy)(self.0) };
    }
}

fn note(frame: u32, key: u8, velocity: u8) -> u64 {
    u64::from(frame) | 0x90 << 32 | u64::from(key) << 40 | u64::from(velocity) << 48 | 3 << 56
}

/// Each block's notes: two, one, none.
fn notes(block: usize) -> Vec<u64> {
    match block {
        0 => vec![note(3, 60, 100), note(9, 67, 80)],
        1 => vec![note(0, 72, 127)],
        _ => Vec::new(),
    }
}

fn bits(samples: &[f32]) -> Vec<u32> {
    samples.iter().map(|sample| sample.to_bits()).collect()
}

/// One test, because the component's statics are process-wide state.
#[test]
fn the_parallel_stages_render_what_the_component_renders() {
    let table = api();
    let parallel = table
        .parallel
        .expect("a parallel processor exports the parallel table");
    assert_eq!(parallel.parallel_abi_version, PARALLEL_ABI_VERSION_V1);
    assert_eq!(parallel.max_units as usize, UNITS);
    assert_eq!(parallel.dispatch_stride, 8);
    assert_eq!(parallel.unit_channels, 3);
    assert_eq!(parallel.report_stride, 4);
    assert_eq!(parallel.shared_capacity, 16);
    assert_eq!(parallel.mix_slot_samples, 128);
    assert!(parallel.begin_block_v2.is_none());

    // The component, through its statics.
    assert_eq!(rackforge_initialize(), STATUS_OK);
    assert_eq!(rackforge_prepare(48_000.0, 64, 0, 2), STATUS_OK);
    let mut component = Vec::new();
    for block in 0..4 {
        let events = notes(block);
        unsafe {
            (&mut *core::ptr::addr_of_mut!(RF_MIDI))[..events.len()].copy_from_slice(&events);
        }
        assert_eq!(
            rackforge_process(FRAMES, 0, 2, events.len() as i32, 0),
            STATUS_OK
        );
        component.extend(bits(unsafe { &(&*core::ptr::addr_of!(RF_OUTPUT))[..64] }));
    }

    // The native table's composed `process`.
    let mut sequential = Native::prepared();
    let mut composed = Vec::new();
    for block in 0..4 {
        let events = notes(block);
        sequential.region::<u64>(region::MIDI)[..events.len()].copy_from_slice(&events);
        let status = unsafe { (table.process)(sequential.0, FRAMES, 0, 2, events.len() as i32, 0) };
        assert_eq!(status, STATUS_OK);
        composed.extend(bits(&sequential.region::<f32>(region::OUTPUT)[..64]));
    }
    assert_eq!(composed, component);

    // The stages, as a scheduling host runs them across two instances.
    let mut coordinator = Native::prepared();
    let mut worker = Native::prepared();
    let stride = parallel.dispatch_stride as usize;
    let slot_samples = parallel.mix_slot_samples as usize;
    let unit_samples = FRAMES as usize * parallel.unit_channels as usize;
    let mut staged = Vec::new();
    for block in 0..4 {
        let events = notes(block);
        coordinator.region::<u64>(region::MIDI)[..events.len()].copy_from_slice(&events);
        let active =
            unsafe { (parallel.begin_block)(coordinator.0, FRAMES, 0, 2, events.len() as i32, 0) };
        assert_eq!(active, UNITS as i32);
        let plan = coordinator.region::<u32>(region::PLAN)[..2 + 2 * UNITS].to_vec();
        let shared_bytes = plan[0] as usize;
        let shared = coordinator.region::<u8>(region::SHARED)[..shared_bytes].to_vec();
        worker.region::<u8>(region::SHARED)[..shared_bytes].copy_from_slice(&shared);
        for entry in plan[2..].as_chunks::<2>().0 {
            let (unit, payload_bytes) = (entry[0] as usize, entry[1] as usize);
            let payload = coordinator.region::<u8>(region::DISPATCH)[unit * stride..]
                [..payload_bytes]
                .to_vec();
            worker.region::<u8>(region::DISPATCH)[unit * stride..][..payload_bytes]
                .copy_from_slice(&payload);
            let status = unsafe {
                (parallel.render_unit)(
                    worker.0,
                    unit as i32,
                    payload_bytes as i32,
                    shared_bytes as i32,
                    FRAMES,
                    2,
                )
            };
            assert_eq!(status, STATUS_OK);
            let audio = worker.region::<f32>(region::OUTPUT)[..unit_samples].to_vec();
            coordinator.region::<f32>(region::MIX)[unit * slot_samples..][..unit_samples]
                .copy_from_slice(&audio);
            let report = worker.region::<u8>(region::REPORTS)[unit * 4..][..4].to_vec();
            coordinator.region::<u8>(region::REPORTS)[unit * 4..][..4].copy_from_slice(&report);
        }
        assert_eq!(
            unsafe { (parallel.end_block)(coordinator.0, FRAMES, 2) },
            STATUS_OK
        );
        staged.extend(bits(&coordinator.region::<f32>(region::OUTPUT)[..64]));
    }
    assert_eq!(staged, component);

    // The stages refuse what the component's exports refuse.
    let fresh = unsafe { (table.create)() };
    assert_eq!(
        unsafe { (parallel.end_block)(fresh, FRAMES, 2) },
        STATUS_INVALID_STATE
    );
    unsafe { (table.destroy)(fresh) };
    for (unit, payload, shared, frames) in [
        (3, 0, 0, FRAMES),
        (0, 9, 0, FRAMES),
        (0, 0, 17, FRAMES),
        (0, 0, 0, 0),
        (0, 0, 0, 65),
    ] {
        assert_eq!(
            unsafe { (parallel.render_unit)(worker.0, unit, payload, shared, frames, 2) },
            STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            rackforge_parallel_render_unit(unit, payload, shared, frames, 2),
            STATUS_INVALID_ARGUMENT
        );
    }
    // Units three channels wide at 64 frames would overrun their region.
    assert_eq!(
        unsafe { (parallel.render_unit)(worker.0, 0, 0, 0, 64, 2) },
        STATUS_INVALID_ARGUMENT
    );
    // An unknown region is absent, and a parallel one present.
    let mut capacity = 9;
    assert!(unsafe { (table.region)(worker.0, 99, &mut capacity) }.is_null());
    assert_eq!(capacity, 0);
    assert_eq!(worker.region::<u8>(region::DISPATCH).len(), UNITS * stride);
}
