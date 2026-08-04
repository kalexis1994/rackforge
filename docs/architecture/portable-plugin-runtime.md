# Portable plugin runtime

Status: first executable `wasm-v1` slice.

RackForge plugins target RackForge, not an operating system or CPU. The
portable package contains one WebAssembly payload. RackForge validates and
compiles that payload outside the audio thread, then caches host-specific code
as derived data. Native dynamic libraries remain a migration adapter only.

## Split contract

The control plane uses versioned WIT types for lifecycle, parameters, state,
programs and capabilities. The real-time DSP plane uses preallocated regions
inside the same isolated linear memory. Canonical WIT lists are deliberately
not lifted and copied for every audio block.

`wasm-v1` currently requires these exports:

```text
memory
rackforge_abi_version() -> i32  # 0x0001_0001
rackforge_input_ptr() -> i32
rackforge_output_ptr() -> i32
rackforge_capacity_input_samples() -> i32
rackforge_capacity_output_samples() -> i32
rackforge_midi_ptr() -> i32
rackforge_capacity_midi_events() -> i32
rackforge_parameter_ptr() -> i32
rackforge_capacity_parameter_events() -> i32
rackforge_transfer_ptr() -> i32
rackforge_capacity_transfer_bytes() -> i32
rackforge_initialize() -> i32
rackforge_prepare(sample_rate: f64, maximum_frames: i32,
                  input_channels: i32, output_channels: i32) -> i32
rackforge_set_parameter(index: i32, value: f64) -> i32
rackforge_get_parameter(index: i32) -> f64
rackforge_reset() -> i32
rackforge_resource_begin(id_length: i32, total_bytes: i64) -> i32
rackforge_resource_write(offset: i64, length: i32) -> i32
rackforge_resource_end() -> i32
rackforge_load_preset(length: i32) -> i32
rackforge_save_state() -> i32
rackforge_load_state(length: i32) -> i32
rackforge_process(frames: i32, input_channels: i32, output_channels: i32,
                  midi_event_count: i32, parameter_event_count: i32) -> i32
```

The Rust guest SDK owns these exports. Plugin authors implement a safe
`Processor` trait and never manipulate linear-memory addresses.

MIDI 1.0 messages of one to three bytes and parameter changes use fixed-size
events containing their sample offset. This covers notes, controllers, program
changes, pitch bend and sample-accurate automation without allocating. Audio
input and output capacities are independent, so an instrument may declare zero
inputs and a stereo output. SysEx is deliberately kept off this real-time path.
`reset` is lifecycle, not UI state: the host can force every voice and tail to
stop while independently placing the application in its idle navigation mode.

Resources, preset IDs and opaque state use a separate bounded transfer region
on the control thread. Large ROMs arrive in chunks, so the boundary never
requires an unbounded shared buffer. RackForge resolves and reads declared
resources; the guest never receives a host path or filesystem capability.

## Package workflow

The distributable contains one payload regardless of the target OS or CPU:

```text
cargo build -p rackforge-gain-portable --target wasm32-unknown-unknown --release
cargo run -p rackforge-store -- pack-wasm \
  plugins/gain-portable/package \
  target/wasm32-unknown-unknown/release/rackforge_gain_portable.wasm \
  target/rackforge-gain-portable-0.1.0.rfplugin
```

`pack-wasm` validates the manifest and WebAssembly magic and injects the built
artifact at the path declared by `[component]`. It does not copy a platform
binary into source control.

## Host invariants

- no ambient imports or WASI access in the DSP module;
- one isolated memory and instance per plugin instance;
- bounded linear memory and execution fuel;
- pointer, alignment, capacity and range validation at the boundary;
- no compilation, filesystem access or resource discovery on the audio thread;
- no platform paths, handles or APIs exposed to plugin code;
- a failed or exhausted instance is silenced and removed at a safe graph
  boundary rather than retried inside the callback.

The live host enforces that last invariant independently for each scope. A
standalone PLAY runtime failure moves audio rendering to silence without
terminating MIDI, control or the process. A failed Rack Slot is quarantined
while the remaining Slots continue rendering. The structured
`PLUGIN_PROCESS_QUARANTINED` diagnostic identifies the affected instance or
Slot. The runtime records the fuel consumed by the last process call even when
Wasmtime traps, so failures can be reproduced without raising the production
limit.

Portable instruments can be exercised outside the device audio loop with the
same sandbox and budget:

```text
rackforge-core stress PACKAGE --resource ID=PATH --preset PRESET_ID \
  --voices 28 --blocks 96 --frames 256
```

The command reports maximum fuel, peak and render-time ratio. It is a bounded
diagnostic and never grants a plugin additional execution budget.

The current host uses Wasmtime/Cranelift behind `rackforge-plugin-runtime`.
Core will depend on a RackForge-owned runtime trait so another compiler or a
non-JIT interpreter can be selected without changing plugin packages or the
SDK.

Production hosts construct `PortableEngine::with_cache` with a RackForge-owned
cache directory. Cranelift compiles the platform-neutral module for the current
CPU outside the audio thread and Wasmtime reuses that derived native code on
later launches. The cache may always be deleted or rebuilt; `.rfplugin` remains
the portable source of truth.

## Staging

The portable Gain plugin is the conformance seed. It proves that one guest
artifact can be loaded, prepared, parameterized and process interleaved audio.
It is not yet evidence of hard real-time behavior. Allocation, timing, cache
identity, fault recovery and ARM64 measurements must pass before an instrument
engine is migrated.
