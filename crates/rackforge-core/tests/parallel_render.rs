//! End-to-end coverage of `parallel_render_v1` against a real packaged
//! wasm-v1 component: the host-side unit instances, the global scheduler and
//! the sequential fallback must all produce bit-identical audio.

#![cfg(not(target_arch = "wasm32"))]

use rackforge_core::parallel_render::{
    ParallelUnits, RenderPool, RenderTelemetry, ScheduledSlot, UnitJob, process_slots_sequential,
};
use rackforge_core::{LoadedPlugin, PluginInstance, PluginPackage};
use rackforge_plugin_api::abi::{MidiEventV1, ParameterEventV1};
use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

/// Same miniature parallel instrument as the runtime's fixture, extended
/// with a working program: loading any catalog program restores three active
/// units. Integer-valued f32 samples keep every comparison exact.
const PARALLEL_SYNTH: &str = r#"
    (module
      (memory (export "memory") 2)
      (global $lfo (mut f32) (f32.const 0))
      (global $active (mut i32) (i32.const 3))
      (global $last_active (mut i32) (i32.const 0))
      (global $fail (mut i32) (i32.const -1))
      (func (export "rackforge_abi_version") (result i32) i32.const 65538)
      (func (export "rackforge_input_ptr") (result i32) i32.const 0)
      (func (export "rackforge_output_ptr") (result i32) i32.const 1024)
      (func (export "rackforge_capacity_input_samples") (result i32) i32.const 256)
      (func (export "rackforge_capacity_output_samples") (result i32) i32.const 256)
      (func (export "rackforge_midi_ptr") (result i32) i32.const 4096)
      (func (export "rackforge_capacity_midi_events") (result i32) i32.const 64)
      (func (export "rackforge_parameter_ptr") (result i32) i32.const 5120)
      (func (export "rackforge_capacity_parameter_events") (result i32) i32.const 64)
      (func (export "rackforge_transfer_ptr") (result i32) i32.const 8192)
      (func (export "rackforge_capacity_transfer_bytes") (result i32) i32.const 1024)
      (func (export "rackforge_parallel_abi_version") (result i32) i32.const 65536)
      (func (export "rackforge_parallel_max_units") (result i32) i32.const 4)
      (func (export "rackforge_parallel_dispatch_stride") (result i32) i32.const 16)
      (func (export "rackforge_parallel_plan_ptr") (result i32) i32.const 12288)
      (func (export "rackforge_parallel_dispatch_ptr") (result i32) i32.const 12352)
      (func (export "rackforge_parallel_mix_ptr") (result i32) i32.const 12544)
      (func (export "rackforge_parallel_shared_ptr") (result i32) i32.const 16704)
      (func (export "rackforge_parallel_shared_capacity") (result i32) i32.const 256)
      (func (export "rackforge_initialize") (result i32) i32.const 0)
      (func (export "rackforge_prepare") (param f64 i32 i32 i32) (result i32) i32.const 0)
      (func (export "rackforge_set_parameter") (param $index i32) (param $value f64) (result i32)
        local.get $index i32.const 0 i32.eq
        if
          local.get $value i32.trunc_f64_s global.set $active
          i32.const 0 return
        end
        local.get $index i32.const 1 i32.eq
        if
          local.get $value i32.trunc_f64_s global.set $fail
          i32.const 0 return
        end
        i32.const -3)
      (func (export "rackforge_get_parameter") (param $index i32) (result f64)
        local.get $index i32.const 0 i32.eq
        if (result f64)
          global.get $active f64.convert_i32_s
        else
          global.get $fail f64.convert_i32_s
        end)
      (func (export "rackforge_reset") (result i32)
        f32.const 0 global.set $lfo
        i32.const 16640 i64.const 0 i64.store
        i32.const 16648 i64.const 0 i64.store
        i32.const 0)
      (func (export "rackforge_resource_begin") (param i32 i64) (result i32) i32.const -3)
      (func (export "rackforge_resource_write") (param i64 i32) (result i32) i32.const -3)
      (func (export "rackforge_resource_end") (result i32) i32.const -3)
      (func (export "rackforge_load_preset") (param i32) (result i32)
        i32.const 3 global.set $active
        i32.const 0)
      (func (export "rackforge_save_state") (result i32)
        i32.const 8192 global.get $active i32.store
        i32.const 4)
      (func (export "rackforge_load_state") (param $length i32) (result i32)
        local.get $length i32.const 4 i32.ne
        if i32.const -1 return end
        i32.const 8192 i32.load global.set $active
        i32.const 0)
      (func $plan_unit (param $i i32) (result i32) local.get $i)
      (func $begin (param $frames i32) (param $midi i32) (param $parameters i32) (result i32)
        (local $count i32) (local $i i32)
        global.get $lfo f32.const 1 f32.add global.set $lfo
        local.get $parameters i32.const 0 i32.gt_s
        if
          i32.const 5128 f64.load i32.trunc_f64_s global.set $active
        end
        local.get $midi i32.const 0 i32.gt_s
        if (result i32)
          i32.const 4
        else
          global.get $active
        end
        local.set $count
        local.get $count global.set $last_active
        i32.const 12288 i32.const 8 i32.store
        i32.const 12292 i32.const 0 i32.store
        i32.const 16704 global.get $lfo f32.store
        (block $done
          (loop $units
            local.get $i local.get $count i32.ge_s br_if $done
            i32.const 12296 local.get $i i32.const 8 i32.mul i32.add
            local.get $i call $plan_unit i32.store
            i32.const 12300 local.get $i i32.const 8 i32.mul i32.add
            i32.const 8 i32.store
            i32.const 12352 local.get $i i32.const 16 i32.mul i32.add
            global.get $lfo f32.store
            i32.const 12356 local.get $i i32.const 16 i32.mul i32.add
            local.get $i i32.const 1 i32.add f32.convert_i32_s f32.store
            local.get $i i32.const 1 i32.add local.set $i
            br $units))
        local.get $count)
      (func $render (param $unit i32) (param $payload i32) (param $shared i32) (param $frames i32) (param $channels i32) (result i32)
        (local $lfo f32) (local $scale f32) (local $phase f32)
        (local $k i32) (local $count i32) (local $sample f32)
        local.get $unit global.get $fail i32.eq
        if unreachable end
        i32.const 16704 f32.load local.set $lfo
        i32.const 12356 local.get $unit i32.const 16 i32.mul i32.add f32.load local.set $scale
        i32.const 16640 local.get $unit i32.const 4 i32.mul i32.add
        i32.const 16640 local.get $unit i32.const 4 i32.mul i32.add f32.load
        f32.const 1 f32.add local.tee $phase
        f32.store
        local.get $lfo f32.const 1000 f32.mul
        local.get $scale f32.const 100 f32.mul f32.add
        local.get $phase f32.add local.set $sample
        local.get $frames local.get $channels i32.mul local.set $count
        (block $done
          (loop $fill
            local.get $k local.get $count i32.ge_s br_if $done
            i32.const 1024 local.get $k i32.const 4 i32.mul i32.add
            local.get $sample f32.store
            local.get $k i32.const 1 i32.add local.set $k
            br $fill))
        i32.const 0)
      (func $end (param $frames i32) (param $channels i32) (result i32)
        (local $k i32) (local $count i32) (local $sum f32) (local $u i32)
        local.get $frames local.get $channels i32.mul local.set $count
        (block $done
          (loop $frames_loop
            local.get $k local.get $count i32.ge_s br_if $done
            f32.const 0 local.set $sum
            i32.const 0 local.set $u
            (block $mixed
              (loop $mix
                local.get $u global.get $last_active i32.ge_s br_if $mixed
                local.get $sum
                i32.const 12544
                local.get $u i32.const 1024 i32.mul i32.add
                local.get $k i32.const 4 i32.mul i32.add
                f32.load f32.add local.set $sum
                local.get $u i32.const 1 i32.add local.set $u
                br $mix))
            i32.const 1024 local.get $k i32.const 4 i32.mul i32.add
            local.get $sum f32.const 0.5 f32.mul global.get $lfo f32.add
            f32.store
            local.get $k i32.const 1 i32.add local.set $k
            br $frames_loop))
        i32.const 0)
      (func (export "rackforge_parallel_begin_block") (param $frames i32) (param $in i32) (param $out i32) (param $midi i32) (param $parameters i32) (result i32)
        local.get $frames local.get $midi local.get $parameters call $begin)
      (func (export "rackforge_parallel_render_unit") (param $unit i32) (param $payload i32) (param $shared i32) (param $frames i32) (param $channels i32) (result i32)
        local.get $unit local.get $payload local.get $shared local.get $frames local.get $channels call $render)
      (func (export "rackforge_parallel_end_block") (param $frames i32) (param $channels i32) (result i32)
        local.get $frames local.get $channels call $end)
      (func (export "rackforge_process") (param $frames i32) (param $in i32) (param $out i32) (param $midi i32) (param $parameters i32) (result i32)
        (local $count i32) (local $u i32) (local $status i32)
        local.get $frames local.get $midi local.get $parameters call $begin
        local.set $count
        (block $done
          (loop $units
            local.get $u local.get $count i32.ge_s br_if $done
            local.get $u i32.const 8 i32.const 8 local.get $frames local.get $out call $render
            local.tee $status i32.const 0 i32.ne
            if local.get $status return end
            i32.const 12544 local.get $u i32.const 1024 i32.mul i32.add
            i32.const 1024
            local.get $frames local.get $out i32.mul i32.const 4 i32.mul
            memory.copy
            local.get $u i32.const 1 i32.add local.set $u
            br $units))
        local.get $frames local.get $out call $end)
    )
"#;

const MANIFEST: &str = r#"
schema_version = 1
id = "org.rackforge.parallel-test"
name = "Parallel Test Synth"
vendor = "RackForge"
version = "0.1.0"
kind = "instrument"
state_version = 1
capabilities = ["audio_output", "presets", "state", "parallel_render_v1"]

[audio]
output_buses = [{ id = "main", name = "Output", channels = 2, layout = "stereo" }]

[api]
major = 1
minor = 10

[component]
abi = "wasm-v1"
path = "component.wasm"
runtime_descriptor = "metadata/runtime.json"
parameter_schema = "metadata/parameters.json"
preset_catalog = "metadata/presets.json"
"#;

const RUNTIME_JSON: &str = r#"{
  "schema_version": 1,
  "id": "org.rackforge.parallel-test",
  "version": "0.1.0",
  "state_version": 1
}"#;

const PARAMETERS_JSON: &str = r#"{
  "schema_version": 1,
  "pages": [{ "id": "units", "name": "Units", "order": 0 }],
  "parameters": [
    {
      "index": 0,
      "id": "active-units",
      "name": "Active Units",
      "page": "units",
      "order": 0,
      "kind": { "type": "float", "minimum": 0.0, "maximum": 4.0, "default": 3.0, "step": 1.0 },
      "flags": { "automatable": true }
    },
    {
      "index": 1,
      "id": "fail-unit",
      "name": "Fail Unit",
      "page": "units",
      "order": 1,
      "kind": { "type": "float", "minimum": -1.0, "maximum": 16.0, "default": -1.0, "step": 1.0 },
      "flags": { "automatable": false }
    }
  ]
}"#;

const PRESETS_JSON: &str = r#"{
  "schema_version": 1,
  "banks": [{ "id": "factory", "name": "Factory", "order": 0 }],
  "presets": [{ "id": "trio", "name": "Trio", "bank": "factory", "order": 0 }]
}"#;

static SERIAL: AtomicU64 = AtomicU64::new(0);

fn build_package() -> &'static LoadedPlugin {
    let root = std::env::temp_dir().join(format!(
        "rackforge-parallel-test-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let metadata = root.join("metadata");
    fs::create_dir_all(&metadata).unwrap();
    fs::write(root.join("rackforge-plugin.toml"), MANIFEST).unwrap();
    fs::write(
        root.join("component.wasm"),
        wat::parse_str(PARALLEL_SYNTH).unwrap(),
    )
    .unwrap();
    fs::write(metadata.join("runtime.json"), RUNTIME_JSON).unwrap();
    fs::write(metadata.join("parameters.json"), PARAMETERS_JSON).unwrap();
    fs::write(metadata.join("presets.json"), PRESETS_JSON).unwrap();
    let package = PluginPackage::open(&root).unwrap();
    // SAFETY: portable wasm-v1 packages execute inside the sandbox.
    let loaded = unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), None) }.unwrap();
    Box::leak(Box::new(loaded))
}

const FRAMES: u32 = 64;
const CHANNELS: u32 = 2;
const SAMPLES: usize = FRAMES as usize * CHANNELS as usize;
/// Program the scripted blocks load; the wat fixture's catalog names it.
const PROGRAM: &str = "trio";

/// The same Slot glue the live host uses: a coordinator instance plus
/// host-owned unit instances, scheduled begin → units → end.
struct TestVoice {
    instance: PluginInstance<'static>,
    parallel: Option<ParallelUnits<'static>>,
    input_channels: u32,
    /// Indices of earlier voices whose finished output feeds this one, the
    /// same shape the live host resolves from Rack cables.
    deps: Vec<usize>,
    input: Vec<f32>,
    output: Vec<f32>,
    events: Vec<MidiEventV1>,
    parameter_events: Vec<ParameterEventV1>,
    faulted: bool,
}

impl TestVoice {
    fn create(plugin: &'static LoadedPlugin, with_units: bool) -> Self {
        Self::create_with_inputs(plugin, with_units, 0, Vec::new())
    }

    fn create_with_inputs(
        plugin: &'static LoadedPlugin,
        with_units: bool,
        input_channels: u32,
        deps: Vec<usize>,
    ) -> Self {
        let mut instance = plugin.create_instance().unwrap();
        instance
            .activate(48_000.0, FRAMES, input_channels, CHANNELS)
            .unwrap();
        let parallel = if with_units {
            ParallelUnits::create(plugin, 48_000.0, FRAMES, input_channels, CHANNELS).unwrap()
        } else {
            None
        };
        Self {
            instance,
            parallel,
            input_channels,
            deps,
            input: vec![0.0; FRAMES as usize * input_channels as usize],
            output: vec![0.0; SAMPLES],
            events: Vec::new(),
            parameter_events: Vec::new(),
            faulted: false,
        }
    }

    /// Control-plane operation applied to the coordinator and mirrored to
    /// every unit instance, exactly as the live host does.
    fn mirrored(&mut self, operation: impl Fn(&mut PluginInstance<'static>) -> anyhow::Result<()>) {
        operation(&mut self.instance).unwrap();
        if let Some(parallel) = &mut self.parallel {
            parallel.mirror(|instance| operation(instance)).unwrap();
        }
    }
}

// SAFETY: the coordinator and every unit instance run in the portable
// backend; unit jobs point at per-unit boxed cells owned by `ParallelUnits`.
unsafe impl ScheduledSlot for TestVoice {
    fn max_units(&self) -> u32 {
        self.parallel.as_ref().map_or(0, ParallelUnits::max_units)
    }

    fn dependency_mask(&self) -> u32 {
        self.deps.iter().fold(0, |mask, index| mask | (1 << index))
    }

    unsafe fn gather_input(
        slot_index: usize,
        slots: *mut Self,
        _slot_count: usize,
        _frames: u32,
        _channels: u32,
    ) {
        // SAFETY: the scheduler grants exclusive access to this voice and
        // finished, immutable upstream voices at lower indices.
        let voice = unsafe { &mut *slots.add(slot_index) };
        if voice.deps.is_empty() {
            return;
        }
        voice.input.fill(0.0);
        for upstream in voice.deps.clone() {
            // SAFETY: as above; a lower, completed index.
            let upstream = unsafe { &*(slots.add(upstream) as *const TestVoice) };
            if upstream.faulted {
                continue;
            }
            for (target, sample) in voice.input.iter_mut().zip(&upstream.output) {
                *target += *sample;
            }
        }
    }

    fn run_single(&mut self, frames: u32, channels: u32) -> bool {
        self.output.fill(0.0);
        if self.faulted {
            return true;
        }
        self.instance
            .process_interleaved(
                &self.input,
                &mut self.output,
                frames,
                self.input_channels,
                channels,
                &self.events,
                &self.parameter_events,
            )
            .is_ok()
    }

    fn run_begin(&mut self, frames: u32, _channels: u32) -> Option<u32> {
        if self.faulted {
            self.output.fill(0.0);
            return Some(0);
        }
        let parallel = self.parallel.as_mut()?;
        parallel
            .begin(
                &mut self.instance,
                &self.input,
                frames,
                &self.events,
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
        if self.faulted {
            return true;
        }
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
        self.faulted = true;
    }
}

enum Action {
    None,
    Midi,
    Automation(f64),
    Program(&'static str),
    Parameter(u32, f64),
    /// Mirrored `reset` while notes are sounding.
    Reset,
}

fn script(program: &'static str) -> Vec<Action> {
    vec![
        Action::None,
        Action::Midi,
        Action::None,
        Action::Automation(2.0),
        Action::Program(program),
        Action::Parameter(0, 1.0),
        Action::None,
        Action::Automation(4.0),
        Action::Midi,
        Action::Reset,
        Action::None,
    ]
}

fn stage_action(voice: &mut TestVoice, action: &Action) {
    voice.events.clear();
    voice.parameter_events.clear();
    match action {
        Action::None => {}
        Action::Midi => voice.events.push(MidiEventV1 {
            frame: 1,
            length: 3,
            data: [0x90, 60, 100],
        }),
        Action::Automation(value) => voice.parameter_events.push(ParameterEventV1 {
            frame: 0,
            parameter_index: 0,
            value: *value,
        }),
        Action::Program(id) => voice.mirrored(|instance| instance.load_preset(id)),
        Action::Parameter(index, value) => {
            let (index, value) = (*index, *value);
            voice.mirrored(move |instance| instance.set_parameter(index, value));
        }
        Action::Reset => voice.mirrored(|instance| instance.reset()),
    }
}

/// Renders the scripted blocks and returns every block's samples.
fn render_scripted(
    voices: &mut [TestVoice],
    program: &'static str,
    mut render: impl FnMut(&mut [TestVoice]),
) -> Vec<Vec<f32>> {
    let mut blocks = Vec::new();
    for action in script(program) {
        for voice in voices.iter_mut() {
            stage_action(voice, &action);
        }
        render(voices);
        blocks.push(voices[0].output.clone());
    }
    blocks
}

#[test]
fn every_worker_count_matches_the_sequential_fallback_exactly() {
    let plugin = build_package();
    let telemetry = RenderTelemetry::new(1);
    let mut reference_voices = vec![TestVoice::create(plugin, false)];
    let reference = render_scripted(&mut reference_voices, PROGRAM, |voices| {
        process_slots_sequential(voices, FRAMES, CHANNELS, &telemetry);
    });
    assert!(reference.iter().flatten().any(|sample| *sample != 0.0));

    // Sequential over the very same begin/unit/end graph.
    let telemetry = RenderTelemetry::new(1);
    let mut voices = vec![TestVoice::create(plugin, true)];
    let sequential = render_scripted(&mut voices, PROGRAM, |voices| {
        process_slots_sequential(voices, FRAMES, CHANNELS, &telemetry);
    });
    assert_eq!(reference, sequential, "sequential unit graph diverged");

    for workers in [2_usize, 3, 4] {
        let telemetry = RenderTelemetry::new(workers);
        let mut pool = RenderPool::with_workers(workers, telemetry);
        if pool.worker_count() < 2 {
            continue;
        }
        let mut voices = vec![TestVoice::create(plugin, true)];
        let produced = render_scripted(&mut voices, PROGRAM, |voices| {
            assert!(pool.process(voices, FRAMES, CHANNELS, 1_000_000_000));
        });
        assert_eq!(reference, produced, "workers={workers} diverged");
        // The final coordinator state must be identical too.
        assert_eq!(
            reference_voices[0].instance.save_state().unwrap(),
            voices[0].instance.save_state().unwrap(),
            "workers={workers} state diverged"
        );
    }
}

#[test]
fn saved_state_restores_identically_across_render_paths() {
    let plugin = build_package();
    let telemetry = RenderTelemetry::new(1);

    // Play a while, capture state from the coordinator.
    let mut voices = vec![TestVoice::create(plugin, true)];
    render_scripted(&mut voices, PROGRAM, |voices| {
        process_slots_sequential(voices, FRAMES, CHANNELS, &telemetry);
    });
    let state = voices[0].instance.save_state().unwrap();

    // Restoring into fresh classic and parallel voices must agree.
    let mut classic = TestVoice::create(plugin, false);
    classic.mirrored(|instance| instance.load_state(&state));
    let mut parallel = TestVoice::create(plugin, true);
    parallel.mirrored(|instance| instance.load_state(&state));

    let mut classic_voices = vec![classic];
    let expected = render_scripted(&mut classic_voices, PROGRAM, |voices| {
        process_slots_sequential(voices, FRAMES, CHANNELS, &telemetry);
    });
    let telemetry_pool = RenderTelemetry::new(2);
    let mut pool = RenderPool::with_workers(2, telemetry_pool);
    let mut parallel_voices = vec![parallel];
    let produced = render_scripted(&mut parallel_voices, PROGRAM, |voices| {
        if !pool.process(voices, FRAMES, CHANNELS, 1_000_000_000) {
            process_slots_sequential(voices, FRAMES, CHANNELS, &telemetry);
        }
    });
    assert_eq!(expected, produced);
}

#[test]
fn a_trapping_unit_is_silenced_reported_and_quarantined() {
    let plugin = build_package();
    let telemetry = RenderTelemetry::new(2);
    let mut pool = RenderPool::with_workers(2, telemetry.clone());
    let mut voices = vec![TestVoice::create(plugin, true)];
    // Unit 1 traps inside its worker instance from now on.
    voices[0].mirrored(|instance| instance.set_parameter(1, 1.0));

    for _ in 0..6 {
        voices[0].events.clear();
        voices[0].parameter_events.clear();
        if !pool.process(&mut voices, FRAMES, CHANNELS, 1_000_000_000) {
            process_slots_sequential(&mut voices, FRAMES, CHANNELS, telemetry.as_ref());
        }
    }
    let voice = &voices[0];
    assert!(!voice.faulted, "one bad unit must not silence the Slot");
    assert!(voice.output.iter().any(|sample| *sample != 0.0));
    assert_eq!(
        voice.parallel.as_ref().unwrap().quarantined_units(),
        0b10,
        "the trapping unit is quarantined after its first failure"
    );
    let snapshot = telemetry.snapshot_and_reset();
    assert_eq!(
        snapshot.unit_faults[0], 1,
        "the fault is reported once, then the unit is skipped"
    );
}

#[test]
fn an_inactive_unit_graph_still_renders_the_global_stage() {
    let plugin = build_package();
    let telemetry = RenderTelemetry::new(2);
    let mut pool = RenderPool::with_workers(2, telemetry);
    let mut voices = vec![
        TestVoice::create(plugin, true),
        TestVoice::create(plugin, true),
    ];
    // Zero units on the first Slot; the second keeps its default three.
    voices[0].mirrored(|instance| instance.set_parameter(0, 0.0));
    for voice in &mut voices {
        voice.events.clear();
        voice.parameter_events.clear();
    }
    assert!(pool.process(&mut voices, FRAMES, CHANNELS, 1_000_000_000));
    // The empty graph still ran begin and end: the block-rate LFO is audible.
    assert!(voices[0].output.iter().all(|sample| *sample == 1.0));
    assert!(voices[1].output.iter().any(|sample| *sample > 1.0));
}

/// End-to-end proof over the real packaged example instrument. Ignored by
/// default because it needs the built component; run it with:
///
/// ```text
/// cargo build --release --target wasm32-unknown-unknown -p rackforge-parallel-demo-synth
/// cargo test -p rackforge-core --test parallel_render -- --ignored
/// ```
#[test]
#[ignore = "requires the wasm32 build of rackforge-parallel-demo-synth"]
fn the_packaged_parallel_demo_synth_matches_its_sequential_fallback() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf();
    let component =
        workspace.join("target/wasm32-unknown-unknown/release/rackforge_parallel_demo_synth.wasm");
    let package_source = workspace.join("plugins/parallel-demo-synth/package");
    let root = std::env::temp_dir().join(format!(
        "rackforge-parallel-demo-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join("metadata")).unwrap();
    for file in [
        "rackforge-plugin.toml",
        "metadata/runtime.json",
        "metadata/parameters.json",
        "metadata/presets.json",
    ] {
        fs::copy(package_source.join(file), root.join(file)).unwrap();
    }
    fs::copy(&component, root.join("component.wasm")).unwrap();
    let package = PluginPackage::open(&root).unwrap();
    // SAFETY: portable wasm-v1 packages execute inside the sandbox.
    let loaded = unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), None) }.unwrap();
    let plugin: &'static LoadedPlugin = Box::leak(Box::new(loaded));
    assert_eq!(plugin.parallel_layout().unwrap().max_units, 5);

    let telemetry = RenderTelemetry::new(1);
    let mut classic_voices = vec![TestVoice::create(plugin, false)];
    let reference = render_scripted(&mut classic_voices, "pad", |voices| {
        process_slots_sequential(voices, FRAMES, CHANNELS, &telemetry);
    });
    assert!(reference.iter().flatten().any(|sample| *sample != 0.0));

    for workers in [2_usize, 3] {
        let telemetry = RenderTelemetry::new(workers);
        let mut pool = RenderPool::with_workers(workers, telemetry);
        let mut voices = vec![TestVoice::create(plugin, true)];
        let produced = render_scripted(&mut voices, "pad", |voices| {
            assert!(pool.process(voices, FRAMES, CHANNELS, 1_000_000_000));
        });
        assert_eq!(reference, produced, "workers={workers} diverged");
    }
}

/// A classic downstream effect: `out = in * 0.5`, two channels.
const EFFECT_WAT: &str = r#"
    (module
      (memory (export "memory") 1)
      (func (export "rackforge_abi_version") (result i32) i32.const 65538)
      (func (export "rackforge_input_ptr") (result i32) i32.const 0)
      (func (export "rackforge_output_ptr") (result i32) i32.const 4096)
      (func (export "rackforge_capacity_input_samples") (result i32) i32.const 256)
      (func (export "rackforge_capacity_output_samples") (result i32) i32.const 256)
      (func (export "rackforge_midi_ptr") (result i32) i32.const 8192)
      (func (export "rackforge_capacity_midi_events") (result i32) i32.const 64)
      (func (export "rackforge_parameter_ptr") (result i32) i32.const 9216)
      (func (export "rackforge_capacity_parameter_events") (result i32) i32.const 64)
      (func (export "rackforge_transfer_ptr") (result i32) i32.const 10240)
      (func (export "rackforge_capacity_transfer_bytes") (result i32) i32.const 1024)
      (func (export "rackforge_initialize") (result i32) i32.const 0)
      (func (export "rackforge_prepare") (param f64 i32 i32 i32) (result i32) i32.const 0)
      (func (export "rackforge_set_parameter") (param i32 f64) (result i32) i32.const 0)
      (func (export "rackforge_get_parameter") (param i32) (result f64) f64.const 0.5)
      (func (export "rackforge_reset") (result i32) i32.const 0)
      (func (export "rackforge_resource_begin") (param i32 i64) (result i32) i32.const -3)
      (func (export "rackforge_resource_write") (param i64 i32) (result i32) i32.const -3)
      (func (export "rackforge_resource_end") (result i32) i32.const -3)
      (func (export "rackforge_load_preset") (param i32) (result i32) i32.const 0)
      (func (export "rackforge_save_state") (result i32) i32.const 0)
      (func (export "rackforge_load_state") (param i32) (result i32) i32.const 0)
      (func (export "rackforge_process") (param $frames i32) (param $in i32) (param $out i32) (param $midi i32) (param $parameters i32) (result i32)
        (local $k i32) (local $count i32)
        local.get $frames local.get $out i32.mul local.set $count
        (block $done
          (loop $copy
            local.get $k local.get $count i32.ge_s br_if $done
            i32.const 4096 local.get $k i32.const 4 i32.mul i32.add
            local.get $k i32.const 4 i32.mul f32.load f32.const 0.5 f32.mul
            f32.store
            local.get $k i32.const 1 i32.add local.set $k
            br $copy))
        i32.const 0)
    )
"#;

const EFFECT_MANIFEST: &str = r#"
schema_version = 1
id = "org.rackforge.parallel-test-effect"
name = "Parallel Test Effect"
vendor = "RackForge"
version = "0.1.0"
kind = "effect"
state_version = 1
capabilities = ["audio_input", "audio_output", "presets"]

[audio]
input_buses = [{ id = "main", name = "Input", channels = 2, layout = "stereo" }]
output_buses = [{ id = "main", name = "Output", channels = 2, layout = "stereo" }]

[api]
major = 1
minor = 10

[component]
abi = "wasm-v1"
path = "component.wasm"
runtime_descriptor = "metadata/runtime.json"
parameter_schema = "metadata/parameters.json"
preset_catalog = "metadata/presets.json"
"#;

fn build_effect_package() -> &'static LoadedPlugin {
    let root = std::env::temp_dir().join(format!(
        "rackforge-parallel-effect-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let metadata = root.join("metadata");
    fs::create_dir_all(&metadata).unwrap();
    fs::write(root.join("rackforge-plugin.toml"), EFFECT_MANIFEST).unwrap();
    fs::write(
        root.join("component.wasm"),
        wat::parse_str(EFFECT_WAT).unwrap(),
    )
    .unwrap();
    fs::write(
        metadata.join("runtime.json"),
        RUNTIME_JSON.replace(
            "org.rackforge.parallel-test",
            "org.rackforge.parallel-test-effect",
        ),
    )
    .unwrap();
    fs::write(metadata.join("parameters.json"), PARAMETERS_JSON).unwrap();
    fs::write(metadata.join("presets.json"), PRESETS_JSON).unwrap();
    let package = PluginPackage::open(&root).unwrap();
    // SAFETY: portable wasm-v1 packages execute inside the sandbox.
    let loaded = unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), None) }.unwrap();
    Box::leak(Box::new(loaded))
}

/// The Rack-with-cables scenario: a parallel synth feeding a downstream
/// effect, next to an independent classic instrument — the whole graph
/// inside the pool, not serialized by the mere presence of a cable. The
/// effect consumes the synth's *final* block (its `end_block` output), and
/// the pool must produce exactly the sequential executor's audio.
#[test]
fn a_parallel_synth_feeds_a_downstream_effect_inside_the_pool() {
    let synth = build_package();
    let effect = build_effect_package();

    let build = |with_units: bool| {
        vec![
            TestVoice::create(synth, with_units),
            TestVoice::create_with_inputs(effect, false, 2, vec![0]),
            TestVoice::create(synth, false),
        ]
    };
    let note = MidiEventV1 {
        frame: 1,
        length: 3,
        data: [0x90, 60, 100],
    };
    let run = |voices: &mut Vec<TestVoice>, render: &mut dyn FnMut(&mut [TestVoice])| {
        let mut blocks = Vec::new();
        for block in 0..6 {
            for voice in voices.iter_mut() {
                voice.events.clear();
                voice.parameter_events.clear();
                if block == 1 {
                    voice.events.push(note);
                }
            }
            render(voices);
            blocks.push(
                voices
                    .iter()
                    .map(|voice| voice.output.clone())
                    .collect::<Vec<_>>(),
            );
        }
        blocks
    };

    let telemetry = RenderTelemetry::new(1);
    let mut sequential_voices = build(true);
    let expected = run(&mut sequential_voices, &mut |voices| {
        process_slots_sequential(voices, FRAMES, CHANNELS, &telemetry);
    });

    for workers in [2_usize, 3] {
        let telemetry = RenderTelemetry::new(workers);
        let mut pool = RenderPool::with_workers(workers, telemetry);
        let mut voices = build(true);
        let produced = run(&mut voices, &mut |voices| {
            assert!(
                pool.process(voices, FRAMES, CHANNELS, 1_000_000_000),
                "a cabled graph must stay schedulable"
            );
        });
        assert_eq!(expected, produced, "workers={workers} diverged");
    }

    // The effect renders exactly half of the synth's finished block: proof
    // that it consumed the post-`end_block` output, not raw units.
    for block in &expected {
        for (synth_sample, effect_sample) in block[0].iter().zip(&block[1]) {
            assert_eq!(*effect_sample, synth_sample * 0.5);
        }
        assert!(block[2].iter().zip(&block[0]).all(|(a, b)| a == b));
    }
    assert!(
        expected
            .iter()
            .flatten()
            .flatten()
            .any(|sample| *sample != 0.0)
    );
}

#[test]
fn a_package_may_not_claim_the_capability_without_the_extension() {
    let root = std::env::temp_dir().join(format!(
        "rackforge-parallel-mismatch-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let metadata = root.join("metadata");
    fs::create_dir_all(&metadata).unwrap();
    // Hiding the version probe hides the whole extension from discovery.
    let stripped = PARALLEL_SYNTH.replace(
        "rackforge_parallel_abi_version",
        "hidden_parallel_abi_version",
    );
    fs::write(root.join("rackforge-plugin.toml"), MANIFEST).unwrap();
    fs::write(
        root.join("component.wasm"),
        wat::parse_str(&stripped).unwrap(),
    )
    .unwrap();
    fs::write(metadata.join("runtime.json"), RUNTIME_JSON).unwrap();
    fs::write(metadata.join("parameters.json"), PARAMETERS_JSON).unwrap();
    fs::write(metadata.join("presets.json"), PRESETS_JSON).unwrap();
    let package = PluginPackage::open(&root).unwrap();
    // SAFETY: portable wasm-v1 packages execute inside the sandbox.
    let error = match unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), None) } {
        Ok(_) => panic!("capability without extension was accepted"),
        Err(error) => error,
    };
    assert!(format!("{error:#}").contains("does not export the extension"));
}
