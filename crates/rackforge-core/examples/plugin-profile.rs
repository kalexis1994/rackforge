//! Where a parallel plugin's block goes, phase by phase, against the
//! deadline it has to meet.
//!
//! ```text
//! cargo run --release -p rackforge-core --example plugin-profile -- <package-directory>
//! ```
//!
//! A plugin author's own profiler measures the whole render, which is the
//! natural thing to measure and the wrong shape for this host. A block is
//! three phases and they land in different places: `begin_block` and
//! `end_block` run on the coordinator, one after the other, and the units
//! run on worker cores. Splitting five voices across four cores does
//! nothing whatever about the serial two, so a fat `begin_block` is the one
//! cost that four cores cannot help with -- and it is invisible in a figure
//! that adds all three together.
//!
//! The other half of the diagnosis is whether a phase is FIXED. A cost that
//! is the same with nothing playing as it is under a chord is control-rate
//! work running at sample rate, and it is paid on every block forever. This
//! prints both, and says which.
//!
//! Every number here is wall time on THIS machine. An appliance is several
//! times slower per core and its wasm engine charges differently, so read
//! the shares and the fixed/scaling split, and take the microseconds on the
//! machine that has to meet the deadline.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rackforge_core::midi2::Midi2Event;
use rackforge_core::parallel_render::{
    ParallelUnits, RenderPool, RenderTelemetry, ScheduledSlot, UnitJob, process_slots_sequential,
};
use rackforge_core::{LoadedPlugin, PluginInstance, PluginPackage};
use rackforge_plugin_api::abi::{MidiEventV1, ParameterEventV1};

const SAMPLE_RATE: f64 = 48_000.0;
/// The appliance's period. Everything is reported against the deadline this
/// implies, because that is the number a block either meets or does not.
const FRAMES: u32 = 128;
const CHANNELS: u32 = 2;
const BLOCKS: usize = 2000;
const WARMUP: usize = 200;

fn deadline_micros() -> f64 {
    FRAMES as f64 / SAMPLE_RATE * 1_000_000.0
}

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
        let mut instance = plugin.create_instance().expect("instance");
        instance
            .activate(SAMPLE_RATE, FRAMES, 0, CHANNELS)
            .expect("activate");
        let parallel = if with_units {
            ParallelUnits::create(plugin, SAMPLE_RATE, FRAMES, 0, CHANNELS).expect("units")
        } else {
            None
        };
        Self {
            instance,
            parallel,
            input: Vec::new(),
            output: vec![0.0; FRAMES as usize * CHANNELS as usize],
            events: Vec::new(),
            parameter_events: Vec::new(),
        }
    }
}

// SAFETY: coordinator and unit instances all run in the portable backend;
// unit jobs point at per-unit cells owned by `ParallelUnits`.
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
        let wide: Vec<Midi2Event> = self.events.iter().map(Midi2Event::from_midi1).collect();
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

fn load(root: &Path) -> &'static LoadedPlugin {
    let package = PluginPackage::open(root)
        .unwrap_or_else(|error| panic!("no se pudo abrir el paquete en {}: {error}", root.display()));
    // SAFETY: portable wasm-v1 packages execute inside the sandbox.
    let loaded = unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), None) }
        .unwrap_or_else(|error| panic!("no se pudo cargar el plugin: {error}"));
    Box::leak(Box::new(loaded))
}

fn mean_micros(samples: &[u64]) -> f64 {
    let kept = &samples[WARMUP.min(samples.len())..];
    kept.iter().sum::<u64>() as f64 / kept.len().max(1) as f64 / 1000.0
}

fn percentile_micros(samples: &[u64], percentile: f64) -> f64 {
    let mut kept = samples[WARMUP.min(samples.len())..].to_vec();
    kept.sort_unstable();
    kept[((kept.len() as f64 * percentile) as usize).min(kept.len() - 1)] as f64 / 1000.0
}

/// A held chord, so the instrument has something to render.
fn chord() -> Vec<MidiEventV1> {
    [48_u8, 55, 60, 64, 67]
        .iter()
        .map(|note| MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0x90, *note, 100],
        })
        .collect()
}

struct Phases {
    begin: Vec<u64>,
    units: Vec<u64>,
    finish: Vec<u64>,
    whole: Vec<u64>,
    pooled: Vec<u64>,
    single: Vec<u64>,
}

fn measure(plugin: &'static LoadedPlugin, first_block: &[MidiEventV1], workers: usize) -> Phases {
    let mut phases = Phases {
        begin: Vec::with_capacity(BLOCKS),
        units: Vec::with_capacity(BLOCKS),
        finish: Vec::with_capacity(BLOCKS),
        whole: Vec::with_capacity(BLOCKS),
        pooled: Vec::with_capacity(BLOCKS),
        single: Vec::with_capacity(BLOCKS),
    };

    // One instance, whole block: what the sequential fallback costs.
    let telemetry = RenderTelemetry::new(1);
    let mut voices = vec![Voice::create(plugin, false)];
    voices[0].events = first_block.to_vec();
    for _ in 0..BLOCKS {
        let started = Instant::now();
        process_slots_sequential(&mut voices, FRAMES, CHANNELS, &telemetry);
        phases.single.push(started.elapsed().as_nanos() as u64);
        voices[0].events.clear();
    }

    // The three phases, run inline on this thread so each one can be timed.
    let mut voices = vec![Voice::create(plugin, true)];
    voices[0].events = first_block.to_vec();
    for _ in 0..BLOCKS {
        let whole = Instant::now();
        let at_begin = Instant::now();
        let Some(mask) = voices[0].run_begin(FRAMES, CHANNELS) else {
            panic!("begin_block fallo");
        };
        phases.begin.push(at_begin.elapsed().as_nanos() as u64);

        let at_units = Instant::now();
        let mut completed = 0_u32;
        let mut pending = mask;
        while pending != 0 {
            let unit = pending.trailing_zeros();
            pending &= !(1 << unit);
            let job = voices[0].unit_job(unit, FRAMES, CHANNELS);
            // SAFETY: the job was just published for this unit and nothing
            // else is touching it.
            if unsafe { (job.run)(job.context, job.unit, FRAMES, CHANNELS) } {
                completed |= 1 << unit;
            }
        }
        phases.units.push(at_units.elapsed().as_nanos() as u64);

        let at_finish = Instant::now();
        assert!(voices[0].run_end(FRAMES, CHANNELS, completed), "end_block fallo");
        phases.finish.push(at_finish.elapsed().as_nanos() as u64);
        phases.whole.push(whole.elapsed().as_nanos() as u64);
        voices[0].events.clear();
    }

    // And across the real pool, which is what the host actually does.
    let telemetry = RenderTelemetry::new(workers);
    let mut pool = RenderPool::with_workers(workers, telemetry);
    let mut voices = vec![Voice::create(plugin, true)];
    voices[0].events = first_block.to_vec();
    for _ in 0..BLOCKS {
        let started = Instant::now();
        assert!(pool.process(&mut voices, FRAMES, CHANNELS, 1_000_000_000));
        phases.pooled.push(started.elapsed().as_nanos() as u64);
        voices[0].events.clear();
    }

    phases
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let Some(root) = arguments.next() else {
        eprintln!(
            "uso: plugin-profile <directorio-del-paquete> [workers]\n\
             \n\
             El directorio es un paquete desplegado: `rackforge-plugin.toml`,\n\
             `component.wasm` y `metadata/`. En un aparato son los que estan\n\
             bajo `plugin-store/packages/<id>/<version>`."
        );
        std::process::exit(2);
    };
    let workers: usize = arguments
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3);
    let plugin = load(&PathBuf::from(root));

    let Some(layout) = plugin.parallel_layout() else {
        eprintln!("este plugin no declara parallel_render_v1: no hay fases que separar.");
        std::process::exit(2);
    };

    let deadline = deadline_micros();
    println!(
        "\n{} unidades, bloque de {FRAMES} frames a {:.0} Hz, deadline {deadline:.0} us\n",
        layout.max_units, SAMPLE_RATE
    );

    // What the host carries across the boundary every block, from the sizes
    // the plugin itself declared. This is arithmetic, not a measurement.
    let unit_width = layout.unit_width(CHANNELS as usize);
    let committed = {
        let mut probe = Voice::create(plugin, true);
        probe.events = chord();
        probe.run_begin(FRAMES, CHANNELS);
        probe.parallel.as_ref().map_or(0, ParallelUnits::shared_len)
    };
    let mix = FRAMES as usize * unit_width * size_of::<f32>() * layout.max_units;
    let reports = layout.report_stride * layout.max_units;
    println!("  lo que cruza la frontera por bloque:");
    println!(
        "    payload compartido   {:7.1} KiB   ({committed} B, leidos del coordinador y escritos en cada una de las {} unidades)",
        (committed * (layout.max_units + 1)) as f64 / 1024.0,
        layout.max_units
    );
    println!(
        "    audio de las unidades{:7.1} KiB   ({unit_width} floats por frame por unidad, ida y vuelta)",
        mix as f64 / 1024.0 * 2.0
    );
    if reports > 0 {
        println!("    reportes             {reports:7} B");
    }

    let idle = measure(plugin, &[], workers);
    let busy = measure(plugin, &chord(), workers);

    let row = |label: &str, quiet: &[u64], loud: &[u64]| {
        let (quiet, loud) = (mean_micros(quiet), mean_micros(loud));
        // A phase that costs the same either way is control-rate work
        // running at sample rate; it is paid on every block forever.
        let fixed = (quiet.min(loud) / loud.max(1e-9) * 100.0).min(100.0);
        println!(
            "  {label:<24} {quiet:8.1} {loud:8.1}   {:5.0} %   {:5.0} %",
            loud / deadline * 100.0,
            fixed
        );
    };

    println!("\n  fase                     en vacio  con acorde   del deadline   fijo");
    row("begin_block (serial)", &idle.begin, &busy.begin);
    row("las unidades", &idle.units, &busy.units);
    row("end_block (serial)", &idle.finish, &busy.finish);
    row("--- bloque entero", &idle.whole, &busy.whole);
    row("una instancia sola", &idle.single, &busy.single);
    row("en el pool", &idle.pooled, &busy.pooled);

    let serial = mean_micros(&busy.begin) + mean_micros(&busy.finish);
    let pooled = mean_micros(&busy.pooled);
    println!(
        "\n  p99 en el pool con acorde: {:.1} us   ({:.0} % del deadline, {workers} workers)",
        percentile_micros(&busy.pooled, 0.99),
        percentile_micros(&busy.pooled, 0.99) / deadline * 100.0
    );
    println!(
        "  las dos fases seriales son {serial:.1} us: {:.0} % de lo que cuesta un bloque\n\
         \x20 en el pool, y ningun numero de nucleos las toca.",
        serial / pooled.max(1e-9) * 100.0
    );

    // The share of the deadline depends on the machine; the FIXED fraction
    // does not. A serial phase that costs the same with nothing playing is
    // the same finding on a laptop and on an appliance, so that is what this
    // keys on.
    let begin_quiet = mean_micros(&idle.begin);
    let begin_loud = mean_micros(&busy.begin);
    let begin_is_fixed = (begin_quiet / begin_loud.max(1e-9)).min(1.0);
    if begin_is_fixed > 0.8 && begin_loud > mean_micros(&busy.finish) {
        println!();
        println!("  OJO: `begin_block` cuesta {begin_quiet:.1} us SIN NADA SONANDO, el");
        println!("  {:.0} % de lo que cuesta bajo un acorde. Un costo serial que no crece", begin_is_fixed * 100.0);
        println!("  con las notas es trabajo de control corriendo a frecuencia de muestreo,");
        println!("  y se paga en todos los bloques para siempre. Vale la pena ver que parte");
        println!("  puede correr una vez por bloque en vez de una vez por frame -- y, si el");
        println!("  plugin compila a wasm, si esta pasando structs grandes por valor en ese");
        println!("  camino: wasm materializa esas copias y una compilacion nativa no, asi");
        println!("  que el costo no aparece midiendo en una maquina de desarrollo.");
    }
}
