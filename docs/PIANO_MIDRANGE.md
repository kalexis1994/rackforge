# The mid register, measured

The player's report: the bass is right; from the tenor up through the
mid-treble the instrument "lacks something". This is what the reference says
that something is, measured before anything was touched, and what the first
change against it did.

Everything below is the model at velocity 125 against the YDP Grand samples
in `tools/piano-targets.json`, through the same extractor the calibration fit
uses (`tools/fit-piano-cal.py`: eight bands, six windows, both sides measured
identically). Renders come from `render_reference` with the shipped
calibration table; the mid-register score in this document is the scratch
script the sweep used, and its two numbers are stated where they appear.

## Where the model leaves the instrument

Per-anchor shape error (mean absolute band-balance difference inside a
window) climbs from 6-9 dB in the bass to 12-16 dB between D#4 and F#5. The
calibration table has already been telling the same story: `felt` sits pinned
at its 4.0 ceiling at D#5, `thump` and `chiff` at their ceilings from C6 up,
and the fit's own docstring says a parameter against its bound is the model
failing to express something, not a value to trust.

Three things separate, and they are not the same defect.

### 1. The strongest band is in the wrong place

Strongest band per window, reference against model, raw levels:

| note | f0 | reference, all six windows | model, first three windows |
| --- | --- | --- | --- |
| A3 | 220 Hz | 120-250 (the fundamental) | 500-1k |
| D#4 | 311 Hz | 250-500 | 500-1k |
| F#4 | 370 Hz | 250-500 | 500-1k, 1k-2k, 1k-2k |
| A4 | 440 Hz | 250-500 | 2k-4k, 500-1k, 250-500 |

The instrument's energy sits on the fundamental from the first thirty
milliseconds; the model's sits one or two partials up for the first quarter
second, and in F#4 it drifts back up again at a second. That is an attack
whose fundamental is too weak against its second and third partials, in
exactly the register where those partials are 700-1500 Hz and the ear places
the note by them.

### 2. The upper partials do not die

Fall in dB from 55 ms to 1.0 s, reference against model:

| note | 2k-4k | 4k-8k |
| --- | --- | --- |
| D#3 | +1.7 vs -2.9 | 9.6 vs 1.9 |
| F#4 | 3.6 vs -1.8 | 15.8 vs 2.5 |
| A4 | 4.5 vs 0.1 | 18.8 vs 5.3 |
| D#5 | 13.9 vs 3.4 | 25.8 vs 4.0 |
| A5 | 12.8 vs 2.8 | 23.3 vs 11.1 |

In the sustained window (0.3-0.6 s), normalised to each window's strongest
band, the model's 1-8 kHz bands sit 10-20 dB above the reference's from D#4
to A5. Its mid-register notes do not darken as they ring: the reference
loses 10-27 dB of 4-8 kHz in the first second, the model loses 2-12.

The cause is in `t60_seconds`. The string's own loss curve was refitted to be
nearly flat (3.4 s at 50 Hz against 2.5 s at 3.2 kHz -- `PIANO_MODEL.md`,
"Frequency-dependent damping"), on 122 partials across eleven notes that
were mostly bass, and the claim that radiation does the darkening holds
there: on A0 a 2 kHz component is partial 70, and the bending term `kappa *
n^2` and the radiation channel both bite. On F#4 the same 2 kHz is partial
five. Bending is negligible, radiation efficiency is flat above 400 Hz, and
the only loss that distinguishes partial eight from partial one is the
viscoelastic term -- a factor of two between them where the instrument shows
five to ten.

### 3. The knock is not there

30-120 Hz sits 20-40 dB below the reference on every mid and treble anchor,
and the noisiness error jumps from 2-5 dB in the bass to 10-17 dB from C5
up. `PIANO_MODEL.md` already records this: the broadband impact of the
action is not modelled, and `chiff` pinned at its ceiling is the fit asking
for a source that does not exist.

## What a knob sweep said

Fifteen single-knob variants rendered and scored on the 51-81 anchors. The
score's two numbers: *excess*, the mean of the 1-8 kHz bands relative to the
strongest band in the 0.3-1.2 s windows, model minus reference; and the fit
cost split by register so the bass could not be traded away unnoticed.

| variant | excess | mid cost | bass cost |
| --- | --- | --- | --- |
| baseline | +5.2 dB | 351 | 206 |
| `STRING_HF_LOSS` 5 → 10 | +2.6 | 340 | **215** |
| `STRING_HF_LOSS` 5 → 20 | -1.0 | 350 | **233** |
| `HORIZONTAL_SHARE` 0.1 → 0.05 | +4.4 | 345 | 206 |
| `RADIATION_RATE` 3.4 → 5 | +4.6 | 342 | 217 |
| strike point ×0.85 / ×1.15 | +5.6 / +5.6 | 353 / 350 | 200 / 205 |
| `RADIATION_COINCIDENCE` 100 / 400 | +5.4 / +4.7 | 351 / 350 | 201 / 210 |

The wire's viscoelastic loss is the lever, and doubling it globally is the
wrong shape: it fixes half the mid-register excess and costs the bass nine
points, which is the same trap `PIANO_MODEL.md` records the earlier global
curves falling into. The bass rate is right where it was fitted.

## The change: the loss is a property of the wire

Bensa, Bilbao, Kronland-Martinet and Smith (*JASA* 114, 2003) give the
string's loss as `sigma = b1 + b3 * omega^2` **per string**; `b3` is the
wire's, not the instrument's. The model now scales its viscoelastic term by
`wire_loss_factor(f0)`: one below `WIRE_LOSS_FROM` (0.31 of the compass,
about C3, where the wound bass ends), climbing log-linearly to
`WIRE_LOSS_TOP` at C8. A top of 1.0 is the fitted rate everywhere, bit for
bit -- the render fingerprint is unchanged at the default.

Swept against the reference, the bass cost does not move and the rest
improves with the factor:

| `WIRE_LOSS_TOP` | excess | mid cost | treble cost | bass cost | total |
| --- | --- | --- | --- | --- | --- |
| 1 (before) | +5.2 | 351.0 | 345.4 | 206.2 | 902.6 |
| 2 | +4.5 | 346.1 | 324.9 | 206.2 | 877.2 |
| 3 | +4.1 | 343.6 | 313.1 | 206.1 | 862.9 |
| 4 | +3.7 | 341.9 | 303.8 | 206.2 | 851.9 |
| 6 | +3.3 | 340.0 | 288.6 | 206.3 | 834.8 |

Extended, the cost keeps falling and never quite stops:

| `WIRE_LOSS_TOP` | excess | mid cost | treble cost | bass cost | total |
| --- | --- | --- | --- | --- | --- |
| 8 | +2.9 | 338.4 | 280.5 | 206.1 | 825.1 |
| 12 | +2.4 | 337.1 | 269.4 | 206.3 | 812.8 |
| 6, from 0.20 | +2.6 | 339.6 | 288.0 | 207.3 | 834.9 |
| 6, from 0.40 | +3.9 | 342.9 | 289.8 | 206.2 | 838.9 |

**Shipped: 6.** Read the columns: past 6 the mid gains a point or two per
doubling while the *treble* cost is what keeps dropping -- and the treble is
the register whose reference is dominated by a knock the model does not have
(defect 3). A cost that cannot see the missing source is rewarding an
over-damped treble for matching band balance the wrong way, which is the
trap the fit's own docstring warns of. Six at C8 puts C7 at about 4.4x the
tenor's rate, where Chaigne and Askenfelt's treble `b3` sits against their
C4 value; the ear takes it from there. The start position barely matters
(0.20, 0.31 and 0.45 within noise), so it is a constant, not a knob.

The default render fingerprint moved from `0x7dd8d95093b622f5` to
`0x0c396512799eb435` with this change: the instrument is different on
purpose, above C3.

## The attack, measured (defect 1)

The sustain is one half of the complaint; this is the other, and it is the
half the phrase "le falta" fits better. Measured with
`tools/measure-attack-fundamental.py`, which exists because the fit cost
normalises each window by its own strongest band and therefore cannot tell
"more high content" from "less fundamental".

### It is one octave and a half, not the whole middle

Level of the fundamental's band against the window's own strongest band,
0-30 ms, reference | model:

| note | f0 | reference | model |
| --- | --- | --- | --- |
| C3 | 131 Hz | strongest | **strongest** |
| F#3 | 185 Hz | strongest | **strongest** |
| A3 | 220 Hz | strongest | **-8.0 dB** |
| C4 | 262 Hz | strongest | **-6.6 dB** |
| D#4 | 311 Hz | strongest | **-7.4 dB** |
| F#4 | 370 Hz | strongest | **-4.9 dB** |
| A4 | 440 Hz | strongest | **-5.7 dB** |
| C5 | 523 Hz | strongest | **strongest** |
| A5 | 880 Hz | strongest | **strongest** |

Mean over the attack windows of the anchors 51-81: **-4.0 dB**. Outside
A3-A4 the model puts its energy where the instrument does.

### The strike-point comb is where that energy goes

Per-partial levels of the model's own render, 0-30 ms, against what
`sin(n·pi·x0)` alone predicts (both normalised to their strongest):

    A4, x0 = 1/8.8      1      2      3      4      5
    model            -3.8   -0.6   -0.8   -2.2    0.0
    comb             -9.0   -3.6   -1.0    0.0   -0.1

    C3, x0 = 1/8.0      1      2      3      4      5
    model             0.0  -10.8   -6.8  -34.6  -20.8
    comb             -8.3   -3.0   -0.7    0.0   -0.7

At A4 the model follows the comb; at C3 something darkens the attack by
thirty decibels at the comb's own peak and leaves the fundamental on top.
Whatever that something is, it stops working between F#3 and A3.

### It is the shape of the contact force, not its length

The simulated contact times match the literature (`strike_profile` and
`how_long_the_hammer_stays`): 3.62 ms at A0 fortissimo, 2.18 at C3, 1.39 at
C4. What changes across the compass is **where the force peaks inside that
contact**:

| note | contact | peak at | peak / contact |
| --- | --- | --- | --- |
| A0 | 3.62 ms | 0.29 ms | 8 % |
| C3 | 2.18 ms | 0.26 ms | 12 % |
| C4 | 1.39 ms | 0.66 ms | 47 % |
| C6 | 0.80 ms | 0.39 ms | 49 % |

In the bass the force rises in a quarter of a millisecond and stays up for
most of the contact -- a long, flat pulse, and a dark attack. From C4 up it
is a short symmetric pulse, and the attack is bright. That is the register
pattern, and it comes from the hammer-string integration, not from any
filter downstream of it.

### What was ruled out, by measurement

* **The recipe.** The strike simulation is what is carrying the fundamental,
  not the drawn recipe underneath it: turning the simulation off
  (`SIM_MIN_MODES` past the mode count) takes the deficit from -4.0 dB to
  **-15.7**, and putting the recipe back as a floor (`RECIPE_FLOOR` 0.5, 1.0)
  gives -5.7 and -9.7. Improving the recipe cannot help; the simulation
  already overrides it everywhere below `SIM_TOP_HZ`.
* **The hammer's contact width.** x0.6 to x2.4 moves the deficit between
  -3.9 and -4.5 dB. Nothing.
* **Stulov's hysteresis.** epsilon 0.1, 0.9 and tau at his published 2 us:
  -4.1, -4.0, -3.8 dB. Nothing.
* **`FELT_K_DECADES` 3.0.** Buys 0.9 dB of fundamental, costs 6 dB of
  brightness everywhere and takes the bass cost from 206 to 250.

### The one lever that works, and why it is not taken

The strike point, exactly as `sin(pi·x0)` predicts: x1.30 takes the deficit
from -4.0 to **-2.6 dB** with the bass cost unmoved, and x0.75 makes it
-5.4. But x1.30 puts the tenor's strike at 1/6.5, outside the published
1/7..1/9, and `PIANO_MODEL.md` records the current value as matching
Conklin and the KTH lectures. Buying 1.4 dB by breaking a measured value is
the trade three of this model's retractions were written about.

### A defect found on the way: the fit has a disconnected lever

`cal(note, 0)` -- the calibration table's `felt` column -- scales the felt
low-pass corner **in the recipe**, and the recipe only survives above
`SIM_TOP_HZ` (8 kHz). Below that the simulation supplies every amplitude.
So across the whole mid register the column does nothing: sweeping it from
its 0.25 floor to its 4.0 ceiling, and setting the entire column to 1.0,
moves the fundamental deficit by **0.0 dB** and the total fit cost by two
points out of 835.

Meanwhile the fit has pinned it at its bounds at three anchors -- 0.25 at
C3, 4.0 at D#5, 2.74 at F#4 -- which is the fit's own docstring's warning
("parameters against their bounds mean the fit is compensating for
something the model cannot express") coming true on a lever that is not
connected to anything it can hear.

Repairing it means making the column scale the *simulation's* felt
stiffness, which is the brightness control the mid register actually has.
That invalidates the fitted table -- D#5's 4.0 would suddenly bite hard --
so it cannot ship without a refit, and a refit is a multi-hour run that has
to be validated on absolute band levels and crest factor rather than on the
cost alone.

## What this does not fix, in the order it is worth doing

1. **The fundamental in the attack (defect 1).** Measured in the section
   above: it is A3-A4 only, it is the shape of the simulated contact force,
   and the recipe and the felt calibration column have nothing to do with
   it. The next move is to reconnect `cal(note, 0)` to the simulation's felt
   stiffness and refit -- a multi-hour run validated on absolute band levels
   and crest factor, not on the cost.
2. **The knock (defect 3).** A source, not a filter; `chiff` cannot scale
   what is absent. Most exposed from C5 up.
3. **The horizontal share.** Halving it helped every register a little with
   no bass cost; it is the aftersound's spectral content in the mid, and a
   candidate for the same per-string treatment as the wire loss.
4. **The soundboard above 1 kHz.** Ege and Boutillon's measured mobility
   (`PIANO_RESEARCH.md` §5, still open) is what would give the mid-treble
   its ragged, individual body; the synthetic three-sine curve is smooth
   where the instrument is not.

## How to measure it again

    CG_RENDER_DIR=target/fit-renders CG_CAL=tools/piano-cal.txt \
      cargo test -p rackforge-concert-grand render_reference --release -- --ignored

renders the 29 anchors. `tools/fit-piano-cal.py`'s `measure` and `note_cost`
score them for balance; `tools/measure-attack-fundamental.py` scores the one
thing that cost is blind to, the fundamental against its own window. A
`CG_TUNING` file of `NAME = value` lines sets any knob in the registry
before the render, which is how every lever above was swept. The A/B of any candidate, as one continuous
file with a lead-in:

    CG_WIRE_TOP=6 cargo test -p rackforge-concert-grand --release wire_loss_render -- --ignored --nocapture
