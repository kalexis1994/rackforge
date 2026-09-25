//! A whole Rack through `RackEngine`, the engine every host without the
//! appliance's audio loop plays a Rack with: two real packaged instruments,
//! each Slot hearing its own MIDI, their outputs mixed.
//!
//! The instruments are the smallest that can say who played: each holds a
//! constant of its own while a key is down, so the mix says which Slots
//! sounded and at what level.

#![cfg(not(target_arch = "wasm32"))]

use rackforge_core::parallel_render::{RenderPool, RenderTelemetry};
use rackforge_core::rack_voice::{
    RackEngine, RackMidiStageRuntimeSpec, RackSlotRuntimeSpec, RackSlotStateLoad,
};
use rackforge_core::{LoadedPlugin, PluginPackage};
use rackforge_midi_api::{IngressMidiEvent, MidiPacket, MidiSourceKey};
use rackforge_performance_api::RackMidiTransform;
use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

const FRAMES: u32 = 64;

/// Holds `value` on both channels while a note is down; silent otherwise.
fn held_note_synth(value: f32) -> String {
    format!(
        r#"
    (module
      (memory (export "memory") 1)
      (global $on (mut i32) (i32.const 0))
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
      (func (export "rackforge_initialize") (result i32) i32.const 0)
      (func (export "rackforge_prepare") (param f64 i32 i32 i32) (result i32) i32.const 0)
      (func (export "rackforge_set_parameter") (param i32 f64) (result i32) i32.const 0)
      (func (export "rackforge_get_parameter") (param i32) (result f64) f64.const 0)
      (func (export "rackforge_reset") (result i32) i32.const 0 global.set $on i32.const 0)
      (func (export "rackforge_resource_begin") (param i32 i64) (result i32) i32.const -3)
      (func (export "rackforge_resource_write") (param i64 i32) (result i32) i32.const -3)
      (func (export "rackforge_resource_end") (result i32) i32.const -3)
      (func (export "rackforge_load_preset") (param i32) (result i32) i32.const 0)
      (func (export "rackforge_save_state") (result i32) i32.const 0)
      (func (export "rackforge_load_state") (param i32) (result i32) i32.const 0)
      (func (export "rackforge_process") (param $frames i32) (param $in i32) (param $out i32) (param $midi i32) (param $parameters i32) (result i32)
        (local $i i32) (local $status i32) (local $k i32) (local $count i32) (local $sample f32)
        (block $read
          (loop $events
            local.get $i local.get $midi i32.ge_s br_if $read
            ;; The status byte, packed after the event's frame.
            i32.const 4100 local.get $i i32.const 8 i32.mul i32.add
            i32.load8_u i32.const 240 i32.and local.set $status
            local.get $status i32.const 144 i32.eq
            if i32.const 1 global.set $on end
            local.get $status i32.const 128 i32.eq
            if i32.const 0 global.set $on end
            local.get $i i32.const 1 i32.add local.set $i
            br $events))
        global.get $on f32.convert_i32_s f32.const {value} f32.mul local.set $sample
        local.get $frames local.get $out i32.mul local.set $count
        (block $done
          (loop $fill
            local.get $k local.get $count i32.ge_s br_if $done
            i32.const 1024 local.get $k i32.const 4 i32.mul i32.add
            local.get $sample f32.store
            local.get $k i32.const 1 i32.add local.set $k
            br $fill))
        i32.const 0)
    )
"#
    )
}

static SERIAL: AtomicU64 = AtomicU64::new(0);

fn instrument(id: &str, value: f32) -> &'static LoadedPlugin {
    let root = std::env::temp_dir().join(format!(
        "rackforge-rack-engine-test-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let metadata = root.join("metadata");
    fs::create_dir_all(&metadata).unwrap();
    fs::write(
        root.join("rackforge-plugin.toml"),
        format!(
            r#"
schema_version = 1
id = "{id}"
name = "Held Note {value}"
vendor = "RackForge"
version = "0.1.0"
kind = "instrument"
state_version = 1
capabilities = ["audio_output", "presets", "state"]

[audio]
output_buses = [{{ id = "main", name = "Output", channels = 2, layout = "stereo" }}]

[api]
major = 1
minor = 10

[component]
abi = "wasm-v1"
path = "component.wasm"
runtime_descriptor = "metadata/runtime.json"
parameter_schema = "metadata/parameters.json"
preset_catalog = "metadata/presets.json"
"#
        ),
    )
    .unwrap();
    fs::write(
        root.join("component.wasm"),
        wat::parse_str(held_note_synth(value)).unwrap(),
    )
    .unwrap();
    fs::write(
        metadata.join("runtime.json"),
        format!(r#"{{"schema_version": 1, "id": "{id}", "version": "0.1.0", "state_version": 1}}"#),
    )
    .unwrap();
    fs::write(
        metadata.join("parameters.json"),
        r#"{
  "schema_version": 1,
  "pages": [{ "id": "main", "name": "Main", "order": 0 }],
  "parameters": [
    {
      "index": 0,
      "id": "unused",
      "name": "Unused",
      "page": "main",
      "order": 0,
      "kind": { "type": "float", "minimum": 0.0, "maximum": 1.0, "default": 0.0, "step": 1.0 },
      "flags": { "automatable": true }
    }
  ]
}"#,
    )
    .unwrap();
    fs::write(
        metadata.join("presets.json"),
        r#"{
  "schema_version": 1,
  "banks": [{ "id": "factory", "name": "Factory", "order": 0 }],
  "presets": [{ "id": "init", "name": "Init", "bank": "factory", "order": 0 }]
}"#,
    )
    .unwrap();
    let package = PluginPackage::open(&root).unwrap();
    // SAFETY: portable wasm-v1 packages execute inside the sandbox.
    let loaded = unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), None) }.unwrap();
    Box::leak(Box::new(loaded))
}

fn slot(
    slot_id: &str,
    plugin_id: &str,
    transform: Option<RackMidiTransform>,
) -> RackSlotRuntimeSpec {
    RackSlotRuntimeSpec {
        slot_id: slot_id.into(),
        plugin_id: plugin_id.into(),
        state: RackSlotStateLoad::Default,
        midi_stages: transform
            .map(|transform| RackMidiStageRuntimeSpec {
                transform,
                keyboard_parts: None,
            })
            .into_iter()
            .collect(),
        audio_sources: Vec::new(),
        sends_to_main: true,
        level_per_mille: 1_000,
        pan_per_mille: 0,
    }
}

fn note(status: u8, key: u8) -> IngressMidiEvent {
    IngressMidiEvent {
        source: MidiSourceKey::new(0),
        packet: MidiPacket {
            frame: 0,
            length: 3,
            data: [status, key, 100],
            wide: None,
        },
    }
}

struct Harness {
    engine: RackEngine<'static>,
    pool: RenderPool,
    telemetry: std::sync::Arc<RenderTelemetry>,
}

impl Harness {
    fn new(specs: &[RackSlotRuntimeSpec], workers: usize) -> Self {
        let a = instrument("org.rackforge.test.held-a", 1.0);
        let b = instrument("org.rackforge.test.held-b", 10.0);
        let plugins = BTreeMap::from([
            ("org.rackforge.test.held-a".to_owned(), a),
            ("org.rackforge.test.held-b".to_owned(), b),
        ]);
        let telemetry = RenderTelemetry::new(workers.max(1));
        Self {
            engine: RackEngine::build(&plugins, specs, 48_000, FRAMES, 2).unwrap(),
            pool: RenderPool::with_workers(workers, telemetry.clone()),
            telemetry,
        }
    }

    /// One block of `frames`; every sample of the mix, which the
    /// instruments hold constant.
    fn block(&mut self, frames: u32) -> f32 {
        let mut output = vec![f32::NAN; frames as usize * 2];
        self.engine
            .render(
                &mut self.pool,
                &self.telemetry,
                frames,
                1_000_000,
                None,
                &mut output,
            )
            .unwrap();
        let first = output[0];
        assert!(output.iter().all(|sample| *sample == first), "{output:?}");
        first
    }
}

fn both_slots() -> Vec<RackSlotRuntimeSpec> {
    vec![
        slot("rack.test/a", "org.rackforge.test.held-a", None),
        slot("rack.test/b", "org.rackforge.test.held-b", None),
    ]
}

#[test]
fn every_instrument_in_a_rack_sounds_at_once() {
    for workers in [0, 4] {
        let mut rack = Harness::new(&both_slots(), workers);
        assert_eq!(rack.block(FRAMES), 0.0);
        rack.engine
            .route(note(0x90, 60), None, &mut Default::default());
        assert_eq!(rack.block(FRAMES), 11.0, "workers={workers}");
        rack.engine
            .route(note(0x80, 60), None, &mut Default::default());
        assert_eq!(rack.block(FRAMES), 0.0, "workers={workers}");
    }
}

#[test]
fn each_slot_hears_only_its_own_keys() {
    let lower = RackMidiTransform {
        note_high: 59,
        ..RackMidiTransform::default()
    };
    let upper = RackMidiTransform {
        note_low: 60,
        ..RackMidiTransform::default()
    };
    let mut rack = Harness::new(
        &[
            slot("rack.test/a", "org.rackforge.test.held-a", Some(lower)),
            slot("rack.test/b", "org.rackforge.test.held-b", Some(upper)),
        ],
        0,
    );
    rack.engine
        .route(note(0x90, 48), None, &mut Default::default());
    assert_eq!(rack.block(FRAMES), 1.0);
    rack.engine
        .route(note(0x90, 72), None, &mut Default::default());
    assert_eq!(rack.block(FRAMES), 11.0);
    rack.engine
        .route(note(0x80, 48), None, &mut Default::default());
    assert_eq!(rack.block(FRAMES), 10.0);
}

#[test]
fn a_slot_plays_at_its_level_and_pan() {
    let mut specs = both_slots();
    specs.truncate(1);
    specs[0].level_per_mille = 500;
    specs[0].pan_per_mille = 1_000;
    let mut rack = Harness::new(&specs, 0);
    rack.engine
        .route(note(0x90, 60), None, &mut Default::default());
    let mut output = vec![0.0; FRAMES as usize * 2];
    rack.engine
        .render(
            &mut rack.pool,
            &rack.telemetry,
            FRAMES,
            1_000_000,
            None,
            &mut output,
        )
        .unwrap();
    assert!(output.chunks_exact(2).all(|frame| frame == [0.0, 0.5]));
}

#[test]
fn a_host_block_shorter_than_the_rack_was_built_for_renders_in_place() {
    let mut rack = Harness::new(&both_slots(), 0);
    rack.engine
        .route(note(0x90, 60), None, &mut Default::default());
    assert_eq!(rack.block(FRAMES / 4), 11.0);
    assert_eq!(rack.block(FRAMES), 11.0);
    let mut too_long = vec![0.0; FRAMES as usize * 4];
    assert!(
        rack.engine
            .render(
                &mut rack.pool,
                &rack.telemetry,
                FRAMES * 2,
                1_000_000,
                None,
                &mut too_long
            )
            .is_err()
    );
}

#[test]
fn a_reset_lets_every_note_go() {
    let mut rack = Harness::new(&both_slots(), 0);
    rack.engine
        .route(note(0x90, 60), None, &mut Default::default());
    assert_eq!(rack.block(FRAMES), 11.0);
    rack.engine.reset();
    assert_eq!(rack.block(FRAMES), 0.0);
}
