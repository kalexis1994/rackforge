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
| `begin_block` | the coordinator | once per block | MIDI, sample-accurate automation, voice allocation, LFOs, noise, every global decision |
| `render_unit` | any host worker | once per **active** unit | that unit's persistent DSP state plus its dispatch payload |
| `end_block` | the coordinator | once per block | the deterministic combine and the global output stages |

The host may run the units in any order, on any of its own threads, or
strictly sequentially — the audio is identical, because:

* units read only their persistent state and the payload `begin_block` wrote
  for them;
* global state advances exactly once per block, in `begin_block`, and its
  values reach units *by value* inside payloads;
* `end_block` combines the deposited unit slots in ascending unit index — a
  fixed float summation order that does not depend on completion order.

## The wasm-v1 exports

Everything below is **optional** and versioned. A component that omits
`rackforge_parallel_abi_version` is a classic single-unit plugin.

```text
rackforge_parallel_abi_version() -> i32      ;; 0x0001_0000
rackforge_parallel_max_units() -> i32        ;; 1..=16
rackforge_parallel_dispatch_stride() -> i32  ;; bytes per payload slot, 8-aligned
rackforge_parallel_dispatch_ptr() -> i32     ;; max_units × stride bytes, 8-aligned
rackforge_parallel_plan_ptr() -> i32         ;; max_units × {unit: u32, payload: u32}
rackforge_parallel_mix_ptr() -> i32          ;; max_units × capacity_output_samples f32

rackforge_parallel_begin_block(frames, input_channels, output_channels,
                               midi_count, parameter_count) -> i32
rackforge_parallel_render_unit(unit, payload_bytes, frames,
                               output_channels) -> i32
rackforge_parallel_end_block(frames, output_channels) -> i32
```

`begin_block` consumes the standard input/MIDI/parameter regions (the same
ones `rackforge_process` uses), returns the number of active units, and
fills the plan region with strictly increasing unit indices plus per-unit
payload sizes. `render_unit` reads its dispatch slot and writes the standard
output region. `end_block` reads the mix region — where the host deposited
each finished unit at its own slot — and writes the final block to the
standard output region.

The classic `rackforge_process` export **must remain present and must sound
identical**: it is the sequential fallback used verbatim by single-core
hosts and by the browser, from exactly the same `.rfplugin`. Plugins built
with `rackforge-plugin-sdk`'s `export_parallel_processor!` get that
composition generated from the same stage functions, so divergence is not
possible by construction.

## How the host runs it

WebAssembly instances are not concurrently reentrant — one store must never
be entered from two threads at once. RackForge therefore creates, per
Rack Slot of a parallel plugin:

* one **coordinator** instance (the one the control plane talks to), and
* one **worker instance per unit**, holding that unit's persistent DSP
  state.

MIDI is delivered only to the coordinator. The host copies each announced
dispatch payload from the coordinator into the matching unit's instance,
schedules the unit renders across its one global worker pool (a worker that
finishes another instrument steals pending units), then deposits every
unit's audio — in unit order, silencing failed units — into the
coordinator's mix region and calls `end_block`. No nested pools, no plugin
threads, no additional block of latency: begin, units and end all happen
inside the same device period.

Control-plane operations (`prepare`, `set_parameter`, `load_preset`,
`load_state`, `reset`, resource delivery) are **mirrored** by the host to
every instance so control state can never depend on scheduling. Per-block
dynamics must still travel through payloads: a worker instance's mirrored
globals are only guaranteed to match at control-plane granularity.

With fewer than two workers (single core, `RACKFORGE_AUDIO_WORKERS=0|1`,
`graph_audio` racks on the sequential path, or the browser) the host uses
the classic `rackforge_process` export and creates no unit instances at
all.

### Faults

A unit that traps (fuel exhaustion, `unreachable`, memory faults) is
silenced for that block, counted in telemetry, and quarantined so later
blocks skip it instead of burning its fuel budget again; the rest of the
plugin keeps sounding. A failing `begin_block`/`end_block` quarantines the
whole Slot, exactly like a classic process failure. Nothing is printed from
the audio threads — the telemetry publisher reports
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

1. Unit output may depend only on: the unit's persistent state, the payload,
   the block input audio, and the mirrored control-plane state.
2. Global state advances only in `begin_block`/`end_block`.
3. `end_block` iterates active units in ascending unit index.
4. Active plan entries are strictly increasing; payloads never exceed the
   declared stride.
5. Save/restore, program changes and voice allocation live in the
   coordinator; restoring state and replaying the same events reproduces
   the same audio on 1, 2, 3 or 4 workers, bit for bit. RackForge's test
   suite holds the reference implementation to *exact* equality; a plugin
   doing its own cross-unit reductions must document any weaker bound.

## Writing one with the SDK

```rust
use rackforge_plugin_sdk::{
    BlockContext, ParallelProcessor, PlanWriter, UnitContext, UnitMix,
    export_parallel_processor,
};

struct Rf5 { /* voice allocator, LFOs, settings, master … */ }
#[derive(Default)]
struct Rf5Voice { /* oscillators, envelope, filter … */ }

impl ParallelProcessor for Rf5 {
    type Unit = Rf5Voice;

    fn begin_block(&mut self, ctx: &BlockContext<'_>, plan: &mut PlanWriter<'_>) {
        // parse ctx.midi / ctx.parameters, advance LFOs once,
        // allocate voices, then for each sounding voice:
        //     plan.activate(unit, &payload_bytes);
    }

    fn render_unit(_unit: u32, voice: &mut Rf5Voice, payload: &[u8],
                   ctx: &UnitContext<'_>, output: &mut [f32]) {
        // render from `voice` + `payload` only — no coordinator access,
        // by construction: there is no `&self` here.
    }

    fn end_block(&mut self, mix: &UnitMix<'_>, output: &mut [f32],
                 frames: u32, channels: u32) {
        // sum mix.active_units() in order, then global filter/FX/master.
    }

    // set_parameter / save_state / load_preset / … as usual.
}

export_parallel_processor!(
    Rf5,
    max_units = 5,
    dispatch_stride = 64,
    max_frames = 4096,
    max_input_channels = 0,
    max_output_channels = 2,
    max_midi_events = 256,
    max_parameter_events = 256,
    max_transfer_bytes = 4096
);
```

`plugins/parallel-demo-synth` is the complete worked example: a five-voice
instrument whose coordinator allocates voices and distributes a shared
vibrato LFO through payloads, with tests proving the composed sequential
export matches manual stage execution sample for sample. Its package under
`plugins/parallel-demo-synth/package` shows the manifest; build the
component with:

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
