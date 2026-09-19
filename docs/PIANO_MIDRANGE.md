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

## What this does not fix, in the order it is worth doing

1. **The fundamental in the attack (defect 1).** No loss curve moves it: it
   is the initial spectrum, and the sweep says the strike point is not the
   lever either (±15 % changed nothing). The suspects are the felt low-pass
   at the mid register's contact times -- 2 ms at C4 is nearly a full period
   of its second partial, and the reference's spectra fall off harder above
   the fundamental than the model's fourth-order felt filter does -- and
   the `max` blend of the strike simulation against the recipe
   (`PIANO_MODEL.md`, "The hammer can only ever brighten"), which by
   construction cannot take energy off partials two to four. That repair is
   a refit of every anchor and is written up there.
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

renders the 29 anchors; `tools/fit-piano-cal.py`'s `measure` and `note_cost`
score them, and a `CG_TUNING` file of `NAME = value` lines sets any knob in
the registry before the render. The A/B of any candidate, as one continuous
file with a lead-in:

    CG_WIRE_TOP=6 cargo test -p rackforge-concert-grand --release wire_loss_render -- --ignored --nocapture
