//! What running as portable WebAssembly costs a plugin, against the same
//! code compiled natively -- taken apart layer by layer.
//!
//! ```text
//! cargo build --release --target wasm32-unknown-unknown -p rackforge-concert-grand
//! cargo run --release -p rackforge-concert-grand --example wasm-tax -- <package-directory>
//! ```
//!
//! The package directory is the Concert Grand's `package/` with the freshly
//! built `component.wasm` beside its manifest. One score is played through
//! each path on this thread, one instance, one block at a time:
//!
//! ```text
//! native           the instrument's Rust compiled by LLVM for this machine,
//!                  called directly: the 100 the others are measured against
//! wasm, cranelift  the component in wasmtime with nothing metered: the
//!                  price of the compiler and the sandbox alone
//! + epoch          the interruption check RackForge compiles into plugins
//!                  that do not spend a budget
//! + fuel           the counter RackForge compiles into plugins that do
//!                  (the Concert Grand is one of them)
//! rackforge host   the host's own path, `LoadedPlugin`/`PluginInstance`,
//!                  with its event translation and buffer copies
//! inlined ...      the same with Cranelift's cross-function inlining on
//! ```
//!
//! Every path must produce the same audio -- the bench checks it -- so what
//! differs is how the same arithmetic is executed, never how much of it.
//!
//! First reading, 2026-09-23, x86_64, 128 frames: native 100, Cranelift 74,
//! with fuel 67, the host's path 66 (its copies cost nothing measurable),
//! Cranelift's inlining 0. On the plugin's side, measured by rebuilding the
//! component: without `+simd128` 58 (vectorisable DSP is worth sixteen
//! points), with fat LTO and one codegen unit 62 (worse, not better), and
//! through `wasm-opt -O3` 79 unmetered and 72 metered. `-O4` and
//! `--converge` add nothing over `-O3`.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use rackforge_concert_grand::ConcertGrand;
use rackforge_core::{LoadedPlugin, PluginPackage};
use rackforge_plugin_api::abi::MidiEventV1;
use rackforge_plugin_sdk::MidiEvent;
use wasmtime::{Config, Engine, Inlining, Instance, Module, OptLevel, Store, TypedFunc};

const SAMPLE_RATE: f64 = 48_000.0;
const FRAMES: u32 = 128;
const CHANNELS: u32 = 2;
/// Eight seconds of playing: long enough for the tails of every chord to be
/// in the measurement, not only the attacks.
const BLOCKS: usize = 3_000;
const ROUNDS: usize = 3;

/// A chord every 250 blocks (two thirds of a second), moving up and down the
/// keyboard so low, middle and high strings all take part.
fn score() -> Vec<Vec<(u8, u8, u8)>> {
    let chords: [&[u8]; 6] = [
        &[36, 43, 48, 52, 55],
        &[60, 64, 67, 72],
        &[41, 48, 53, 57, 60],
        &[72, 76, 79, 84],
        &[29, 36, 45, 48, 52],
        &[65, 69, 72, 77, 81],
    ];
    let mut blocks = vec![Vec::new(); BLOCKS];
    let mut held: Vec<u8> = Vec::new();
    for (index, block) in (0..BLOCKS).step_by(250).enumerate() {
        for note in held.drain(..) {
            blocks[block].push((0x80, note, 0));
        }
        for &note in chords[index % chords.len()] {
            blocks[block].push((0x90, note, 90 + (note % 30)));
            held.push(note);
        }
    }
    blocks
}

/// A named way of playing the score.
type BenchPath<'a> = (&'static str, Box<dyn Fn() -> Timing + 'a>);

struct Timing {
    total_ns: u128,
    per_block_ns: Vec<u64>,
    fingerprint: u64,
}

fn fingerprint(fingerprint: &mut u64, output: &[f32]) {
    for sample in output {
        *fingerprint = fingerprint.rotate_left(5) ^ u64::from(sample.to_bits());
        *fingerprint = fingerprint.wrapping_mul(0x100_0000_01b3);
    }
}

fn native(score: &[Vec<(u8, u8, u8)>]) -> Timing {
    let mut piano = Box::new(ConcertGrand::default());
    assert!(piano.prepare(SAMPLE_RATE, FRAMES, 0, CHANNELS));
    let mut output = vec![0.0_f32; (FRAMES * CHANNELS) as usize];
    let mut events = Vec::new();
    let mut per_block_ns = Vec::with_capacity(BLOCKS);
    let mut print = 0xcbf2_9ce4_8422_2325_u64;
    let started = Instant::now();
    for block in score {
        events.clear();
        events.extend(block.iter().map(|&(status, note, velocity)| MidiEvent {
            frame: 0,
            data: [status, note, velocity],
            length: 3,
        }));
        let at = Instant::now();
        piano.process(&[], &mut output, &events, &[], FRAMES, 0, CHANNELS);
        per_block_ns.push(at.elapsed().as_nanos() as u64);
        fingerprint(&mut print, &output);
    }
    Timing {
        total_ns: started.elapsed().as_nanos(),
        per_block_ns,
        fingerprint: print,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Metering {
    None,
    Epoch,
    Fuel,
}

fn raw_wasm(
    component: &[u8],
    metering: Metering,
    inlining: bool,
    score: &[Vec<(u8, u8, u8)>],
) -> Timing {
    let mut config = Config::new();
    // Exactly the compiler settings RackForge's PortableEngine uses.
    config.cranelift_opt_level(OptLevel::Speed);
    config.wasm_multi_memory(false);
    config.wasm_memory64(false);
    config.consume_fuel(metering == Metering::Fuel);
    config.epoch_interruption(metering == Metering::Epoch);
    // Cranelift does not inline across functions unless asked: a lever on
    // the host's side, left off by RackForge today.
    config.compiler_inlining(if inlining {
        Inlining::Yes
    } else {
        Inlining::No
    });
    let engine = Engine::new(&config).expect("engine");
    let module = Module::new(&engine, component).expect("module");
    let mut store = Store::new(&engine, ());
    if metering == Metering::Fuel {
        store.set_fuel(u64::MAX).expect("fuel");
    }
    if metering == Metering::Epoch {
        store.set_epoch_deadline(u64::MAX);
    }
    let instance = Instance::new(&mut store, &module, &[]).expect("instance");
    let call0 = |store: &mut Store<()>, name: &str| -> i32 {
        instance
            .get_typed_func::<(), i32>(&mut *store, name)
            .unwrap_or_else(|_| panic!("export {name}"))
            .call(&mut *store, ())
            .expect(name)
    };
    assert_eq!(call0(&mut store, "rackforge_initialize"), 0);
    let prepare: TypedFunc<(f64, i32, i32, i32), i32> = instance
        .get_typed_func(&mut store, "rackforge_prepare")
        .expect("prepare");
    assert_eq!(
        prepare
            .call(&mut store, (SAMPLE_RATE, FRAMES as i32, 0, CHANNELS as i32))
            .unwrap(),
        0
    );
    let midi_at = call0(&mut store, "rackforge_midi_ptr") as usize;
    let output_at = call0(&mut store, "rackforge_output_ptr") as usize;
    let process: TypedFunc<(i32, i32, i32, i32, i32), i32> = instance
        .get_typed_func(&mut store, "rackforge_process")
        .expect("process");
    let memory = instance.get_memory(&mut store, "memory").expect("memory");
    let samples = (FRAMES * CHANNELS) as usize;
    let mut output = vec![0.0_f32; samples];
    let mut per_block_ns = Vec::with_capacity(BLOCKS);
    let mut print = 0xcbf2_9ce4_8422_2325_u64;
    let started = Instant::now();
    for block in score {
        let at = Instant::now();
        {
            let data = memory.data_mut(&mut store);
            for (index, &(status, note, velocity)) in block.iter().enumerate() {
                let packed = u64::from(status) << 32
                    | u64::from(note) << 40
                    | u64::from(velocity) << 48
                    | 3_u64 << 56;
                let at = midi_at + index * 8;
                data[at..at + 8].copy_from_slice(&packed.to_le_bytes());
            }
        }
        let status = process
            .call(
                &mut store,
                (FRAMES as i32, 0, CHANNELS as i32, block.len() as i32, 0),
            )
            .expect("process");
        assert_eq!(status, 0);
        {
            let data = memory.data(&store);
            for (index, sample) in output.iter_mut().enumerate() {
                let at = output_at + index * 4;
                *sample = f32::from_le_bytes(data[at..at + 4].try_into().unwrap());
            }
        }
        per_block_ns.push(at.elapsed().as_nanos() as u64);
        fingerprint(&mut print, &output);
    }
    Timing {
        total_ns: started.elapsed().as_nanos(),
        per_block_ns,
        fingerprint: print,
    }
}

fn rackforge(plugin: &LoadedPlugin, score: &[Vec<(u8, u8, u8)>]) -> Timing {
    let mut instance = plugin.create_instance().expect("instance");
    instance
        .activate(SAMPLE_RATE, FRAMES, 0, CHANNELS)
        .expect("activate");
    let mut output = vec![0.0_f32; (FRAMES * CHANNELS) as usize];
    let mut events = Vec::new();
    let mut per_block_ns = Vec::with_capacity(BLOCKS);
    let mut print = 0xcbf2_9ce4_8422_2325_u64;
    let started = Instant::now();
    for block in score {
        events.clear();
        events.extend(block.iter().map(|&(status, note, velocity)| MidiEventV1 {
            frame: 0,
            length: 3,
            data: [status, note, velocity],
        }));
        let at = Instant::now();
        instance
            .process_interleaved(&[], &mut output, FRAMES, 0, CHANNELS, &events, &[])
            .expect("process");
        per_block_ns.push(at.elapsed().as_nanos() as u64);
        fingerprint(&mut print, &output);
    }
    Timing {
        total_ns: started.elapsed().as_nanos(),
        per_block_ns,
        fingerprint: print,
    }
}

fn percentile(samples: &[u64], p: f64) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[((sorted.len() as f64 * p) as usize).min(sorted.len() - 1)] as f64 / 1000.0
}

fn main() {
    // The instrument is large enough to overflow the main thread's stack
    // while it is built; the lab runs it on a thread of its own for the same
    // reason.
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(run)
        .expect("bench thread")
        .join()
        .expect("bench");
}

fn run() {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: wasm-tax <package-directory>"),
    );
    let component = std::fs::read(root.join("component.wasm")).expect("component.wasm");
    let package = PluginPackage::open(&root).expect("package");
    // SAFETY: a portable wasm-v1 package executes inside the sandbox.
    // A data root of its own, as a host has, so the host's path compiles
    // what a host would: the component as binaryen leaves it, cached there.
    let data_root = std::env::temp_dir().join(format!("rackforge-wasm-tax-{}", std::process::id()));
    let loaded = unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), Some(&data_root)) }
        .expect("load");
    let score = score();
    let deadline = f64::from(FRAMES) / SAMPLE_RATE * 1e6;

    let paths: [BenchPath<'_>; 8] = [
        ("native (LLVM)", Box::new(|| native(&score))),
        (
            "wasm, cranelift",
            Box::new(|| raw_wasm(&component, Metering::None, false, &score)),
        ),
        (
            "wasm + epoch",
            Box::new(|| raw_wasm(&component, Metering::Epoch, false, &score)),
        ),
        (
            "wasm + fuel",
            Box::new(|| raw_wasm(&component, Metering::Fuel, false, &score)),
        ),
        ("rackforge host", Box::new(|| rackforge(&loaded, &score))),
        (
            "inlined",
            Box::new(|| raw_wasm(&component, Metering::None, true, &score)),
        ),
        (
            "inlined + epoch",
            Box::new(|| raw_wasm(&component, Metering::Epoch, true, &score)),
        ),
        (
            "inlined + fuel",
            Box::new(|| raw_wasm(&component, Metering::Fuel, true, &score)),
        ),
    ];
    // Best of several rounds, interleaved, so no path is favoured by a warm
    // cache or a quiet moment on the machine.
    let mut best: Vec<Option<Timing>> = (0..paths.len()).map(|_| None).collect();
    for _ in 0..ROUNDS {
        for (index, (_, run)) in paths.iter().enumerate() {
            let timing = run();
            if best[index]
                .as_ref()
                .is_none_or(|kept| timing.total_ns < kept.total_ns)
            {
                best[index] = Some(timing);
            }
        }
    }
    let best: Vec<Timing> = best.into_iter().map(Option::unwrap).collect();
    let native_ns = best[0].total_ns as f64;
    println!(
        "{BLOCKS} blocks of {FRAMES} frames at {SAMPLE_RATE} Hz, deadline {deadline:.0} us, best of {ROUNDS}\n"
    );
    println!(
        "{:<18} {:>10} {:>10} {:>10} {:>9} {:>8}  audio",
        "path", "mean us", "p99 us", "% of dl", "speed", "cost"
    );
    for ((name, _), timing) in paths.iter().zip(&best) {
        let mean = timing.total_ns as f64 / BLOCKS as f64 / 1000.0;
        let speed = native_ns / timing.total_ns as f64 * 100.0;
        println!(
            "{:<18} {:>10.1} {:>10.1} {:>9.0}% {:>8.0}% {:>7.0}%  {}",
            name,
            mean,
            percentile(&timing.per_block_ns, 0.99),
            mean / deadline * 100.0,
            speed,
            (timing.total_ns as f64 / native_ns - 1.0) * 100.0,
            if timing.fingerprint == best[0].fingerprint {
                "same"
            } else {
                "DIFFERS"
            },
        );
    }
    println!("\nspeed: native = 100. cost: extra time over native.");
}
