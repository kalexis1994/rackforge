# The RackForge Concert Grand model

RackForge ships an instrument in every host, and that instrument cannot carry a
sample library: the browser host starts from kilobytes, and the bundled plugin
is the one thing that must always sound. The Concert Grand is therefore a
physical model — every sample is computed, none is recorded.

This document is the model's ledger. Each mechanism it implements names the
physics it comes from and the paper that measured it; each simplification is
stated rather than hidden. The unit tests in `plugins/concert-grand` verify the
claims marked *tested* — the model is allowed to be approximate, but not to
drift from what this document says it does.

## What is modelled

### Stiff strings: inharmonicity (tested)

A real piano string is not an ideal string; bending stiffness raises each
partial above its harmonic position. The model uses Fletcher's formula for the
frequency of partial *n*:

    f_n = n · f0 · sqrt(1 + B·n²)

— H. Fletcher, "Normal vibration frequencies of a stiff piano string",
*JASA* 36 (1964).

The inharmonicity coefficient `B` varies across the compass: smallest in the
tenor, larger in the wound bass strings, largest in the short treble. Published
measurements (Fletcher & Rossing, *The Physics of Musical Instruments*, ch. 12)
span roughly 10⁻⁴ in the middle to above 10⁻² at the top octave. The model fits
that shape with a quadratic in log-space, minimum near A2:

    log10 B(n) = -3.95 + 4.9e-4 · (n - 45)²   (n = MIDI note)

This is a fit to the published range, not a measurement of any particular
instrument.

### The hammer contact emerges, in physical units (tested by measurement)

The contact time used to be IMPOSED: a drawn law (2 ms at the bottom tapering
to 0.4, swung linearly by the Dynamics control) cut the integration loop, and
the felt stiffness was derived from that same duration — circular, so the one
phenomenon that carries touch into timbre was scripted. Measured, C4 grew
1.06× brighter from pp to ff and A5 1.02×.

Now the hammer, felt and string are in SI units and the contact ENDS WHEN THE
STRING THROWS THE HAMMER OFF:

* String: the scale's tension (850 N) plus the speaking length — itself
  corrected from a pure geometric taper to measured anchors (A0 1.9 m,
  C4 0.62 m, C8 5.2 cm), because the geometric law put C4 at 0.38 m — derive
  the linear density: μ = T/(2Lf₀)², A0 ≈ 66 g/m, C4 ≈ 7, both published.
* Hammer: Askenfelt's head masses in kilograms (11 g at A0, ~5.2 g at C4,
  3.5 g at the top, curved not linear), shared between the strings struck;
  speed in m/s (6 at full velocity, with `dynamics` setting the action's span).
* Felt: K in N/m^p climbing 3.2 decades across the compass; exponent
  3.2 + 1.8·position; Stulov relaxation at **0.5 ms, not 6 µs** — at 6 µs the
  history caught the compression inside any contact and the rate-hardening
  (felt stiffer against a fast blow, Stulov's actual point) was erased.

Emergent contact times land on the measured ones: A0 3.9/2.9 ms (pp/ff),
C2 3.1/1.7, C4 2.2/1.7 — right ranges, right pp/ff ratios. The felt pulse's
physics identified two regimes: the pp–mf range, where felt hardening rules
(C4's 10th partial spans **32 dB** from v40 to v125, against 4 dB before),
and the top quarter (v100→v125), where the pulse sits on the string-spring
floor π·√(m/k_s), k_s = T·L/(x₀(L−x₀)) — the same floor the real instrument
sits on. The YDP still gains ~8 dB per partial up there; that share comes
from the string's own nonlinearity and the action noise, which are the next
mechanisms, not from the hammer.

### Stretched tuning, derived rather than drawn (tested)

Pianos are not tuned to equal temperament; tuners stretch octaves so the
second partial of the lower note — sharp, because of B — beats against the
fundamental of the upper. The Railsback curve (O. L. Railsback, "Scale
temperament as applied to piano tuning", *JASA* 9, 1938) documents the result.

The model does not hard-code that curve. It derives it: octave anchors are
tuned outward from A4 = 440 Hz so that each octave is beatless against the
lower note's second partial,

    f0(n+12) = f_2(n) = 2·f0(n)·sqrt((1 + 4B)/(1 + B)),

and notes between anchors interpolate the stretch in cents. Because the
stretch comes from B, a change to the inharmonicity fit re-tunes the
instrument consistently — the two cannot disagree.

### The hammer: strike point and felt (tested)

Two facts shape the initial spectrum:

* **Strike point.** The hammer strikes at a fraction `x0` of the string's
  length, which suppresses partials with a node near that point: amplitude
  carries the factor `sin(n·π·x0)`. Real instruments strike near 1/8 in the
  bass, closer to 1/13 toward the treble (Fletcher & Rossing, ch. 12); the
  model interpolates across that range. The comb is audible: it is much of why
  a piano sounds "hollow" in a characteristic way.
* **Felt compression.** Hammer felt is a hardening spring — contact force
  grows faster than compression, with exponent measured between roughly 2.2
  and 3.5 (A. Chaigne & A. Askenfelt, "Numerical simulations of piano strings
  I–II", *JASA* 95, 1994). The practical consequence is that harder blows
  shorten the hammer–string contact and brighten the tone. The model renders
  this as a second-order low-pass over the partial amplitudes whose cutoff
  scales with the reciprocal of the contact time; measured contacts run from
  about 4 ms at pianissimo in the bass to under 1 ms fortissimo in the treble
  (A. Askenfelt & E. Jansson, "From touch to string vibrations", *JASA*
  1990–93). The cutoff's constant of proportionality is empirical — a strict
  1/(2·t) reading of the pulse width comes out far darker than measured piano
  spectra, because the felt hardens during contact — and is stated as such in
  the code. Velocity
  therefore changes the spectrum, not just the level — the single most
  important dynamic trait of the instrument.

**The radiated spectrum is bridge force, not string displacement.** What
reaches the ear is the force the string exerts on the bridge, proportional to
the string's slope at its termination. A velocity strike leaves modal
displacements falling as `sin(n·π·x0)/n`, but taking the slope multiplies
each mode by `n`: the factors cancel, and the spectrum the soundboard is
driven with is the strike-point comb times the felt filter, flat otherwise
(Fletcher & Rossing, ch. 12: the bridge force spectrum of struck strings).
An earlier revision radiated the `1/n` displacement law directly, which is a
fundamental-heavy, electric-piano voicing; the felt low-pass and the
frequency-dependent decay now do all the darkening, which is why a bass note
starts broad and darkens as it rings instead of starting dark.

**The hammer has width.** A point excitation is a pluck: full-depth comb,
every high partial alive — a classical guitar. The felt contact spreads over
a finite fraction of the string, and the model multiplies the spectrum by
the transform of a smooth (Gaussian-like) force distribution — smooth
because felt is: an early revision used the rectangle's sinc, whose first
null on A0 fell at partial ~33 and erased the top three octaves of the bass
ladder that real A0 spectra plainly contain. The width is scaled by real
string lengths — ~0.6% on a 2 m bass string under a ~12 mm contact, ~15% on
a 5 cm treble string — and hard blows compress the felt and narrow the
contact, so the window itself brightens with velocity (Chaigne & Askenfelt
model the hammer force with a spatial window g(x); the hardening is theirs
too). Together with the felt low-pass this is what separates "struck by a
felt hammer" from "plucked at a point".

The strike-point comb keeps the sign of `sin(n·π·x0)`: a struck string's
partials alternate polarity around each node, and discarding the alternation
is part of what makes additive attacks sound synthetic.

**The board does not radiate below its first mode.** A grand's first
soundboard mode sits near 60–70 Hz; below and around it the board is a poor
radiator, so the lowest notes' fundamentals — and even second partials —
reach the air tens of dB down, and the ear reconstructs the pitch from the
partial ladder (the missing-fundamental percept; see the KTH *Five Lectures
on the Acoustics of the Piano*, and published C1 spectra where the strongest
partial is n≈4–6). The model applies a sixth-order radiation high-pass with
a 66 Hz corner to every partial — calibrated against the YDP Grand samples,
where A0's fundamental measures ~−40 dB against its strongest partial while
78 Hz already passes at full strength. Radiating the fundamental of A0 at
full strength is what a synthesizer does, and it is instantly audible as one.

**Calibration against a recorded instrument.** The YDP Grand Piano soundfont
(the same open bank RF-Soundfonts ships) serves as the model's measurement
reference: per-partial spectra and early decay slopes extracted from its
samples set the radiation corner and order, the felt filter's order (fourth —
measured spectra hold a plateau and then fall ~30 dB within a couple of
partials: C4 ff cliffs near 2.5 kHz, A4 near 3.5 kHz), the fortissimo contact
times behind those cliffs, and the ±15 dB neighbour-to-neighbour raggedness
the board response and per-note irregularity now supply. Measured early decay
(−5…−11 dB/s in the bass ff) matched the model's existing prompt stage and
was left alone.

**The board response is a fixed, ragged filter.** Measured bridge mobility
swings ±10 dB and more in narrow peaks and dips across the whole compass
(N. Giordano, "Simple model of a piano soundboard", *JASA* 102, 1997), and
every partial of every note samples the same curve — which is why each note
of one instrument has an individual character that never changes, and why
notes sharing a frequency share its colour. The model uses a fixed synthetic
curve (three incommensurate sines in log-frequency, stated as such, ~±5 dB)
that shapes each partial's level *and* pulls its decay in opposition: where
the board takes energy readily, the partial speaks louder and dies faster. A
small additional ±1 dB per-note irregularity stands in for what the fit
misses.

The hammer's own impact is present as a short computed noise burst — a
low-passed transient, heavier in the bass — standing in for the thump the
hammer and soundboard contribute before the string tone establishes. Its
bandwidth starts wider for harder blows and contracts with a ~25 ms time
constant as it fades, matching the observation that a tapped soundboard
sounds like low-passed noise whose bandwidth contracts over time (J. O.
Smith, *Physical Audio Signal Processing*, "Piano Synthesis").

### Two-stage decay and the aftersound (tested)

A struck note decays fast at first, then settles into a long quiet tail, often
with slow beats. Weinreich showed why: the two or three strings of a unison
are never perfectly tuned, and their coupling at the bridge — together with
the two polarisations of each string — moves energy into configurations that
radiate poorly and therefore decay slowly (G. Weinreich, "Coupled piano
strings", *JASA* 62, 1977).

The model now *simulates* the coupling rather than imitating its outcome.
Every partial holds its unison strings (or a lone string's two
polarisations) as separate oscillators, each detuned by its own fraction of
a cent and carrying only its intrinsic internal/air losses. At control rate
(every 256 samples) the bridge removes the same slice of the *coherent sum*
from every string: the in-phase configuration the hammer leaves radiates
strongly and dies fast, the dephased configurations that follow couple
weakly and live on, and the energy the bridge cannot reach is what carries
the tail. Two-stage decay, unison beats and the churn of a long sustain are
consequences of this dynamics, not scripted envelopes; a unit test verifies
that coherence falls with time while stored energy survives. The coupling
coefficient is calibrated so the initial coherent decay matches the measured
prompt rates.

Each partial's detune carries a fixed per-note jitter around the nominal
value, so no two partials beat alike — a uniform detune ratio would beat
every partial at a rate proportional to its frequency, precisely the
synthesizer "shimmer" a real unison does not have. The third string exists
only where the scale has one; the deep bass couples two polarisations of a
single string.

### The honest unison, and the two-stage decay it generates (tested)

Each partial now holds what a note physically has: up to **three vertical
polarisations** — one per string of the real stringing, equal shares, equal
full bridge coupling, spread by the unison detune — plus **one horizontal**
at a tenth of the bridge coupling and a fraction-of-a-cent offset (bridge
anisotropy), plus the onset bloom. For years one component (`aftersound`)
was both "the second string" (it took the detune) and "the horizontal
polarisation" (it coupled at 0.12), which is a contradiction, and the
two-stage decay had to be scripted around it.

The two stages now emerge from three curves and no shapes:

* intrinsic (per component) = the string alone — internal/air losses,
  bending, and the **incoherent share** of radiation (dephased strings
  still radiate; Weinreich's second slopes are slower, not flat);
* the bridge drain removes coherent energy at exactly the rate that turns
  the slow curve into the measured audible one (the fast stage);
* the knee's position, depth and register dependence fall out of the
  difference — deleted with this change: `tail = 1.8+2.6/(1+(f/420)^1.2)`,
  `prompt_t60 = t60*1.94/(1.4+1.1*pos)`, and the 300 Hz coupling fade.

The success criterion the bridge work set — **raising the unison detune must
stop costing energy** — now holds: C2's partials 8–11 hold the same band
energy at full detune as at zero (52.7 vs 52.6 dB; the old model lost 7–13
dB monotonically). Two hard lessons came out in calibration: the tension
rotation was not norm-preserving (each nudge scaled the oscillator's decay
factor by √(1+step²); A0 grew for 1.5 s and died in non-finite samples —
now renormalised exactly), and the fifth component per partial re-tripped
the wasm shadow stack, caught by `concert_grand_instantiates.rs` BEFORE
shipping this time. Thirteen five-component voices occupy the bytes sixteen
four-component voices did.

Calibration state after refit (SLOW_STAGE_RATIO 3.5, incoherent share 0.55,
horizontal share 0.2–0.5 falling treble-ward): per-partial bands 0.75×–1.37×
of the instrument, whole-note durations mean 0.84×, fit cost 25.93, fuel
54 % worst case.

### Frequency-dependent damping (tested)

Higher partials die faster — losses from air drag, internal friction and
thermoelasticity all grow with frequency (C. Valette & C. Cuesta, *Mécanique
de la corde vibrante*, Hermès 1993). The model gives every partial its decay
time from one smooth curve fitted to the published order of magnitude — tens
of seconds for the lowest fundamentals, under a second at the top of the
compass:

    T60(f) ≈ 6 / (1 + (f/20)^0.14) + 0.6   seconds

**This is the only decay-against-frequency curve in the model, and it took
three tries to make it the only one.** It used to be one of three. The curve
itself read `24 / (1 + (f/180)^1.25) + 0.6`; a second correction shortened the
aftersound of bass partials by `4.05 · f^-0.357`; a third divided the whole
note by up to 2.6 for its register. Each had been fitted honestly, but each
was fitted on top of the others' errors, so they double-counted: a bass note's
500 Hz partial was divided by 2.13 for its register and then by another 2.4 for
its frequency, and lived a fifth of the time the curve claimed.

Measured, the damage was flat and large. Reading the T60 of each partial off
its own envelope — 122 partials across eleven notes of the YDP, ours against
the instrument's:

    band          before   after
    0–120 Hz       1.47×    1.45×
    120–300 Hz     1.24×    1.30×
    300–700 Hz     1.06×    1.83×
    700–1600 Hz    0.51×    1.15×
    1600–8000 Hz   0.47×    0.93×

Everything above 700 Hz was dying at twice the proper rate while the bass rang
on — the two corrections were aimed at the bass and hit the whole note. Fitting
one curve against all 122 points at once cut the mean error in log-decay from
0.485 to 0.377, and the whole fit cost from 30.95 to 21.31, with the spectral
centroid moving from 10.03 semitones off the instrument to 4.87.

The knee is the substance of it. The physics wanted Valette's shape — a
frequency-independent floor plus a term climbing with wave number — and the
old curve had that shape but put its knee at 180 Hz, nearly three octaves
below where the instrument puts it. That is why it was too steep for any
single note to sit on, and why both corrections were needed to drag the ends
back.

Refitted against the render rather than against the formula — sweeping the
curve and reading each partial's T60 back out of the audio, because what
`t60_seconds` returns is not what is heard until the two-stage `tail` has
multiplied it — the curve came out **nearly flat**: 3.4 s at 50 Hz against
2.5 s at 3.2 kHz, a factor of 1.4 across seven octaves where the old one
spanned a factor of 41. That is a claim about the instrument worth stating
plainly: **the string's own losses are close to frequency-independent, and
what actually shapes a piano's decay against frequency is radiation.** The
model already had radiation as a separate parallel channel with a measured
coincidence corner, and with the string curve no longer duplicating its job
the two together land every band between 0.98x and 1.39x of the instrument:

    band          three curves   one curve
    0–120 Hz         1.47×         1.03×
    120–300 Hz       1.24×         1.39×
    300–700 Hz       1.06×         1.09×
    700–1600 Hz      0.51×         0.98×
    1600–8000 Hz     0.47×         1.05×

The curve is read at the partial's frequency scaled by the string's weight,
`(f0/220)^0.55`, and that scaling is not decoration: refitting without it is
measurably worse (0.414 against 0.357 in log-decay), because the same 2 kHz is
partial 218 on a massive A0 string and partial 92 on C2, and the bass one is
far more heavily damped. With the weight carrying the note dependence, the
curve needs no register term at all — `t60_seconds` no longer takes the note's
position in the compass, and the compiler said so.

On top of the smooth curve, each partial's decay is pulled by the board
response below — where the board takes energy readily the partial dies
faster — so neighbouring partials of one note decay at visibly different
rates, the way measured piano partials do.

Each partial reads the curve at its own frequency, so a bass note's high
partials fade like the treble notes they overlap — which is what makes the
model's bass notes darken as they ring, the way real ones do.

### The bridge is passive, and the unison is the real string count (tested)

Weinreich's coupling is simulated rather than scripted: each string loses a
slice of the sum the three of them push the bridge with, so a coherent
configuration radiates and dies while a dephased one nearly cancels at the
termination and lives on. Two things about that were wrong.

**The drive and the reaction used different weights.** The horizontal
polarisation took back 0.12 of the bridge's motion but contributed its full
amplitude to it, making the update `I - k w 1ᵀ` — not symmetric, and a
non-symmetric contraction is not a contraction. Its largest singular value is
1.0035 at the coupling this model uses: there are string configurations it
feeds energy *into*. Weighting the sum the way the reaction is weighted makes
it `I - k w wᵀ`, symmetric and positive semidefinite, largest singular value
exactly 1. That is Maxwell–Betti reciprocity, and it is what makes a
termination passive. Measured on three components with no intrinsic loss, so
every joule that leaves is the bridge's doing, the energy left after two
seconds against the unison's detune:

    cents     before    after
      0.0     0.345     0.256
      2.9     0.145     0.285
     12.0     0.066     0.274

Before, the detune the model uses cost it well over half the stored energy and
the dependence was not even monotone; after, it costs nothing.

**The string count was wrong by about eleven semitones.** A grand is
single-strung only across the very bottom — A0 to roughly E1 — doubles through
the rest of the wound bass, and is fully triple-strung from about C2 upward.
The ramp used to start at E♭2 and not finish until B2, which gave **C2 exactly
one string** and C3 barely two. That is not bookkeeping: with no second string
there is nothing for the unison detune to act on, so it was being applied
between the two *polarisations* of a single string instead. The third string
was also cut off above the twelfth partial — the same mistake the model had
already made once at partial 32, eleven partials lower.

### Phantom partials in the bass (tested)

Large-amplitude string motion couples transverse vibration into longitudinal
modes, whose nonlinear mixing puts extra components near sums of transverse
partial frequencies — most audibly near twice each low partial (H. A.
Conklin, "Generation of partials due to nonlinear mixing in a stringed
instrument", *JASA* 105, 1999; B. Bank & L. Sujbert, "Generation of
longitudinal vibrations in piano strings: from physics to sound synthesis",
*JASA* 117, 2005). They are the metallic edge of a hard bass note.

The model renders them for the bottom third of the compass: components at
twice the frequency of each of the strongest low partials, at a level scaling
with the square of velocity (a large-amplitude effect: absent pianissimo,
prominent fortissimo), decaying faster than the transverse partials they ride
above. The mixing itself is not simulated; the components it produces are.

Two further products of the same nonlinearity:

* **Sum-frequency phantoms**, `f_m + f_n`. Inharmonicity leaves each one
  slightly flat of the real partial it lands near, and the slow beat between
  the two is the growl of a hard bass note — roughness with a rate, not
  noise.
* **The longitudinal clang.** The fast longitudinal wave sounds its modes as
  a formant near ~17·f0 on wound bass strings (Bank & Sujbert measure
  ~1.15 kHz for C2): a short tonal knock with a pitch of its own, swelling in
  within a millisecond, nearly absent below forte. The hammer noise supplies
  the breath of the attack; the clang supplies its bark.

### Longitudinal vibration, generated at the bridge (tested by measurement)

The longitudinal force a string hands its termination is the transverse slope
squared: T/2·(∂y/∂x)² at x=L, and with y = Σ q_h·sin(hπx/L) that slope is
Σ q_h·(hπ/L)·(−1)^h. Its square contains **every pair product** q_m·q_n at
frequency f_m±f_n with weight m·n — Bank & Sujbert's excitation table without
the table. Each partial carries its slope weight; one accumulator per voice
per sample computes the drive; four broad resonators at 17.5·j·f₀ (T60 tens
of ms — the compressional wave damps fast, and Bank's measured formant is a
hump, not a line) do the selection. Content near their poles rings (the
formant); everything below passes at the stiffness response (the phantom
ladder).

What this replaced, and why it never worked: the pair sums were indexed by
the RESONATOR number — mode k of the bank was driven by pairs with m±n = k,
k=1..4, whose products lie at k·f₀: 65–260 Hz on C2, three octaves under
resonators at 1.1 kHz+. Measured, the bank's output peaked at 1.0×f₀, which
is why its mix sat parked at zero. The scripted phantom partials (placed at
2·fₙ) and the scripted clang (a formant parked at 17·f₀, T60 0.25 s) are
retired with it; the Phantoms and Clang panel controls now scale the real
mechanism (drive level, and the upper formants' share).

Calibrated against the instrument on a structural metric the band totals
cannot see — how far the partials stand above the floor BETWEEN them:

    C2, 900–1800 Hz     real  9.0 dB   before 36.2   now 14.3
    A0, 1–2 kHz         real  0.0 dB   now 0.4

A model whose inter-partial floor sits 36 dB down is a clean synthesizer; the
real bass is nearly full between its partials. The Kirchhoff–Carrier scalar
tension also gained its wave-number weighting (Σ h²q_h², no cross-string
terms — the cosines are orthogonal), and the measured ff glide is +2.6 cents
decaying over two seconds, in the published range.

### The soundboard, from the measured plate (tested)

Everything the strings produce radiates through one bank of 136 two-pole
resonators. Neither its spacing nor its damping is chosen by ear; both come
from published measurements of real piano soundboards, and the bank is
generated from those laws rather than written out.

**Damping.** Every mode carries the loss factor of spruce, η = 2.3% — the mean
Ege, Boutillon and Rébillat measured resolving a soundboard's modes from
50 Hz to 3 kHz, where loss factors run 1–3% up to about 1.2 kHz, essentially
the material values ("Vibroacoustics of the piano soundboard: (non)linearity
and modal properties in the low- and mid-frequency ranges", *JSV* 332, 2013).
A mode's decay follows `T60 = ln(10³)/(π·f·η)`.

**Density.** Modal spacing is 16.7 Hz — 0.06 modes/Hz — flat to 1.1 kHz, the
constant density a piano soundboard tends to below that frequency, where the
ribbed board behaves as a homogeneous plate and a thin plate's modal density
does not depend on frequency (Boutillon & Ege, "Global and local synthetic
descriptions of the piano soundboard"). Above 1.1 kHz the spacing tapers to
the ~115 Hz that the measured 60% modal overlap implies at 3 kHz, and past
3 kHz it holds that overlap rather than extrapolating the exponent beyond the
data. The resulting overlap tracks the measurement across the compass: 0.21
against 0.30 at 150 Hz, 0.76 against 0.70 at 550 Hz, 1.38 against 1.00 at
1 kHz, 0.61 against 0.60 at 3 kHz.

This replaced a pair of banks — a sparse parallel "body" and a serial through
path — holding 45 resonators between them, against the 200–500 a modelled
board needs. Two things were wrong with them, and the second was the one that
mattered.

The first was the sparse bank's reach. It ran to 3.1 kHz with spacing over
bandwidth of 5.8 at 1 kHz and 9.9 at 2.5 kHz, so between 500 Hz and 4 kHz it
imposed a *fixed* comb of 15–22 dB peak-to-dip ripple, the same resonances on
every note regardless of pitch. That is the definition of a formant filter and
it read as a nasal tint; above its last mode the response fell at 12 dB per
octave, taking the top two octaves of every bass and tenor note with it. A0
measured a spectral centroid of 344 Hz against the YDP's 1112 Hz.

The second was damping, and it was the reason the instrument read as an
electric piano rather than a grand. The old decays were set by hand — `14/f`
for the through path, 140 ms at 62 Hz for the parallel modes. The *form* was
right, since a constant loss factor gives exactly T60 ∝ 1/f, but those decays
imply loss factors of **15.7% and 25.3%**. Spruce is 1–3%. The board was made
of something with roughly the loss factor of rubber, which is to say the model
had a struck string and no resonating plate behind it — a clavinet. A knock
test on a finished soundboard gives a broadband T60 near 0.6 s; this one was
gone in 140 ms at the bottom of the compass and 25 ms at 550 Hz.

Together the two changes move the mean spectral centroid error against the
YDP from 0.80 octaves to 0.21, the bass and tenor from 1.18 to 0.15, and the
per-band decay profile — how far each band falls from the attack window to the
sustain, against how far the reference's falls — from 7.8 dB out to 3.6 dB.

The bank is synthetic and stated as such: its mode frequencies are drawn from
a density law, not from any particular instrument. A measured soundboard
response (commuted synthesis: J. O. Smith & S. A. Van Duyne, ICMC 1995) would
be a sample by another name.

### Blooming (tested)

A struck tone does not switch on: each partial swells in over a few of its
own periods — tens of milliseconds for a bass fundamental (one period of A0
is 36 ms), effectively instantly in the treble. The model renders the rise
without envelopes: each partial carries a third, negative component at the
base frequency whose decay equals the rise time, so the sum is
`A·(e^(−t/τ_slow) − e^(−t/τ_rise))` — zero at the strike, swelling into the
tone, gone shortly after. An additive attack where every partial appears
fully formed at t=0 is the signature sound of a synthesizer switching on.

### The lid, the near field, and the chamber (tested)

The output carries a handful of sparse early reflections (9–33 ms, different
delays per side): the lid and rim of an open grand reflecting the near
field. Behind them sits a small chamber — a six-line feedback delay network
with a Householder feedback matrix, mutually non-divisible line lengths for
a dense colourless tail, one-pole damping in each feedback path so highs die
faster than lows, RT60 ≈ 1.4 s at the bottom. A stage around a piano, not a
cathedral.

Both are stated as acoustic staging rather than instrument physics — but a
bone-dry direct-injected tone is precisely what an electric piano is, the
comparison samples are recordings made in a room, and direct A/B listening
named the missing tail as the single largest audible difference.

### The recording chain, derived rather than drawn (tested)

Four panel controls describe a physical situation — **Room Size**, **Wall
Hardness**, **Mic Distance**, **Mic Pattern** — and the ambience falls out:

* Sabine per band: RT60(f) = 0.161·V/(S·α(f) + 4·m_air·V). Hardness is one
  axis of α(f): soft surfaces (seats, curtains — an auditorium) eat the top
  first; hard ones (concrete, sheet metal — a galpón) keep the highs ringing
  and let the lows boom (a low shelf inside the FDN loop carries RT_low ≠
  RT_mid). The air takes the top of big rooms no matter what the walls do.
* The mean free path 4V/S sets the six delay lines, so a big room is
  sparser and slower, not just longer.
* Early reflections are first-order mirror images of the piano in the six
  surfaces: delays and levels are geometry (piano a third down the hall,
  pair at `distance`), three images read left, three right.
* The pattern control is the physical omni→figure-8 axis, p(θ)=(1−b)+b·cosθ.
  Its random-energy efficiency (1−b)²+b²/3 sets how much room the pair
  hears (a cardioid takes ≈4.8 dB less reverb than an omni), and the
  gradient term's 1/r rise IS the proximity effect — none on an omni, most
  on a ribbon up close (rendered as a 120 Hz shelf whose gain follows b/r).
* Distance divides the direct sound by r_ref/r while the reverberant field
  stays put; the calibration reference (gain 1) sits at 2.5 m.
* **The pair is spaced in time, not just level.** Width sets the spacing
  (0 = coincident, up to 0.9 m AB), and each string's mono output reaches
  each microphone through its own arrival tap: Δt = S·x/(c·√(x²+D²)) from
  the note's lateral place on the bridge and the pair's distance. The bass
  reaches the left microphone first, the treble the right, and a coincident
  pair hears no time differences at all — which is what coincident means.
  One write and two reads per voice per sample; the delay is geometry fixed
  at note-on, so nothing interpolates and nothing zippers.

Rendered check — the same chord in four described spaces: the galpón comes
out distant and bright (hard walls hold the top), the auditorium darker and
fuller, the close ribbon loud and warm with proximity lows. Presets now
carry a room and a pair, not just string settings. One test moved with the
physics: the damper test now waits 0.2 s after release, because the hall is
allowed its own tail.

### Sympathetic resonance (tested)

With the dampers up, every other string's coinciding partials pick a struck
note's energy up through the bridge and ring on slowly — the pedal's halo.
The model renders it as a shadow voice per pedalled strike: the same partial
ladder ~24 dB down, each component detuned by its own few cents (many
strings, none exactly aligned), swelling in over ~30 ms, decaying in a single
slow stage, and released when the pedal lifts like any sustained string. The
simplification is stated: the halo shadows new strikes only — energy does not
yet flow between already-sounding strings, and catching the pedal after a
note adds nothing.

### Duplex scale (tested)

The short string segments between bridge and hitch pin are tuned high
(Conklin's duplex scaling), excited only through the bridge — and undamped.
The model gives every treble-half note two faint components near 2× and 4×
its fundamental (each with a fixed per-note mistuning) that ignore the
damper: a staccato treble note leaves their ping hanging where a bass note
goes silent.

### Una corda (tested)

CC 67 shifts the hammer: it meets the strings with the felt's unworn side
and strikes one string fewer. The model plays the strike softer and darker
(velocity-to-felt path) and raises the aftersound — the unstruck third
string is all aftersound.

### Three strings, tension glide, and the re-struck string (tested)

Where the scale has three unison strings (from the tenor up), every partial
carries a third sustained component at its own detune: the triple beat of a
real unison never settles into a clean exponential, and the churn is most of
what keeps a long sustain alive. Under una corda the shifted hammer misses
the third string almost entirely.

A hard blow also stretches the string: the note starts several cents sharp
and settles over ~250 ms (tension modulation; Askenfelt & Jansson report the
effect on heavy bass strings). The glide runs at control rate by nudging
each component's rotation — the audio loop never notices.

Re-striking a sounding note no longer damps it like a released key: the old
vibration eases out over ~250 ms underneath the new strike.

Sustained partials above ~2 kHz now die in fractions of a second (an extra
loss term in the decay curve): bright *sustain* is a guitar's signature, a
piano's treble energy lives in its attack.

### The open register (tested by measurement)

From roughly F6 to the top, a grand's strings have no dampers: some twenty
strings and their partial ladders ring sympathetically with everything
played. Measured on the YDP, C4's 3-8 kHz band decays only ~3 dB between
80 ms and 600 ms — far beyond what the struck string sustains; that silvery
halo is the open register. The model renders it twice: fourteen discrete
two-pole string resonances (1.5-6.6 kHz, T60 1-2.4 s) for the pinged
identities, and — because twenty ladders behave collectively like a dense
undamped reverberator — a four-line high-passed feedback network
(input above ~1.8 kHz, T60 ≈ 2.2 s, no damping in the loop) for the
statistics. Both are fed from the bridge sum and calibrated against the
YDP's windowed high-frequency decay.

### The action's noise (tested by measurement)

A piano strikes wood and felt as well as strings, and in the top octaves the
string is too small to mask any of it. Measured on the reference's own
samples: A6's mechanism noise sits only ~10 dB under its fundamental in the
recording, and this model had it **25–31 dB short** between 300 Hz and
3 kHz — the missing source the fit had been pointing at for weeks with
`chiff` pinned to its ×4 ceiling at the top calibration anchors.

Three mechanisms, each calibrated against the samples per register:

* **The keybed thud** (existing burst + low components, levels raised to the
  measured ones). Its old register taper — −16 dB by the top — was
  compensating a treble tone that measured too weak; with the tone healthy,
  the taper inverted the truth. The key and keybed are the same size up
  there; only the string got small.
* **The clack**: the let-off and the hammer shank are wood, and knocked wood
  rings briefly at its own modes. Three damped components in the 0.7–3 kHz
  body, T60s of tens of ms, frequencies climbing ~30 % across the compass
  (shorter shanks knock higher), jittered per note.
* **Release and pedal**: a key released clunks even over a silent string
  (floor added to the release thud), and the pedal is a mechanism too — a
  one-shot dark burst on each CC64 edge, heavier on release, when the whole
  damper rail lands back on the strings.

After calibration the treble attack bands sit within ~3 dB of the samples
(F#7: −15.3/−6.9/−8.8 dB against the real −12.9/−10.0/−6.4), and the fit
cost's own noise component fell from 8.24 to 6.54 dB — the metric had been
pricing exactly this absence.

### Dampers, half-pedalling and sostenuto (tested)

CC64 is read as the continuous control it is: a dead zone at the bottom of
the travel, fully lifted by ~three quarters, and the span between is the
half pedal. A damper's full fall multiplies each component's per-sample
decay factor once; partial pressure raises that factor to the pressed
fraction — decay continuously between free and stopped, which is what
half-pedalling IS. The felt's light touch weighs the upper partials more
(the harmonic number rides in each partial's slope weight), so a half
pedal cleans the overtones and lets the fundamental sing, exactly the
gesture pianists use it for. A moving pedal presses or relieves only the
DIFFERENCE per voice; relieving stops the energy loss but never gives
energy back. CC66 is the sostenuto rod: it captures exactly the dampers
that are up at the moment it is pressed and keeps those clear until
released, indifferent to CC64. Tests hold both: a half-pedalled release
must sit between free and seated half a second on, and sostenuto must
separate the captured note from a later one by 30x.


Releasing a key drops a damper on the string: the model multiplies each
partial's per-sample decay so the note dies in tens of milliseconds. The felt
landing on a still-moving string also thuds — a short, dark noise burst
scaled by the energy left in the voice; this is the release noise sampled
libraries ship as separate recordings because playback without it sounds
synthetic. With the sustain pedal down (MIDI CC 64), release does nothing and
the natural decay continues; lifting the pedal damps every released note.
CC 120/123 damp everything at once.

## How it is rendered

Modal synthesis: each partial is a damped quadrature oscillator — a 2×2
rotation whose matrix is pre-scaled by the per-sample decay factor, so one
partial component costs four multiplies and two adds per sample and needs no
envelope, no renormalisation and no transcendental calls in the audio loop.
Components that have decayed below audibility are retired at block boundaries,
which is what keeps a big pedalled chord affordable: the prompt components do
most of the arithmetic and die first by design.

All tuning, spectrum and decay computation happens at note-on, at control
rate, using small float implementations of `sin`, `exp`, `ln`, `sqrt` and
`pow` (the component is `no_std`; the accuracy of each is stated beside its
code and is far beyond audibility at control rate).

## Known defects

* **RETRACTED: "F#1 runs ~0.55x of the real duration".** Measured across the
  whole bass neighbourhood with the same early-window method: the REAL
  instrument's notes swing from 9.2 s (D#2) to 20.7 s (F#1) — adjacent
  semitones 2.25x apart, dispersion +/-24% — while this model ran a uniform
  +/-7%. Our F#1 at ~10.5 s sits exactly in our family; the reference's F#1
  is simply its liveliest note. The defect was never "one short note": it
  was **the model being too even**. Third retraction, same family of lesson:
  before chasing an outlier, measure which side of the comparison the
  outlier is on.

  What shipped instead: per-note unevenness, partly emergent. Each note now
  draws a fixed **unison tuning precision** (x0.5–1.5 on the detune — a just
  unison stays coherent and drains, a wide one dephases, traps its energy
  and sings; note 39's draw measurably rings 1.7x its neighbours through
  nothing but Weinreich) and a modest **string-loss spread** (+/-12%). The
  envelope-level dispersion reaches ~0.10 against the reference's 0.24 —
  the residual gap is recorded, and the seeds are a documented lever if a
  particular draw offends the ear.

* **RETRACTED: "the bass carries ~11 dB of surplus in 4–8 kHz".** Measured
  note-against-note on 0.75.0, C2's sustained 4–8 kHz sits at −22.4 dB
  against the real note's −21.0 — matched. Two things buried the old
  finding: several later pieces each legitimately removed high content (the
  coincidence corner moving to 1200 Hz, the scripted phantoms retiring, the
  slow stage gaining its incoherent radiation), and the original measurement
  was itself flawed — it compared OUR SIX-NOTE CHORD against the REAL
  SINGLE C2, so the chord's upper notes filled the band being blamed on the
  bass. Same lesson as the earlier retraction, one door over: a comparison
  is only a comparison when both sides are the same experiment.

  ~~What the honest measurement showed instead~~ — CLOSED on 0.77.0: the
  bass attack's 2–4 kHz deficit was the longitudinal voice's upper modes.
  Measured, the real burst is BROADBAND (0.0 dB of partial structure in the
  attack band — not ladder, not a fader), and the real growl keeps its
  2–4 kHz body through the sustain. Two mechanisms closed it: the impact's
  own kick seeded into the compressional modes at note-on (the hammer
  stretches the string under its head — the tension pulse rings the bank
  once, hard; scales v², wound strings take it hardest, the Clang fader
  owns it), and a per-mode drive profile that lifts the second and third
  longitudinal modes (2.3/3.4 kHz on C2) while keeping the fourth down,
  register-gated because the phenomenon belongs to the wound strings.
  After: attack 2–4 kHz +3.4 dB vs the real +4.3; sustained −4.5 vs −4.7;
  4–8 kHz −20.4 vs −21.0. C4 and A5 untouched. The fit cost's noise term
  dislikes the burst; the direct band comparison against the samples is
  the ground truth, and the fit's blindness there is already on record.

  AMENDED on 0.78.0 — the user's ear caught what the band metric missed:
  "un soplete que suena como en paralelo luego de explotar." The per-mode
  drive profile was calibrated in the attack window but BAKED into the
  resonator gains, so the ×16 surplus fed the upper modes for the life of
  the note; measured against the low bands (200 Hz–2 kHz), C2's sustained
  2–4 kHz ran at +3.4 dB — a stationary hiss as loud as the tone under it.
  The physics was pointing at the fix: the surplus feed exists because the
  freshly struck wire is full of high transverse partials, and it dies as
  they do. The surplus now rides a per-voice envelope (τ 80 ms, re-armed on
  re-strike) and the sustain keeps only the base y² feed. After: sustained
  2–4 kHz +0.8 dB rel. low (from +3.4), 4–8 kHz −23.8 (from −19.0), attack
  2–4 kHz gives back 1.2 dB — the seeded kick, which is state not gain,
  carries the burst unchanged. Metric lesson, same family as the chord
  trap: a band level fitted in one time window and then applied outside it
  is not the measurement it came from.

  AMENDED AGAIN on 0.79.0 — the seeded kick itself was wrong in SHAPE:
  writing it into the resonator's state (`y1 = kick`) is an instantaneous
  jump in the output, which is a broadband click by construction, and the
  user heard the click. The real tension pulse lasts as long as the hammer
  touches the string, so the kick is now a drive pulse with the contact's
  timescale (τ 1.5 ms) added after the surplus scaling — same energy into
  the modes' bands, nothing at the discontinuity frequencies. It also got
  its own fader (Impact Burst, index 30, off/×1/×16 like the noises,
  default 0.4 — below the calibrated centre, by the user's ear), decoupled
  from Clang, which keeps the sustained longitudinal voice. Measured at
  the top of its travel the fader adds +10 dB to C2's attack 1–2 kHz (the
  first longitudinal mode's band) and +0.8 dB at 2–4 kHz.

  On 0.80.0 the burst's dynamic gate steepened from v² to v⁴ — the user's
  verdict on v²: "está bueno pero no siempre se debe escuchar fuerte eso."
  Physically the gate SHOULD be steeper than v²: the compressional
  excitation goes as the square of the transverse amplitude, and the
  amplitude grows faster than the blow because the felt stiffens into it.
  v⁴ keeps pianissimo clean and saves the bark for the hard strike; the
  fader picks how much, the touch picks when. The house voicing ships
  with the fader off (0.02) by the user's ear.

  On 0.81.0 the travel itself was recalibrated. The first base put the
  reference-matched ff level at the fader's centre, and the level the
  user's ear actually wanted landed at the bottom of the travel with no
  room under it ("esos 0.02 deberían ser como un 0.5 para poder bajar
  más"). The base dropped 28 → 3.5 (−18 dB): the subtle level now sits at
  mid-fader, the bottom half is fine adjustment below it, and the top
  reaches +3.4 dB in C2's 1–2 kHz attack instead of +10. The house
  voicing ships at 0.5. General fader lesson worth keeping: the centre of
  a travel should be the level someone WANTS, not the level a reference
  measured — the reference is one point on the travel, not its anchor.

* **FIXED (v0.82.0): the soft bass blow still sounded like a hammer blow.**
  The user: "toco apenas y suena el martillazo" — a pianissimo bass note
  kept a percussive, bright attack. Isolated by renders: NOT the action
  noise (velocity 45 with the knock on and off measured identical); the
  tone itself. Measured on C2, attack brightness (500–3000 Hz rel.
  60–250) moved only 7.7 dB from vel 30 to vel 110 — the felt at 1e13 was
  so stiff that even a soft blow bottomed out against the string-as-spring
  floor π√(m/k_s), making contact time velocity-INDEPENDENT in the bass.
  The bass felt came down a decade (K_A0 1e13→1e12, decades 3.2→4.2 so the
  treble end is untouched): the soft blow darkens 3.5 dB and loses 2.6 dB
  of peak-to-sustain, the fortissimo keeps its character within 3.4 dB,
  and 1e12 is the more physical bass-hammer value in Chaigne & Askenfelt's
  tables anyway. Ruled out by the same sweep: three decades (1e10) guts
  the fortissimo (−11 dB); raising Stulov's τ toward the bass contact time
  (0.8–2 ms) re-brightens the pianissimo instead of separating the
  dynamics, because less of the felt relaxes under the slow blow too. The
  emergent contact-time tests still pass untouched.



* **FIXED (v0.62.0): a fixed low drone under every note, audible only in
  chords.** The user played the packaged build and heard "esa discrepancia de
  octavas" — a bass note under the music that follows nothing. No render in
  this repository could have shown it, because every one of them is a single
  note and one note buries it.

  Measured, a tone at a **fixed 46–50 Hz** sat under every note in the
  compass, at −75.8 dB under G2 and rising to −68.9 dB under C4 — stronger
  for higher notes, because a sharper strike is broader in spectrum. Six
  voices of a chord each contribute their own copy and it sums.

  Two sources, both the same omission: the model's measured radiation law
  (the board radiates ~nothing below its first mode — −25 dB at 46 Hz, ~0 dB
  by 78 Hz) was being applied to the string partials only.

  1. The board bank's own lowest mode sits at 50 Hz and went out at full
     drive. It now passes through the same corner.
  2. The lid and the chamber were fed the full signal. The chamber is a
     six-line feedback network with a 1.4 s decay and only a 4.2 kHz lowpass
     in the loop, so **nothing damped its low modes at all** and it rang at
     one of them. The air is driven by what the board radiates, so its feed
     is now high-passed at the same corner.

  Chord content below the lowest fundamental, before and after:

    | band | before | after |
    |---|---|---|
    | 33–50 Hz (an octave below) | −36.5 dB | **−47.2** |
    | 20–33 Hz (two octaves) | −44.8 | −50.7 |
    | 5–20 Hz | −49.5 | −56.6 |

  Ruled out along the way, with measurements: the sample rate (48 kHz and
  44.1 kHz are the same to within a decibel); the output saturator (its peak
  input is 0.29, far below the knee, and rewriting it changed nothing); and
  any cross-voice interaction at all — six single notes summed digitally give
  the identical spectrum to a real six-note chord.

  **The gap this exposes is in the test suite, not the model.** Everything is
  measured one note at a time. `CG_CHORD` now renders a chord, and it should
  be part of how a change is judged.


* **RETRACTED: "C2's partials 8 to 11 die 10-17 dB too fast".** They do the
  opposite. Measured without the flawed normalisation, every one of C2's
  partials from the sixth to the twelfth rings LONGER than the instrument's,
  by factors of 1.2 to 5:

  | n | Hz | instrument | model |
  |---|---|---|---|
  | 6 | 393 | 14.7 s | 24.6 (1.67x) |
  | 8 | 526 | 8.8 | 16.3 (1.86x) |
  | 9 | 592 | 6.3 | 26.1 (4.17x) |
  | 10 | 659 | 10.3 | 21.7 (2.10x) |
  | 11 | 726 | 10.3 | 19.6 (1.91x) |
  | 12 | 793 | 11.1 | 21.0 (1.89x) |

  The old finding came from a measurement that took each partial's band energy
  in two time windows and normalised each window by ITS OWN LOUDEST PARTIAL.
  The loudest is the second in the instrument and the third in the model, so
  the two signals were anchored to different partials and every reading
  carried the difference between them. The measure is differential in a way
  that was never checked; when it is replaced by each partial's own T60, read
  off its own envelope and normalised against nothing, the sign of the whole
  result flips.

  Days went into the retracted version. It sent three physically sound changes
  the wrong way -- a passive bridge, the real string count, and per-sample
  coupling -- none of which moved it, which is what finally made the metric
  itself the suspect. The lesson is narrow and worth keeping: **a measurement
  that normalises two signals by different references is not a comparison.**

  The corrected reading also agrees with the user's ear, which had been saying
  so for weeks in different words -- "mucha cuerda", "se escucha muy clavinet",
  "le falta esa oscuridad que tiene el piano en bajas". A bass note whose
  400-800 Hz partials ring twice as long as they should is a string, not a
  piano.

* **The whole-note duration and the per-partial decay disagree, and the
  balance between them is a choice.** With the decay curve refitted, the two
  measures cannot both be satisfied: shortening the second stage until every
  partial's own T60 matches (`tail` floor 1.8, depth 1.8) takes the whole-note
  duration to 0.82x the instrument, and lengthening it until the durations
  match (depth 3.5) leaves the partials 1.1 to 1.5x long. The model ships at
  depth 2.6, between them -- durations 0.91x, partial bands 0.98 to 1.39x.
  A0, C2 and C3 sit within 4% of the instrument; F#1 and A4 are the ones the
  compromise costs, at 0.62x and 0.51x.

  plucked.** The user's description was precise -- "la nuestra parece una
  guitarra tocada con los dedos" -- and it has an exact physical counterpart:
  a finger releases a string from a static displacement, a hammer hands it
  velocity. The model stores both, so the balance can be read: measured, the
  strike leaves 60-84% of its energy in DISPLACEMENT.

  | note | position | velocity |
  |---|---|---|
  | A0 ff | 70% | 30% |
  | C2 ff | 60% | 40% |
  | C3 pp | 84% | 16% |

  That looked conclusive. It was not. Forcing a pure-velocity start -- the
  output leaving zero at maximum speed, which is exactly a hammer's condition
  -- was rendered against the current one and the user reported no audible
  difference, though the fit score preferred it (31.2 -> 29.1). The physical
  phases are kept for being physical, and the hypothesis is closed.

  Worth recording because the reasoning was sound and the measurement
  confirmed the premise; only the conclusion failed. The contact duration was
  also ruled out along the way: shortening it from 1.5x to 0.35x moves A0 only
  from 70/30 to 63/37, so the balance is not set by how long the hammer
  pushes.


* **C2's body is missing because its strongest partial lands in a synthetic
  notch.** Found by putting the user in front of three level-matched renders
  of the same note -- the YDP sample, a reference renderer, and this model --
  and asking where it stops sounding like a piano. A0 passed. C2 did not:
  "mucha cuerda, le falta esa oscuridad". Partial by partial, in the body of
  the note, in dB below each take's own strongest:

  | n | Hz | real | model (v0.47) | model (v0.48) |
  |---|---|---|---|---|
  | 2 | 131 | **0.0** | -9.5 | -5.9 |
  | 3 | 196 | -7.6 | 0.0 | 0.0 |
  | 9 | 590 | -11.9 | **-50.8** | -43.3 |
  | 10 | 656 | -11.5 | -28.8 | -26.1 |

  The second partial is the STRONGEST in both references and was ninth-loudest
  here. That is the missing darkness, and it is not a tuning of levels: the
  partial was sitting in a notch of `board_response`, a curve of three
  synthetic sines whose dips land wherever they happen to land.

  **Two artefacts repaired in v0.48.0.** The scatter now fades out below
  ~320 Hz, because a soundboard is not ragged down there -- raggedness comes
  from high modal density and heavy modal overlap, and at 130 Hz a board has
  a handful of modes and a smooth response. Applying +/-6 dB of synthetic
  scatter that low is a lottery, and C2 lost it. And `COMB_FLOOR` went 0.12 ->
  0.26, so the strike-point comb caps at about 12 dB, which is where the real
  instrument has C2's ninth partial (the ideal comb's zero for this note falls
  at n = 8.86, right on it).

  The fit score went 19.66 -> 19.31, the best measured on this model, with the
  shape term 6.06 -> 5.83 and the centroid 3.93 -> 3.72 semitones.

  **Still open, and it is the next thing to find:** partial 9 remains 31 dB
  below the instrument even though the comb now caps at 12 dB and the felt,
  the contact window and the audibility cull all account for less than a dB
  at 590 Hz. A third mechanism is burying it and has not been identified.
  Partial 2 is also still 5.9 dB short of being the note's strongest.

  Why this note and not A0: A0 places 144 partials and a notch on any one of
  them averages away. C2 places a few dozen, and a notch on the second takes
  the body out of the note.


* **RESOLVED (v0.47.0): the instrument lived inside its own limiter, and that
  was the electric piano.** Measured at what reaches `soften`, which clamps
  hard at 1.5:

  | | before | after |
  |---|---|---|
  | single ff bass note | 1.60 | 0.29 |
  | bass octave ff | 2.53 | 0.46 |
  | five-note chord ff | 5.49 | 1.00 |
  | ten-note chord ff | 6.58 | 1.20 |

  Everything above mezzoforte was flat-topped. The output peak was **0.462 for
  a single note and 0.462 for a ten-note chord** -- identical, because both
  were pinned against the clamp.

  What that does to a piano: no dynamics above mezzoforte; attacks decapitated,
  because the attack IS the peak; chords no louder than single notes; and
  intermodulation across 144 partials filling the gaps between them. A piano
  with no dynamic contrast and a flattened attack is an electric piano, which
  is what this model has been called for forty versions.

  It also explains the complaint that no panel control seemed to do anything.
  Any parameter that raised the level just pushed further into the clamp and
  came back the same flattened shape -- so the panel really was inert, and not
  for any of the reasons investigated before it.

  Why it hid for so long: every offline measurement in this file renders ONE
  note and normalises it. A single note at v120-125 sits at 1.07x the clamp --
  barely into it, invisible in a normalised spectrum. The user plays chords,
  which were four times in. The measurements and the ear were not listening to
  the same signal.

  The fix is headroom (`HEADROOM`, sized so the loudest chord the instrument
  can be asked for lands near 1.2 and stays out of the clamp), not a gentler
  saturator: makeup gain belongs in the host, which already runs +6 dB and
  allows +12. Single notes are about 11 dB quieter as a result. That is the
  dynamic range coming back, not a loss.


* **Our partials sit in mush: 6-10 dB less relief than either reference.**
  Measured against BOTH the YDP samples and a licensed reference renderer
  driven through its documented command-line exporter
  (`tools/compare-reference-render.py` -- it renders audio and measures it,
  it does not read anything), on the peaks' height over the floor between
  2 and 4 kHz in the sustained part:

  | note | YDP | reference | ours (v0.44) |
  |---|---|---|---|
  | A0 | 30.3 dB | 26.8 | 23.3 |
  | F#1 | 29.6 | 33.4 | 24.2 |
  | C2 | 34.0 | 34.6 | 24.2 |

  Density does NOT separate us -- the reference has 27 peaks in A0's band
  where we have 36 and the instrument has 82, so it is not chasing count
  either. Relief is the measure where we are consistently, and only, the
  odd one out. Both references put sharp partials over a quiet floor; ours
  are blunted into a raised one.

  Undoing the unison collapse already moved this a long way: A0's relief was
  16.5 dB before it and is 23.3 after, which is most of the gap closed in one
  change. The remainder splits as:

  * **the room and lid, worth 2-3 dB.** With `air` at zero, relief goes to
    25.9 / 24.8 / 27.1. This is a taste call, not a defect -- the user has
    rejected both a dry instrument and an obviously reverberant one -- but
    the number says the staging is filling the gaps between partials, and
    `air` is the control that trades one against the other.
  * **the phantom partials, worth 3.3 dB, corrected in v0.45.0.** They are
    placed BETWEEN the ladder's positions, so their level decides how deep
    the gaps stay, and the gaps are what make a partial read as a pitch
    rather than as mush. Isolating each ingredient: phantoms account for
    +3.3 / +0.6 / +2.4 dB of relief, and clang, chiff and thump for exactly
    nothing. Their scale went 0.64 -> 0.21, which lifts A0's relief from
    23.3 to 26.2 against the reference's 26.8 -- and improves the fit cost
    at the same time (19.91 -> 19.61), which is not the usual trade and is
    why it was taken at face value rather than split down the middle.

  Two things that are NOT it, both ruled out by measurement: the output
  saturation (relief is identical at a quarter of the level, so the
  intermodulation a soft clip makes across 144 partials is not filling the
  gaps) and the HF floor (with it at zero relief gets *worse* --
  23.2 / 21.4 / 22.7 -- because removing weak partials moves the median it
  is measured against).

  Where it stands after v0.45.0, against both references:

  | note | YDP | reference | ours |
  |---|---|---|---|
  | A0 | 30.3 | 26.8 | 26.2 |
  | F#1 | 29.6 | 33.4 | 24.6 |
  | C2 | 34.0 | 34.6 | 25.9 |

  A0 has essentially reached the reference. F#1 and C2 are still 5-9 dB
  short, and the phantom correction bought them only 0.4 and 1.7 dB, so
  whatever blunts them is something else and is the next thing to isolate --
  the same stage-by-stage sweep with `CG_PARAMS` is the way to find it.


* **RESOLVED (v0.44.0): the unison collapsed above partial 32, and that was
  most of the missing bass.** The code read:

  ```
  // Above the low partials the beat between the strings is beyond
  // hearing, so they collapse into one oscillator instead of three.
  let (w1, w2) = if n < 32 { ... } else { (remainder, 0.0) };
  ```

  The reasoning is backwards. The detune is a constant in CENTS, so the beat
  rate GROWS with frequency. At A0's fundamental 3 cents is 0.05 Hz, a
  twenty-second beat. At its eightieth partial, up at 3 kHz, the same 3 cents
  is 5 Hz -- not beyond hearing, but roughness, and roughness across a dense
  band is what a piano bass sounds like.

  Measured on the YDP A0 between 2 and 4 kHz, at 0.67 Hz resolution: the real
  instrument shows **82 sharp peaks clustered a few Hz apart, standing 30 dB
  above the floor**; the model showed **26, one per partial, spaced at the
  ladder's full 57 Hz and standing 16 dB proud**. The collapse was deleting
  two thirds of what is audible in that band.

  And it deleted the most exactly where the complaint is. An A0 has 112
  partials above the old threshold; a C6 has none. That is why the treble has
  read as a piano for many versions while the bottom octave has not.

  It cost nothing to undo: the second oscillator was allocated either way and
  simply given zero amplitude.

* **The bass unison was seven times too narrow, and the fit cost disagrees
  about fixing it.** `detune_cents` began at 0.3 in the bass, which at the
  default unison setting is 0.43 cents, against 2.9 measured. Widening it is
  in direct tension with the fit:

  | bass detune | fit cost | audible peaks in A0's 2-4 kHz |
  |---|---|---|
  | 0.3 | 19.59 | 24 |
  | 0.9 (shipped) | 19.91 | 36 |
  | 1.5 | 20.45 | 41 |
  | the instrument | -- | 82 |

  The cost scores band levels inside windows. It cannot see whether a band's
  energy sits in 24 components or 82, and it reads beating as decay error, so
  it walks away from the instrument on this axis while claiming improvement.
  0.9 is a considered middle, not a fitted optimum. **Do not run the fitter
  over this parameter**: it will drive it back to zero and take the bass with
  it.


* **RETRACTED: there is no excess fixed formant.** An earlier entry here
  claimed the model carried nearly twice the real instrument's fixed colour
  (8.2 dB rms against 4.7) and blamed it for the "nasal" bass. The measurement
  was wrong: it averaged 26 real notes against 9 model renders, several of
  them the same pitch. With too few notes the structure that belongs to each
  note does not average away, and what survives reads as colour. Re-measured
  over the same 29 notes on both sides:

  | | real | model |
  |---|---|---|
  | whole compass | 4.46 dB rms | 4.66 |
  | bass, 21-45 | 5.95 | 6.34 |
  | treble, 60-96 | 6.23 | 5.61 |

  No meaningful difference anywhere. Isolating stages agrees: turning the room
  and lid off makes the figure worse, not better, so the staging is not
  colouring either. Whatever "nasal" is, it is not a fixed formant.

  The lesson is the measurement discipline, not the conclusion: any average
  meant to cancel note-dependent structure needs the SAME notes on both sides
  and enough of them. `render_reference` now takes `CG_CHROMATIC=1` for a
  30-note sweep and `CG_PARAMS="index=value,..."` to isolate a stage.

  The `board_response` change made under the wrong diagnosis (v0.42.0, finer
  and shallower scatter) is kept on its own merit: it took the fit score from
  20.02 to 19.05, the best measured on this model, with the shape term
  6.49 -> 5.97 and the centroid 4.24 -> 4.00 semitones.

* **The bottom octave is half as dense as it should be, in the one band that
  matters.** Counting peaks above -45 dB of the note's own maximum in the
  sustained part (0.1-0.6 s), per octave band:

  | note | 2-4 kHz real | model |
  |---|---|---|
  | A0 | 66 | 19 |
  | F#1 | 38 | 14 |
  | C2 | 28 | 13 |

  Below 1 kHz the model has MORE peaks than the real instrument (A0: +14 at
  250-500, +20 at 500-1000). So the error is not level, it is distribution:
  our bass is heavy underneath and thin on top, which is a bass guitar's
  spectrum. The real A0's densest band is 2-4 kHz -- that density IS the
  growl of a concert grand's bottom octave.

  Two separate deficits sit inside that one number, and they need different
  repairs:

  1. **Our own ladder is only half audible there.** A0's stiff-string ladder
     puts 38 transverse partials between 2 and 4 kHz (n = 61 to 98). Only 19
     clear the threshold. The felt cliff and the per-partial `rough` factor
     are burying partials the model has already placed and already pays for.
  2. **The real one has 28 peaks MORE than any harmonic ladder can supply.**
     No stiff-string series accounts for them. That is the longitudinal
     (compressional) content and the phantom partials the tension modulation
     makes -- which the model does place, and places at roughly -24 dB, where
     the transverse partials mask them completely (see the clang note below).

  This is the most specific measured account so far of why the lowest notes
  do not read as a piano, and it is measured on something the fit cost cannot
  see: the cost scores band *levels*, and a band can hold the right total
  energy with half the right number of things in it.

  **And that is exactly what happens.** Measured in the same window with each
  spectrum normalised to its own strongest band, A0's *balance* at 2-4 kHz is
  only 8.5 dB short. So the real instrument spreads nearly the same energy
  across 3.5x as many components while the model concentrates it into fewer,
  louder ones. The bass does not need more energy up there. It needs the
  energy it already has divided among three times as many things -- texture
  where we have grain. That is the difference between a growl and a handful
  of mid-high tones sticking out of a bass note, which is what the user has
  been describing all along.

  **Where the density actually came from.** Between measurements the count at
  A0's 2-4 kHz went 19 -> 25. That was NOT the undamped bank added in v0.43.0
  (with the bank at zero the count is still 25); it was the finer, shallower
  `board_response` of v0.42.0. Deep board notches were burying partials the
  model had already placed, and a shallower curve lets them clear audibility.
  The gain was credited to the wrong change at first.

  **The undamped-length bank (v0.43.0) is shipped without a measured
  benefit.** 48 scattered resonators from 1.9 to 7 kHz, driven by the bridge,
  never damped -- every other string's segments behind the bridge and in front
  of the agraffe. The mechanism is genuinely missing: the per-voice duplex
  already in the model only fires above position 0.45, so the bass, which is
  where the complaint is, had none of it at all. But no measure moved: score
  19.05 -> 19.02, density 25 -> 25, and its control reads 1.6 dB of authority.
  It costs 4% of the fuel budget at idle.

  It is shipped anyway, at a modest default and wired to the Sympathy control
  so it can be dialled or turned off, on the explicit understanding that the
  only instrument that has reliably detected these differences all along is
  the user's ear, and every metric here says nothing changed. If it does not
  earn its place by ear, remove it -- do not let it accumulate.

  Two repairs were tried and measured against this:

  * **Flooring the strike-point comb** (v0.40.0, kept). In the bass the strike
    point is almost exactly 1/8, so an ideal `sin(pi n x0)` is exactly
    periodic and deletes every eighth partial outright. A real bridge is not
    a rigid node -- it has finite admittance, which is why the instrument
    sounds at all -- so the mode shapes are not exact sines and measured
    combs are 10-20 dB dips, never nulls. Correct, and kept for that reason,
    but honestly: it did not move the density at all (19 before and after)
    and the score went 19.95 -> 19.97, inside the noise. Only five partials
    in the band are exact multiples of eight.
  * **Raising the audible content by level.** Ruled out by the balance
    measurement above before it was attempted: the energy is nearly right.

  The remaining 28 components are the actionable target, and they cannot come
  from any harmonic ladder. They are the longitudinal (compressional) modes
  and the phantom partials of the tension modulation -- placed today at about
  -24 dB, where the transverse partials mask them completely.


* **Half the lab panel had no authority.** Measured with
  `sweep_every_parameter` (ignored test), which sweeps each control end to
  end and reports how far the *energy-weighted* spectrum moves: a third of
  the controls moved it by under 0.3 dB anywhere on the compass. Two causes,
  both since addressed or recorded. The per-note weights pivoted at the
  middle of the compass, so they were inert by construction around middle C —
  fixed by anchoring at the treble. And the range topped out at x4, while the
  fit had driven the ingredients several of them scale down to near nothing
  (chiff 0.083, clang 0.059, HF floor 0.035); x4 of nearly nothing cannot
  reach an audible level, so the top now reaches x16.

  Note when reading that sweep: an unweighted per-band maximum is worthless
  here. A band holding no energy swings tens of dB under any change at all,
  which made several dead controls look powerful.

* **Clang is masked, not broken.** It is placed and it sounds, but at roughly
  -24 dB inside a band the bass string's own partials already fill, so it
  moves the weighted spectrum by ~0.0 dB. Nothing is miswired; the ingredient
  is simply too quiet to survive its own neighbourhood. Fixing it means
  placing it where the string is not, or letting it carry the attack rather
  than the sustain — not turning it up.


* **The hammer's mass cancelled itself out.** Fixed in v0.39.0. The stiffness
  was derived from the mass (`stiffness = mass * (pi/contact)^2 * ...`), and
  the integration divides the force by the mass -- so a force proportional to
  the mass left the hammer's trajectory identical, and the only surviving
  effect, the force delivered to the string, was removed by the
  renormalisation. The mass control was inert by construction and divided by
  zero at its bottom. The stiffness now comes from the felt alone, so the
  contact time `tau ~ pi*sqrt(m/K)` actually responds: a heavier hammer stays
  in contact longer and speaks darker, which is why a bass note is dark.
  At the control's centre the mass is the nominal one, so the default voice
  is unchanged -- the score is 19.95 before and after, to the digit.

* **The `max` blend is the binding constraint, and testing it needs a refit.**
  Confirmed by measurement rather than argument: the two hammer *weights*
  only scale their control by about +/-2x, and across that range the
  calibrated recipe wins the `max` comparison at every partial, so they
  measure 0.0 dB exactly. The base controls move only because their travel
  includes the degenerate end (stiffness 0 = no strike at all).

  A geometric crossfade was tried at trust 1.0 / 0.6 / 0.3 and scored
  20.81 / 20.06 / 20.07 against 19.95 -- but that is not a fair test, because
  the calibration table was fitted with the `max` in place. The honest
  experiment is crossfade AND refit together, validated on absolute band
  levels and crest factor, not on the fit cost alone.

  (An earlier note here claimed `colour[n]` was the recipe's spectrum being
  multiplied into the simulation. It is not -- it is the board response and
  radiation, which the simulated strike's output legitimately passes through
  too.)

* **The hammer can only ever brighten.** The strike simulation's spectrum is
  normalised to the calibrated recipe's peak and then blended into it with
  `if candidate > amplitudes[n]` — a maximum, not a crossfade. Two things
  follow. The renormalisation throws away the level the simulated strike
  computed, so hardness and mass change the *shape* and little else; and the
  maximum means a softer felt can only fail to add brightness, never remove
  it. Measured by sweeping the panel (`sweep_every_parameter`, ignored), the
  hammer's two per-note weights move no band by more than 1 dB anywhere on
  the compass, while controls either side of them move bands by 10-20 dB.

  This matters beyond the panel. A grand's softness *is* the hammer, and a
  tone whose top cannot be taken off by playing into the felt is the
  reachable-brightness-only tone of a struck electric — which is the timbre
  this model has been chased over for thirty versions. Repairing it means
  crossfading the simulation against the recipe by a stated felt weight and
  letting it carry its own level, then refitting: it moves every anchor, so
  it needs the YDP measurement loop and validation on absolute band levels
  and crest factor, not just the fit cost.

## What is deliberately not modelled yet

Stated so nobody mistakes silence for coverage:

* **A measured soundboard.** The bank follows measured density and damping
  laws, but its mode frequencies come from those laws rather than from any
  particular instrument's response.
* **The broadband knock.** The impacts of the action and the keys — everything
  in a piano's sound that does not come from the strings — are not modelled.
  It is most exposed in the extreme treble, where the tonal fundamental sits
  above 4 kHz and stops masking it, and that is exactly where the model
  measures 10–20 dB short of the reference between 30 and 1200 Hz. The fit
  can be watched asking for it: `chiff` sits pinned to its ceiling at the top
  three calibration anchors, because no multiplier can scale a source that
  is not there.
* **Ambience and sympathetic resonance.** The undamped top octave, the
  open-register shimmer, the lid and rim reflections and the room are all
  written and all mixed at zero, and a test holds them there. A dry
  direct-injected tone is what an electric piano is; a real grand's note
  carries an aura of the whole instrument.
* **The longitudinal mixing itself.** Phantom partials are placed at the
  frequencies the mixing produces, but the mixing is not integrated, so their
  level tracks velocity by a stated rule rather than emerging.
* ~~Re-strike interaction~~ — DONE: striking a note that still rings free
  merges the new blow into the living voice, partial by partial by harmonic
  number: phasors in phase with the strike grow, phasors against it cancel —
  the flutter of fast repetition, emergent. Only a voice already under a
  damper (its decay rates scaled) takes the legacy ease-out-and-strike-fresh
  path, because a scaled rate cannot be honestly un-scaled.
* ~~String-to-string energy flow~~ — DONE for sounding strings: every free
  string receives the previous sample's total bridge signal (everyone but
  itself), weighted by its own drain coupling — reciprocity — and scaled by
  the Sympathy control. A rotating phasor fed a broadband signal IS a driven
  resonator: coinciding partials accumulate, everything else averages out.
  The damper gates it continuously (a half-pressed damper takes a string
  halfway out of the conversation). Tested: a pianissimo C4 two seconds in
  carries measurably more energy when a fortissimo C3 rings under it with
  the pedal down. Still not modelled: strings with NO sounding voice (a
  silent key or a late pedal wakes nothing discrete — the open-register
  statistics still cover that wash).

A broad survey of these techniques and their trade-offs: B. Bank, F. Avanzini,
G. Borin, G. De Poli, F. Fontana, D. Rocchesso, "Physically informed signal
processing methods for piano sound synthesis: a research overview", *EURASIP
Journal on Applied Signal Processing* 2003.
