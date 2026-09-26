//! Where a parallel block's fixed cost actually goes.
//!
//! This was written to hunt a ~450 µs fixed overhead, inferred from an idle
//! instrument costing 607 µs a block in one instance and 1051 µs across
//! four. The hunt found no such thing. On the appliance, with 128-frame
//! blocks against a 2666 µs deadline:
//!
//! ```text
//!                                 idle      a held chord
//!   A  one instance, whole block  1039 us      1491 us
//!   B  same units, INLINE         1069 us      1602 us
//!   C  same units, real pool      1127 us      1388 us
//!   B-A  the transport costs        30 us       111 us
//!   C-B  the threads cost           58 us      -214 us
//! ```
//!
//! Thirty microseconds of transport and fifty-eight of threading, and once
//! there is real work the pool comes in 103 µs AHEAD of a single instance.
//! The 450 µs came from comparing two numbers measured in different states,
//! one of them from a period when units were being refused outright.
//!
//! What the split does show is the thing worth working on. An idle block
//! costs 1039 µs before a single note sounds, and `finish` is 926 µs of it.
//! The transport accounts for 30 of those, so about 900 µs -- a third of the
//! deadline -- is the global stage: the soundboard, the sympathetic bank and
//! the room, which run every block whether or not anything is ringing. That
//! is the serial floor Amdahl charges for, it is paid in both render paths,
//! and no amount of scheduling moves it.
//!
//! The three cases are the only split that distinguishes the candidates:
//!
//!   A  one instance renders the whole block          (no transport at all)
//!   B  begin, every unit run INLINE on this thread, finish
//!   C  begin, the units across the real worker pool, finish
//!
//! B - A is the transport: four extra wasm calls, the input written into
//! each worker, each unit's audio read back out and written into the
//! coordinator's mix region. C - B is the threading: publishing the block,
//! waking the workers and waiting for them.
//!
//! Numbers from a developer machine are not the answer -- x86 puts the
//! global stage at 9 % of the deadline and the appliance at 34 %. Run it
//! where the answer matters, against an installed package, with the
//! appliance stopped so its realtime threads are not competing:
//!
//! ```text
//! RACKFORGE_BENCH_PACKAGE=~/rackforge/plugin-store/packages/org.rackforge.concert-grand/0.171.34 //!   ./parallel_overhead --ignored --nocapture
//! ```
#![cfg(not(target_arch = "wasm32"))]

use rackforge_core::parallel_render::{
    ParallelUnits, RenderPool, RenderTelemetry, ScheduledSlot, UnitJob, process_slots_sequential,
};
use rackforge_core::{LoadedPlugin, PluginInstance, PluginPackage};
use rackforge_plugin_api::abi::{MidiEventV1, ParameterEventV1};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

/// The appliance's block: 128 frames at 48 kHz is the 2666 µs deadline every
/// number in this file is measured against.
const FRAMES: u32 = 128;
const CHANNELS: u32 = 2;
const SAMPLES: usize = FRAMES as usize * CHANNELS as usize;
/// Enough blocks that a stray scheduling hiccup cannot move the mean much.
const BLOCKS: usize = 2000;
/// Discarded before timing: instantiation, first-touch page faults and the
/// branch predictors all settle in the first few blocks.
const WARMUP: usize = 200;

struct Voice {
    instance: PluginInstance<'static>,
    parallel: Option<ParallelUnits<'static>>,
    input: Vec<f32>,
    output: Vec<f32>,
    events: Vec<MidiEventV1>,
    parameter_events: Vec<ParameterEventV1>,
}

impl Voice {
    fn create(plugin: &'static LoadedPlugin, with_units: bool) -> Self {
        let mut instance = plugin.create_instance().unwrap();
        instance.activate(48_000.0, FRAMES, 0, CHANNELS).unwrap();
        let parallel = if with_units {
            ParallelUnits::create(plugin, 48_000.0, FRAMES, 0, CHANNELS).unwrap()
        } else {
            None
        };
        Self {
            instance,
            parallel,
            input: Vec::new(),
            output: vec![0.0; SAMPLES],
            events: Vec::new(),
            parameter_events: Vec::new(),
        }
    }
}

// SAFETY: the coordinator and every unit instance run in the portable
// backend; unit jobs point at per-unit boxed cells owned by `ParallelUnits`.
unsafe impl ScheduledSlot for Voice {
    fn max_units(&self) -> u32 {
        self.parallel.as_ref().map_or(0, |units| units.max_units())
    }

    fn run_single(&mut self, frames: u32, channels: u32) -> bool {
        self.output.fill(0.0);
        self.instance
            .process_interleaved(
                &self.input,
                &mut self.output,
                frames,
                0,
                channels,
                &self.events,
                &self.parameter_events,
            )
            .is_ok()
    }

    fn run_begin(&mut self, frames: u32, _channels: u32) -> Option<u32> {
        let wide = self
            .events
            .iter()
            .map(rackforge_core::midi2::Midi2Event::from_midi1)
            .collect::<Vec<_>>();
        let parallel = self.parallel.as_mut()?;
        parallel
            .begin(
                &mut self.instance,
                &self.input,
                frames,
                &wide,
                &self.parameter_events,
            )
            .ok()
    }

    fn unit_job(&mut self, unit: u32, frames: u32, channels: u32) -> UnitJob {
        self.parallel
            .as_mut()
            .expect("unit job on a classic slot")
            .unit_job(unit, &self.input, frames, channels)
    }

    fn run_end(&mut self, frames: u32, channels: u32, completed: u32) -> bool {
        let Some(parallel) = self.parallel.as_mut() else {
            return false;
        };
        parallel
            .finish(
                &mut self.instance,
                &mut self.output,
                frames,
                channels,
                completed,
            )
            .is_ok()
    }

    fn quarantine(&mut self) {
        self.output.fill(0.0);
    }
}

/// An installed package when one is named, otherwise one assembled from the
/// working tree around a freshly built component.
fn plugin() -> &'static LoadedPlugin {
    let root = match std::env::var_os("RACKFORGE_BENCH_PACKAGE") {
        Some(path) => PathBuf::from(path),
        None => {
            let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .to_path_buf();
            let root = std::env::temp_dir().join("rackforge-overhead-bench");
            let _ = fs::remove_dir_all(&root);
            copy_tree(&workspace.join("plugins/concert-grand/package"), &root);
            fs::copy(
                workspace
                    .join("target/wasm32-unknown-unknown/release/rackforge_concert_grand.wasm"),
                root.join("component.wasm"),
            )
            .expect("build the component first: cargo build --release --target wasm32-unknown-unknown -p rackforge-concert-grand");
            root
        }
    };
    let package = PluginPackage::open(&root).unwrap();
    // SAFETY: portable wasm-v1 packages execute inside the sandbox.
    let loaded = unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), None) }.unwrap();
    Box::leak(Box::new(loaded))
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Mean nanoseconds per block, warm-up discarded.
fn mean_ns(samples: &[u64]) -> f64 {
    let kept = &samples[WARMUP.min(samples.len())..];
    kept.iter().sum::<u64>() as f64 / kept.len().max(1) as f64
}

fn percentile_ns(samples: &[u64], percentile: f64) -> u64 {
    let mut kept = samples[WARMUP.min(samples.len())..].to_vec();
    kept.sort_unstable();
    kept[((kept.len() as f64 * percentile) as usize).min(kept.len() - 1)]
}

/// A held chord, so the instrument has something to render in the busy case.
fn chord() -> Vec<MidiEventV1> {
    [52_u8, 56, 59, 64]
        .iter()
        .map(|note| MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0x90, *note, 90],
        })
        .collect()
}

#[test]
#[ignore = "a measurement, not an assertion"]
fn where_a_parallel_block_spends_its_time() {
    let plugin = plugin();
    let layout = plugin.parallel_layout().expect("a parallel instrument");
    let per_unit_bytes = FRAMES as usize * layout.unit_channels * size_of::<f32>();
    println!(
        "\n{} units, {} floats a frame each: {} KiB read out of the workers and \
         written into the coordinator every block, each way.",
        layout.max_units,
        layout.unit_channels,
        layout.max_units * per_unit_bytes / 1024
    );

    for (label, first_block) in [("en vacio", Vec::new()), ("con un acorde", chord())] {
        println!("\n=== {label} ===");

        // ---- A: one instance renders the whole block ---------------------
        let telemetry = RenderTelemetry::new(1);
        let mut voices = vec![Voice::create(plugin, false)];
        voices[0].events = first_block.clone();
        let mut single = Vec::with_capacity(BLOCKS);
        for _ in 0..BLOCKS {
            let started = Instant::now();
            process_slots_sequential(&mut voices, FRAMES, CHANNELS, &telemetry);
            single.push(started.elapsed().as_nanos() as u64);
            voices[0].events.clear();
        }

        // ---- B: the same units, inline, no threads -----------------------
        let mut voices = [Voice::create(plugin, true)];
        voices[0].events = first_block.clone();
        let (mut inline, mut begins, mut units_ran, mut finishes) = (
            Vec::with_capacity(BLOCKS),
            Vec::with_capacity(BLOCKS),
            Vec::with_capacity(BLOCKS),
            Vec::with_capacity(BLOCKS),
        );
        for _ in 0..BLOCKS {
            let started = Instant::now();
            let at_begin = Instant::now();
            let mask = voices[0].run_begin(FRAMES, CHANNELS).unwrap();
            begins.push(at_begin.elapsed().as_nanos() as u64);

            let at_units = Instant::now();
            let mut completed = 0_u32;
            let mut pending = mask;
            while pending != 0 {
                let unit = pending.trailing_zeros();
                pending &= !(1 << unit);
                let job = voices[0].unit_job(unit, FRAMES, CHANNELS);
                // SAFETY: the job was just published for this unit and this
                // thread is the only one touching it.
                if unsafe { (job.run)(job.context, job.unit, FRAMES, CHANNELS) } {
                    completed |= 1 << unit;
                }
            }
            units_ran.push(at_units.elapsed().as_nanos() as u64);

            let at_finish = Instant::now();
            assert!(voices[0].run_end(FRAMES, CHANNELS, completed));
            finishes.push(at_finish.elapsed().as_nanos() as u64);

            inline.push(started.elapsed().as_nanos() as u64);
            voices[0].events.clear();
        }

        // ---- C: the same units across the real pool ----------------------
        let telemetry = RenderTelemetry::new(3);
        let mut pool = RenderPool::with_workers(3, telemetry);
        assert!(
            pool.worker_count() >= 2,
            "this machine cannot schedule units"
        );
        let mut voices = vec![Voice::create(plugin, true)];
        voices[0].events = first_block.clone();
        let mut pooled = Vec::with_capacity(BLOCKS);
        for _ in 0..BLOCKS {
            let started = Instant::now();
            assert!(pool.process(&mut voices, FRAMES, CHANNELS, 1_000_000_000));
            pooled.push(started.elapsed().as_nanos() as u64);
            voices[0].events.clear();
        }

        let (a, b, c) = (mean_ns(&single), mean_ns(&inline), mean_ns(&pooled));
        let micros = |ns: f64| ns / 1000.0;
        println!(
            "  A  una instancia, bloque entero   {:8.1} us   (p99 {:.1})",
            micros(a),
            micros(percentile_ns(&single, 0.99) as f64)
        );
        println!(
            "  B  unidades en linea, sin hilos   {:8.1} us   (p99 {:.1})",
            micros(b),
            micros(percentile_ns(&inline, 0.99) as f64)
        );
        println!(
            "       begin {:.1} us | unidades {:.1} us | finish {:.1} us",
            micros(mean_ns(&begins)),
            micros(mean_ns(&units_ran)),
            micros(mean_ns(&finishes))
        );
        println!(
            "  C  unidades en el pool real       {:8.1} us   (p99 {:.1}, {} workers)",
            micros(c),
            micros(percentile_ns(&pooled, 0.99) as f64),
            pool.worker_count()
        );
        println!("  --------");
        println!(
            "  B-A  el transporte cuesta        {:8.1} us",
            micros(b - a)
        );
        println!(
            "  C-B  los hilos cuestan           {:8.1} us",
            micros(c - b)
        );
        println!(
            "  C-A  la sobrecarga total         {:8.1} us",
            micros(c - a)
        );
        // `finish` is the mix writes plus `end_block`, and the mix writes are
        // the whole of what the transport costs, so the rest of `finish` is
        // the global stage: the soundboard, the sympathetic bank and the
        // room. That stage runs every block whether or not a note sounds,
        // and on the appliance it is by far the largest number here.
        println!(
            "  la etapa global (inferida)       {:8.1} us   = {:.0} % del deadline de 2666 us",
            micros(mean_ns(&finishes) - (b - a)),
            (mean_ns(&finishes) - (b - a)) / 2_666_000.0 * 100.0
        );
    }
}
