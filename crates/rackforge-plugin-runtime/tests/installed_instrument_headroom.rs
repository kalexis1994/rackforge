//! What the installed instruments actually peak at.
//!
//! The desktop multiplies every rendered sample by its output gain and then
//! hard-clamps to full scale, so what a plugin sends decides whether the host
//! flat-tops it. The Concert Grand's own suite measures this for the piano;
//! the pinned instruments are packages rather than crates, so they need
//! loading from the store the way the host loads them.
//!
//! Not pass/fail: run it to see the gain staging.
//!
//!     cargo test -p rackforge-plugin-runtime --test installed_instrument_headroom -- --ignored --nocapture

use std::path::PathBuf;

use rackforge_plugin_runtime::MidiEvent;
use rackforge_plugin_runtime::{PortableEngine, RuntimeLimits};

/// Every version of every instrument the store holds, newest last.
fn installed(id: &str) -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var("LOCALAPPDATA").ok()?)
        .join("RackForge/plugin-store/packages")
        .join(id);
    let mut versions: Vec<_> = std::fs::read_dir(root)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.join("component.wasm").is_file())
        .collect();
    // Lexical order is wrong for versions ("0.9" after "0.10"), so take the
    // one the store touched last instead of guessing at the numbering.
    versions.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
    });
    versions.pop().map(|path| path.join("component.wasm"))
}

fn note_on(note: u8, velocity: u8) -> MidiEvent {
    MidiEvent {
        frame: 0,
        data: [0x90, note, velocity],
        length: 3,
    }
}

#[test]
#[ignore]
fn report_installed_instrument_peaks() {
    const RATE: f64 = 48_000.0;
    const FRAMES: u32 = 512;
    // The desktop's default: +6 dB, then a brick wall at full scale.
    let ceiling = 1.0f32 / 10.0f32.powf(6.0 / 20.0);

    for id in [
        "org.rackforge.concert-grand",
        "org.rackforge.rf-106",
        "org.rackforge.rf-5",
    ] {
        let Some(path) = installed(id) else {
            eprintln!("{id}: not installed, skipping");
            continue;
        };
        let runtime = PortableEngine::new(RuntimeLimits::default()).expect("runtime");
        let module = runtime
            .compile(&std::fs::read(&path).expect("read wasm"))
            .expect("compile");
        let mut instance = module.instantiate().expect("instantiate");
        instance.prepare(RATE, FRAMES, 0, 2).expect("prepare");

        for (label, notes) in [
            ("one note ff", vec![60u8]),
            ("five-note chord ff", vec![28, 35, 40, 44, 47]),
            (
                "ten-note chord ff",
                vec![28, 33, 40, 45, 47, 52, 57, 59, 64, 69],
            ),
        ] {
            let mut instance = module.instantiate().expect("instantiate");
            instance.prepare(RATE, FRAMES, 0, 2).expect("prepare");
            let events: Vec<MidiEvent> = notes.iter().map(|n| note_on(*n, 127)).collect();
            let mut peak = 0.0f32;
            let mut over = 0usize;
            let mut total = 0usize;
            // 0.6 s, the same window the piano's own headroom survey uses.
            let blocks = (RATE * 0.6 / f64::from(FRAMES)) as usize;
            for block in 0..blocks {
                let mut output = vec![0.0f32; FRAMES as usize * 2];
                let midi: &[MidiEvent] = if block == 0 { &events } else { &[] };
                instance
                    .process_interleaved_with_midi(&[], &mut output, FRAMES, midi)
                    .expect("render");
                for sample in &output {
                    peak = peak.max(sample.abs());
                    if sample.abs() > ceiling {
                        over += 1;
                    }
                    total += 1;
                }
            }
            println!(
                "{id:>28} {label:>20}: peak {peak:.3}  clipped {:.2}% at the desktop's default",
                100.0 * over as f32 / total as f32
            );
        }
    }
}
