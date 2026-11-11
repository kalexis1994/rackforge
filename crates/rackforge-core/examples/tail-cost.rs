//! What a plugin's block costs AFTER the keys are released, second by
//! second, while its tail decays towards silence.
//!
//! ```text
//! cargo run --release -p rackforge-core --example tail-cost -- <package-directory> [--program ID] [--ftz] [--effect]
//! ```
//!
//! A decaying recursive filter heads for zero exponentially and never gets
//! there. On its way it crosses into the subnormal range, where x86 does
//! each float operation in microcode, tens of times slower. Nothing in the
//! audio says so -- the samples are far below anything audible -- but the
//! block time climbs while nobody is playing and falls the moment someone
//! does, because a fresh note puts the state back in the normal range.
//! `--ftz` does to this thread what `realtime::engage` does to every DSP
//! thread of the host, so the run without it is the plugin as it behaved
//! before the host set the flags, and the run with it is what the host
//! renders now. `--effect` feeds a second of noise instead of a chord.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use rackforge_core::{LoadedPlugin, PluginPackage};
use rackforge_plugin_api::abi::MidiEventV1;

const SAMPLE_RATE: f64 = 48_000.0;
const FRAMES: u32 = 128;
const CHANNELS: u32 = 2;
const BLOCKS_PER_SECOND: usize = (SAMPLE_RATE as usize) / (FRAMES as usize);
const HELD_SECONDS: usize = 1;
const TAIL_SECONDS: usize = 90;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let Some(root) = arguments.next() else {
        eprintln!("uso: tail-cost <directorio-del-paquete> [--program ID] [--ftz] [--effect]");
        std::process::exit(2);
    };
    let mut program: Option<String> = None;
    let mut ftz = false;
    let mut effect = false;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--program" => program = arguments.next(),
            "--ftz" => ftz = true,
            "--effect" => effect = true,
            other => panic!("argumento desconocido: {other}"),
        }
    }
    if ftz {
        rackforge_core::realtime::flush_subnormals();
    }

    let input_channels = if effect { CHANNELS } else { 0 };
    let package = PluginPackage::open(PathBuf::from(&root)).expect("paquete");
    // SAFETY: portable wasm-v1 packages execute inside the sandbox.
    let plugin =
        unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), None) }.expect("plugin");
    let mut instance = plugin.create_instance().expect("instance");
    instance
        .activate(SAMPLE_RATE, FRAMES, input_channels, CHANNELS)
        .expect("activate");
    if let Some(program) = &program {
        instance.load_preset(program).expect("programa");
    }

    let notes = [48_u8, 55, 60, 64, 67];
    let message = |status: u8, velocity: u8| -> Vec<MidiEventV1> {
        notes
            .iter()
            .map(|note| MidiEventV1 {
                frame: 0,
                length: 3,
                data: [status, *note, velocity],
            })
            .collect()
    };
    // An instrument declares no audio input; an empty slice is what the
    // host passes it. An effect is fed a second of noise and then silence,
    // which is the same "stop playing" as releasing the keys.
    let mut input: Vec<f32> = if effect {
        vec![0.0; (FRAMES * CHANNELS) as usize]
    } else {
        Vec::new()
    };
    let mut noise_state = 0x2545_f491_u32;
    let mut output = vec![0.0_f32; (FRAMES * CHANNELS) as usize];
    let deadline = f64::from(FRAMES) / SAMPLE_RATE * 1_000_000.0;

    println!(
        "\n{root}{} -- bloque de {FRAMES} frames, deadline {deadline:.0} us, ftz={ftz}\n",
        program.map(|p| format!(" [{p}]")).unwrap_or_default()
    );
    println!("  seg   media us  max us  %deadline   pico salida   subnormales");
    let mut block = 0_usize;
    let total_blocks = (HELD_SECONDS + TAIL_SECONDS) * BLOCKS_PER_SECOND;
    let mut bucket_nanos = 0_u128;
    let mut bucket_max = 0_u128;
    let mut bucket_peak = 0.0_f32;
    let mut bucket_subnormal = 0_usize;
    while block < total_blocks {
        let events = if block == 0 {
            message(0x90, 100)
        } else if block == HELD_SECONDS * BLOCKS_PER_SECOND {
            message(0x80, 0)
        } else {
            Vec::new()
        };
        if effect {
            let playing = block < HELD_SECONDS * BLOCKS_PER_SECOND;
            for sample in &mut input {
                noise_state ^= noise_state << 13;
                noise_state ^= noise_state >> 17;
                noise_state ^= noise_state << 5;
                *sample = if playing {
                    (noise_state as f32 / u32::MAX as f32 - 0.5) * 0.5
                } else {
                    0.0
                };
            }
        }
        output.fill(0.0);
        let started = Instant::now();
        instance
            .process_interleaved(
                &input,
                &mut output,
                FRAMES,
                input_channels,
                CHANNELS,
                &events,
                &[],
            )
            .expect("process");
        let nanos = started.elapsed().as_nanos();
        bucket_nanos += nanos;
        bucket_max = bucket_max.max(nanos);
        for sample in &output {
            bucket_peak = bucket_peak.max(sample.abs());
            if sample.is_subnormal() {
                bucket_subnormal += 1;
            }
        }
        block += 1;
        if block.is_multiple_of(BLOCKS_PER_SECOND) {
            let second = block / BLOCKS_PER_SECOND;
            let mean = bucket_nanos as f64 / BLOCKS_PER_SECOND as f64 / 1000.0;
            let label = if second <= HELD_SECONDS {
                "tocando"
            } else {
                ""
            };
            println!(
                "  {second:3}  {mean:9.1} {:7.1}   {:6.1} %   {bucket_peak:11.3e}   {bucket_subnormal:9}  {label}",
                bucket_max as f64 / 1000.0,
                mean / deadline * 100.0,
            );
            bucket_nanos = 0;
            bucket_max = 0;
            bucket_peak = 0.0;
            bucket_subnormal = 0;
        }
    }
}
