//! The packaged Concert Grand, rendered across four worker instances,
//! against the same component rendering the same notes in one.
//!
//! This is the test that was missing, and two bugs went through the gap it
//! left. The plugin's own equivalence test drives the three phases by hand
//! inside ONE instance and passes bit for bit; the host's parallel tests use
//! a hand-written WAT fixture. Neither exercises the thing that actually
//! ships: a real instrument whose per-unit state lives in four separate wasm
//! instances, with every byte it needs carried there and back by the host.
//!
//! What went through the gap: the host sized each unit's slot by the
//! instrument's two output channels rather than the twenty floats a string
//! section writes, so every unit was refused, counted as failed and filled
//! with silence -- an instrument whose strings had all stopped while its
//! body still rang. That was found by ear, on the appliance. And then
//! something else, still unnamed when this test was written: the same
//! component sounds right in one instance and robotic in four.
//!
//! Run with the wasm built:
//!
//!     cargo build --release --target wasm32-unknown-unknown -p rackforge-concert-grand
//!     cargo test -p rackforge-plugin-runtime --test concert_grand_across_instances -- --nocapture

use std::path::PathBuf;

use rackforge_plugin_runtime::{MidiEvent, ParallelPlanEntry, PortableEngine, RuntimeLimits};

fn wasm_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-unknown-unknown/release/rackforge_concert_grand.wasm")
}

const FRAMES: u32 = 128;
const BLOCKS: usize = 200;

/// A pedalled passage: strikes, releases, a moving sustain pedal and the
/// sostenuto rod, which is what the per-voice work across the boundary is
/// made of.
fn script(block: usize) -> Vec<MidiEvent> {
    let mut midi = Vec::new();
    if block.is_multiple_of(3) {
        midi.push(MidiEvent {
            frame: 0,
            data: [0xB0, 64, ((block * 37) % 128) as u8],
            length: 3,
        });
        midi.push(MidiEvent {
            frame: 0,
            data: [0x90, 28 + (block * 7 % 60) as u8, 92],
            length: 3,
        });
    }
    if block.is_multiple_of(7) {
        midi.push(MidiEvent {
            frame: 40,
            data: [0x80, 28 + (block.saturating_sub(21) * 7 % 60) as u8, 64],
            length: 3,
        });
    }
    if block.is_multiple_of(41) {
        midi.push(MidiEvent {
            frame: 64,
            data: [0xB0, 66, if block.is_multiple_of(82) { 127 } else { 0 }],
            length: 3,
        });
    }
    midi
}

#[test]
fn four_instances_render_what_one_renders() {
    let path = wasm_path();
    if !path.is_file() {
        eprintln!("build the wasm first: {}", path.display());
        return;
    }
    let runtime = PortableEngine::new(RuntimeLimits::default()).expect("runtime");
    let module = runtime
        .compile(&std::fs::read(&path).expect("read wasm"))
        .expect("load");

    let mut sequential = module.instantiate().expect("instantiate");
    sequential.prepare(48_000.0, FRAMES, 0, 2).expect("prepare");
    let mut coordinator = module.instantiate().expect("instantiate");
    coordinator
        .prepare(48_000.0, FRAMES, 0, 2)
        .expect("prepare");

    let layout = coordinator
        .parallel_layout()
        .expect("the packaged instrument must export parallel render");
    let units = layout.max_units;
    let width = if layout.unit_channels > 0 {
        layout.unit_channels
    } else {
        2
    };
    eprintln!(
        "unidades {units}, ancho {width} flotantes por cuadro, reporte {} bytes",
        layout.report_stride
    );

    let mut workers: Vec<_> = (0..units)
        .map(|_| {
            let mut worker = module.instantiate().expect("instantiate");
            worker.prepare(48_000.0, FRAMES, 0, 2).expect("prepare");
            worker
        })
        .collect();

    let mut one = vec![0.0f32; FRAMES as usize * 2];
    let mut four = vec![0.0f32; FRAMES as usize * 2];
    let mut payload = vec![0u8; layout.dispatch_stride];
    let mut shared = vec![0u8; layout.shared_capacity];
    let mut slot = vec![0.0f32; FRAMES as usize * width];
    let mut report = vec![0u8; layout.report_stride];

    for block in 0..BLOCKS {
        let midi = script(block);

        sequential
            .process_interleaved_with_midi(&[], &mut one, FRAMES, &midi)
            .expect("the one-instance render");

        let mut plan = [ParallelPlanEntry::default(); 8];
        let active = coordinator
            .parallel_begin_block(&[], FRAMES, &midi, &[], &[], &mut plan)
            .expect("begin_block");
        assert_eq!(
            active.active_units, units,
            "bloque {block}: el coordinador activo {} de {units} unidades",
            active.active_units
        );
        let shared_bytes = active.shared_bytes;
        coordinator
            .parallel_read_shared(&mut shared[..shared_bytes])
            .expect("read shared");

        for entry in &plan[..active.active_units] {
            let unit = entry.unit as usize;
            let bytes = entry.payload_bytes as usize;
            coordinator
                .parallel_read_dispatch(entry.unit, &mut payload[..bytes])
                .expect("read dispatch");
            let worker = &mut workers[unit];
            worker
                .parallel_write_shared(&shared[..shared_bytes])
                .expect("write shared");
            worker
                .parallel_write_dispatch(entry.unit, &payload[..bytes])
                .expect("write dispatch");
            worker
                .parallel_render_unit(entry.unit, bytes, shared_bytes, &[], &mut slot, FRAMES)
                .expect("render_unit");
            coordinator
                .parallel_write_mix_slot(entry.unit, &slot)
                .expect("write mix");
            if layout.report_stride > 0 {
                worker
                    .parallel_read_report(entry.unit, &mut report)
                    .expect("read report");
                coordinator
                    .parallel_write_report(entry.unit, &report)
                    .expect("write report");
            }
        }
        coordinator
            .parallel_end_block(&mut four, FRAMES)
            .expect("end_block");

        if let Some(at) = one
            .iter()
            .zip(four.iter())
            .position(|(a, b)| (a - b).abs() > 0.0)
        {
            panic!(
                "se separan en el bloque {block}, cuadro {}, canal {}: \
                 una instancia {} contra cuatro {}",
                at / 2,
                at % 2,
                one[at],
                four[at]
            );
        }
    }
}
