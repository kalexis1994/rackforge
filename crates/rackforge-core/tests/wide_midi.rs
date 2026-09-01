//! The wide-MIDI contract, end to end through the host: a packaged wasm-v1
//! probe that exports the four optional symbols receives the families it
//! declared as 16-byte events and everything else as MIDI 1.0 bytes, each
//! event exactly once; a probe without the symbols is entered through the
//! narrow path and never sees a wide event; a probe exporting only part of
//! the contract is refused at load.
//!
//! The probe writes what it was handed into its output block, so the test
//! reads the delivered event back at the byte offsets the host ABI documents
//! (frame; kind, channel, index, flags one byte each; value; extra) -- the
//! layout the runtime writes and an SDK component decodes.
#![cfg(not(target_arch = "wasm32"))]

use rackforge_core::midi2::{Midi2Event, Midi2Message};
use rackforge_core::{LoadedPlugin, PluginPackage};
use rackforge_plugin_api::abi::{
    MIDI_FAMILY_NOTE, MIDI2_FLAG_ORIGIN_7BIT, MIDI2_KIND_CONTROL_CHANGE, MIDI2_KIND_NOTE_ON,
    MidiEventV1,
};
use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

const WIDE_EXPORTS: &str = r#"
      (func (export "rackforge_midi2_ptr") (result i32) i32.const 6144)
      (func (export "rackforge_capacity_midi2_events") (result i32) i32.const 4)
      (func (export "rackforge_midi2_families") (result i32) i32.const 1)
      (func (export "rackforge_process_v2") (param $frames i32) (param $in i32) (param $out i32) (param $midi i32) (param $parameters i32) (param $midi2 i32) (result i32)
        i32.const 1024 local.get $midi f32.convert_i32_s f32.store
        i32.const 1028 local.get $midi2 f32.convert_i32_s f32.store
        i32.const 1032 i32.const 6144 i32.load f32.convert_i32_u f32.store
        i32.const 1036 i32.const 6148 i32.load8_u f32.convert_i32_u f32.store
        i32.const 1040 i32.const 6149 i32.load8_u f32.convert_i32_u f32.store
        i32.const 1044 i32.const 6150 i32.load8_u f32.convert_i32_u f32.store
        i32.const 1048 i32.const 6151 i32.load8_u f32.convert_i32_u f32.store
        i32.const 1052 i32.const 6152 i32.load f32.convert_i32_u f32.store
        i32.const 1056 i32.const 6156 i32.load f32.convert_i32_u f32.store
        i32.const 1060 i32.const 4100 i32.load8_u f32.convert_i32_u f32.store
        i32.const 0)
"#;

/// The narrow entry leaves a marker no wide block produces, plus the count.
fn probe(wide: &str) -> String {
    format!(
        r#"
    (module
      (memory (export "memory") 1)
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
      (func (export "rackforge_reset") (result i32) i32.const 0)
      (func (export "rackforge_resource_begin") (param i32 i64) (result i32) i32.const -3)
      (func (export "rackforge_resource_write") (param i64 i32) (result i32) i32.const -3)
      (func (export "rackforge_resource_end") (result i32) i32.const -3)
      (func (export "rackforge_load_preset") (param i32) (result i32) i32.const 0)
      (func (export "rackforge_save_state") (result i32)
        i32.const 8192 i32.const 0 i32.store
        i32.const 4)
      (func (export "rackforge_load_state") (param i32) (result i32) i32.const 0)
      (func (export "rackforge_process") (param $frames i32) (param $in i32) (param $out i32) (param $midi i32) (param $parameters i32) (result i32)
        i32.const 1024 f32.const -1 f32.store
        i32.const 1028 local.get $midi f32.convert_i32_s f32.store
        i32.const 0)
      {wide}
    )
"#
    )
}

const MANIFEST: &str = r#"
schema_version = 1
id = "org.rackforge.wide-midi-probe"
name = "Wide MIDI Probe"
vendor = "RackForge"
version = "0.1.0"
kind = "instrument"
state_version = 1
capabilities = ["audio_output", "midi_input", "presets", "state"]

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
  "id": "org.rackforge.wide-midi-probe",
  "version": "0.1.0",
  "state_version": 1
}"#;

const PARAMETERS_JSON: &str = r#"{
  "schema_version": 1,
  "pages": [{ "id": "main", "name": "Main", "order": 0 }],
  "parameters": [
    {
      "index": 0,
      "id": "level",
      "name": "Level",
      "page": "main",
      "order": 0,
      "kind": { "type": "float", "minimum": 0.0, "maximum": 4.0, "default": 3.0, "step": 1.0 },
      "flags": { "automatable": true }
    }
  ]
}"#;

const PRESETS_JSON: &str = r#"{
  "schema_version": 1,
  "banks": [{ "id": "factory", "name": "Factory", "order": 0 }],
  "presets": [{ "id": "init", "name": "Init", "bank": "factory", "order": 0 }]
}"#;

static SERIAL: AtomicU64 = AtomicU64::new(0);

fn load(wat_source: &str) -> anyhow::Result<LoadedPlugin> {
    let root = std::env::temp_dir().join(format!(
        "rackforge-wide-midi-test-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let metadata = root.join("metadata");
    fs::create_dir_all(&metadata).unwrap();
    fs::write(root.join("rackforge-plugin.toml"), MANIFEST).unwrap();
    fs::write(
        root.join("component.wasm"),
        wat::parse_str(wat_source).unwrap(),
    )
    .unwrap();
    fs::write(metadata.join("runtime.json"), RUNTIME_JSON).unwrap();
    fs::write(metadata.join("parameters.json"), PARAMETERS_JSON).unwrap();
    fs::write(metadata.join("presets.json"), PRESETS_JSON).unwrap();
    let package = PluginPackage::open(&root)?;
    // SAFETY: portable wasm-v1 packages execute inside the sandbox.
    unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), None) }
}

const FRAMES: u32 = 64;

fn narrow(frame: u32, data: [u8; 3]) -> MidiEventV1 {
    MidiEventV1 {
        frame,
        length: 3,
        data,
    }
}

/// What the probe reported: narrow count, wide count, then the first wide
/// event's frame, kind, channel, index, flags, value, extra, and the first
/// narrow event's status byte.
fn report(output: &[f32]) -> [f32; 10] {
    output[..10].try_into().unwrap()
}

#[test]
fn a_declared_family_arrives_wide_and_the_rest_narrow() {
    let loaded = load(&probe(WIDE_EXPORTS)).unwrap();
    let mut instance = loaded.create_instance().unwrap();
    assert_eq!(instance.midi2_families(), MIDI_FAMILY_NOTE);
    instance.activate(48_000.0, FRAMES, 0, 2).unwrap();
    let mut output = vec![0.0f32; FRAMES as usize * 2];

    // Through the MIDI 1.0 API: the note is lifted, cut wide, and carries the
    // origin flag; the controller stays three bytes. Both counted once.
    let on = narrow(5, [0x90, 60, 100]);
    let events = [narrow(0, [0xB0, 1, 2]), on];
    instance
        .process_interleaved(&[], &mut output, FRAMES, 0, 2, &events, &[])
        .unwrap();
    let expected = Midi2Event::from_midi1(&on).to_v2().unwrap();
    assert_eq!(expected.flags, MIDI2_FLAG_ORIGIN_7BIT);
    assert_eq!(
        report(&output),
        [
            1.0,
            1.0,
            5.0,
            MIDI2_KIND_NOTE_ON as f32,
            0.0,
            60.0,
            MIDI2_FLAG_ORIGIN_7BIT as f32,
            expected.value as f32,
            0.0,
            0xB0 as f32,
        ]
    );

    // Through the vocabulary: a velocity no seven-bit source can express
    // reaches the component whole, with no origin flag.
    let wide = Midi2Event {
        frame: 7,
        channel: 2,
        message: Midi2Message::NoteOn {
            note: 61,
            velocity: 0xFFFF,
        },
        origin_7bit: false,
    };
    instance
        .process_wide(&[], &mut output, FRAMES, 0, 2, &[wide], &[])
        .unwrap();
    // (The narrow region is written only as far as the block's narrow
    // count, so with none delivered its first status byte is stale.)
    assert_eq!(
        report(&output)[..9],
        [
            0.0,
            1.0,
            7.0,
            MIDI2_KIND_NOTE_ON as f32,
            2.0,
            61.0,
            0.0,
            65535.0,
            0.0
        ]
    );

    // A family the component did not declare stays narrow even when the
    // host holds it wide: the 32-bit controller is scaled back to its byte.
    let controller = Midi2Event {
        frame: 3,
        channel: 0,
        message: Midi2Message::ControlChange {
            controller: 64,
            value: u32::MAX,
        },
        origin_7bit: false,
    };
    assert_eq!(controller.to_v2().unwrap().kind, MIDI2_KIND_CONTROL_CHANGE);
    instance
        .process_wide(&[], &mut output, FRAMES, 0, 2, &[controller], &[])
        .unwrap();
    let delivered = report(&output);
    assert_eq!(delivered[0], 1.0);
    assert_eq!(delivered[1], 0.0);
    assert_eq!(delivered[9], 0xB0 as f32);
}

#[test]
fn a_component_without_the_contract_is_entered_narrow() {
    let loaded = load(&probe("")).unwrap();
    let mut instance = loaded.create_instance().unwrap();
    assert_eq!(instance.midi2_families(), 0);
    instance.activate(48_000.0, FRAMES, 0, 2).unwrap();
    let mut output = vec![0.0f32; FRAMES as usize * 2];
    let wide = Midi2Event {
        frame: 0,
        channel: 0,
        message: Midi2Message::NoteOn {
            note: 60,
            velocity: 0xFFFF,
        },
        origin_7bit: false,
    };
    instance
        .process_wide(&[], &mut output, FRAMES, 0, 2, &[wide], &[])
        .unwrap();
    // The narrow entry ran, and it received the note as one narrow event.
    assert_eq!(output[0], -1.0);
    assert_eq!(output[1], 1.0);
}

#[test]
fn a_partial_contract_is_refused_at_load() {
    let without_entry: String = WIDE_EXPORTS
        .lines()
        .take_while(|line| !line.contains("rackforge_process_v2"))
        .collect::<Vec<_>>()
        .join("\n");
    let error = load(&probe(&without_entry))
        .err()
        .expect("partial contract must not load");
    assert!(
        format!("{error:#}").contains("part of the wide-MIDI contract"),
        "{error:#}"
    );
}
