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

### Frequency-dependent damping (tested)

Higher partials die faster — losses from air drag, internal friction and
thermoelasticity all grow with frequency (C. Valette & C. Cuesta, *Mécanique
de la corde vibrante*, Hermès 1993). The model gives every partial its decay
time from one smooth curve fitted to the published order of magnitude — tens
of seconds for the lowest fundamentals, under a second at the top of the
compass:

    T60(f) ≈ 24 / (1 + (f/180)^1.25) + 0.6   seconds

On top of the smooth curve, each partial's decay is pulled by the board
response below — where the board takes energy readily the partial dies
faster — so neighbouring partials of one note decay at visibly different
rates, the way measured piano partials do.

Each partial reads the curve at its own frequency, so a bass note's high
partials fade like the treble notes they overlap — which is what makes the
model's bass notes darken as they ring, the way real ones do.

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

### Dampers and the sustain pedal (tested)

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
* **Re-strike interaction** — striking a sounding string adds energy to the
  existing vibration; the model instead damps the old voice quickly and starts
  a new one.
* **String-to-string energy flow.** The sympathetic halo shadows new strikes
  under the pedal; energy does not move between strings that are already
  sounding, and catching the pedal late adds nothing.

A broad survey of these techniques and their trade-offs: B. Bank, F. Avanzini,
G. Borin, G. De Poli, F. Fontana, D. Rocchesso, "Physically informed signal
processing methods for piano sound synthesis: a research overview", *EURASIP
Journal on Applied Signal Processing* 2003.
