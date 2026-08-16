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

The hammer's own impact is present as a short computed noise burst — a
low-passed transient, heavier in the bass — standing in for the thump the
hammer and soundboard contribute before the string tone establishes.

### Two-stage decay and the aftersound (tested)

A struck note decays fast at first, then settles into a long quiet tail, often
with slow beats. Weinreich showed why: the two or three strings of a unison
are never perfectly tuned, and their coupling at the bridge — together with
the two polarisations of each string — moves energy into configurations that
radiate poorly and therefore decay slowly (G. Weinreich, "Coupled piano
strings", *JASA* 62, 1977).

The model renders the outcome rather than integrating the coupled equations:
every partial is the sum of a *prompt* component (full level, fast decay) and
an *aftersound* component (about −10 dB, slow decay, detuned by a fraction of
a cent to a cent, wider toward the treble). The superposition produces both
the two-stage envelope and the unison beats. This is the model's largest
simplification and is stated as such: the coupling itself is not simulated.

### Frequency-dependent damping (tested)

Higher partials die faster — losses from air drag, internal friction and
thermoelasticity all grow with frequency (C. Valette & C. Cuesta, *Mécanique
de la corde vibrante*, Hermès 1993). The model gives every partial its decay
time from one smooth curve fitted to the published order of magnitude — tens
of seconds for the lowest fundamentals, under a second at the top of the
compass:

    T60(f) ≈ 22 / (1 + (f/220)^1.4) + 0.3   seconds

Each partial reads the curve at its own frequency, so a bass note's high
partials fade like the treble notes they overlap — which is what makes the
model's bass notes darken as they ring, the way real ones do.

### Dampers and the sustain pedal (tested)

Releasing a key drops a damper on the string: the model multiplies each
partial's per-sample decay so the note dies in tens of milliseconds. With the
sustain pedal down (MIDI CC 64), release does nothing and the natural decay
continues; lifting the pedal damps every released note. CC 120/123 damp
everything at once.

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

* **Sympathetic resonance** — undamped strings excited through the bridge when
  the pedal is down. The pedal currently affects dampers only.
* **Phantom partials** — longitudinal string modes and their nonlinear mixing
  (H. A. Conklin, "Generation of partials due to nonlinear mixing in a
  stringed instrument", *JASA* 105, 1999). Audible in hard bass playing.
* **A measured soundboard.** The soundboard's radiation is approximated by the
  hammer-noise voicing and the register-dependent spectral shaping, not by a
  modal or measured body response. Commuted synthesis (J. O. Smith &
  S. A. Van Duyne, "Commuted piano synthesis", ICMC 1995) would fold a body
  response into the excitation — at the cost of shipping one, which is a
  sample by another name; a small synthetic modal body is the honest next
  step.
* **Re-strike interaction** — striking a sounding string adds energy to the
  existing vibration; the model instead damps the old voice quickly and starts
  a new one.

A broad survey of these techniques and their trade-offs: B. Bank, F. Avanzini,
G. Borin, G. De Poli, F. Fontana, D. Rocchesso, "Physically informed signal
processing methods for piano sound synthesis: a research overview", *EURASIP
Journal on Applied Signal Processing* 2003.
