# The real-time budget

A physically modelled instrument costs what its model costs, and the machines
RackForge runs on differ by nearly an order of magnitude. A plugin is told how
many frames to render and, until this existed, never how much machine there was
to render them with — so every plugin was calibrated against whatever desk its
author sat at.

Concert Grand carried the evidence in its own source. `PARTIAL_BUDGET` is
documented there as "a fuel budget, not a taste one", sized against a 512-frame
block on a desktop. A Raspberry Pi 4 gets the same number and breaks up.

This is the mechanism that tells a plugin what it may spend. It is implemented,
it runs on the appliance, and its limits are stated at the end.

## The shape of it

```text
  the plugin                 the host

  renders a block   ──────►  times it, and reads the fuel it cost
                             ns_per_fuel  ← this machine's speed
                             is the block late?  ← the thing that is wrong

  rebuilds, cheaper ◄──────  rackforge_set_realtime_budget(fuel)
```

Two quantities, for two jobs.

**The decision is made on wall time**, because a deadline is wall time. A block
that ran long is an xrun whatever it cost in any other unit.

**The budget is denominated in fuel**, because a plugin cannot read a clock.
Fuel is wasmtime's instruction counter, so the same block of audio costs the
same fuel on a phone, a Pi and a desktop. A number in fuel therefore means
something inside the plugin, and it makes the contract testable: a test injects
a rate instead of racing a timer.

## The loop is closed, and that is the point

The obvious design computes a budget from a cost model and trusts the plugin to
fit inside it. That was tried here and the model would not hold still: Concert
Grand's cost per voice, measured through `rackforge-core stress`, came out about
three times higher for freshly struck notes than for notes a Raspberry Pi had
been holding for twelve seconds, because the partial cull had thinned them in
between. A constant baked into the plugin would have been wrong for one of those
two, quietly.

So nothing trusts a cost model. The host publishes a budget, watches what the
slot then does with the period, and multiplies the budget down until blocks stop
arriving late. Only the first budget is feed-forward — the allowance divided by
the measured rate — and everything after it is feedback. A plugin whose
arithmetic is off by a factor of three still converges; it takes a few seconds
longer.

## Late is not the same as over budget

The allowance (`DEFAULT_HEADROOM`, 0.6 of the period) is what a budget is *sized*
against. The line the governor *acts* on is `LATE_AT`, 0.95 of the period, and it
acts only when more than two percent of a window's blocks cross it.

They have to be different numbers. Measured on a Raspberry Pi 4, two notes
rendered in 63 % of the period and missed nothing at all — and an earlier
version of this treated that as trouble because it sat above a 60 % allowance,
cut the budget, and kept cutting. At 0.9 with a one-percent tolerance it was
still cutting on the tail of renders that never missed: twelve held notes, no
deadline lost, five cuts.

## Why it is slow, sticky, and gives up

* **Slow.** At most one change every two seconds, and none that moves the budget
  by less than 15 %, because a plugin is allowed to rebuild coefficients when it
  is told.
* **One-directional.** It falls as soon as blocks run late and rises only after
  twenty seconds of comfort. Quality that oscillates with CPU noise sounds worse
  than lower quality held steady — the timbre would breathe with the load.
* **It gives up.** After three cuts that buy nothing, the governor logs
  `reason=exhausted` and stops. A budget that keeps falling while the render
  does not is not controlling anything; it is thinning an instrument for no
  reason. Before this existed, a Raspberry Pi took Concert Grand from 340,068
  fuel to 5,649 in sixteen seconds while the render went the *wrong* way, from
  1,050 µs to 3,598 µs — every cut rebuilt the soundboard, and the rebuild was
  what was blowing the deadline.

Stubbornness is measured against where a streak of cuts started, not against the
cut before it. Block times wander by a few percent on their own, and comparing
consecutive windows let noise reset the count.

**A window is a window.** The cadence hangs off when the window was last
*read*, not when something was last published -- and getting that wrong is
what made this mechanism untrustworthy for a whole day of measurements. With
the two on one field, a poll that got past the interval, read the window and
then found the change too small to publish left the timer stale; every block
after it passed the interval too and was judged **on its own**. The log with
the raw counts in it said so in one line:

    late_pct=100.0  load=0.57  blocks=1  late=1

A hundred percent of a window of one. Every cut the governor made on a ramp
that missed no deadline at all was a single unlucky block -- a note-on
running the strike simulation, or a bank rebuild -- read as the whole
instrument running late. Three threshold changes (`LATE_AT` 0.9, 0.95, 1.0
and the tolerance with them) could not touch it, because none of them was the
cause. After the fix, the same ramp:

    tightened  late=110/749  load=1.31
    tightened  late=63/745   load=1.06
    settled    late=0/750    load=0.50

Two justified cuts and a settle. That is why `AUDIO_QUALITY_BUDGET` carries
`blocks` and `late` and not only the rate: one late block in one and seven
hundred in seven hundred and fifty read the same as a percentage, and they
are completely different faults.

* **It never raises within a session.** A raise rebuilds banks, and the
  player hears the instrument change under their hands -- reported from the
  appliance as "se nota como cambia la calidad en vivo": quality came back
  between pieces, twenty seconds after the passage that had cut it, and the
  next dense passage cut it again, every piece. So in a session quality only
  goes down, rarely, when the machine proves it must. Cuts still land
  whenever blocks run late, because the alternative to a cut is an xrun.

## It learns once, and remembers

A governor that only learns under load learns in front of the player, since
load only exists while someone plays. So what a machine settles on is written
down and used as the starting point next time:

* A budget **settles** when the governor has given up cutting (`exhausted`),
  or when it has not moved for sixty seconds under observation. The audio
  loop notes it once; the telemetry thread writes it to
  `state/realtime-budget.txt` -- one line per plugin and period, `<plugin id>
  <deadline ns> <fuel>` -- under the plugin's name.
* When a voice is built, the control thread copies that plugin's remembered
  `(period, fuel)` pairs into it. On the first block whose period is known the
  governor is **seeded** from the matching one and hands it to the plugin at
  once, without waiting to measure anything: the instrument is built at this
  machine's quality before the first note.
* A seed is a start, not a pin. If the machine proves slower than last time
  the budget is still cut, and the new floor is what gets remembered.

The render thread never touches the store: it scans a few pairs it was given
by value, and it stores two integers for the publisher.

## A rebuild does not tick

Even a rare cut used to be audible as a click plus a step in timbre. Measured
in the plugin: swapping a bank under a sounding note made a seam 3.5 times
the note's own largest sample-to-sample step -- seventy-three modes that were
ringing, gone in one sample -- and carrying each resonator's state across the
rebuild did nothing for it, because the composition of the bank is what
changes. So a budget-driven rebuild is deferred behind a fade: the soundboard,
undamped and bed sums ramp to nothing over forty milliseconds, both banks
rebuild while they are silent, and the sums ramp back. One multiply per
sample, no block rendered twice, seam 0.005 against the note's own 0.010
(`a_budget_landing_under_a_note_does_not_tick`).

## The plugin side

One optional export, through `rackforge-plugin-sdk`:

```rust
fn set_realtime_budget(&mut self, fuel_per_call: u64) -> bool {
    self.budget_fuel_per_call = fuel_per_call;
    self.budget_pending = true;
    true          // false — the default — means "I do not scale myself"
}
```

Every SDK-built plugin exports the symbol; the **return value** is the opt-in.
A plugin that answers `false` is never asked again and is left exactly as its
author shipped it. Nothing here is required to render audio: the sandbox's hard
fuel cap still stops a runaway, and per-slot telemetry still names which plugin
is over.

Three rules the export has to keep:

* **Real-time safe.** It is called between blocks, on the thread that just
  rendered one. No allocation, no blocking.
* **Rebuild at most once per budget.** Concert Grand recomputed its soundboard
  spacing on every block instead, and when the bank could not reach the mode
  count being asked for — the builder stops at a ceiling frequency, not at a
  count — the correction oscillated and rebuilt the board 375 times a second.
* **The budget is per call.** It is worth only as much as the call is long: the
  same number over 512 frames buys a quarter of what it buys over 128. Divide
  by the frame count.

## What it does today, on a Raspberry Pi 4

`AUDIO_QUALITY_BUDGET slot=… fuel=… reason=… ns_per_fuel=…`, published by the
telemetry thread — the audio thread only stores two integers, because formatting
a string on it would allocate.

Concert Grand, at 128 frames: the host measures `ns_per_fuel` around 0.20 and
hands over about 5 M fuel; the instrument answers on three axes at once,
driven by one quality scalar so the losses arrive together -- the soundboard's
modes spaced wider (169 down to a floor near 96), the undamped top register's
partials thinned from the top down, and the partial ceiling every new note is
given a ladder from. The polyphony ramp of `RELIABILITY.md`, before and after:

| notes held | before | after |
| --- | --- | --- |
| 6 | 145 misses / 10 s | 0 |
| 8 | 261 | 0 |
| 12 | 670 | 59 |

The first missed deadline moved from six notes to twelve. What it costs the
ear is rendered, not claimed: `budget_quality_render` writes the instrument
as voiced and as the appliance settled it into one file, and the player
judges.
