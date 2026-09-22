# Host performance review

What to examine in the RackForge host when an instrument does not fit its
deadline. Written from the RF-5 case on a Raspberry Pi 4B, but every item is a
host-side property that applies to any plugin.

Each item states what it is, the evidence behind it, how to close it and what
to expect. **Measured** means a number exists. **Hypothesis** means it is
reasoning that has not been tested on hardware yet, and saying so is the point:
a wrong item labelled *hypothesis* costs an afternoon, a wrong item labelled
*measured* costs a week looking in the wrong place.

## The case that motivates this

RF-5 does not hold its deadline on a Pi 4B at 128 frames, and by later report
not at 256 either. The profiler figures from the appliance:

| Period | p99 vs deadline | spread (p99 / own mean) |
| --- | --- | --- |
| 128 frames | 105 % | 1.44× |
| 256 frames | 89 % | 1.28× |
| 512 frames | 76 % | 1.16× |

Its mean sits at 73 % of the deadline at 128 frames.

**That shape matters.** A mean at 73 % means the aggregate throughput is
there. What fails is the tail. This is a variance problem, not a capacity
problem, and the two have different fixes — more parallelism does not shorten
a unit whose own work is data-dependent, it only leaves other workers idle
while the block waits.

---

## A. Measure before changing anything

Nothing here costs a code change and every item can invalidate the ones below.

### A1. Confirm the audio threads actually got real-time scheduling

`crates/rackforge-core/src/realtime.rs:138` reports whether `SCHED_FIFO` was
**granted**, not merely requested, and the module comment states the failure is
"inaudible until the worst possible moment". Workers engage it at
`crates/rackforge-core/src/parallel_render.rs:999`.

- Read `RealtimeStatus::is_degraded()` and `remedy()` on the Pi itself.
- `scheduling=denied:requested=75,granted=0` means `RLIMIT_RTPRIO` is not
  granted for that user, the workers are on the ordinary scheduler, and
  everything else in this document is secondary.
- Closure: the appliance audit prints a fully-engaged status before a set.

*Evidence: the diagnostic exists and is authoritative. Whether it currently
reports degraded on the Pi is unverified — hypothesis until read.*

### A2. Rule out the board before blaming the code

- `vcgencmd get_throttled` after ten minutes of sustained playing. The 4B
  throttles thermally at 80 °C and also under undervoltage; a passively cooled
  board under a full set reaches both.
- CPU governor: Raspberry Pi OS defaults to `ondemand`. A clock ramp at the
  start of a block is a textbook p99 killer. Set `performance`.

*Evidence: hypothesis. Both are two-minute checks and either would produce
exactly the observed signature — healthy mean, ugly tail.*

### A3. Localise the variance by phase

The profiler reports each phase's p99 against **its own mean**, which is the
measurement that separates "this path is expensive" from "this path is
uneven". Only the second is fixed by scheduling.

Prime suspect: the filter's Newton closure in the resonant range. An iterative
solver has a data-dependent iteration count — it varies with Q and with the
signal's proximity to self-oscillation — which produces a comfortable mean and
a tail that spikes. If that is the source it is inherent to solving a
nonlinear system per sample and does not yield to micro-optimisation.

*Evidence: hypothesis. The tool to confirm or refute it in minutes already
exists.*

---

## B. wasm engine configuration

Everything here is in `crates/rackforge-plugin-runtime/src/native.rs:54-72`.

### B1. Fuel metering is charged to every plugin — highest priority

`config.consume_fuel(true)` is set unconditionally at `native.rs:56`. Wasmtime
puts a counter in every basic block of generated code. A plugin that never
reads a realtime budget pays all of it for nothing, and most do not.

**Measured: ~12 % of a voice on the appliance; −8 % at both 128 and 256 frames
when removed, with bit-identical audio.**

The two-engine fix — fuel for plugins that spend a budget, epoch interruption
for everyone else, decided by asking the plugin rather than reading its
exports — exists on the `web-app-split` branch (PR #109) and **is not on
`main`**. Landing it is the single cheapest confirmed win available.

### B2. SIMD — check the plugin side

The host sets no SIMD configuration. Recent wasmtime enables `simd128` by
default, so the runtime accepts it; the open question is whether the plugins
are **compiled** with `+simd128` and whether their DSP is actually vectorised.

If RF-5's inner loops are scalar wasm, this is probably the largest win
available without touching the architecture: four f32 lanes, and Cranelift
maps `simd128` to NEON reasonably. Five voices do not vectorise cleanly, but
the four filter cells per voice, or the four samples of 4× oversampling, do.

*Evidence: hypothesis — the host configuration confirms nothing is enabled or
disabled here; the plugin build flags have not been checked.*

### B3. Bounds-check elision

No memory configuration is set: `memory_reservation`, `memory_guard_size`,
`memory_may_move`. With a large enough guard region Cranelift can elide the
bounds check on linear-memory accesses. In DSP that check is paid on every
sample of every voice.

*Evidence: hypothesis. Cheap to try, measurable immediately.*

### B4. `relaxed_simd` — a deliberate trade, not a free win

Relaxed SIMD enables FMA, which is close to 2× on multiply-accumulate chains.
It also permits results to differ per platform, which breaks the bit-exact
render fingerprints the instruments rely on (Concert Grand's
`0x0c396512799eb435`, RF-5's oracle comparisons).

If it is ever adopted it has to be a declared property of the distributed path
only, never of the fidelity oracle, and the determinism contract in
`docs/PARALLEL_RENDER.md` has to say so.

### B5. Code cache — already present

`Cache`/`CacheConfig` at `native.rs:60` persists compiled artifacts, so
compilation cost is not paid per launch. This removes startup latency; it does
not change codegen quality, so it does not affect xruns.

---

## C. The compilation backend — the real ceiling

### C1. wasm the format is not wasmtime the runtime

Cranelift is built to compile fast and be safe, not to squeeze numeric code.
LLVM is materially better at DSP: better vectoriser, better instruction
scheduling, better handling of multiply-accumulate chains.

Both of these keep **one distributed `.wasm`** and change only how it becomes
machine code on a given host:

- **WAMR AOT** with its LLVM backend;
- **`wasm2c` (wabt)** to C, then clang or gcc with full optimisation — the
  path Firefox uses in production for library isolation.

The browser keeps using the same module. The plugin author ships one file.

*Honest expectation: typically within 10–30 % of native, against Cranelift's
20–100 % penalty. A real improvement, possibly not enough for RF-5 alone.
Measurable in an afternoon and it costs nothing in the ABI.*

### C2. The escape hatch already designed

`ROADMAP.md` states that where native transition artifacts are required "they
live inside the same package under explicit target keys", and
`crates/rackforge-plugin-runtime/src/native.rs` already exists as the
transition path.

So the format already anticipates a `.rfplugin` carrying the portable
component **and** a native build for a specific target, with the host
choosing. Last resort, per plugin and per platform, declared — which is more
honest than lowering an instrument's fidelity to defend a rule.

---

## D. Thread topology and scheduling

### D1. On a 4-core board there is no slack — concrete finding

`automatic_audio_worker_capacity` (`parallel_render.rs:726`) returns
`cpus - 1`. On a Pi 4B that is **3 workers plus the coordinator: 4 busy
threads on 4 cores.**

The comment says the spare core "remains available for the coordinator, device
IRQs and the rest of the host" — but the coordinator occupies that core, and
during the serial floor it occupies it for most of the block. Nothing is left
for kernel work, USB interrupts or the web surface.

- Try `RACKFORGE_AUDIO_WORKERS=2` on the Pi and measure. Less parallelism can
  beat more contention.
- Consider a different formula below some core count, or treating the
  coordinator's cost explicitly rather than assuming it is idle.

*Evidence: the formula is confirmed in source. That 2 workers beats 3 on a Pi
is a hypothesis — and a one-environment-variable experiment.*

### D2. Core isolation and IRQ affinity

`isolcpus` to reserve cores, explicit affinity for the audio threads, and
`irqaffinity` to push USB and network interrupts onto a core audio does not
use. This does not add compute; it stops losing it. Closest thing to "more
cores" without changing boards.

Also worth trying: the `threadirqs` kernel parameter. Raspberry Pi OS does not
ship a `PREEMPT_RT` kernel by default.

### D3. Work stealing, and the test that guards it

`parallel_render::tests::unbalanced_load_improves_the_worst_block_and_workers_share_units`
is the right direction: a worker that finishes cheap units takes pending units
of an expensive one.

**That test currently fails on a 4-vCPU machine.** Its functional assertions
pass — unit sharing measured `[49,52,49]` against the old scheduler's
`[0,0,0]` — and what fails is the wall-clock assertion
`new_worst < old_worst * 8 / 10`. It already carries a retry and a `cpus < 4`
skip, which says the flakiness is known.

Split it: the sharing invariants (`busy_workers >= 2`, `total_units ==
blocks * 5`) stay a hard test; the latency comparison becomes a benchmark that
reports without failing. A test that fails because of the machine teaches its
author to ignore red.

### D4. The serial floor does not divide

Amdahl, already documented in the Concert Grand work: board bank, sympathetic
banks and room are roughly 1380 µs of a 2191 µs block and cannot be split. The
coordinator equivalent in RF-5 advances the LFO, the scan cycle, the noise
sources, the master VCA and the output coupling, and mixes in fixed physical
order.

Additionally, five units across three workers does not divide evenly. That is
structural imbalance before any data-dependence is considered.

---

## E. Block-size policy

### E1. Make the rule specific, not looser

"Every plugin must run on a Pi 4B" becomes "every plugin must run on a Pi 4B
**at its declared block size**". RF-5 declares 256 (or whatever it holds).

This is not relaxing the rule. It is more informative: a user taking a Pi to a
stage learns something true and actionable instead of a pass/fail. And the
reason gets documented, which is how the rest of these repositories already
work.

### E2. What the latency actually costs

A longer period is not a fidelity degradation — the measured 256-frame render
is bit-identical. It is a buffer.

For perspective on the perceptual side: a real piano action takes roughly
20–100 ms, **variable with velocity**, between key press and hammer strike.
Organists play pipe organs with tens of milliseconds of acoustic delay. Eight
added milliseconds sits well inside what a keyboard player is already
calibrated for.

---

## F. Ruled out — do not spend time here

- **Core count.** "Not enough cores" and "mean at 73 % of deadline" cannot both
  be true. Four A72s deliver the aggregate work; the tail is the problem.
- **Ethernet sharing the USB bus.** True of the Pi 3B+ and earlier, **false on
  the 4B**, where Ethernet has a dedicated interface straight to the SoC. What
  the four USB ports share is the VL805 controller over one PCIe Gen2 x1 link
  — USB contending with USB, not with the network. Disabling Ethernet is an
  interrupt-jitter measure, second order, and it costs the web control
  surface.
- **"Six cores are the minimum for this class of instrument."** Inferred from
  the AstroLab's bill of materials, which reflects price, power, supply
  guarantees and a product that also drives a screen, storage and networking.
  Most SoCs in that class are big.LITTLE, so a "6-core" part may be two usable
  cores plus housekeeping. Reasoning backwards from a competitor's BOM is not
  evidence.
- **The Pi being the ceiling.** Pianoteq — a commercial physically modelled
  piano — is supported on a Pi 4. setBfree runs a complete tonewheel organ and
  rotary cabinet there. RackForge's own Concert Grand already runs well on it.
  The board is not the wall.

---

## G. The factor that is not the host's

RF-5 does circuit-level integration: five voices × four nonlinear CEM3320
cells with Newton closure per sample, 4× oversampled oscillators, OTA transfer
functions, transformer cores. Behavioural modelling — band-limited oscillators
plus a topology-preserving filter with a saturator — is roughly an order of
magnitude cheaper per voice and is what the commercial emulations do.

That is a deliberate choice, made for defensible reasons, and it is not a host
defect. It does mean RF-5 is very likely the most arithmetically expensive
instrument in this ecosystem, and that it is currently paying three penalties
at once: the expensive method, possibly no SIMD, and Cranelift codegen.

If it is ever revisited: the 4× complete path retained as a non-distributed
fidelity oracle is already the ground truth a reduced model would be fitted
against, and the train/reserve methodology already used in the RF-Tines
studies is what keeps such a fit honest. That is a research project, not a
patch.

---

## Priority

| # | Item | Cost | Expected effect | Evidence |
| --- | --- | --- | --- | --- |
| 1 | B1 — land the two-engine fuel fix | low | −8 %, measured | **Measured** |
| 2 | A1 — verify `SCHED_FIFO` granted on the Pi | none | decisive if denied | Hypothesis |
| 3 | A2 — throttling and governor | none | possibly large on tail | Hypothesis |
| 4 | B2 — confirm `+simd128` and vectorisation | low | potentially largest | Hypothesis |
| 5 | D1 — try `RACKFORGE_AUDIO_WORKERS=2` | none | unknown, one variable | Hypothesis |
| 6 | A3 — per-phase p99 vs own mean | low | locates the variance | Hypothesis |
| 7 | D2 — `isolcpus`, IRQ affinity | medium | reduces jitter | Hypothesis |
| 8 | B3 — bounds-check elision | low | small to moderate | Hypothesis |
| 9 | D3 — split the flaky scheduler test | low | CI trust | **Confirmed failing** |
| 10 | C1 — measure an LLVM AOT backend | medium | 10–30 % vs native | Hypothesis |
| 11 | E1 — declared block size per plugin | low | policy clarity | — |
| 12 | C2 — native target key, last resort | high | closes the gap | — |

Locate before hypothesising: items 2, 3 and 6 cost nothing and can make most
of the rest unnecessary.
