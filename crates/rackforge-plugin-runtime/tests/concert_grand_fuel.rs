//! Measures what the packaged Concert Grand actually spends per audio call.
//!
//! The host gives a real-time call a fixed fuel budget, and a plugin that
//! exceeds it has its block aborted — which takes the audio stream down. That
//! failure is invisible from a native benchmark, because native code and wasm
//! fuel are different currencies. This measures the currency the host bills in.
//!
//! Run with the wasm built:
//!
//!     cargo build --release --target wasm32-unknown-unknown -p rackforge-concert-grand
//!     cargo test -p rackforge-plugin-runtime --test concert_grand_fuel -- --nocapture

use std::path::PathBuf;

use rackforge_plugin_runtime::MidiEvent;
use rackforge_plugin_runtime::{PortableEngine, RuntimeLimits};

fn wasm_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-unknown-unknown/release/rackforge_concert_grand.wasm")
}

#[test]
#[ignore]
fn report_fuel_per_block() {
    let path = wasm_path();
    if !path.is_file() {
        eprintln!("build the wasm first: {}", path.display());
        return;
    }
    let mut limits = RuntimeLimits::default();
    let budget = limits.fuel_per_call;
    // Measure what the instrument wants, not what today's ceiling allows.
    limits.fuel_per_call = 20_000_000_000;
    let runtime = PortableEngine::new(limits).expect("runtime");
    let module = runtime
        .compile(&std::fs::read(&path).expect("read wasm"))
        .expect("load");

    const FRAMES: u32 = 512;
    let mut output = vec![0.0f32; FRAMES as usize * 2];

    for (label, notes, pedal) in [
        ("idle", 0usize, false),
        ("single note", 1, false),
        ("ten-note chord", 10, false),
        ("twenty notes, pedal down", 20, true),
        ("treble note", 1, false),
    ] {
        let mut instance = module.instantiate().expect("instantiate");
        instance.prepare(48_000.0, FRAMES, 0, 2).expect("prepare");
        if pedal {
            let cc = [MidiEvent {
                frame: 0,
                data: [0xb0, 64, 127],
                length: 3,
            }];
            instance
                .process_interleaved_with_midi(&[], &mut output, FRAMES, &cc)
                .expect("pedal");
        }
        // Strike them a few notes at a time, the way a player would.
        for chunk in (0..notes).collect::<Vec<_>>().chunks(3) {
            let events: Vec<MidiEvent> = chunk
                .iter()
                .map(|i| MidiEvent {
                    frame: 0,
                    data: [0x90, 33 + 3 * *i as u8, 115],
                    length: 3,
                })
                .collect();
            instance
                .process_interleaved_with_midi(&[], &mut output, FRAMES, &events)
                .expect("strike");
        }
        // Steady state: what every block costs while the notes ring.
        let mut peak = 0u64;
        for _ in 0..20 {
            instance
                .process_interleaved_with_midi(&[], &mut output, FRAMES, &[])
                .expect("render");
            peak = peak.max(instance.last_realtime_fuel_consumed());
        }
        println!(
            "{label}: {peak} fuel ({:.0}% of the {budget} budget)",
            peak as f64 / budget as f64 * 100.0
        );
    }
}
