# Reliability qualification

RackForge treats real-time reliability as a release qualification, not as a
subjective listening test. The `v0.2.0` milestone tracks five complementary
layers: deterministic MIDI replay, device hotplug, audio recovery, Android
lifecycle, and long-running native-host soak tests.

## Deterministic MIDI traces

`rackforge_core::midi_trace` defines the first qualification layer. A trace is
portable JSON and identifies inputs by their stable RackForge MIDI identity.
Runtime-only source keys and display names are never persisted.

```json
{
  "schema_version": 1,
  "events": [
    {
      "frame": 0,
      "source_id": "usb.arturia.keylab-essential-mk3",
      "message": [144, 60, 110]
    },
    {
      "frame": 127,
      "source_id": "usb.arturia.keylab-essential-mk3",
      "message": [128, 60, 0]
    }
  ]
}
```

Compilation is atomic. Before replay begins, RackForge verifies the schema,
monotonic frame order, stable source identity, MIDI status, message length, and
data-byte ranges. Events at the same frame retain their file order. The replay
visitor owns pacing, which lets unit tests run without sleeping and lets a
future soak runner use the same trace against a real audio clock.

Run the portable trace coverage with:

```text
cargo test -p rackforge-core midi_trace
```

The suite currently covers dense chords, Control Change, Pitch Bend, Channel
Pressure, Poly Pressure, stable same-frame ordering, malformed traces, unknown
devices, and delivery of every Note Off through the normal MIDI route.

## MIDI disconnect and reconnect

The platform-neutral `SupervisedMidiSources` state records a connection only
after opening the external port succeeds. Failed opens remain pending for the
next scan. A successful disconnect injects source-aware sustain release and
All Notes Off messages through the ordinary MIDI ingress and routing path.

Automated scenarios verify that sustained and physically held notes stop before
reconnection, that an ALSA client/port address change retains the same stable
identity and compiled route key, and that 10,000 unplug/replug cycles do not
require a host restart or produce duplicate transitions. Ambiguous source keys
or identities are rejected before the supervisor thread starts.

Run this layer with:

```text
cargo test -p rackforge-core midi_hotplug
```

## How much instrument a machine can hold

An xrun is not a fault to recover from when the render simply does not fit:
the engine renders a block, the deadline passes, and the gap is audible. The
question is then how much instrument fits, which is a property of three
things together -- the machine, the instrument and the period size -- and not
of any one of them.

Measured on a Raspberry Pi 4 running the appliance, Concert Grand, 128 frames
at 48 kHz (a 2667 us deadline), with `tools/measure-appliance-polyphony.py`:

| notes held | mean block | share of deadline | misses / 10 s |
| --- | --- | --- | --- |
| 2 | 1671 us | 63% | 0 |
| 4 | 1872 us | 70% | 0 |
| 6 | 1941 us | 73% | 3 |
| 8 | 2083 us | 78% | 390 |
| 12 | 2131 us | 80% | 488 |

**Six to eight sustained notes.** Above that the tail crosses the deadline and
the audio breaks up. Silence already costs about half the budget.

Read that `misses / 10 s` column as a floor, not a count. It was measured
before `measure-appliance-polyphony.py` was fixed (2026-09-19), when the tool
read a ten-second window inside a twelve-second hold and so could not see a
miss that landed in the strike. It is defensible here because at these block
costs -- 63 to 80 % of the deadline -- the sustain itself was missing, which
is what the column is counting. It is not defensible once a budget brings the
sustain down: see the corrected ramp in `REALTIME_BUDGET.md`, where every
remaining miss is in the transient and the old window reported zero.

### Where the cost is, so it is not looked for in the wrong place

On a desktop, with the board bank switched off to weigh it:

| | with the board | without it | the board's share |
| --- | --- | --- | --- |
| idle | 202 us | 84 us | 118 us (58%) |
| 8 notes | 827 us | 627 us | 200 us (24%) |
| 24 notes | 1326 us | 1061 us | 265 us (20%) |

The 256-mode board bank dominates *silence* and is a fifth of the cost of
*playing*. The voices are the rest: about 41 us each per block, and a voice is
a hundred and forty-four partials of a physical model. So the ceiling moves by
changing what a voice costs, or how many of them there are -- not by making
the board loop faster. A perfect fourfold speedup of the board would buy one
more note.

That is worth stating because the board loop looks like the answer and is not.
It was rewritten into arrays per coefficient and stepped four modes at a time,
bit-exactly (`render_fingerprint` in the plugin's tests confirmed the audio was
unchanged); it measured 20% faster on x86 and 2% slower on the Pi, in an
A/B/A of sixty silent blocks each -- 1323 us, 1348 us, 1329 us. LLVM was
already vectorising that loop. The rewrite is not in the tree.

### Other things that were measured and were not the cause

* **The buffer size.** Doubling the period to 256 frames doubles the budget
  and the work with it: 63% of the deadline before, 62% after. It buys room
  for jitter, not for notes.
* **The interface polling the engine.** The Web interface asks Core for
  changes four times a second per open tab, and it uses the incremental
  `events` path rather than a snapshot. With a generated load of eight notes
  a second, the misses were zero with the interface connected.
* **The note-on itself.** Re-measured on the appliance (2026-09-19), and the
  old numbers here -- 69 us a strike, of which 63 the hammer-string
  integration -- are both wrong. `tools`-free, in the plugin's own
  `strike_transient`, twelve notes landed in one block on a Raspberry Pi 4:

  | notes struck together | the strike block | against a 2666 us deadline |
  | --- | --- | --- |
  | 1 | 880 us over settled | |
  | 6 | 3820 us | |
  | 12 | **5797 us** | **2.2x the whole period** |

  **RETRACTED: "and the hammer-string integration is not in it".** Gating it
  off through `SIM_MIN_MODES` did leave the same chord at 6006 us against
  5797, and that was read here as the contact model costing nothing. It was
  a confound. Two effects cancel: the simulation is expensive, and skipping
  it leaves MORE partials above the amplitude floor, so the ladder that
  follows grows by about what the simulation saved. The settled cost gives
  it away in the same runs -- 1855 us with the simulation off against 1762
  with it on, a bigger instrument for the same twelve notes.

  What settled it was a profiler rather than an ablation. `note_on_profile`
  times the phases of `start_voice_unit` directly -- explicit probes, not
  sampling, because that function is very large and everything in it inlines
  onto one symbol. On the appliance:

  | phase | one note | twelve notes |
  | --- | --- | --- |
  | the frequency recipe | 4.3 % | 3.0 % |
  | felt and contact setup | 0.1 % | 0.1 % |
  | **the strike simulation** | **47.3 %** | **41.8 %** |
  | the partial ladder | 35.7 % | 37.4 % |
  | the halo (pedal down) | 10.1 % | 15.2 % |
  | phantoms, noise, the re-strike merge | 1.6 % | 1.7 % |
  | allocating the voice | 0.8 % | 0.8 % |

  So the contact model is about half of it after all, the ladder is a bit
  over a third, and the halo -- a second, smaller ladder built only under
  the pedal -- is a sixth. Four earlier rounds of guessing had each landed
  under ten percent because all four were about the ladder.

  **Tried and not kept: an exact integrator for the contact.** The
  simulation is symplectic Euler at a four-microsecond step, running until
  the hammer is thrown off -- 250 to 2000 steps -- over every mode below
  8 kHz, which for a bass note is about a hundred. Cost is steps times
  modes, and the step looked like the lever: each mode is a harmonic
  oscillator driven by the contact force, and over one step with the force
  held constant its exact solution is a rotation about the forced
  equilibrium, with the sine and cosine computed once per mode. Same
  arithmetic per step, and the step would then be limited by the FORCE
  rather than by the top mode.

  It was built and measured against a reference eight times finer than the
  shipped step (`contact_convergence`, C4 mezzo-forte, worst partial):

  | step | symplectic Euler | exact rotation |
  | --- | --- | --- |
  | 4 us (shipped) | 1.23 dB | 1.30 dB |
  | 8 us | 3.51 dB | 2.56 dB |

  The two schemes are the same to within noise at the shipped step. That is
  the answer: the accuracy bottleneck is not the string modes, it is the
  hammer and the felt -- `hammer_y` is integrated with plain Euler and the
  felt's compression is nonlinear with a relaxation memory, so the force
  itself needs the resolution. Making the oscillators exact buys no step
  size, and the implementation was reverted.

  One thing survives it: `SIM_DT_S`, a knob outside the registry, so the
  sweep can be re-run.

  **And the step was then judged by ear, and it holds (2026-09-19).** The
  sweep had found that the instrument as voiced carries the discretisation
  error -- at the shipped step the worst partial of a mezzo-forte C4 sits
  1.23 dB from converged, and on a bass note the error is enough to push a
  partial across the amplitude floor and change the ladder's LENGTH. That
  raised a worse possibility than any xrun: that some of the instrument's
  calibrations were compensating for the integrator's error rather than for
  physics.

  `contact_step_render` put it where the ear could reach it -- one file, a
  lead-in, then the shipped four microseconds against one microsecond, A B
  A B, on a bass chord, a mezzo-forte C4 and a pianissimo C4. Measured, the
  difference peaks at -39.9 dB below the signal on the most exposed of them
  and sits at -63 dB rms. The player's verdict: "no noto diferencia
  prácticamente".

  So the step is justified rather than inherited, the voicing is not built
  on the artefact, and the contact integrator closes as a line of work. What
  remains of the note-on is `steps x modes` with both set by physics that
  has already been calibrated against recordings.

  What is left of the note-on, then, is `steps x modes` with both set by
  physics: `SIM_TOP_HZ` already stops at 8 kHz on the grounds that higher
  modes contribute almost nothing to the contact shape, and `strike_budget`
  already has its documented audible price. SIMD is not the answer either
  -- `simd128` has been on for every wasm build in the workspace since
  before this, for exactly these loops.

  It is the ladder. `strike_cost_by_ladder` counts a voice's partials as they
  age, which the first version of it failed to do -- counting them at the end
  counts what the cull LEFT, and reported the same 220 at every budget. As
  they age, twelve voices together:

      547 partials at block 1  ->  507  ->  464  ->  388  ->  222 at 2.4 s

  A fresh chord carries two and a half times the partials it settles at, and
  the block cost tracks that curve exactly: 1.74x settled at 3 ms, 1.54x at
  133 ms, 1.31x at 533 ms, 1.02x at 1.9 s. So one cause explains both halves
  of the transient -- the ladder is expensive to BUILD, which is the note-on
  block, and expensive to RUN until the cull thins it, which is the second
  that follows.

  **`strike_budget` is not the lever**, and the reason is recorded rather
  than guessed: it was three once, and a seven-note chord came out 3.25 dB
  down in its fundamentals and 10.34 dB up at 4-8 kHz against the same notes
  struck alone, losing body and growing an edge from its fourth note on.
  `a_chord_is_the_sum_of_its_notes` guards that. Nor is the real-time budget:
  swept from four million fuel down to four hundred thousand, the birth
  ladder moved only from 547 to 505. The governor cannot reach this, which is
  the same conclusion `REALTIME_BUDGET.md` reaches from the other side.

  What is left is a design choice with an audible price, and it is stated
  here rather than taken: a voice could be born closer to the ladder it will
  settle on, which would roughly halve the note-on block and remove most of
  the tail -- and those partials are genuinely sounding while they last, so
  it is the attack that pays. That is the open item.

## Where the time goes, measured by taking things away

The ceiling above says how much fits. This says what is filling it, which is
what any attempt to move the ceiling has to start from.

Two measurements, on a Raspberry Pi 4 at 128 frames.

**The soundboard, by sweeping it.** Board Density is the only control that
changes how many modes the bank has, so
`tools/measure-appliance-stage-split.py` sweeps it on the running appliance
and watches the block cost move:

| board modes | silence | six notes | misses / 10 s |
| --- | --- | --- | --- |
| 106 | 1145 us | 1601 us | 0 |
| 153 | 1280 us | 1777 us | 0 |
| 200 | 1422 us | 1962 us | 39 |
| 235 | 1530 us | 2011 us | 12 |
| 256 | 1586 us | 2106 us | 157 |

A mode costs 2.97 us a block, the full 256-mode bank costs 759 us, and
everything that is not the bank costs 829 us.

**Everything else, by building the plugin without it.** One build per bank,
each measured on the appliance at 169 modes (1321 us of silence):

| taken out | silence | what it cost | what it is |
| --- | --- | --- | --- |
| nothing | 1321 us | -- | |
| `undamped` | 928 us | **393 us** | 192 resonators: the sympathetic partials of the undamped top register |
| bridge projection | 1166 us | **155 us** | the 16x16 product that puts the strings' force on the board |
| `bed` | 1261 us | 60 us | 40 resonators: the damped strings' bed |
| open top octave | 1306 us | 15 us | 14 resonators |

The two methods agree: the ablations plus the room, lid, halo and rim sum to
819 us against the sweep's 829 us, within 1.2 %.

So **the instrument spends 59 % of the period rendering silence**, and at the
polyphony where it breaks up the voices are only about a quarter of the work.
That is why `REALTIME_BUDGET.md` scales the soundboard and the undamped
register before it touches the notes: those are the parts that cost the most
and are heard the least.

### Two exact savings found this way, and one that was not there

Both savings are bit-identical -- the Concert Grand's render fingerprint is
unchanged -- and both came out of the table above rather than from guessing:

* The bridge projection spends 512 multiplies a frame proving that zero times
  a basis is zero. Skipping it when no voice contributed saves **151 us**, all
  of it while nothing is sounding.
* The undamped bank asked `note_sounding` per resonator per sample, when the
  answer is settled once a block. Hoisting it saves **37 us**.

The second is worth recording for what it disproved. An undamped length cost
2.05 us a block where the open top octave's identical resonator cost 1.07 us,
and the lookup looked like the whole difference. It was not: removing it
bought 37 us of a 190 us gap. The rest is the working set -- 192 resonators
against 14 is a cache story, not a branch one.

### The third was chased, built, and measured at nothing (2026-09-19)

That last sentence was the starting point, and the chase is recorded here
because the mistake in it is easy to repeat.

One `BodyMode` served every bank and carried every bank's needs: 88 bytes, of
which the soundboard's per-sample loop reads 60 and a sympathetic string's
reads 32. The banks are swept whole every sample, so half of every cache line
fetched was a field that loop never looks at.

**Whether the stride costs anything is real, and it was measured on the
appliance.** `tools/measure-bank-stride.rs` sweeps the same arithmetic over
the same count in the same order and changes only the stride:

| bytes per mode | bank | ns per mode per sample, Pi 4 | on a desktop |
| --- | --- | --- | --- |
| 32 | 23.7 KB | 4.57 | 1.27 |
| 56 | 41.5 KB | 5.11 | 1.50 |
| 88 | 65.1 KB | 6.82 | 1.57 |
| 152 | 112.5 KB | 9.82 | 1.53 |

The Pi tracks the stride, 2.15x across that range, where a desktop flattens at
1.2x. That part holds, and it is worth keeping: **a desktop cannot answer a
cache question about this appliance.**

So the struct was split by what its loop reads -- `BodyMode` at 32 bytes for
the sympathetic banks, `BoardMode` at 60 for the soundboard, and a parallel
`BoardCold` for what only the builder reads. All of it bit-identical.

**And on the appliance it bought nothing.** Measured in silence, with the
budget written to the store and seeded at the same 2,897,563 fuel for every
run so the instrument was the same size in all of them, alternating:

| | mean block |
| --- | --- |
| before, pass 1 | 586 us |
| before, pass 2 | 607 us |
| after, pass 1 | 612 us |
| after, pass 2 | 610 us |

The spread between the two *before* runs is larger than the gap between the
conditions. No gain.

**Why, and it was knowable in advance.** The 65 KB in that table is the sum of
the array CAPACITIES -- `BOARD_MODES`, `UNDAMPED_COUNT`, `SILENT_MODES`. The
instrument never runs there. The density law places 185 modes in the board's
range at the default Board Density, not 256; the undamped bank stops at
`UNDAMPED_HIGH_HZ` after about 90 of its 192; and `silent` ticks only the
slots a held silent key has taken. `bank_working_set` now reports what is
live rather than what is allocated:

| Board Density | board modes | KB before | KB after |
| --- | --- | --- | --- |
| 0.63 | 106 | 21.5 | 10.7 |
| 1.11 (the appliance) | 185 | **28.3** | 15.3 |
| 1.58 (maximum) | 256 | 34.4 | 19.5 |

Twenty-eight kilobytes against thirty-two of L1. **It already fit**, and a
bank that fits cannot be made faster by making it fit. The benchmark that
justified the work was sized from the capacities, which is the same error as
comparing a computed mode count against an overlap law: the number was right
and it was a number about nothing.

The packing is kept -- it is bit-identical, strictly smaller, and it is the
difference between fitting and not at maximum density or with silent keys
held, where the old layout runs 34 KB and over. It is recorded here as a
saving that is **available and not currently collected**, so that nobody
measures it again expecting the 139 us the native benchmark predicted.

The lesson for the next one: before optimising a sweep, measure how long the
sweep actually is.

## Audio device arrival and loss

The engine binds one output when it starts and renders through it until it
stops, which is right for the stream and wrong for an appliance: a Raspberry
Pi is imaged before it meets the interface it will be played through.
`rackforge_core::audio_hotplug` watches the inventory beside the running
engine and asks systemd for a restart when the binding has gone stale --
the same move the MIDI supervisor makes for a keyboard that arrives after
boot, and for the same reason. Re-binding a live ALSA stream is a larger
promise than the restart is worth.

It leaves a working output in two cases only:

* the bound device is no longer present (`AUDIO_OUTPUT_LOST`);
* an output of a **better kind of connection** appears and can serve the
  running profile (`AUDIO_OUTPUT_ARRIVED`).

Kind, never make: USB outranks the board's own output, which outranks an
unclassified one, which outranks HDMI -- on a headless appliance HDMI usually
leads to a screen that is not there. Equal kinds never displace each other, so
a second interface plugged in beside a working one changes nothing, and a
performance is never moved off the interface it is playing through.

The decision is a pure function over one inventory reading
(`rackforge_audio_api::assess`), so it is tested without a sound card:

```text
cargo test -p rackforge-audio-api
```

## Audio fault injection and recovery

`rackforge_core::audio_reliability` owns the bounded stereo render queue and
the dropout/stream recovery state used by Android. The queue allocates its
complete ring during construction. Push, pop, concealment, recovery, and
telemetry use only preallocated memory and atomics after startup, so fault
handling adds no allocation, mutex, channel, sleep, or system call to the audio
callback.

The deterministic suite fills the queue to saturation, verifies that rejected
writes cannot expose partial stereo frames, forces partial and empty reads,
fades the last valid sample to silence, records a stream loss, and verifies a
finite click-reduced fade-in after restart. Android exposes the same counters
through its native audio status: saturated pushes, underrun callbacks and
frames, concealed/recovered callbacks, stream health, losses, and recoveries.

Run this layer with:

```text
cargo test -p rackforge-core audio_reliability
```

## Android lifecycle and USB recovery

`tools/qualify-android-lifecycle.py` drives a debug APK through ADB and records
machine-readable snapshots from a debug-only receiver. It proves that native
audio callbacks continue while the screen is locked and while the Activity is
in the background, then verifies clean resume transitions. The full hardware
mode also waits for a physical USB disconnect and reconnect, requires MIDI
ports to reopen under a newer generation, and compares AAudio's real device ID
with the restored selected interface. This catches a UI that claims to have
returned to USB while the stream still uses the fallback output.

With one authorized device connected, run the complete scenario from the
repository root:

```text
python tools/qualify-android-lifecycle.py --usb-cycle
```

The operator disconnects and reconnects the hub when prompted. Use `--serial`
when multiple ADB devices are online. Without `--usb-cycle`, the harness runs a
short lock/background smoke test and explicitly records the USB stage as
skipped. Every run writes a timestamped JSON report below
`dist/qualification/`; only a report whose top-level outcome is `passed` is a
valid qualification result.

## Native Android soak

`tools/soak-android.py` keeps the normal plugin and audio engine active by
injecting short notes through RackForge's MIDI ingress while sampling the
debug qualification snapshot. Duration, MIDI cadence, and sampling cadence are
configurable; a release qualification requires at least 120 uninterrupted
minutes:

```text
python tools/soak-android.py --duration-minutes 120
```

A shorter run is intended for development and receives the explicit outcome
`passed_with_duration_waiver`. The report accumulates counters across native
stream restarts rather than allowing a reset to hide a fault. It exports AAudio
xruns, render-queue underruns, callback deadline misses, MIDI drops, MIDI
reconnect attempts, disconnect panics, stream losses/recoveries, render errors,
lock misses, non-finite samples, callback stalls, process restarts, callback
load, thermal state, and total PSS memory.

The initial release thresholds are deliberately strict:

- zero xruns, queue underruns, missed deadlines, MIDI drops, render errors,
  lock misses, stream losses/recoveries, invalid samples, callback stalls,
  process restarts, snapshot failures, or unhealthy samples;
- maximum measured callback load of 85%;
- maximum Android thermal status of `MODERATE` (`2`).

Memory and reconnect-attempt counts are retained for trend comparison but do
not yet have a device-independent limit. The supported Android qualification
setup is an ARM64 phone on API 26 or newer, a powered USB hub, an Arturia KeyLab
Essential mk3, and a Focusrite Scarlett USB interface. The report records the
exact phone, Android build, attached USB identities, and runtime version; a
hardware result without that metadata is not accepted.

The manual `Qualify Android hardware` GitHub workflow targets a dedicated
self-hosted Windows runner labelled `rackforge-android`. It does not run on
ordinary pushes. The runner must have Python, ADB, an authorized phone, the
Android build toolchain, and the test hardware connected. The workflow builds
and installs the exact Git commit before testing it, embeds that revision in the
report, and retains the JSON as a 30-day Actions artifact even when a threshold
fails.

## Qualification still required for v0.2.0

- Record supported test hardware and pass/fail thresholds with every retained
  qualification report.

The live checklist and acceptance criteria are tracked in
[GitHub issue #12](https://github.com/kalexis1994/rackforge/issues/12).
