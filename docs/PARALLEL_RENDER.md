# Parallel render (`parallel_render_v1`)

RackForge schedules audio as a global pool of ready jobs. A classic plugin is
one indivisible job per block. A plugin that declares `parallel_render_v1`
splits its block into a serial pre-stage, independent **units** the host may
render concurrently, and a serial post-stage — while the host keeps exclusive
ownership of every thread.

```toml
capabilities = [
    "audio_output",
    "parallel_render_v1",
]

[api]
major = 1
minor = 10
```

The capability requires a portable `wasm-v1` component and the
`audio_output` capability; the manifest validator rejects anything else, and
the loader rejects a package whose manifest and component disagree about the
extension (in either direction).

## Why not just a flag

A `multi_core = true` property cannot be scheduled. The extension is a real
three-phase contract:

| Phase | Runs on | Cardinality | Owns |
| --- | --- | --- | --- |
| `begin_block` | the coordinator | once per block | MIDI, sample-accurate automation, voice allocation, LFOs, noise, program state — every global decision |
| `render_unit` | any host worker | once per **active** unit | that unit's persistent DSP state, plus its dispatch payload and the block-shared payload |
| `end_block` | the coordinator | once per block | the deterministic combine and the global output stages |

The host may run the units in any order, on any of its own threads, or
strictly sequentially — the audio is identical, because:

* units read only their persistent state, their dispatch payload, the
  block-shared payload and the block's input audio;
* global state advances exactly once per block, in `begin_block`, and its
  values reach units *by value* inside payloads;
* `end_block` combines the deposited unit slots in ascending unit index — a
  fixed float summation order that does not depend on completion order.

## Two payloads: per-unit and block-shared

`begin_block` produces two kinds of data for the units:

* **Dispatch payloads** — one bounded slot per unit (`dispatch_stride`
  bytes): note events with their exact frames, per-voice assignments,
  anything unit-specific.
* **The block-shared payload** — one immutable region per block
  (`shared_capacity` bytes) that *every* unit receives identically. This is
  where sample-accurate shared signals live: per-frame LFO and noise
  arrays, mod-wheel / pitch-bend / aftertouch curves rendered per frame,
  intra-block automation segments. The coordinator computes them once;
  units never run their own generators, so nothing can drift between host
  paths.

Worker instances are isolated, so RackForge transports both payloads by
copying between the bounded, preallocated regions the component itself
exports — no per-sample imports, no allocation in the callback. MIDI and
parameter events themselves reach only the coordinator, with their exact
frame offsets.

## The wasm-v1 exports

Everything below is **optional** and versioned. A component that omits
`rackforge_parallel_abi_version` is a classic single-unit plugin.

```text
rackforge_parallel_abi_version() -> i32       ;; 0x0001_0000
rackforge_parallel_max_units() -> i32         ;; 1..=16
rackforge_parallel_dispatch_stride() -> i32   ;; bytes per payload slot, multiple of 8
rackforge_parallel_dispatch_ptr() -> i32      ;; max_units × stride bytes, 8-aligned
rackforge_parallel_shared_ptr() -> i32        ;; shared_capacity bytes, 8-aligned
rackforge_parallel_shared_capacity() -> i32   ;; positive multiple of 8
rackforge_parallel_plan_ptr() -> i32          ;; header + max_units entries, 4-aligned
rackforge_parallel_mix_ptr() -> i32           ;; max_units × capacity_output_samples f32

rackforge_parallel_begin_block(frames, input_channels, output_channels,
                               midi_count, parameter_count) -> i32
rackforge_parallel_render_unit(unit, payload_bytes, shared_bytes, frames,
                               output_channels) -> i32
rackforge_parallel_end_block(frames, output_channels) -> i32
```

`begin_block` consumes the standard input/MIDI/parameter regions (the same
ones `rackforge_process` uses) and returns the number of active units. The
**plan region** starts with an 8-byte header `{shared_payload_bytes: u32,
reserved: u32}` followed by one `{unit: u32, payload_bytes: u32}` entry per
active unit, with strictly increasing unit indices. The host validates all
of it: duplicate or out-of-range units, payloads beyond the stride and
shared sizes beyond the capacity are rejected and quarantine the Slot.

`render_unit` reads its dispatch slot and the shared region (the host wrote
both into the worker instance) and writes the standard output region.
`end_block` reads the mix region — where the host deposited each finished
unit at its own slot — and writes the final block to the standard output
region.

The classic `rackforge_process` export **must remain present and must sound
identical**: it is the sequential fallback used verbatim by single-core
hosts and by the browser, from exactly the same `.rfplugin`. Plugins built
with `rackforge-plugin-sdk`'s `export_parallel_processor!` get that
composition generated from the same `begin_block`/`render_unit`/`end_block`
kernels, so a second algorithm cannot exist by construction.

## How the host runs it

WebAssembly instances are not concurrently reentrant — one store must never
be entered from two threads at once. RackForge therefore creates, per
Rack Slot of a parallel plugin:

* one **coordinator** instance (the one the control plane talks to), and
* one **worker instance per unit**, holding that unit's persistent DSP
  state.

### Unit identity is physical

Unit *k* always renders inside worker instance *k*: its oscillator phases,
envelopes, filters, calibration, drift and numeric history stay put for the
lifetime of the Slot. What migrates between workers is only the *job* of
entering that instance for one block. A unit can never "become" another
unit by being claimed by a different worker; the scheduler claims per-unit
bits and the job table is indexed by unit, not by worker.

### The block

MIDI is delivered only to the coordinator. Per block the host:

1. runs `begin_block` on the coordinator (as one pool job);
2. copies each announced dispatch payload and one copy of the shared
   payload from the coordinator into the matching unit instances;
3. schedules the unit renders across its one global worker pool — a worker
   that finishes another instrument steals pending units;
4. deposits every unit's audio, in ascending unit order and silencing
   failed units, into the coordinator's mix region and runs `end_block`.

No nested pools, no plugin threads, no additional block of latency: begin,
units and end all happen inside the same device period.

This applies to **every render mode**, not only Racks. The pool is untyped:
each block publishes the Slot type's entry points alongside the job graph,
so PLAY mode — the single standalone instrument on the embedded host and on
the desktop — schedules its units across exactly the same workers a Rack
would use. One pool per process, whatever is playing.

### Cabled Racks are part of the same graph

A Rack with cables does **not** fall back to serial processing. Every Slot
carries a dependency mask over earlier Slots (the compiled Rack order is
topological by construction); the scheduler holds a downstream Slot in a
blocked phase until each of its sources completed its block, then a worker
gathers the finished upstream outputs into the Slot's input and runs it.
So `RF-5 → effect → master` executes as:

```text
rf5.begin ──► rf5.render_unit × N ──► rf5.end ──► effect ──► (host mix)
                     (any workers)
```

while independent branches — and independent instruments — run in parallel
with the whole chain. Only a graph that is not this shape (a true cycle,
feedback, a mask naming a later Slot) is refused by the pool and executed
by the sequential fallback in declaration order; today's Rack compiler
cannot produce such a graph.

### Control-plane synchronization

The coordinator is canonical for MIDI, voice allocation, shared generators,
program, automation and global state. Control operations are synchronized
by **mirroring the same canonical input** to every instance:

| Operation | What the host does |
| --- | --- |
| `prepare` | applied to coordinator and every unit instance |
| `reset` | applied to coordinator and every unit instance |
| `set_parameter` | same index/value applied to every instance |
| `load_preset` | same program id applied to every instance |
| `load_state` | same canonical snapshot bytes applied to every instance |
| `save_state` | read from the **coordinator only** — it is the authority |
| resources | delivered identically to every instance at creation |

Worker instances therefore never evolve global state on their own: their
mirrored globals only change at control-plane granularity, and everything
per-block flows through the payloads. Plugins must not let `render_unit`
read mirrored globals for anything automation can change — the SDK enforces
this shape by giving `render_unit` no access to the coordinator at all.

### Adaptive scheduling

The pool is never mandatory. Per block it computes the graph's *width* —
`Σ max(1, max_units)` over the Slots, plain integer math, no measurement,
no allocation — and hands anything with width < 2 (one classic plugin, one
single-unit plugin) to the sequential executor, where synchronization would
cost more than the work. Fewer than two workers (single core,
`RACKFORGE_AUDIO_WORKERS=0|1`, the browser) always means the sequential
fallback, and in that case no unit instances are created at all.

### Faults

A unit that traps (fuel exhaustion, `unreachable`, memory faults) is
silenced for that block, counted in telemetry, and quarantined so later
blocks skip it instead of burning its fuel budget again; the rest of the
plugin keeps sounding. A failing `begin_block`/`end_block` — including an
invalid plan (duplicate units, oversized payloads, oversized shared size) —
quarantines the whole Slot, exactly like a classic process failure.
Nothing is printed from the audio threads — the telemetry publisher reports
`AUDIO_RENDER_UNIT_FAULT` / `AUDIO_RENDER_SLOT_FAULT` lines from its own
thread.

### Memory trade-off

Each unit instance is a full instantiation of the component, including its
linear memory and delivered resources. A five-unit synth costs six
instances. This is the deliberate price of isolation; plugins with very
large resident resources (multi-hundred-megabyte sample banks) should weigh
it before declaring the capability. The sequential fallback is always legal
for a host under memory pressure.

## Determinism rules (normative)

1. Unit output may depend only on: the unit's persistent state, its
   dispatch payload, the block-shared payload, the block input audio, and
   the mirrored control-plane state.
2. Global state advances only in `begin_block`/`end_block`; shared signals
   reach units by value, per frame where sample accuracy matters.
3. `end_block` combines active units in ascending unit index.
4. Plan entries are strictly increasing; payloads never exceed the declared
   stride; the shared size never exceeds the declared capacity.
5. Save/restore, program changes and voice allocation live in the
   coordinator; restoring state and replaying the same events reproduces
   the same audio on 1, 2, 3 or 4 workers, bit for bit. RackForge's test
   suite holds the reference implementation to *exact* equality — audio and
   final state — including across mid-activity program changes and resets.
   A plugin doing its own cross-unit reductions must document any weaker
   bound.

## Writing one with the SDK

```rust
use rackforge_plugin_sdk::{
    BlockContext, ParallelProcessor, PlanWriter, UnitContext, UnitMix,
    export_parallel_processor,
};

struct Rf5 { /* voice allocator, LFOs, settings, master … */ }
#[derive(Default)]
struct Rf5Voice { /* oscillators, envelope, filter, calibration … */ }

impl ParallelProcessor for Rf5 {
    type Unit = Rf5Voice;

    fn begin_block(&mut self, ctx: &BlockContext<'_>, plan: &mut PlanWriter<'_>) {
        // parse ctx.midi / ctx.parameters (exact frames), advance the
        // global LFOs and noise once — per frame — into the shared payload:
        //     let shared = plan.shared_buffer();
        //     … write per-frame arrays …
        //     plan.commit_shared(bytes);
        // then allocate voices and, per sounding voice:
        //     plan.activate(unit, &payload_bytes);
    }

    fn render_unit(_unit: u32, voice: &mut Rf5Voice, payload: &[u8],
                   ctx: &UnitContext<'_>, output: &mut [f32]) {
        // render from `voice`, `payload` and `ctx.shared` only — there is
        // no `&self` here, so coordinator state is unreachable by design.
    }

    fn end_block(&mut self, mix: &UnitMix<'_>, output: &mut [f32],
                 frames: u32, channels: u32) {
        // sum mix.active_units() in order, then global filter/FX/master.
    }

    // prepare / set_parameter / save_state / load_preset / … as usual;
    // reset_unit resets one voice's persistent state.
}

export_parallel_processor!(
    Rf5,
    max_units = 5,
    dispatch_stride = 64,
    shared_capacity = 16384,   // e.g. one f32 of LFO per frame
    max_frames = 4096,
    max_input_channels = 0,
    max_output_channels = 2,
    max_midi_events = 256,
    max_parameter_events = 256,
    max_transfer_bytes = 4096
);
```

`plugins/parallel-demo-synth` is the complete worked example: a five-voice
instrument whose coordinator allocates voices, renders a per-frame vibrato
LFO into the block-shared payload and distributes note events with exact
frames through dispatch payloads, with tests proving the composed
sequential export matches manual stage execution sample for sample. Its
package under `plugins/parallel-demo-synth/package` shows the manifest;
build the component with:

```bash
cargo build --release --target wasm32-unknown-unknown -p rackforge-parallel-demo-synth
```

## Telemetry

The host aggregates, off the audio threads, and prints once per second:

```text
AUDIO_RENDER_BLOCK blocks=… avg_us=… p95_us=… p99_us=… max_us=… deadline_us=… budget_avg_pct=… budget_max_pct=… deadline_misses=…
AUDIO_RENDER_STAGE slot=<id> stage=process|begin|unit|end count=… avg_us=… p95_us=… p99_us=… max_us=…
AUDIO_RENDER_DEADLINE_MISS slot=<id> stage=… count=…
AUDIO_RENDER_SLOT_FAULT / AUDIO_RENDER_UNIT_FAULT slot=<id> count=…
AUDIO_RENDER_WORKERS units=[…] busy_pct=[…]
```

Worker count remains automatic (`cpus - 1`, capped by the Slot limit) and
`RACKFORGE_AUDIO_WORKERS` still overrides it.
