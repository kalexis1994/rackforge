# The action as a mechanism: five behaviours, and what each costs in real time

A feasibility study, 2026-09-08. The Concert Grand's action is calibrated in
level and colour but is not a *mechanism*: a note-on strikes at once with
whatever velocity it carries, a note-off seats the damper in the same sample,
and a key that makes no sound does not exist. The RF-73 model, which solves
its action as coupled bodies, measured how a real mechanism departs from
that, and the departures are audible. This document says, for each of the
five behaviours, what the grand does, what the model does today (with the
line it does it at), what would change, what it costs on the audio thread,
and what has to be measured before the number is trusted.

The short answer first: **all five run in real time for nothing.** Every one
is per-event logic plus, at most, one countdown per voice decremented once
per sample beside the two the voice already keeps (`cull_in`,
`tension_in`, `lib.rs:9031-9041`). None allocates, none adds a partial.
The cost is engineering and measurement, not CPU. The one expensive item on
the same list, the hammer that can only brighten, costs nothing per sample
either; it costs a refit.

Where a figure below comes from RF-73 it is labelled so. Those figures are a
Rhodes action's — a hammer thrown at a tine, a damper lifted by the hammer
through a bridle. The *shapes* carry over to a grand; the numbers are to be
re-measured, and the trigger of the damper does not carry over at all: in a
grand the key lifts the damper, not the hammer.

## The MIDI we have to work with

MIDI gives the model a note-on with a velocity, a note-off with a release
velocity when the keyboard sends one (`release_velocity`, `lib.rs:7571`), and
the time between them. It does not give key position. Every behaviour below
therefore infers the key's state from three things: the velocities, the time
since the last event on that key, and what the model itself did at those
events. That inference is the design; the physics is what it is fed.

## 1. The soft threshold and the escapement

**The grand.** The jack lets the hammer off 1.5-3 mm before the string and
the hammer flies free the rest of the way. That flight has a toll — gravity
over the flight, the repetition spring, friction — and a hammer released
with less energy than the toll turns back before the string: the key goes
down and nothing sounds. The threshold is at the bottom of the dynamic
range a player can reach, and the curve above it is steep: just above the
threshold, a little more key speed is a lot more hammer speed.

**RF-73 measured.** The toll is a fixed *energy* set by the flight distance,
not a fraction of the hammer's energy: 1.74-2.21 mJ across a fivefold range
of release energy over the same 1.6 mm flight
(`RF-73/docs/LOADED-FLIGHT-BUDGET.md`). So the impact speed follows
`v_impact² = max(0, v_release² − v_toll²)`: nothing below the threshold,
then a curve that rises almost vertically. At drive 1.125 m/s the hammer
arrived at 0.106 m/s; at 1.3125, 16.7 % more drive, it arrived at 0.719 —
6.8 times faster (`LOADED-DYNAMICS.md`). A let-off spread over 0.6 mm of key
travel loses the soft strike altogether (`LOADED-LETOFF.md`); the release
must be sharp.

**The model today.** There is no threshold. `start_voice_unit`
(`lib.rs:6029`) strikes at any velocity; the only clamp is on the MIDI 2.0
path, `max(1.0/127.0)` (`lib.rs:7929`). The velocity law is
`velocity0 = HAMMER_V_FF · span^(v − 1)` (`lib.rs:6399`), 0.58 m/s at MIDI 1
and 6.8 at 127, and that speed goes straight into the contact ODE
(`simulate_strike`, `lib.rs:4574`) as the hammer's speed *at the string*.

**The change.** Read the existing law as the speed at *let-off* — the
action's mapping from key to hammer, which is what its two knobs describe —
and put the flight between it and the string:

```text
v_string = sqrt(max(0, v_letoff² − v_toll²))
```

with `v_toll` for a grand from the flight itself: gravity over 2-3 mm is
`sqrt(2·g·d)` ≈ 0.2-0.25 m/s, plus the repetition spring and friction,
which the measurement below decides. Below the threshold the note-on does
not strike; it becomes a *silent key* (behaviour 2). The existing law's
bottom (0.58 m/s at MIDI 1) sits above such a toll, so the mapping's low end
needs re-anchoring so that the softest MIDI velocities land under the
threshold: otherwise the threshold exists and no keyboard can reach it. That
re-anchoring is a refit of the `pp` end of the scorecard only; `mf` and `ff`
do not move.

**Cost.** One square root per note-on. Nothing per sample.

**Measure first.** Two things. The toll: in the Salamander's softest layers
(`piano-reference-banks`, sixteen velocity layers) the level of layer 1
against layer 2 says how steep the curve is at the bottom, and whether layer
1 is itself near the threshold. And the current model's own `pp` ladder,
which the scorecard already says is too bright in the tenor and treble: a
threshold that makes the softest strikes *slower at the string* is exactly
the mechanism the calibration has been missing there, and the refit should
be done with it in place rather than before it.

## 2. The silent key that lifts its damper

**The grand.** Press a key too slowly to sound and its damper still lifts:
the string is free, and it rings by sympathy with whatever else is
sounding. A real technique — silent re-take of a chord, a bass string left
open under a melody — and a direct consequence of behaviour 1, since every
strike below the threshold produces exactly this state.

**The model today.** A key with no strike mints no voice, so nothing
represents it: `note_sounding[]` (`lib.rs:8871-8879`) and `bed_busy[]`
(`lib.rs:9094`) are built from voices, and the free-string bank
(`undamped[192]`, `lib.rs:4133`) covers only the notes above the last
damper (`tune_undamped`, `lib.rs:5171`). The docs say so:
`PIANO_MODEL.md`, "Still not modelled: strings with NO sounding voice".

**The change.** A key-state table on the instrument, `keys: [KeyState; 88]`
(down, up, time since the last transition, whether it struck), written by
every note-on and note-off, including the ones that do not strike. And a
small pool of *free strings for held silent keys*: on a silent key-down,
allocate one of, say, sixteen entries — eight partials with unison pairs,
tuned to that note exactly as `tune_undamped` tunes the top octave — driven
by the bridge like the rest of the bank and released on key-up. A key that
strikes needs nothing new: its voice is already free while held. The bed
(`bed[40]`) should skip a fundamental whose key is down, since that string
is no longer damped.

Not the whole compass in the bank: 88 × 8 × 2 rotations per sample would be
the cost of a second instrument. Sixteen strings on demand is 256 rotations,
under 4 % of what the voices already tick, and a player rarely holds more
than a handful of silent keys.

**Cost.** Per sample: the allocated strings only, ≤ 256 rotations. Per
event: one tuning of eight partials (a few dozen transcendental calls at
note-on, off the per-sample path as the top-octave bank's are).

**Measure first.** Nothing new: the top-octave bank was calibrated against
the Salamander's `harmL` samples, and a silent key's string is the same
object one octave down. A listening check that a silently held octave under
a struck note is *audible* and *not* a second note — the same test the
sympathy control already has (`lib.rs`, "a pianissimo C4 two seconds in
carries measurably more energy when a fortissimo C3 rings under it").

## 3. The damper lifts before the strike, and lands late, and bounces

**The grand.** The key lifts the damper at about half its travel, so the
string is free some milliseconds before the hammer arrives — long at `pp`,
almost nothing at `ff`. On release the damper does not stop the string
where the key-off is: the felt takes tens of milliseconds to reach the
string, lands with a speed, and bounces; the string keeps ringing between
the bounces, at the level each contact left it.

**RF-73 measured.** The felt landed 22-32 ms after key-up in every
configuration tried, at 0.18-0.38 m/s, and bounced 4-9 times over 17-31 ms
before seating; the output reached −20 dB 42-58 ms after key-up and −40 dB
not within 60 ms (`LOADED-DAMPER-SEATING.md`, `LOADED-DAMPER-LIFT.md`). The
landing time did not depend on how far the felt had been lifted nor on its
spring, because the arm was held up by the bridle until the hammer fell —
**the Rhodes trigger, which does not carry over.** In a grand the key lets
the damper down, so the landing delay is the key's return time to the
damper's contact height: it depends on the release velocity, which MIDI
sends and the model already reads (`damper_span`, `lib.rs:7593`).

**The model today.** Damping is one multiplication of every partial's
per-sample rotation radius, in the same sample as the note-off
(`damp`, `lib.rs:3611-3654`; the multiply at `:3637`). The rate is scaled by
release velocity (`damper_for`, `lib.rs:7670`), the horizontal lane is
gripped less (`DAMPER_HORIZONTAL_GRIP`), and the felt's own thud is fired
there too. There is no per-voice damper state ticked over time; the closest
thing is the half-pedal ledger, `damper_applied` and `press_damper`
(`lib.rs:3548`), which presses and relieves *through the same firmness* so
that every relief undoes exactly one press — the constraint that makes a
bounce expressible at all.

**The change.** A per-voice damper state machine:

```text
Lifted --key-off--> Falling(samples) --lands--> Bounce(k, airborne, samples)
                                                  --k exhausted--> Seated
```

`release()` (`lib.rs:7718-7752`) arms `Falling` with the landing delay from
the release velocity instead of calling `damp()`; the per-sample voice loop
decrements it beside `cull_in`; on landing, `press_damper(own, +1)` and the
felt thud; each bounce is `press_damper(own, −1)` for the airborne samples
and `press_damper(own, +1)` on the next contact, with the airborne spans
shrinking geometrically (RF-73's restitution was 0.5-0.55; 4-9 contacts over
17-31 ms). The key-off knock stays at key-up, where it belongs; the damper
thud moves to the landing. `held` and `sustained` keep their meaning; the
new state only says where the felt is.

The lift-before-strike side is smaller than it sounds: the struck voice is
free while held already, and the only thing a few milliseconds of early
freedom would change is sympathetic pickup by that string before its own
strike, which is below anything the ear will find. It costs one countdown
to do properly; it can wait.

**Cost.** One countdown per voice per sample. Per note-off: up to about ten
`press_damper` calls over the next 60 ms, each a multiply over the voice's
partials × lanes (≈ 240 multiplies) — a few thousand multiplies per note-off
against tens of thousands of rotations per sample. Nothing allocates.

**Measure first.** The landing delay and its dependence on release speed,
in the Salamander's release samples (`rel*` are the key-off knocks; `harmL*`
carry the string under the damper): time from sample start to the −6 dB
drop in the note's own band, per note, per layer. The bounce count and
spacing from the ripple on that decay. The memory of the first calibration
says the drop is 25-30 dB within 20 ms and then slow; that "within 20 ms"
is where the landing delay hides.

## 4. Repetition that depends on the key and the hammer

**The grand.** The repetition lever holds the hammer part-way up after a
strike and the back check catches it; the key needs to rise only to the
repetition point — about a third of its travel — for the jack to reset, and
the second blow is struck from there, sooner and from nearer the string.
Below the repetition point the jack has not reset: the second press pushes
a hammer that is not engaged, and the blow is weak or absent.

**RF-73 measured.** The second strike's strength depends on the state the
first left behind: after a 60 ms wait the soft second blow was 13.5 times
the first, after 300 ms 6.7 times, while strong blows repeated within 9-11 %
(`LOADED-REPETITION.md`); a second gesture that catches the hammer still
bouncing carries it to the same let-off and repeats within 1 %
(`LOADED-LANDING.md`). That is a Rhodes returning its hammer by a spring
with no gravity; the grand's back check and repetition lever exist to make
exactly this repeatable, which is why the shape (a repetition point below
which the blow fails, a strike from check above it) is the thing to carry,
not the ratios.

**The model today.** A re-strike merges into the living voice partial by
partial (`lib.rs:7164-7317`) — momentum added to the string, the
displacement continuous — and that is right. But it is shipped **off**
(`RESTRIKE_MERGE = 0.0`, `lib.rs:2374`) since 0.171.5, because the push
into the phasors in one sample was audible as a pop; and its only condition
is voice state, `held || sustained` (`lib.rs:6085`), never the time since
the key-off. With the merge off a repetition eases the old voice out over
30 ms and strikes fresh (`lib.rs:6096-6119`).

**The change.** Three parts. The key state from behaviour 2 gives the time
since the key-off, `Δt`; the key's return is a curve of its own, from the
release velocity (a key let go fast returns in ~40 ms, a key eased up in
more), and the repetition point is a fraction of it, so `Δt` maps to a
*jack-reset fraction* `r ∈ [0, 1]`: below `r_rep` no strike (a silent key,
behaviour 2), above it a strike whose let-off speed is scaled by the shorter
throw — the hammer from check has a third of the travel to gain speed, so
the same finger produces a slower hammer and the threshold of behaviour 1
sits higher. Second, the damper: at `Δt` under the landing delay of
behaviour 3 the felt has not landed, the string still rings, and the merge
is the physical path; the merge's pop has a known cause — the push lands in
one sample — and its fix is to spread the momentum over the contact time
(1-3 ms), the same ramp fresh voices already use. Third, `RESTRIKE_MERGE`
comes back on, gated by that ramp.

**Cost.** Per note-on: a few comparisons and one scale. The merge is the
existing code; the ramp is a per-partial linear step over the contact
samples, which fresh voices already pay.

**Measure first.** The Salamander cannot say: a sampler has no mechanism.
The repetition point and the key-return curve are geometry and can be
taken from any regulation manual (repetition at roughly a third of the
10 mm dip; key return 30-60 ms); the second-blow strength from check is
what a struck-string measurement on a real action would give, and until
one exists the honest position is the geometric one, stated as such.

## 5. The time between the strike and the key bottom

**The grand.** After let-off the key still has its aftertouch to travel
and lands on the bed while the hammer is in flight. At `pp` the hammer's
flight is long and the key lands 1-5 ms *after* the string is struck; at
`ff` the two nearly coincide. And the bed's knock scales with the *key's*
speed squared — the finger's energy into the felt — not with the hammer's.

**RF-73 measured.** With a sharp let-off the key landed on its bed 1.0 ms
after let-off at the strong drive, having accelerated through its 1 mm
aftertouch, and the bed destroyed the finger's 4-6 mJ; the hammer's flight
at the soft drive took 3.0 ms across 1.5 mm (`LOADED-LETOFF.md`,
`LOADED-FLIGHT-BUDGET.md`).

**The model today.** The thump fires in the note-on's own sample
(`lib.rs:7444-7471`) with a 4 ms rise (`lib.rs:7105`), its level
`velocity^3` (`THUMP_VELOCITY_POWER`, `lib.rs:2084`) times the register laws.
There is no delayed-event queue anywhere in the plugin, and the event
slices are borrowed for the block only, so nothing can be posted into the
future (`process_wide`, `lib.rs:8950-8967`).

**The change.** A per-voice `thump_in` countdown armed at note-on with the
flight time minus the aftertouch time — from the let-off speed, `d / v` for
the 2-3 mm flight, less the ~1 ms of key travel — so the thump lands after
the strike at `pp` and with it at `ff`; and its level from the *key* speed,
which the action ratio gives from the let-off speed. A key below the
threshold (behaviour 1) still lands on the bed, softly; that needs a thump
without a voice, which is a small ring of noise generators beside the free
strings of behaviour 2, allocated the same way.

**Cost.** One countdown per voice per sample; the noise ring is the thump
biquads the voice already runs, counted separately.

**Measure first.** The Salamander's softest layers carry the bed knock in
the same sample as the tone: the time from tone onset to knock onset, per
layer, is the delay curve, and its level against layer is the power law.
The calibration notes already say the reference's knocks are dark and loud
(`salamander-scorecard`); this is the same measurement one step further.

## The order, and the one that is not free

By what the ear gains per line changed:

1. **Threshold + silent key** (1 and 2): one square root, a key table, a
   small string pool. Changes how `pp` feels and fixes a documented gap in
   one move. Needs the `pp` re-anchoring.
2. **Damper landing and bounce** (3): a per-voice state machine on the
   existing press/relief ledger. Changes every release. Needs the landing
   measurement from the reference's release samples.
3. **Repetition with state** (4): the key table again, the ramp that lets
   the merge back on, a geometric repetition point. Changes fast playing.
4. **Key-bottom timing** (5): a countdown and a level law. Changes the
   feel of soft touch. Needs the onset measurement from the softest layers.

And apart from the mechanism, the item the model's own ledger names as the
cause of the timbre it has chased for thirty versions: **the hammer can
only brighten** (`PIANO_MODEL.md`, "The `max` blend is the binding
constraint"). It is not an action behaviour and costs nothing per sample —
the blend happens at strike time — but repairing it is a crossfade against
the recipe by a stated felt weight and letting the simulation carry its own
level, and that moves every anchor: it is a refit with the measurement
loop, validated on absolute band levels and crest factor, not a patch. It
belongs after the mechanism, because the threshold changes the hammer
speeds at the bottom of the range the refit would fit.

## What "real time" means here, in numbers

The plugin's per-sample work is the rotations: up to 32 voices × up to 48
partials × 5 lanes, plus the banks. Every item above adds, per sample, at
most one integer decrement and compare per active voice and the free
strings of the silent keys actually held. Per event it adds tens of
operations, and per note-off a few thousand multiplies spread over 60 ms.
No allocation, no transcendental call on the sample path, no new partial
per voice. The `no_std` wasm build and the Pi are not at issue; the
measurement loop is.
