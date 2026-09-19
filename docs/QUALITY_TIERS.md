# Scalable quality (`quality_tiers_v1`) — a design

A physically modelled instrument costs what its model costs, and the machines
RackForge runs on differ by nearly an order of magnitude. Concert Grand holds
six to eight sustained notes on a Raspberry Pi 4 at 128 frames before the
audio breaks up (see `RELIABILITY.md`), and many more on a desktop. Today the
instrument does not know which machine it is on, so it plays the same way
everywhere and the weakest machine is the one that clicks.

This is the design for letting a plugin spend a budget instead of being
trapped by one. It is not implemented.

## The mistake to avoid first: benchmarking at startup

The obvious answer — time a few blocks when the plugin loads, pick a quality —
is the wrong mechanism, for reasons that were measured rather than supposed:

* **It measures the wrong moment.** A Raspberry Pi with no interface
  connected, cold caches and an idle governor is a different machine from the
  one the player performs on. The same appliance measured 60.8% and 75.8% of a
  core depending on what else was running.
* **It makes the instrument non-deterministic.** The same build on the same
  machine would render differently depending on how it started. RackForge
  compares renders against references and the Concert Grand's tests carry a
  bit-exact fingerprint; a quality that depends on the weather breaks both,
  and turns "it sounds different on my machine" into a true statement nobody
  can act on.
* **It goes stale.** Thermal throttling, a governor change, a second plugin
  added to the rack, an interface at a different period size.

The host is already measuring the real thing, continuously, under the real
load. That is a better instrument than any synthetic one.

## What the host already has

Two halves of this design exist today.

**Deterministic work metering.** The portable runtime runs with
`consume_fuel(true)`, and `PortableInstance::last_realtime_fuel_consumed`
reports what one real-time call actually spent. Fuel counts instructions, so
the same block of audio costs the same fuel on a phone, a Pi and a desktop.

**A hard cap.** `RuntimeLimits::fuel_per_call` bounds a real-time call: a
plugin that runs away traps instead of holding the audio thread. The rack is
already defended against a plugin that will not stop.

What is missing is the negotiation between them: a plugin that could have
spent less never learns that it should.

### Why fuel, and not microseconds

Fuel is the plugin's own cost; wall time is the machine's. Their ratio is the
machine's speed for that plugin, which is the only machine-dependent number in
the system, and the host measures it every block already
(`AUDIO_RENDER_BLOCK`, per slot).

So the budget handed to a plugin is **in fuel**:

    fuel_budget = (deadline_us × headroom × share) ÷ microseconds_per_fuel

Everything machine-dependent is in that one division, computed by the host.
A plugin choosing its tier from a fuel budget makes the same choice on every
machine of the same speed, and that choice can be tested in CI by injecting a
rate — no timing threshold, no flaky runner. Today's benchmarks in the
Concert Grand print numbers and never assert, precisely because a wall-clock
assertion is a flake waiting to happen. A fuel-denominated contract can assert.

## The contract

### The plugin declares its tiers

```toml
capabilities = ["audio_output", "midi_input", "quality_tiers_v1"]

[[quality.tiers]]
id = "full"
relative_cost = 1.0
loses = "nothing; the model as published"

[[quality.tiers]]
id = "voiced"
relative_cost = 0.62
loses = "partials below -78 dBFS; -0.4 dB above 9 kHz on a fortissimo C4"

[[quality.tiers]]
id = "sparse"
relative_cost = 0.35
loses = "half the soundboard modes and the sympathetic bed; the body reads drier"
```

Two rules make a tier list worth having:

* `loses` is a **measured sentence about what the ear gets**, not an adjective.
  "Low quality" tells a player nothing; "-0.4 dB above 9 kHz" can be checked,
  argued with, and regression-tested.
* Tiers are ordered and total. A plugin that declares them promises every one
  of them renders correct audio — a tier is a smaller instrument, never a
  broken one.

### The host hands a budget and names a tier

At `prepare`, and again whenever the situation changes, the host passes the
fuel budget and the tier it has selected. A plugin may rebuild coefficients
for a tier change, so the host changes tiers **between blocks and rarely**,
never inside one.

### Selection is slow, sticky and written down

* Start from the tier persisted for this machine, plugin and period size. A
  measurement that was true yesterday is still the best first guess today.
* With no persisted value, start at `full` and let the first seconds of real
  rendering decide. The real workload with its real neighbours is the
  measurement; a glitch in the first phrase is the price, and it is paid once
  per machine rather than once per boot.
* Move down when the p99 crosses a threshold for several seconds, and up only
  after a long quiet margin. **Hysteresis is not a detail**: quality that
  oscillates with CPU noise sounds worse than lower quality held steady —
  the timbre would breathe with the load.
* Log every change (`QUALITY_TIER id=… from=… reason=p99-over-budget`), show
  the current tier in the interface, and let the player pin one. A pinned tier
  is a deterministic render again, which is what references and bug reports
  need.

Never select by platform name. An Android phone from this year outruns a
Raspberry Pi 4; the number comes from measurement or it is a guess with a
brand on it.

## Plugins that do not participate

Most will not, and that is the author's choice to make. The host's obligation
is that one plugin's choice is not paid for by the rack:

* The fuel cap already stops a runaway.
* Per-slot telemetry already exists (`AUDIO_RENDER_STAGE slot=…`), so the host
  can **name the plugin that is over budget** instead of reporting that the
  machine is slow.
* A slot that keeps missing can be rendered silent and reported as such —
  one instrument lost, deliberately, with the reason on screen, rather than
  every instrument clicking.

## Worked example: Concert Grand

From the measurements in `RELIABILITY.md`, on a Raspberry Pi 4 at 128 frames:

| | cost | scales with |
| --- | --- | --- |
| board bank, 256 modes | 58% of silence, 20% of playing | nothing |
| a sustained voice | ~41 us per block each | polyphony |
| a note-on | 69 us once | notes played |

So its tiers have to move on two axes, not one. Polyphony alone does not save
a machine that cannot afford the floor, and a cheaper floor does not save a
machine drowning in voices:

* **fixed** — board modes, the sympathetic bed, the open-string halo.
* **per voice** — partials per voice, and the polyphony cap above which the
  quietest voice is stolen rather than a deadline missed.

A cap with stealing is what turns the cliff at eight notes into a piano with
less polyphony. Every hardware piano made before 2000 did exactly that, and it
is unambiguously better than clicking.

## Open questions

* Does a tier change cost a block? Rebuilding 256 board modes is not free.
  Either the change is amortised across blocks like `tune_pair` already is, or
  the host schedules it where a glitch is acceptable.
* Should a tier be part of session state? A performance recorded at `sparse`
  and replayed at `full` is a different recording.
* Does the Web browser host, which has no fuel metering (see
  `rackforge-plugin-runtime/src/browser.rs`), get tiers at all, or does it
  select from wall time with wider hysteresis?
