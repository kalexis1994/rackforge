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
rackforge_parallel_unit_channels() -> i32     ;; OPTIONAL; f32 a unit writes per frame

rackforge_parallel_begin_block(frames, input_channels, output_channels,
                               midi_count, parameter_count) -> i32
rackforge_parallel_render_unit(unit, payload_bytes, shared_bytes, frames,
                               output_channels) -> i32
rackforge_parallel_end_block(frames, output_channels) -> i32

;; with the wide-MIDI contract (rackforge_midi2_* and rackforge_process_v2):
rackforge_parallel_begin_block_v2(frames, input_channels, output_channels,
                                  midi_count, parameter_count,
                                  midi2_count) -> i32
```

`begin_block` consumes the standard input/MIDI/parameter regions (the same
ones `rackforge_process` uses) and returns the number of active units. A
component that also takes MIDI at 2.0 widths — the four optional exports in
[PLUGIN_ABI.md](PLUGIN_ABI.md) — must export `begin_block_v2` as well, and
the host then enters the coordinator through it on every block, wide events
or not, exactly as it enters `rackforge_process_v2`: the families the
component declared wide arrive in the MIDI 2.0 region at their full width,
everything else as MIDI 1.0 bytes, no event in both. A pooled render and
the sequential fallback therefore hand the coordinator the same events. The
host refuses at load a component that exports one side of that pair without
the other. The
**plan region** starts with an 8-byte header `{shared_payload_bytes: u32,
reserved: u32}` followed by one `{unit: u32, payload_bytes: u32}` entry per
active unit, with strictly increasing unit indices. The host validates all
of it: duplicate or out-of-range units, payloads beyond the stride and
shared sizes beyond the capacity are rejected and quarantine the Slot.

### A unit does not always produce audio

`rackforge_parallel_unit_channels` says how many floats a unit writes per
frame. A component that does not export it, or exports zero, means the
plugin's output channel count -- what every component meant before the
export existed, and what is right whenever a unit produces finished audio.
The host then copies `frames × output_channels` out of each unit, as it
always did.

It is not right for every decomposition, and the case that forced this is
worth stating. An instrument with ONE resonating body has units that produce
an **intermediate** signal: the Concert Grand's four string sections each
hand over a bridge force, sixteen bridge drive points and two keybed
contributions -- nineteen floats a frame -- and one shared serial stage
turns those into sound. The board cannot be divided with them, because its
modes are driven by bridge points that depend on every section, and reading
those a block late was rendered and rejected by ear ("pierde una pizca de
ataque"). Two channels cannot carry nineteen floats, and truncating them in
silence is worse than refusing them.

So a unit declares its own width, through `ParallelProcessor::UNIT_CHANNELS`
in the SDK. Two rules come with it:

* The region a unit writes is sized `max_frames × max_output_channels`, so a
  widened unit must keep `frames × UNIT_CHANNELS` inside that -- it borrows
  the headroom a short block leaves rather than growing the static. The
  generated `render_unit` checks and returns `STATUS_INVALID_ARGUMENT`
  rather than writing past the end.
* `UnitMix::unit()` hands `end_block` `frames × UNIT_CHANNELS`, so the
  combine reads what the units actually wrote. For an audio decomposition
  that is unchanged.

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
| program editing (`rackforge_program_*`) | coordinator only — the SDK forwards the `ParallelProcessor` program-editing methods, which default to none |

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

### Timed work for a unit

`PlanWriter::activate` takes bytes, which is the right primitive and the
wrong place to stop. A unit renders a whole span at once and cannot be told
anything halfway through, so an instrument needs to hand it a LIST: a note
struck at frame 5, a damper released at frame 37. Without a shared shape for
that, every plugin invents one and makes its own mistakes about bounds and
about frames outside the block.

`UnitWork` is that shape. Records of `{frame: u16, length: u16, bytes}`,
four-byte aligned, pushed in ascending frame order and read back the same
way:

```rust
// begin_block, on the coordinator
let mut work = UnitWork::new(plan.dispatch_buffer(unit));
work.push(5, &strike.to_bytes());
work.push(37, &release.to_bytes());
plan.activate(unit, work.finish());

// render_unit, in the worker
for (frame, bytes) in UnitWork::read(payload) {
    // apply it where it belongs
}
```

The bytes inside a record stay the plugin's own business -- the host never
looks at a payload. What the shape buys is the three refusals: a record
longer than a `u16`, a frame that goes backwards, and a payload that is
full are all refused at the push rather than truncated at the read. And a
malformed payload ends the walk instead of panicking, because a unit is on
the audio thread.

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

Add a `midi2 = { max_events = 256, families = MIDI_FAMILY_NOTE | … }` clause
to the same invocation for a coordinator that takes those families at MIDI
2.0 widths: `begin_block` then finds them in `ctx.midi2`, the macro exports
the wide contract and `rackforge_parallel_begin_block_v2` alongside, and
the package declares `[api] minor = 12`. The `ParallelProcessor` trait also
carries the program-editing methods of `Processor`, so an instrument with an
editor keeps it when it splits into units.

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


## The browser host

The full-web RackForge runs the same extension on Web Workers. The mapping
from the native design, piece for piece:

| native                            | browser                                  |
| --------------------------------- | ---------------------------------------- |
| coordinator wasmtime instance     | coordinator instance in the AudioWorklet |
| isolated worker instance per unit | unit instances in Web Workers            |
| shared process memory             | one `SharedArrayBuffer`                  |
| workers park on a futex           | workers sleep in `Atomics.wait`          |
| audio thread parks briefly        | worklet spins with a bounded budget      |

The audio thread cannot block (`Atomics.wait` is forbidden in an
AudioWorkletGlobalScope) and cannot spawn workers, so the page owns the pool:
the worklet asks for one (`pool_request`), the page builds workers and the
buffer and attaches them (`pool_attach`), and until the attach lands — or on
any page that is not cross-origin isolated, ever — every block takes the
classic sequential `rackforge_process` path the extension guarantees.

Two deliberate departures from native:

* **Unit affinity replaces work stealing.** A unit's persistent state lives
  inside one worker's wasm instance, so unit N is always rendered by worker
  `N % workers`. Determinism is unaffected: the combine still happens in the
  coordinator's `end_block` in ascending unit order.
* **A missed deadline costs audio, not coherence.** The worklet spins for
  three quarters of the block budget; a unit that does not arrive contributes
  silence for that block and increments a miss counter in the shared header.
  Its state still advanced inside its worker, so the next block is correct.

Cross-origin isolation is served three ways: the desktop and Pi gateways set
`Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp` on every SPA response, the Vite
dev server mirrors them, and on hosts that cannot set headers (GitHub Pages)
the service worker stamps them onto the responses it answers — the first
visit runs sequential, every controlled visit after is isolated.

Scope: the browser pool parallelises the units of the active instrument, the
dominant browser-host case. Cabled-rack parallelism across plugin instances
stays native-only — it rides on the core `RenderPool`, which needs threads
the wasm host does not have.

## What it bought, on real hardware

The Concert Grand is the first instrument to declare the extension: four
string sections, one unit each, twenty floats a frame per unit folded down to
stereo by `end_block`. Measured on the Raspberry Pi 4 appliance, 128-frame
blocks against a 2666 µs deadline, the densest thirty seconds of La
Campanella, the same binary and the same component both ways:

| | mean | p99 | deadline misses | governor |
| --- | --- | --- | --- | --- |
| sequential (`RACKFORGE_AUDIO_WORKERS=0`) | 1430 µs | 2883 µs | 54 | tightened, `late_pct=23.4` |
| four cores (auto, three workers) | 1112 µs | 2359 µs | 5 | no cuts |

The gap is wider than the means suggest: the sequential figure is what the
instrument costs *after* the governor cut quality to survive, while the
parallel one never had to give ground.

`tests/parallel_overhead.rs` splits the cost three ways — one instance
rendering a whole block, the same units run inline on one thread, and the
same units across the real pool — which is the only split that separates
transport from threading. On the appliance the transport costs 30 µs a block
idle and 111 µs under a chord, and the threading 58 µs idle and *minus*
214 µs under a chord: once there is real work the pool finishes ahead of a
single instance. An earlier claim in this project that the transport cost
~450 µs was wrong, and it was wrong in the usual way — two numbers compared
across different states, one of them measured while every unit was being
refused.

### The floor, which is where the remaining work is

The same measurement says an idle block costs 1039 µs before a single note
sounds, and about 900 µs of that is the global stage: the soundboard, the
sympathetic bank and the room, which run every block whether or not anything
is ringing. A third of the deadline, paid identically in both render paths.

That is why the notes barely show. La Campanella's densest passage means
1112 µs and the idle floor is 1039: the string work is spread across cores
and disappears into the space the global stage was not using. Nothing in the
scheduler moves this, because Amdahl charges for it either way.

So any further gain has to come from the global stage itself, and the only
obvious lever — not running it, or running it cheaper, when no string holds
energy — is exactly the mechanism that cuts a decaying tail and silences the
sympathetic bloom under a chord that is still breathing. That would be an
ear-level decision, not a profiling one — and the measurement below says to
exhaust the arrangements of the bank first, because none of those requires a
decision about how the instrument sounds.

### What the global stage is made of

Priced by ablation — building the instrument with one group of banks given
an empty range, so everything downstream still runs on the zeros those loops
would have left. Each figure is therefore a floor on what that group costs,
never an overstatement. Measured on a developer machine, so read the
proportions and not the microseconds; the appliance puts the whole stage at
about three and a half times these numbers.

| | idle | a held chord | share |
| --- | --- | --- | --- |
| the soundboard's modal bank | 137 µs | 202 µs | **~69 %** |
| the sympathetic banks (open, undamped, silent keys, bed) | 90 µs | 113 µs | ~39 % |
| the room (early reflections and the chamber) | 0 µs | 17 µs | ~6 % |

The room is very nearly free and is not worth touching. The soundboard's
256-mode bank is the bill.

Two things about that bank were measured rather than assumed, and the
measurements disagree with each other depending on where they are taken.

It is an array of 56-byte structs. `simd128` is on for every wasm build in
this workspace, and consecutive modes' state is 56 bytes apart, so a
four-wide load would need gathers — it looks like a textbook failure to
vectorise. Laying the bank out as one array per field is bit-for-bit
identical, and:

| | x86 | the appliance's ARM cores, natively |
| --- | --- | --- |
| array of structs, as it is | 1.00× | 1.00× |
| one array per field | 0.93× | **1.41×** |
| one array per field, sum split four ways | 1.28× | 0.99× |
| only the sum split, layout untouched | 0.78× | 0.89× |

Neither loop vectorises on either machine: adding 256 results into one
accumulator is a float reduction, which a compiler may not reassociate on
its own, so what moves on ARM is how the bank is walked rather than how wide
it is walked. Built for `wasm32` with `simd128` the loop emits no v128
instructions in either layout — and the version with the sum split four ways
emits them, which looked like the one arrangement that would arrive on the
appliance vectorised.

**All of it evaporates in the real thing.** The bank was rewritten as one
array per field, held to the render fingerprints (identical, both of them),
built as wasm and measured on the appliance against the arrangement it
ships with, three rounds each, with a third build adding the split sum:

| whole block, one instance | idle | a held chord |
| --- | --- | --- |
| array of structs, as it ships | 1060 / 1054 / 1043 µs | 1553 / 1540 / 1565 µs |
| one array per field | 1048 / 1035 / 1073 µs | 1531 / 1613 / 1575 µs |
| + the sum split four ways | 1107 / 1018 / 1046 µs | 1568 / 1533 / 1506 µs |

The spread within one arrangement is larger than any difference between
them. Whatever the layout is worth on ARM directly, wasmtime's addressing
and codegen level it, and the rewrite was discarded rather than landed: a
large diff in the instrument's hottest structure for a change that measures
as noise is churn, and the 1.41× that justified it did not survive contact
with the target.

So the soundboard's bank is 69 % of the global stage and there is no
arrangement of it that helps. What is left there genuinely is an ear-level
decision — fewer modes, or a cheaper mode — and that is a different kind of
question from this one.

## What is not covered

**The browser pool refuses an instrument that reports.** A plugin whose
coordinator decides things from what its units did — which string is busy,
which is quietest — has no way home for that on the web: the worker arena
carries audio and nothing else. Such a block takes the sequential fallback,
which is the same component rendering the same audio on one thread. The
Concert Grand is exactly that kind of plugin, so on the web it is currently
single-threaded. Closing this needs a report region in the arena, a copy on
the worker side and an `rf_par_report_read`.

**Nothing exercises the browser transport the way the native one is now
exercised.** `tests/parallel_render.rs` holds the native path to its
sequential fallback with a fixture whose units are deliberately wider than
its instrument, and holds the packaged Concert Grand to the same standard
through the real `ParallelUnits` and `RenderPool`. `browser.rs` is
`#[cfg(target_arch = "wasm32")]` and calls into the embedder, so nothing in
this workspace can run it at all. The one rule it used to get wrong on its
own now lives on `ParallelLayout::unit_width`, which every host asks and
which has a test that compiles on every target — but that is a rule with a
guard, not a transport with a guard.

**`live.rs` is invisible to a host build on Windows.** It is behind
`#[cfg(target_os = "linux")]`, so cargo reports success for a crate whose
changed file it never compiled — a commit once shipped with five
constructors missing a field that way. `tools/cross-build-raspberry-pi.ps1`
is the only build on a Windows machine that compiles it, and any change
there has to go through it.

**The two packaged equivalence tests are `#[ignore]`d** because they need a
`wasm32` build of their component. CI builds the components and runs them in
their own step; a developer has to ask for them:

```bash
cargo build --release --target wasm32-unknown-unknown -p rackforge-concert-grand
cargo test -p rackforge-core --test parallel_render -- --ignored
```
