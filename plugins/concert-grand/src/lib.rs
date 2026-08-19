#![cfg_attr(target_arch = "wasm32", no_std)]

//! The RackForge Concert Grand: a physically modelled piano with no samples.
//!
//! Every mechanism here is documented in `docs/PIANO_MODEL.md`, with the paper
//! it comes from and the tests that hold it to its claims. In brief:
//!
//! * stiff-string inharmonicity per Fletcher (1964), with stretched tuning
//!   *derived* from it the way tuners derive it — octaves beatless against the
//!   lower note's second partial (Railsback 1938 is the observed outcome);
//! * hammer strike-point comb and velocity-dependent felt brightness
//!   (Chaigne & Askenfelt 1994; Askenfelt & Jansson 1990–93);
//! * two-stage decay and unison beats rendered as prompt + detuned aftersound
//!   components per partial (the outcome of Weinreich 1977, not the coupled
//!   equations);
//! * frequency-dependent damping fitted to published decay ranges
//!   (Valette & Cuesta 1993);
//! * dampers and the sustain pedal (CC 64).
//!
//! Rendering is modal: each partial component is a damped quadrature
//! oscillator whose 2×2 rotation is pre-scaled by its decay, so the audio
//! loop is four multiplies and two adds per component with no envelopes and
//! no transcendental calls. Components below audibility retire at block
//! boundaries.

mod math;

use math::{expf, log2f, powf, sincosf, sqrtf};
use rackforge_plugin_sdk::{MidiEvent, ParameterEvent, Processor, export_processor};

/// The piano compass, A0..=C8.
const LOW_NOTE: u8 = 21;
const NOTE_COUNT: usize = 88;

/// Voices, and the number is bounded by the wasm shadow stack rather than by
/// taste. `ConcertGrand::default()` builds the whole bank on the stack, and at
/// twenty voices the model sat close enough to the limit that adding four
/// small resonators per voice pushed it over: the module then trapped with
/// "out of bounds memory access" during instantiation, and nothing in the
/// test suite noticed, because native stacks are megabytes.
///
/// Sixteen leaves real headroom. `concert_grand_instantiates.rs` guards it.
const MAX_VOICES: usize = 16;
/// Room for the full transverse ladder of the lowest notes plus their
/// nonlinear extras — A0 alone fills ~120 slots with real partials.
const MAX_PARTIALS: usize = 144;
/// Ceiling on simultaneously active partials across all voices, so a pedalled
/// fortissimo run degrades new notes' partial counts instead of the callback.
///
/// This is a fuel budget, not a taste one. The host gives a real-time call
/// 50M units of wasmtime fuel; a partial now carries four components, so
/// 2000 of them cost about 4M oscillator ticks per 512-frame block and land
/// exactly on that ceiling — which is why dense playing was tripping
/// `all fuel consumed` and taking the audio stream down with it. Half that
/// leaves the headroom the strike simulation and the boards also need.
const PARTIAL_BUDGET: usize = 900;

/// Below the soundboard's first mode the board radiates almost nothing.
/// Calibrated against the YDP Grand samples: A0's fundamental measures
/// ~-40 dB against its strongest partial, 46 Hz ~-25 dB, 78 Hz ~0 dB — a
/// steep transition this sixth-order corner reproduces.
const RADIATION_CORNER_HZ: f32 = 66.0;
/// How deep the strike-point comb can cut. A finite bridge admittance keeps a
/// real one to 10-20 dB dips, never a null. Measured on the YDP C2, whose
/// ninth partial sits right on the ideal comb's zero: the real instrument has
/// it only 12 dB down, this model had it 39 dB down and gone.
const COMB_FLOOR: f32 = 0.26;
/// Where the long second stage stops being long. Fitted to the measured C2
/// partial decays: the trapped polarisation of a high partial does not last,
/// because what traps it is weak coupling to the bridge and what it still
/// does is radiate.
const TAIL_KNEE_HZ: f32 = 420.0;
/// How long the string's own losses let a partial ring, at the bottom of the
/// curve, and where that curve turns over.
///
/// These three numbers ARE the decay-against-frequency curve, and there is
/// only one of them now. There used to be three: this one, a `4.05 * f^-0.357`
/// correction gated to the bass, and a `1/(1 + 1.6 * register)` divisor on the
/// whole note. Each was fitted on its own against a different measurement, and
/// together they double-counted -- a bass note's 500 Hz partial was divided by
/// 2.13 for its register and then by another 2.4 for its frequency, so it
/// lived a fifth of the time the curve said.
///
/// Measured, per partial, across eleven notes of the YDP and eleven partials
/// each: the model ran 1.47x long below 120 Hz and 0.51x short above 700 --
/// the bass rang on while everything above it died at twice the proper rate.
/// A single curve, refitted against all 122 of those points at once, is both
/// simpler and closer: the mean error in log-decay falls from 0.55 to 0.36.
///
/// The shape is the one the physics asks for -- Valette's sigma = b1 + b2
/// kappa^2, a floor plus a term that climbs with wave number -- and the fit
/// only moved the knee to where the instrument actually puts it. It was at
/// 180 Hz, nearly three octaves too low, which is why the curve was so steep
/// that no single note could sit on it and both corrections were needed to
/// drag the ends back.
const STRING_T60_S: f32 = 21.0;
const STRING_KNEE_HZ: f32 = 20.0;
const STRING_TILT: f32 = 0.56;
/// Below this the soundboard has too few modes to be ragged, so the synthetic
/// scatter is faded out rather than gambling on where its notches land.
const SCATTER_KNEE_HZ: f32 = 320.0;
/// How often a voice retires inaudible components, in samples.
const CULL_INTERVAL: u32 = 256;
/// Where the bridge stops taking energy from a partial, in hertz. Above it
/// the string's impedance swamps the bridge's admittance and the termination
/// is effectively rigid.
const COUPLING_TOP_HZ: f32 = 300.0;
/// How often the string's tension is recomputed from its own motion, in
/// samples. Faster than the cull, because this carries the tension's
/// oscillating part and not just its envelope: it needs to resolve twice the
/// frequency of the modes that hold the energy, which in the bass is tens to
/// a few hundred Hz. Every 32 samples reaches 690 Hz, comfortably past that,
/// and costs a thirty-second of what doing it per sample would.
const TENSION_INTERVAL: u32 = 32;
/// How many longitudinal modes each voice carries.
///
/// The string's compressional wave, which is a different wave from the one
/// that carries the pitch and travels some thirty times faster. Bank and
/// Sujbert (JASA 117, 2005) measured what it does and it is not a detail:
/// "longitudinal vibration of piano strings greatly contributes to the
/// distinctive character of low piano notes", and it is "responsible for the
/// metallic character of low notes".
///
/// This model had a stand-in for it -- two components at 17*f0, placed once
/// at note-on with a fixed level and a quarter-second ring. Three things are
/// wrong with that, and the paper is explicit about each:
///
/// * "the longitudinal motion is continuously excited by the transverse
///   vibration along the string and NOT ONLY during the hammer-string
///   contact". Ours fired once and decayed. The real one is driven for as
///   long as the string moves.
/// * its amplitude is "a nonlinear function of the amplitude of the
///   transverse one... faster than a simple quadratic".
/// * it is not one formant but "a quasi-harmonic spectrum with formantlike
///   peaks at the longitudinal modal frequencies".
///
/// The synthesis recipe the paper gives is a bank of second-order resonators
/// at those modal frequencies, driven by the tension the transverse motion
/// makes. This model already computes that tension for the Kirchhoff glide,
/// so the expensive half is paid for.
const LONGITUDINAL_RATIO: f32 = 17.5;
const LONGITUDINAL_MODES: usize = 4;
/// How many transverse partials feed the longitudinal excitation. They hold
/// nearly all the energy, and summing all 144 per sample would cost more than
/// the whole rest of the voice.
const LONGITUDINAL_SOURCES: usize = 32;
/// Where the first longitudinal mode sits, as a multiple of the note's own
/// pitch. Bank: "around 16 to 20 times higher than that of the transverse
/// vibration".
/// Wet level of the longitudinal bank.
///
/// The bank barely sounds, and the reason is not the level.
///
/// C2's first longitudinal mode sits at 1149 Hz. Its excitation is the pairs
/// y_n*y_(n+1), which put their content at (2n+1)*f0 -- so reaching 1149 Hz
/// takes n around 8.3. The partials that drive it are the EIGHTH and NINTH,
/// and those are the ones this model has at -35 and -46 dB against the
/// instrument's -26 and -12.
///
/// The longitudinal content is downstream of the transverse ladder. It cannot
/// appear while the partials that feed it are missing, which means the hole
/// in partials 6-10 and the absent metallic character are one problem and not
/// two. Raising this constant cannot substitute for the partials.
const LONGITUDINAL_MIX: f32 = 0.0;
/// How hard the string's own stretch pulls it sharp. Sized so a fortissimo
/// bass strike sharpens a few cents and settles as it decays, which is what
/// measured piano glides do.
const TENSION_GAIN: f32 = 0.052;
/// A component whose squared magnitude falls below this is inaudible even
/// summed eighty times: kill it and spend the arithmetic elsewhere.
const DEAD_MAGNITUDE_SQUARED: f32 = 3e-8;

/// Parameter indices, matching the packaged schema.
const PARAM_BRIGHTNESS: u32 = 0;
const PARAM_DYNAMICS: u32 = 1;
const PARAM_UNISON: u32 = 2;
const PARAM_DECAY: u32 = 3;
const PARAM_WIDTH: u32 = 4;
const PARAM_LEVEL: u32 = 5;
/// Lab parameters 6..=22: raw multipliers the player sweeps by ear while we
/// hunt the piano. Each 0..1 value maps to a x0.25..x4 multiplier.
///
/// 15 and 16 are the staging pair. Everything a piano radiates that is not
/// the struck string itself used to be written and then multiplied by zero:
/// the undamped top octave, the shimmer under it, the lid reflections and the
/// room. A note with none of that is a note with no aura of the instrument
/// around it, which is what a direct-injected electric keyboard sounds like.
const LAB_COUNT: usize = 17;
/// The per-voice lab controls carry a companion weight, because a global
/// multiplier is the wrong shape for an instrument: turning the knock down
/// until the treble is right takes it out of the bass too, and the bass
/// wanted it. 0.5 is flat and behaves exactly as the control did before;
/// away from it the control is scaled in the bass while the treble is left
/// where the lab slider put it.
///
/// The treble is the anchor, not the middle of the compass. Pivoting in the
/// middle made every one of these do *nothing* around middle C -- the
/// exponent is zero there by construction -- which is exactly where anyone
/// dialling by ear plays first, so the whole page read as broken. Anchoring
/// at the top also matches what it is for: changing what the bass does
/// without disturbing a treble that was already right.
///
/// Only the first 14 get one. The last three -- board, sympathy, air -- are
/// applied after the voices are summed, where there is one soundboard and one
/// room and so no note to lean towards. A tilt there would be a control that
/// cannot move anything, which is worse than a missing one: it invites the
/// ear to chase a change that is not being made.
/// A 60 dB fall is a factor of 1000 in amplitude, so decay rate = LN_1000/T60.
const LN_1000: f32 = 6.907_755;
/// Where the soundboard's bending wavelength overtakes the wavelength in air
/// and it starts radiating in earnest. Fitted to the YDP A0's measured loss
/// rates, which bend over between 1.5 and 3 kHz.
const RADIATION_COINCIDENCE: f32 = 2500.0;
/// The loss rate a fully radiating partial carries, in nepers per second.
/// Fitted to the same measurement, less what the string's own losses already
/// account for.
const RADIATION_RATE: f32 = 0.9;
/// The wire's own bending loss, per partial squared. Bensa et al.'s
/// b2*kappa^2 term, which is what makes a bass string's two-hundredth
/// partial die while its fundamental rings for half a minute.
const KAPPA_LOSS: f32 = 2.0e-5;

const LAB_TILT_COUNT: usize = 14;
const LAB_TILT_BASE: usize = 6 + LAB_COUNT;
const PARAM_COUNT: usize = 6 + LAB_COUNT + LAB_TILT_COUNT;

/// One damped quadrature pair: state (s, c) advanced by a rotation whose
/// entries are pre-scaled by the per-sample decay factor `g`, so magnitude
/// decays by `g` each sample with no separate envelope.
#[derive(Clone, Copy, Default)]
struct Component {
    s: f32,
    c: f32,
    /// `g · cos ω` and `g · sin ω`.
    rc: f32,
    rs: f32,
}

impl Component {
    fn start(amp: f32, frequency: f32, decay_per_sample: f32, sample_rate: f32) -> Self {
        if amp == 0.0 {
            return Self::default();
        }
        let omega = core::f32::consts::TAU * frequency / sample_rate;
        let (sin, cos) = sincosf(omega);
        // Starting at (0, amp) means the output — the `s` channel — rises from
        // zero like a struck string's displacement: no click at note-on.
        Self {
            s: 0.0,
            c: amp,
            rc: decay_per_sample * cos,
            rs: decay_per_sample * sin,
        }
    }

    /// Starts from an arbitrary (position, velocity/omega) state — the
    /// state a hammer simulation leaves the mode in at contact end.
    fn start_state(s0: f32, c0: f32, frequency: f32, decay_per_sample: f32, sample_rate: f32) -> Self {
        if s0 == 0.0 && c0 == 0.0 {
            return Self::default();
        }
        let omega = core::f32::consts::TAU * frequency / sample_rate;
        let (sin, cos) = sincosf(omega);
        Self { s: s0, c: c0, rc: decay_per_sample * cos, rs: decay_per_sample * sin }
    }

    #[inline(always)]
    fn tick(&mut self) -> f32 {
        // Retired components keep their slot but stop costing a rotation:
        // the bloom is gone within tens of milliseconds and the third string
        // does not exist below the tenor, so this is most of the bank.
        if self.rc == 0.0 && self.rs == 0.0 {
            return 0.0;
        }
        let s = self.s * self.rc + self.c * self.rs;
        let c = self.c * self.rc - self.s * self.rs;
        self.s = s;
        self.c = c;
        s
    }

    /// Silences one component for good, so `tick` can skip it.
    fn retire(&mut self) {
        self.s = 0.0;
        self.c = 0.0;
        self.rc = 0.0;
        self.rs = 0.0;
    }

    fn magnitude_squared(&self) -> f32 {
        self.s * self.s + self.c * self.c
    }

    /// Rescales the built-in decay, which is how a damper falls on a string:
    /// the rotation keeps its angle and loses magnitude faster from now on.
    fn damp(&mut self, factor: f32) {
        self.rc *= factor;
        self.rs *= factor;
    }
}

/// One partial: the prompt component, its detuned slower aftersound, and a
/// negative fast-decaying bloom component. prompt + bloom sums to
/// `A·(e^(−t/τ_slow) − e^(−t/τ_rise))` — the partial swells in over its rise
/// time instead of appearing fully formed, which is what separates a tone
/// that blooms from a synthesizer that switches on.
/// One partial of a coupled unison. The three components are the strings
/// themselves (or a lone string's two polarisations), each rotating at its
/// own detuned frequency with only its *intrinsic* (internal/air) loss.
/// Radiation happens through `coupling`: at control rate the bridge removes
/// a slice of the coherent sum from every string, so the in-phase
/// configuration the hammer leaves decays fast, the dephased configurations
/// that follow radiate poorly and linger, and the churn between those states
/// is dynamics, not design (Weinreich 1977 — simulated now, not imitated).
#[derive(Clone, Copy, Default)]
struct Partial {
    prompt: Component,
    aftersound: Component,
    bloom: Component,
    third: Component,
    /// Bridge radiation per control step, as the fraction of the coherent
    /// sum each string loses. Zero for components that bypass the bridge
    /// (phantoms, thump, halo).
    coupling: f32,
}

/// Deterministic 0..1 hash (Wang-style avalanche): the model's source of
/// per-partial irregularity. Same note, same partial, same instrument —
/// repeatability is part of sounding like one particular piano.
fn hash01(mut seed: u32) -> f32 {
    seed = (seed ^ 61) ^ (seed >> 16);
    seed = seed.wrapping_mul(9);
    seed ^= seed >> 4;
    seed = seed.wrapping_mul(0x27d4_eb2d);
    seed ^= seed >> 15;
    (seed >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// One soundboard mode: a driven two-pole resonator
/// `y[n] = a1·y[n-1] + a2·y[n-2] + drive·x[n]`. The resonator's peak gain is
/// `≈ 1 / ((1-r)·2·sin ω0)`, so the drive is normalised by that whole factor
/// — normalising by `1 - r` alone leaves a residual `1/sin ω0 ∝ 1/f` that
/// turns the low modes into a bass boost of tens of times.
#[derive(Clone, Copy, Default)]
struct BodyMode {
    y1: f32,
    y2: f32,
    a1: f32,
    a2: f32,
    drive: f32,
    pan_left: f32,
    pan_right: f32,
}

impl BodyMode {
    fn tune(frequency: f32, t60: f32, pan: f32, sample_rate: f32) -> Self {
        let r = expf(-6.907_755 / (t60 * sample_rate));
        let omega = core::f32::consts::TAU * frequency / sample_rate;
        let (sin, cos) = sincosf(omega);
        Self {
            y1: 0.0,
            y2: 0.0,
            a1: 2.0 * r * cos,
            a2: -r * r,
            drive: (1.0 - r) * 2.0 * sin,
            pan_left: 1.0 - pan,
            pan_right: pan,
        }
    }

    #[inline(always)]
    fn tick(&mut self, input: f32) -> f32 {
        let y = self.a1 * self.y1 + self.a2 * self.y2 + self.drive * input;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// How loud the action's broadband knock is, before the per-note calibration.
///
/// This used to taper with pitch — `0.083 - 0.065 * position`, four and a half
/// times quieter at C8 than at A0 — and the taper is backwards. The knock is
/// *more* exposed toward the top, not less: a treble note's tonal fundamental
/// sits above 4 kHz and stops masking it (Applied Acoustics, "Separation of
/// piano keyboard vibrations into tonal and broadband components").
///
/// Measured against the YDP recordings as a noise floor over the attack, the
/// model came out between 1.4x and 54x too clean, worst through the middle of
/// the keyboard — 1.5% against 37.9% at C5. The calibration had been fighting
/// the taper with `chiff` pinned to its 4.0 ceiling at the top three anchors
/// and still could not reach. Flat here, with the level set by measurement;
/// the per-note column carries the shape from there.
const KNOCK_LEVEL: f32 = 0.02;

/// The soundboard's modal loss factor.
///
/// This one number decides whether the instrument has a body at all. Ege,
/// Boutillon & Rébillat resolved the modes of a piano soundboard from 50 Hz to
/// 3 kHz and measured loss factors of 1-3% up to ~1.2 kHz, mean η ≈ 2.3% —
/// essentially the material values of spruce ("Vibroacoustics of the piano
/// soundboard: (non)linearity and modal properties in the low- and
/// mid-frequency ranges", *JSV* 332, 2013). A knock test on a finished board
/// gives a broadband T60 near 0.6 s, against ~0.3 s for the raw panel.
///
/// The bank this replaced set its decays by hand: `T60 = 14/f` for the through
/// path and 0.14 s at 62 Hz for the parallel modes. The *form* was right — a
/// constant loss factor gives exactly T60 ∝ 1/f — but those decays imply loss
/// factors of 15.7% and 25.3%. That is not wood; it is roughly the loss factor
/// of rubber, and an instrument whose board is made of rubber is a clavinet:
/// a struck string with no resonating plate behind it.
const BOARD_LOSS_FACTOR: f32 = 0.023;

/// Amplitude T60 of a board mode: `ln(10^3) / (π·f·η)`.
fn board_t60(frequency: f32) -> f32 {
    6.907_755 / (core::f32::consts::PI * frequency * BOARD_LOSS_FACTOR)
}

/// How many resonators the board bank can hold. The density law below needs
/// 136 to reach 8.5 kHz — 64 of them under 1.1 kHz, which is the 0.06 modes/Hz
/// the measurement calls for — and the rest is headroom, so the loop never
/// runs out mid-compass. It did once, at 128: the bank stopped at 5 kHz and
/// the 4-8 kHz octave came out 11 dB down.
const BOARD_MODES: usize = 152;

/// Level of the board against the string sum that drives it. There is one
/// board, so there is one gain; it is set by measurement against the YDP
/// reference, not by taste.
/// Divides everything on its way to the output saturator, so that the
/// loudest chord the instrument can play still has shape.
const HEADROOM: f32 = 0.182;
const BOARD_MIX: f32 = 1.0;

/// Where the board's modes stop. Above this a real board still radiates, but
/// weakly and without resolvable structure.
const BOARD_TOP_HZ: f32 = 8500.0;
/// The lowest board mode. A grand's first soundboard mode sits near 60-70 Hz;
/// the bank starts just below so the region around it is covered rather than
/// bounded.
const BOARD_BOTTOM_HZ: f32 = 50.0;

/// Modal spacing, in hertz, at a given frequency — the measured density law.
///
/// Below ~1.1 kHz a piano soundboard's modal density tends to a constant
/// 0.06 modes/Hz, independent of where on the board it is measured: the
/// ribbed plate behaves as a homogeneous one, and a thin plate's modal
/// density does not depend on frequency (Boutillon & Ege, "Global and local
/// synthetic descriptions of the piano soundboard"). That is 16.7 Hz between
/// modes. Above, the density falls; the measured modal overlap runs 30% at
/// 150 Hz, 70% at 550 Hz, 100% at 1 kHz and back to 60% at 3 kHz, and 60%
/// overlap at a 2.3% loss factor means spacing ≈ 0.038·f.
///
/// Both measurements are honoured directly: 16.7 Hz flat to 1.1 kHz, then a
/// power law steep enough to reach the ~115 Hz spacing that 60% overlap
/// implies at 3 kHz. Past 3 kHz the exponent is beyond the data, so the
/// spacing is capped at constant 60% overlap rather than extrapolated — an
/// unbounded power law would leave the top octave with a handful of modes.
///
/// Getting this wrong is audible. A first attempt used constant 60% overlap
/// from 440 Hz up, which put the 1 kHz spacing at 38 Hz against a 23 Hz
/// bandwidth — overlap 0.6 where the measurement says 1.0 — and the bank came
/// out with 11 dB of ripple through 500-1000 Hz where the over-damped bank it
/// replaced had 1.3 dB.
fn board_spacing(frequency: f32) -> f32 {
    const KNEE_HZ: f32 = 1100.0;
    const FLAT: f32 = 1.0 / 0.06;
    if frequency < KNEE_HZ {
        return FLAT;
    }
    let taper = FLAT * powf(frequency / KNEE_HZ, 1.92);
    let floor_overlap = 0.038 * frequency;
    if taper < floor_overlap { taper } else { floor_overlap }
}

/// The open top octave: from ~F6 to C8 a piano's strings have no dampers.
/// They ring sympathetically with everything, and they are why every note
/// of a real grand carries a long silvery high halo — measured on the YDP
/// C4, the 3-8 kHz band decays only ~3 dB between 80 ms and 600 ms, far
/// beyond anything the struck string itself sustains.
/// (Frequency Hz, T60 s, pan 0..1.)
const OPEN_STRINGS: [(f32, f32, f32); 14] = [
    (1480.0, 2.4, 0.42), (1661.0, 2.3, 0.58), (1865.0, 2.2, 0.46),
    (2093.0, 2.0, 0.55), (2349.0, 1.9, 0.40), (2637.0, 1.8, 0.60),
    (2960.0, 1.7, 0.48), (3322.0, 1.6, 0.54), (3729.0, 1.5, 0.44),
    (4186.0, 1.4, 0.56), (4699.0, 1.3, 0.50), (5274.0, 1.2, 0.46),
    (5920.0, 1.1, 0.54), (6645.0, 1.0, 0.48),
];
/// Wet level of the open-string halo.
const OPEN_MIX: f32 = 0.012;

/// The instrument's undamped lengths, as a bank: the segments BEHIND the
/// bridge and in front of the agraffe on every string other than the one
/// being played. (The played note's OWN duplex segment is separate, on the
/// voice, and covers only the treble half where builders fit them -- so the
/// bass had none of this at all, which is where the complaint is.)
/// in front of the agraffe. They carry no damper, they are excited through
/// the bridge by whatever else is sounding, and on a grand there are a
/// hundred and fifty of them.
///
/// Why it matters here rather than as a refinement. Counting audible peaks in
/// the sustained part of A0, the real instrument has 66 things between 2 and
/// 4 kHz and the model has 19 -- while the total ENERGY in that band is only
/// 8.5 dB short. The real bass spreads nearly the same energy across three
/// and a half times as many components. A handful of strong isolated partials
/// is heard as pitched tone; a dense mat is heard as body. That difference is
/// the word the user keeps reaching for.
///
/// Density is the whole point, so this bank is large and its spacing is
/// irregular. The existing fourteen open strings sit a whole tone apart and
/// were heard as "an inconsistent faint bell on attacks" -- a sparse bank of
/// resonators is a glockenspiel. Real duplex lengths are set by where the
/// duplex bar happens to cross each string, so their pitches are scattered,
/// not scalar, and that is what makes them read as texture.
const UNDAMPED_COUNT: usize = 48;
const UNDAMPED_LOW_HZ: f32 = 1900.0;
const UNDAMPED_HIGH_HZ: f32 = 7000.0;
/// Undamped, but not endless: these are short, light, well-terminated lengths.
const UNDAMPED_T60_LOW_S: f32 = 2.6;
const UNDAMPED_T60_HIGH_S: f32 = 0.9;
const UNDAMPED_MIX: f32 = 0.12;

/// The open register as a statistic: twenty undamped strings with their
/// partial ladders behave collectively like a short, dense, undamped
/// high-frequency reverberator. Four short lines, input high-passed at
/// ~1.8 kHz, T60 ≈ 2.2 s, no damping in the loop — the silvery shimmer
/// under every note of a real grand.
const HALO_DELAYS_S: [f32; 4] = [0.0071, 0.0097, 0.0127, 0.0163];
const HALO_RT60_S: f32 = 2.2;
const HALO_HP_HZ: f32 = 1800.0;
const HALO_BUFFER: usize = 2048;
const HALO_MIX: f32 = 0.05;

/// Near-field reflections off the lid and rim: a handful of sparse early
/// taps, different per side so the image widens, no tail — this is the air
/// around an open grand, not a hall. A dry direct-injected tone is precisely
/// what an electric piano is. (Delay in seconds, gain.)
const LID_TAPS_LEFT: [(f32, f32); 4] =
    [(0.0113, 0.17), (0.0191, 0.12), (0.0257, 0.09), (0.0331, 0.06)];
const LID_TAPS_RIGHT: [(f32, f32); 4] =
    [(0.0097, 0.15), (0.0179, 0.11), (0.0243, 0.08), (0.0311, 0.05)];
/// Delay line length: covers the longest tap at rates up to 74 kHz
/// (higher rates shorten the taps via the clamp in tune_lid).
const LID_BUFFER: usize = 4096;

/// The chamber: a six-line feedback delay network with a Householder
/// feedback matrix. Line lengths are mutually non-divisible so the tail is
/// dense and colourless; each feedback path carries a one-pole low-pass so
/// high frequencies die faster than lows, the way air and walls damp a real
/// room. RT60 ≈ 1.4 s at the bottom, ~a third of that at the top.
const ROOM_LINES: usize = 6;
const ROOM_DELAYS_S: [f32; ROOM_LINES] = [0.0239, 0.0293, 0.0347, 0.0419, 0.0473, 0.0551];
const ROOM_RT60_S: f32 = 1.4;
/// One-pole damping coefficient target frequency, Hz.
const ROOM_DAMP_HZ: f32 = 4200.0;
const ROOM_BUFFER: usize = 4096;
/// Wet level of the chamber against the direct sound.
const ROOM_MIX: f32 = 0.09;

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    note: u8,
    channel: u8,
    /// Held by the key, or by the sustain pedal after release.
    held: bool,
    sustained: bool,
    partials: [Partial; MAX_PARTIALS],
    partial_count: usize,
    /// Hammer/soundboard thump: a decaying low-passed noise burst whose
    /// bandwidth contracts as it fades, like a tapped soundboard's.
    noise_amp: f32,
    noise_decay: f32,
    noise_lp: f32,
    /// Slow pole of the knock's high-pass: what gets subtracted away.
    noise_body: f32,
    noise_body_coefficient: f32,
    noise_coefficient: f32,
    /// Per-sample shrink applied to the noise low-pass coefficient.
    noise_shrink: f32,
    noise_seed: u32,
    pan_left: f32,
    pan_right: f32,
    /// Duplex-scale components: the string segments behind the bridge have
    /// no dampers, so these ring on after damp() silences the partials.
    duplex: [Component; 2],
    /// Samples until the next audibility cull.
    cull_in: u32,
    /// Rough loudness, refreshed at cull time; used to steal the quietest.
    energy: f32,
    /// Tension-modulation glide: relative frequency step per cull, and how
    /// many culls of settling remain.
    glide_rate: f32,
    glide_steps: u32,
    /// Kirchhoff-Carrier tension modulation: how strongly this string's own
    /// stretch pulls its modes sharp, and the stretch it was tuned at.
    tension_gain: f32,
    tension_rest: f32,
    tension_applied: f32,
    tension_in: u32,
    /// The string's longitudinal modes, driven by its own tension.
    longitudinal: [BodyMode; LONGITUDINAL_MODES],
    longitudinal_drive: [f32; LONGITUDINAL_MODES],
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            note: 0,
            channel: 0,
            held: false,
            sustained: false,
            partials: [Partial::default(); MAX_PARTIALS],
            partial_count: 0,
            noise_amp: 0.0,
            noise_decay: 0.0,
            noise_lp: 0.0,
            noise_body: 0.0,
            noise_body_coefficient: 0.0,
            noise_coefficient: 0.0,
            noise_shrink: 1.0,
            noise_seed: 1,
            pan_left: 0.0,
            pan_right: 0.0,
            duplex: [Component::default(); 2],
            cull_in: CULL_INTERVAL,
            energy: 0.0,
            glide_rate: 0.0,
            glide_steps: 0,
            tension_gain: 0.0,
            tension_rest: 0.0,
            tension_applied: 0.0,
            tension_in: TENSION_INTERVAL,
            longitudinal: [BodyMode::default(); LONGITUDINAL_MODES],
            longitudinal_drive: [0.0; LONGITUDINAL_MODES],
        }
    }
}

impl Voice {
    /// Renders one mono sample and advances every live component.
    #[inline(always)]
    fn tick(&mut self) -> f32 {
        let mut sum = 0.0;
        // The tension the string is under RIGHT NOW, taken from the partials
        // that carry the energy.
        //
        // The longitudinal modes are excited at the sum and difference
        // frequencies of the transverse ones, which are audio rate, so the
        // control-rate stretch computed for the glide cannot drive them -- a
        // resonator at 1150 Hz needs excitation at 1150 Hz, and a figure
        // updated every 32 samples is an envelope. Only the low partials are
        // summed here: they hold nearly all the energy, and Bank and Sujbert
        // note that the excitation need not be computed where the resonator
        // bank has little gain.
        let mut source = [0.0f32; LONGITUDINAL_SOURCES];
        for (n, partial) in self.partials[..self.partial_count].iter_mut().enumerate() {
            let voice = partial.prompt.tick()
                + partial.aftersound.tick()
                + partial.bloom.tick()
                + partial.third.tick();
            if n < LONGITUDINAL_SOURCES {
                source[n] = voice;
            }
            sum += voice;

        }
        // The excitation of each longitudinal mode, as Bank derives it.
        //
        // "A longitudinal mode with mode number k is excited by such
        // transverse mode pairs m and n only, for which either the sum m + n
        // or the difference |m - n| of their mode numbers equal to k", and
        // the terms are "the products of the instantaneous amplitudes of two
        // transverse modes, y_m(t) y_n(t)".
        //
        // So mode 1 is driven by ADJACENT pairs, sum over n of y_n*y_(n+1);
        // mode 2 by y_1^2 and the pairs two apart; and so on. An earlier
        // version here fed the bank a plain sum of squares, which is only the
        // m = n terms -- it drops every cross product, which is most of the
        // excitation, and it excites nothing odd. That is why the bank made
        // no sound.
        let live = LONGITUDINAL_SOURCES.min(self.partial_count);
        for (k, slot) in self.longitudinal_drive.iter_mut().enumerate() {
            let gap = k + 1;
            let mut force = 0.0f32;
            // The difference terms: |m - n| = k.
            let mut n = 0;
            while n + gap < live {
                force += source[n] * source[n + gap];
                n += 1;
            }
            // The sum terms: m + n = k, with both mode numbers at least one.
            let mut m = 0;
            while m + 1 < gap {
                let other = gap - m - 2;
                if other < live && m < live {
                    force += source[m] * source[other];
                }
                m += 1;
            }
            *slot = force;
        }
        sum += self.duplex[0].tick() + self.duplex[1].tick();
        // The compressional wave. Held at the tension read at control rate,
        // which is a zero-order hold on the excitation -- enough, because what
        // these resonators pick out of it is their own frequency.
        for (mode, force) in self.longitudinal.iter_mut().zip(self.longitudinal_drive) {
            if force != 0.0 {
                sum += mode.tick(force * LONGITUDINAL_MIX);
            }
        }
        if self.noise_amp > 1e-7 {
            // Park–Miller-style LCG: white noise costs one multiply-add.
            self.noise_seed = self.noise_seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let white = (self.noise_seed >> 9) as f32 * (1.0 / 4_194_304.0) - 1.0;
            self.noise_lp += self.noise_coefficient * (white - self.noise_lp);
            // A light treble hammer on a short key cannot radiate the low
            // end a bass one does; without this the knock is a thud at every
            // pitch — a bag being hit rather than an action working.
            self.noise_body += self.noise_body_coefficient * (self.noise_lp - self.noise_body);
            sum += (self.noise_lp - self.noise_body) * self.noise_amp;
            self.noise_amp *= self.noise_decay;
            // The knock darkens as it dies: a tapped soundboard's noise is a
            // low-pass whose bandwidth contracts over time.
            self.noise_coefficient *= self.noise_shrink;
        }
        sum
    }

    /// Retires components that have decayed below audibility and refreshes the
    /// loudness estimate. Runs at block cadence, never per sample.
    /// One step of tension modulation, from the string's own motion.
    ///
    /// A real string is not linear once it is displaced far enough to matter:
    /// stretching it raises its tension, and the tension is what sets every
    /// mode's frequency, so at large amplitude the modes stop being
    /// independent and become one coupled system. That is the Kirchhoff
    /// tension law, T = T0 + (EA/2L)*integral of (dy/dx)^2.
    ///
    /// In modal coordinates that integral is a sum with no cross terms,
    /// `sum (n*pi/L)^2 q_n^2`, and there is a happy accident in how this
    /// model stores things: its amplitudes are already BRIDGE FORCE, which
    /// carries the factor of n, so `(n q_n)^2` is just the component's own
    /// squared value. The whole nonlinearity is one running sum.
    ///
    /// Two things fall out of it that this model has until now written by
    /// hand, as three separate scripted mechanisms:
    ///
    /// * the slow part is the pitch glide -- sharp under the hammer, settling
    ///   as the note decays -- which was a hand-drawn 28-step envelope;
    /// * the part oscillating at twice each mode's frequency modulates every
    ///   other mode, putting sidebands at f_i +/- 2f_j. Those are Conklin's
    ///   phantom partials, which this model places by hand at frequencies it
    ///   computes in advance. Here they are generated only while the string
    ///   is actually moving that far, which is what a real one does.
    fn tension_step(&mut self) {
        let mut stretch = 0.0f32;
        for partial in &self.partials[..self.partial_count] {
            let a = partial.prompt.s + partial.aftersound.s + partial.third.s;
            stretch += a * a;
        }
        // The offset the current tension asks for, as a fractional shift in
        // frequency, and then only the DIFFERENCE from what is already
        // applied. The rotation nudge is permanent -- it edits the
        // oscillator's own matrix -- so asking for the full offset every step
        // would accumulate it, and the note would sail upward for as long as
        // it kept moving. Measured, that was 46 cents per second on A0.
        let desired = self.tension_gain * (stretch - self.tension_rest);
        let rate = desired - self.tension_applied;
        self.tension_applied = desired;
        if rate == 0.0 {
            return;
        }
        for partial in &mut self.partials[..self.partial_count] {
            for component in [
                &mut partial.prompt,
                &mut partial.aftersound,
                &mut partial.bloom,
                &mut partial.third,
            ] {
                // Rotate by an extra angle proportional to the component's own
                // frequency, so the whole ladder sharpens together.
                let step = rate * component.rs;
                let rc = component.rc - component.rs * step;
                component.rs += component.rc * step;
                component.rc = rc;
            }
        }
    }

    fn cull(&mut self) -> usize {
        // Tension modulation settles here, at control rate: each step nudges
        // every component's rotation by a small angle proportional to its own
        // frequency (d ~ rate·sin w), so the whole ladder glides together.
        if self.glide_steps > 0 {
            self.glide_steps -= 1;
            let rate = self.glide_rate;
            for partial in &mut self.partials[..self.partial_count] {
                for component in [
                    &mut partial.prompt,
                    &mut partial.aftersound,
                    &mut partial.bloom,
                    &mut partial.third,
                ] {
                    let step = rate * component.rs;
                    let rc = component.rc - component.rs * step;
                    component.rs += component.rc * step;
                    component.rc = rc;
                }
            }
        }
        // The bridge: each string loses a slice of the coherent sum, in
        // proportion to how hard the three are pushing the termination
        // together. In phase they drive it and die fast; dephased they nearly
        // cancel there and live on. That is Weinreich, and the two-stage decay
        // and the churn of the sustain come from it.
        //
        // This was moved into the per-sample loop to see whether running it at
        // the cull rate was aliasing away energy. It was not: the measured
        // loss on C2's partials 8 to 11 did not move at all, while the fuel
        // went from 45% of the budget to 64%. Twenty points for nothing is not
        // a trade, so it is back here, and the frequency dependence below is
        // what actually earns its place.
        for partial in &mut self.partials[..self.partial_count] {
            let k = partial.coupling;
            if k > 0.0 {
                let sum_s = partial.prompt.s + partial.aftersound.s + partial.third.s;
                let sum_c = partial.prompt.c + partial.aftersound.c + partial.third.c;
                partial.prompt.s -= k * sum_s;
                partial.prompt.c -= k * sum_c;
                // The second component is the horizontal polarisation: it
                // drives the bridge sideways and couples an order of
                // magnitude more weakly -- it IS the long tail (Weinreich).
                partial.aftersound.s -= 0.12 * k * sum_s;
                partial.aftersound.c -= 0.12 * k * sum_c;
                partial.third.s -= k * sum_s;
                partial.third.c -= k * sum_c;
            }
        }
        let mut removed = 0;
        let mut energy = 0.0;
        let mut index = 0;
        while index < self.partial_count {
            let partial = &mut self.partials[index];
            for component in [
                &mut partial.prompt,
                &mut partial.aftersound,
                &mut partial.bloom,
                &mut partial.third,
            ] {
                if component.magnitude_squared() < DEAD_MAGNITUDE_SQUARED {
                    component.retire();
                }
            }
            let magnitude = partial.prompt.magnitude_squared()
                + partial.aftersound.magnitude_squared()
                + partial.third.magnitude_squared();
            if magnitude < DEAD_MAGNITUDE_SQUARED {
                self.partial_count -= 1;
                self.partials[index] = self.partials[self.partial_count];
                removed += 1;
            } else {
                energy += magnitude;
                index += 1;
            }
        }
        let duplex_energy =
            self.duplex[0].magnitude_squared() + self.duplex[1].magnitude_squared();
        self.energy = energy + duplex_energy;
        if self.partial_count == 0
            && self.noise_amp <= 1e-7
            && duplex_energy < DEAD_MAGNITUDE_SQUARED
        {
            self.active = false;
        }
        removed
    }

    /// Drops the damper: the partials die fast, and the felt landing on the
    /// moving string thuds — the release noise every sampled library ships,
    /// scaled by how hard the string was still vibrating.
    fn damp(&mut self, factor: f32, thud_coefficient: f32, thud_decay: f32) {
        for partial in &mut self.partials[..self.partial_count] {
            partial.prompt.damp(factor);
            partial.aftersound.damp(factor);
            partial.bloom.damp(factor);
            partial.third.damp(factor);
        }
        let thud = (0.10 * sqrtf(self.energy)).min(0.025);
        if thud > self.noise_amp {
            self.noise_amp = thud;
            self.noise_coefficient = thud_coefficient;
            self.noise_decay = thud_decay;
            self.noise_shrink = 1.0;
        }
        self.held = false;
        self.sustained = false;
    }
}

/// The performer-facing controls, all normalised 0..=1.
#[derive(Clone, Copy)]
struct Controls {
    brightness: f32,
    dynamics: f32,
    unison: f32,
    decay: f32,
    width: f32,
    level: f32,
    lab: [f32; LAB_COUNT],
    lab_tilt: [f32; LAB_TILT_COUNT],
}

impl Default for Controls {
    fn default() -> Self {
        // The "Concert" preset.
        Self {
            brightness: 0.44,
            dynamics: 0.71,
            unison: 0.5,
            decay: 0.8,
            width: 0.47,
            level: 0.68,
            lab: [0.5; LAB_COUNT],
            lab_tilt: [0.5; LAB_TILT_COUNT],
        }
    }
}

impl Controls {
    /// Lab multiplier i: 0..1 slider -> off..x16, centre = x1.
    /// A lab control as it applies to one note. `position` is 0 at A0 and 1
    /// at C8; a weight of 0.5 ignores it, and the extremes swing the control
    /// by a factor of four each way, full at A0 and fading to nothing at C8.
    fn lab_at(&self, i: usize, position: f32) -> f32 {
        let amount = self.lab(i);
        if amount == 0.0 {
            return 0.0;
        }
        // The tail of the lab has no weight, so an unweighted control asks
        // for none: reading past the array means flat, not a panic.
        let slope = (self.lab_tilt.get(i).copied().unwrap_or(0.5) - 0.5) * 2.0;
        if slope == 0.0 {
            return amount;
        }
        amount * powf(4.0, slope * (1.0 - position))
    }

    fn lab(&self, i: usize) -> f32 {
        // The bottom of the travel is off, not a quarter. Mapping the whole
        // range to x0.25..x4 meant a control could never remove what it
        // controlled — and once a base level moved, its slider's floor could
        // sit above where that level used to be, so turning it "all the way
        // down" made things louder than before.
        let value = self.lab[i];
        if value <= 0.02 {
            return 0.0;
        }
        // The top reaches x16, not x4. Several of these scale ingredients the
        // fit drove down to near nothing -- chiff 0.083, clang 0.059, the HF
        // floor 0.035 -- and x4 of nearly nothing is still nearly nothing, so
        // the control could not reach an audible level no matter where it was
        // put. Measured with `sweep_every_parameter`, a third of the panel
        // moved the audible spectrum by under 0.3 dB across its whole travel.
        // Centre is still x1, so a setting dialled in by ear still means what
        // it meant.
        powf(256.0, value - 0.5)
    }

    fn get(&self, index: u32) -> Option<f64> {
        let value = match index {
            PARAM_BRIGHTNESS => self.brightness,
            PARAM_DYNAMICS => self.dynamics,
            PARAM_UNISON => self.unison,
            PARAM_DECAY => self.decay,
            PARAM_WIDTH => self.width,
            PARAM_LEVEL => self.level,
            6..=22 => self.lab[index as usize - 6],
            23..=36 => self.lab_tilt[index as usize - LAB_TILT_BASE],
            _ => return None,
        };
        Some(value as f64)
    }

    fn set(&mut self, index: u32, value: f64) -> bool {
        if !(0.0..=1.0).contains(&value) {
            return false;
        }
        let value = value as f32;
        match index {
            PARAM_BRIGHTNESS => self.brightness = value,
            PARAM_DYNAMICS => self.dynamics = value,
            PARAM_UNISON => self.unison = value,
            PARAM_DECAY => self.decay = value,
            PARAM_WIDTH => self.width = value,
            PARAM_LEVEL => self.level = value,
            6..=22 => self.lab[index as usize - 6] = value,
            23..=36 => self.lab_tilt[index as usize - LAB_TILT_BASE] = value,
            _ => return false,
        }
        true
    }
}

/// Per-note calibration anchors (MIDI notes) and parameter count. The
/// fitting harness optimises `cal` against tools/piano-targets.json; 1.0
/// everywhere means the hand-calibrated base model.
const CAL_ANCHORS: [u8; 10] = [21, 30, 39, 48, 57, 66, 75, 84, 96, 108];
const CAL_PARAMS: usize = 9;
// felt, floor, thump, chiff, decay, clang, phantoms, level, treble life

pub struct ConcertGrand {
    controls: Controls,
    sample_rate: f32,
    /// Fundamental of each note after the derived octave stretch.
    fundamental: [f32; NOTE_COUNT],
    /// Fletcher inharmonicity coefficient per note.
    inharmonicity: [f32; NOTE_COUNT],
    voices: [Voice; MAX_VOICES],
    pedal: bool,
    /// Una corda (CC 67): the shifted hammer strikes two of the three
    /// strings, softer and darker, and the free third string feeds the
    /// aftersound.
    soft: bool,
    /// Live count of active partials, the budget the callback answers to.
    active_partials: usize,
    /// Delay line feeding the lid/rim early reflections.
    lid: [f32; LID_BUFFER],
    lid_write: usize,
    /// Tap offsets in samples and gains, per side.
    lid_left: [(usize, f32); LID_TAPS_LEFT.len()],
    lid_right: [(usize, f32); LID_TAPS_RIGHT.len()],
    /// Full hammer simulations left in this callback. The strike ODE is the
    /// most expensive thing the model does and it runs on the audio thread,
    /// so a dense chord or a fast run cannot be allowed to spend the whole
    /// buffer on it: past the budget a strike falls back to the calibrated
    /// recipe, which is what the model sounded like before the simulation
    /// existed. Losing a little attack detail beats losing the stream.
    strike_budget: u32,
    /// Per-note calibration table: [anchor][param] multipliers.
    cal: [[f32; CAL_PARAMS]; 10],
    /// The soundboard: one dense modal bank the ENTIRE string sum radiates
    /// through — no partial reaches the air unfiltered. Mode spacing and
    /// damping both follow the measured plate, so the bank is continuous
    /// where a real board is continuous and rings as long as spruce rings.
    ///
    /// It replaced a pair of banks — a sparse parallel "body" and a serial
    /// through path — which between them held 45 resonators against the
    /// 200-500 a modelled board needs, and damped them like rubber.
    board: [BodyMode; BOARD_MODES],
    /// How many slots the generator actually filled at this sample rate.
    board_count: usize,
    /// The undamped top-octave strings, always listening to the bridge.
    open_strings: [BodyMode; OPEN_STRINGS.len()],
    undamped: [BodyMode; UNDAMPED_COUNT],
    /// The open-register shimmer: short undamped HF feedback delay network.
    halo: [[f32; HALO_BUFFER]; 4],
    halo_len: [usize; 4],
    halo_gain: [f32; 4],
    halo_lp: f32,
    halo_hp_k: f32,
    halo_write: usize,
    /// The chamber's delay lines, write head, per-line feedback gain and
    /// damping state.
    room: [[f32; ROOM_BUFFER]; ROOM_LINES],
    room_len: [usize; ROOM_LINES],
    room_gain: [f32; ROOM_LINES],
    room_lp: [f32; ROOM_LINES],
    room_damp: f32,
    room_write: usize,
}

impl Default for ConcertGrand {
    fn default() -> Self {
        let mut piano = Self {
            controls: Controls::default(),
            sample_rate: 48_000.0,
            fundamental: [0.0; NOTE_COUNT],
            inharmonicity: [0.0; NOTE_COUNT],
            voices: [Voice::default(); MAX_VOICES],
            pedal: false,
            soft: false,
            active_partials: 0,
            // Per-note calibration fitted against the YDP samples: ten
            // anchors from A0 to C8, nine multipliers each (felt, HF floor,
            // thump, chiff, decay, clang, phantoms, level, treble life).
            // Level is pinned to 1.0: the fitting cost normalises every
            // window, so it is blind to absolute level and the tilt it
            // "found" there was drift, audible as a loud bass and a muffled
            // treble. The rest are physical — decay runs x2.0 in the bass and
            // x0.38 in the treble, which a single global number can only
            // average into being wrong everywhere.
            //
            // Refitted twice, once after the radiation path was corrected
            // and again after the board was rebuilt from the measured plate.
            // A calibration is only ever a fit to the model underneath it, so
            // changing the model obsoletes the table by construction. Against
            // the measured board: mean centroid error 0.80 -> 0.21 octaves,
            // the bass 1.18 -> 0.15, and the per-band decay profile — how far
            // each band falls from the attack to the sustain, against how far
            // the YDP's does — from 7.8 dB out to 3.6 dB.
            //
            // `felt` sits on its 4.0 ceiling at anchor 75 and `chiff` on its
            // ceiling at the top three anchors. A parameter against its bound
            // is the fit asking for something the model cannot produce, and
            // here it is asking for attack energy under the treble: those are
            // the notes measured 10-20 dB short of the reference in 30-1200
            // Hz, where a real piano carries the broadband knock of its
            // action. No multiplier can scale a source that is not there.
            strike_budget: 0,
            cal: [
                [0.8445, 2.3440, 0.5097, 0.3392, 1.3207, 0.9236, 1.6842, 1.0000, 0.3686],
                [0.7931, 2.7660, 0.4900, 0.3392, 1.1193, 0.4180, 0.4024, 1.0000, 0.6902],
                [0.2500, 2.4965, 0.7820, 0.3392, 1.1920, 0.5434, 0.8348, 1.0000, 0.6229],
                [0.3239, 1.9095, 1.5685, 0.2891, 0.9485, 0.4932, 0.2500, 1.0000, 0.7351],
                [2.3340, 2.4650, 1.7729, 0.3334, 0.6545, 0.3269, 1.3111, 1.0000, 1.7943],
                [3.5576, 2.3624, 2.0355, 2.6568, 0.4078, 0.4731, 0.2952, 1.0000, 0.6440],
                [3.9996, 0.8523, 3.1826, 1.0000, 0.4986, 0.7958, 0.2500, 1.0000, 0.8474],
                [0.5556, 0.8146, 4.0000, 3.9996, 0.3813, 1.0000, 1.9942, 1.0000, 0.3461],
                [0.7394, 0.6903, 1.6997, 4.0000, 0.2950, 1.1800, 0.3836, 1.0000, 0.3835],
                [1.5869, 1.0000, 1.8832, 4.0000, 0.3835, 1.0000, 1.0000, 1.0000, 0.6556],
            ],
            board: [BodyMode::default(); BOARD_MODES],
            board_count: 0,
            open_strings: [BodyMode::default(); OPEN_STRINGS.len()],
            undamped: [BodyMode::default(); UNDAMPED_COUNT],
            halo: [[0.0; HALO_BUFFER]; 4],
            halo_len: [1; 4],
            halo_gain: [0.0; 4],
            halo_lp: 0.0,
            halo_hp_k: 0.2,
            halo_write: 0,
            lid: [0.0; LID_BUFFER],
            lid_write: 0,
            lid_left: [(0, 0.0); LID_TAPS_LEFT.len()],
            lid_right: [(0, 0.0); LID_TAPS_RIGHT.len()],
            room: [[0.0; ROOM_BUFFER]; ROOM_LINES],
            room_len: [1; ROOM_LINES],
            room_gain: [0.0; ROOM_LINES],
            room_lp: [0.0; ROOM_LINES],
            room_damp: 0.5,
            room_write: 0,
        };
        piano.tune();
        piano.tune_board();
        piano.tune_open_strings();
        piano.tune_undamped();
        piano.tune_halo();
        piano.tune_lid();
        piano.tune_room();
        piano
    }
}

/// `sqrt((1 + 4B) / (1 + B))`: how sharp a stiff string's second partial is,
/// relative to twice its fundamental. The stretch derives from this.
fn octave_stretch_ratio(b: f32) -> f32 {
    sqrtf((1.0 + 4.0 * b) / (1.0 + b))
}

/// Modes carried through the hammer-contact simulation.
/// How many string modes the strike simulation integrates against.
///
/// This was 48, and the consequence was structural rather than a matter of
/// degree: A0 places 144 partials, so the strike could reach a THIRD of its
/// note and the other two thirds were the drawn recipe, unconditionally. C4
/// places 45 and was generated in full.
///
/// That is the shape of the complaint this model has carried for forty
/// versions. The treble reads as a piano and the bass as a struck electric,
/// and the line between them is exactly the line between the notes the strike
/// can reach and the notes it cannot. It costs only at note-on, bounded by
/// the per-block strike budget.
const SIM_MODES: usize = 144;
/// What is left of the old analytic recipe under the simulated strike.
///
/// Zero: the strike owns every partial it reaches. Measured at 0.12, 0.04 and
/// 0, the fit score moves by a third of a point, so the recipe was already
/// contributing nothing where the simulation runs. It still stands unchanged
/// ABOVE the simulated range -- partials past 8 kHz are outside the
/// integration and keep their analytic amplitude.
/// The hammer's mass against the string's, in the integration's units.
///
/// One, meaning unchanged. Lightening it by fifty makes the hammer separate
/// on time -- a fortissimo A0 at 1.94 ms against the 2.00 ms asked -- and it
/// was tried, because the hammer not separating is a real defect. But it
/// starves the note: measured on C2's first 30 ms, 720-1500 Hz fell 21 dB
/// below the instrument and 40-90 Hz fell 15 dB, and the user heard exactly
/// that as "se escucha como un click, le falta gordura". At one, those bands
/// land within 2 dB of the real piano.
///
/// So the hammer still does not leave the string, and that is still wrong.
/// Whatever is mis-scaled lives in the coupling between the hammer and the
/// modal masses, not in this number, and making this number pay for it costs
/// more than the defect does.
const HAMMER_MASS_SCALE: f32 = 1.0;
/// How much longer the integration runs than the nominal contact time. The
/// hammer is still in contact when it stops, so this sets how heavily it
/// pushes the low modes: measured on C2's first 30 ms, stretching it puts
/// 40-90 Hz within 2 dB of the real instrument where the nominal time leaves
/// it 12 dB short.
const CONTACT_STRETCH: f32 = 1.0;
const RECIPE_FLOOR: f32 = 0.0;
#[cfg(test)]
static CONTACT_STEPS: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);

/// Integrates the felt hammer against the string's modal system from first
/// touch to release: nonlinear felt (F ∝ ξ^2.5), the string pushing back,
/// the returning wave reshaping the pulse while contact lasts. Returns each
/// mode's (position, velocity/ω) state at contact end — amplitudes AND
/// phases of the attack, emergent instead of scripted (Chaigne & Askenfelt).
/// Normalised units: string modal masses are 1, the hammer carries `mass`.
/// Integrates the hammer against the string modes during contact.
///
/// KNOWN DEFECT, measured by `how_long_the_hammer_stays`: the hammer does not
/// separate. Contact lasts 4.80 ms for every note at every velocity, and
/// 4.80 ms is exactly this function's own window -- 1200 steps of 4 us. The
/// integration ends because it runs out of steps, not because the hammer
/// leaves.
///
/// So the contact time is not modelled at all. `contact_time` asks for 1.7 ms
/// at a fortissimo C2 and gets 4.8; it asks for 5.9 ms at a soft A0 and gets
/// 4.8 truncated. The velocity dependence -- the thing that makes a piano
/// respond to touch -- is absent from the strike entirely, and a hammer still
/// pressing while the string vibrates damps it.
///
/// It is not the stiffness. Sweeping that constant from 34 to 4000 moves a
/// fortissimo A0 only from 4.80 ms to 2.53 ms and leaves C2 at 4.26. Whatever
/// holds the hammer on is in the balance between the hammer's mass and the
/// string's -- the modal masses here are unity and there are up to 144 of
/// them, so the string is some hundreds of times heavier than the hammer,
/// which is backwards for a bass note.
fn simulate_strike(
    frequencies: &[f32],
    modes: usize,
    x0: f32,
    contact_width: f32,
    mass: f32,
    stiffness: f32,
    exponent: f32,
    velocity0: f32,
    contact_seconds: f32,
) -> ([f32; SIM_MODES], [f32; SIM_MODES]) {
    let dt = 4.0e-6_f32;
    // The hammer does not separate on its own (see HAMMER_MASS_SCALE), so the
    // loop's length is what ends contact. Make it the contact time the model
    // already computes from Askenfelt's measurements, rather than a constant.
    //
    // A fixed count gave every note and every velocity the same contact, and
    // measured that inverts the instrument's dynamics: a fortissimo blow
    // pushed the low modes for the same milliseconds as a soft one, its peak
    // ran away, and the audibility cull then took its upper partials -- 42
    // partials at ff against 115 at pp, where a piano gets richer as it gets
    // louder, not poorer.
    let steps = ((contact_seconds / dt) as usize).clamp(200, 4000);
    let mut omega = [0.0f32; SIM_MODES];
    let mut shape = [0.0f32; SIM_MODES];
    for n in 0..modes {
        omega[n] = core::f32::consts::TAU * frequencies[n];
        // The hammer has a WIDTH. It does not touch the string at a point.
        //
        // A bass hammer lies on about 12 mm of a two-metre string, and its
        // felt spreads the force further still. The modal projection of a
        // force spread over that width is the point projection times the
        // transform of the spread -- smooth, so the high modes are driven
        // progressively less, which is most of what makes a bass hammer sound
        // heavy instead of sharp.
        //
        // This filter used to live in the analytic recipe, and retiring the
        // recipe took it out of the model altogether: the integration was
        // striking the string at a single point. The user heard it
        // immediately -- "como si le pegaran con un martillito finito, le
        // falta gordura" -- which is exactly what a point hammer is.
        //
        // Both uses of `shape` want it: the force each mode receives, and the
        // string displacement the hammer feels back, which is likewise an
        // average over the contact rather than a reading at one point.
        let nf = (n + 1) as f32;
        let spread = nf * contact_width;
        // The strike-point comb needs its floor here too, for the same reason
        // it has one in the recipe: the bridge is not a rigid node, so the
        // mode shapes are not exact sines and the comb dips rather than nulls.
        //
        // Retiring the recipe took that floor out of the model, because the
        // integration carried the raw sine. Measured on C2, whose ninth
        // partial sits on the ideal comb's zero (x0 = 1/8.86): the real
        // instrument has it 11.9 dB down and this had it 65.8 -- gone. Its
        // sixth through tenth partials were 8 to 54 dB short, which is the
        // body of a wound bass string, and a note with a strong fundamental
        // and no middle harmonics is heard as a thinner string at the same
        // pitch.
        let ideal = sincosf(core::f32::consts::PI * nf * x0).0;
        let comb = if ideal < 0.0 { -1.0 } else { 1.0 }
            * sqrtf(ideal * ideal + COMB_FLOOR * COMB_FLOOR);
        shape[n] = comb * expf(-1.2 * spread * spread);
    }
    let mut q = [0.0f32; SIM_MODES];
    let mut v = [0.0f32; SIM_MODES];
    let mut hammer_y = 0.0f32;
    let mut hammer_v = velocity0;
    let mut touched = false;
    #[cfg(test)]
    let mut steps_in_contact = 0u32;
    // Stulov's hereditary felt: wool has memory. The force follows
    // F = F0·(x^p − e·h) where h is x^p passed through an exponential
    // history kernel — loading is stiffer than unloading, the loop
    // dissipates, the pulse comes out shorter and asymmetric (JASA 97,
    // 1995). e and t0 are his hereditary parameters.
    const STULOV_EPSILON: f32 = 0.94;
    const STULOV_TAU_S: f32 = 6.0e-6;
    let history_keep = expf(-dt / STULOV_TAU_S);
    let mut history = 0.0f32;
    for _ in 0..steps {
        let mut string_y = 0.0;
        for n in 0..modes {
            string_y += q[n] * shape[n];
        }
        let felt = hammer_y - string_y;
        let force = if felt > 0.0 {
            touched = true;
            #[cfg(test)]
            {
                steps_in_contact += 1;
            }
            // Chabassier et al. (M2AN 2014): the felt exponent varies from
            // ~1.5 in the bass to ~3.5 in the treble, not one fixed power.
            let compressed = powf(felt, exponent);
            history = history * history_keep + compressed * (1.0 - history_keep);
            (stiffness * (compressed - STULOV_EPSILON * history)).max(0.0)
        } else {
            if touched {
                break;
            }
            history = 0.0;
            0.0
        };
        hammer_v -= force / mass * dt;
        hammer_y += hammer_v * dt;
        for n in 0..modes {
            v[n] += (-omega[n] * omega[n] * q[n] + 2.0 * shape[n] * force) * dt;
            q[n] += v[n] * dt;
        }
    }
    #[cfg(test)]
    CONTACT_STEPS.store(steps_in_contact, core::sync::atomic::Ordering::Relaxed);
    let mut over_omega = [0.0f32; SIM_MODES];
    for n in 0..modes {
        over_omega[n] = v[n] / omega[n].max(1.0);
    }
    (q, over_omega)
}

impl ConcertGrand {
    /// Interpolated per-note calibration multiplier.
    fn cal(&self, note: u8, param: usize) -> f32 {
        let n = note.clamp(CAL_ANCHORS[0], CAL_ANCHORS[9]);
        let mut i = 0;
        while i < 8 && CAL_ANCHORS[i + 1] < n {
            i += 1;
        }
        let (a, b) = (CAL_ANCHORS[i], CAL_ANCHORS[i + 1]);
        let t = (n - a) as f32 / (b - a) as f32;
        self.cal[i][param] * (1.0 - t) + self.cal[i + 1][param] * t
    }

    /// Fits published inharmonicity measurements: a quadratic in log-space,
    /// minimum near A2, ~1e-4 in the middle of the compass to ~1e-2 at C8.
    fn inharmonicity_for(note: u8) -> f32 {
        let n = note as f32;
        let exponent = -3.95 + 4.9e-4 * (n - 45.0) * (n - 45.0);
        powf(10.0, exponent)
    }

    /// Tunes the instrument the way a tuner does: A4 = 440, octave anchors
    /// beatless against the lower note's second (sharp) partial, and the
    /// stretch interpolated in cents between anchors. Railsback's curve is
    /// the output of this procedure, not an input to it.
    fn tune(&mut self) {
        for index in 0..NOTE_COUNT {
            self.inharmonicity[index] = Self::inharmonicity_for(LOW_NOTE + index as u8);
        }

        // Stretch in cents at the octave anchors around A4 (index 48).
        let mut anchor_cents = [0.0_f32; NOTE_COUNT];
        let a4 = 69 - LOW_NOTE as usize;
        let mut cents = 0.0;
        let mut index = a4;
        while index + 12 < NOTE_COUNT {
            cents += 1200.0 * log2f(octave_stretch_ratio(self.inharmonicity[index]));
            anchor_cents[index + 12] = cents;
            index += 12;
        }
        cents = 0.0;
        index = a4;
        while index >= 12 {
            cents -= 1200.0 * log2f(octave_stretch_ratio(self.inharmonicity[index - 12]));
            anchor_cents[index - 12] = cents;
            index -= 12;
        }

        for index in 0..NOTE_COUNT {
            // Interpolate the stretch between the surrounding anchors.
            let below = a4 as isize + (index as isize - a4 as isize).div_euclid(12) * 12;
            let above = below + 12;
            let fraction = (index as isize - below) as f32 / 12.0;
            let below_cents = anchor_cents
                .get(below.max(0) as usize)
                .copied()
                .unwrap_or(0.0);
            let above_cents = anchor_cents
                .get((above as usize).min(NOTE_COUNT - 1))
                .copied()
                .unwrap_or(below_cents);
            let stretched = below_cents + (above_cents - below_cents) * fraction;
            let semitones = index as f32 - a4 as f32;
            self.fundamental[index] =
                440.0 * powf(2.0, semitones / 12.0 + stretched / 1200.0);
        }
    }

    /// Converts the lid tap times to sample offsets at the current rate.
    fn tune_lid(&mut self) {
        let convert = |taps: [(f32, f32); 4]| {
            let mut out = [(0usize, 0.0f32); 4];
            for (slot, (seconds, gain)) in out.iter_mut().zip(taps) {
                *slot = (
                    ((seconds * self.sample_rate) as usize).clamp(1, LID_BUFFER - 1),
                    gain,
                );
            }
            out
        };
        self.lid_left = convert(LID_TAPS_LEFT);
        self.lid_right = convert(LID_TAPS_RIGHT);
    }

    /// Lays out the soundboard: walk up from 50 Hz taking the measured modal
    /// spacing at each step, give every mode the loss factor of spruce, and
    /// jitter frequency, strength and pan so the bank is ragged rather than
    /// regular.
    fn tune_board(&mut self) {
        let ceiling = if BOARD_TOP_HZ < 0.45 * self.sample_rate {
            BOARD_TOP_HZ
        } else {
            0.45 * self.sample_rate
        };
        let mut frequency = BOARD_BOTTOM_HZ;
        let mut index = 0;
        while index < BOARD_MODES && frequency < ceiling {
            let seed = index as u32;
            // ±3% of the local spacing, so neighbouring modes crowd and part
            // the way a real plate's do instead of marching in step.
            let jitter = 1.0 + 0.06 * (hash01(0xB0A2D ^ seed << 3) - 0.5);
            let placed = frequency * jitter;
            let pan = 0.35 + 0.30 * hash01(0x5EA1 ^ seed << 5);
            let mut mode = BodyMode::tune(placed, board_t60(placed), pan, self.sample_rate);
            // A real plate's mobility is ragged: per-mode strength swings
            // ~±8 dB — a bank of equal modes is only a volume knob.
            mode.drive *= 0.65 + 0.8 * hash01(0xF00D ^ seed << 7);
            self.board[index] = mode;
            frequency += board_spacing(frequency);
            index += 1;
        }
        self.board_count = index;
        for slot in self.board.iter_mut().skip(index) {
            *slot = BodyMode::default();
        }
    }

    /// Retunes the undamped top-octave strings.
    /// Lays the duplex bank out across its range with scattered spacing.
    ///
    /// Geometric progression for the placement, then each pitch is pushed off
    /// it by up to a third of the gap to its neighbour. Even spacing in log
    /// frequency would beat against the partial ladder of every note in the
    /// same way and read as a chord; scattered spacing reads as a mat.
    fn tune_undamped(&mut self) {
        let span = UNDAMPED_HIGH_HZ / UNDAMPED_LOW_HZ;
        let step = powf(span, 1.0 / (UNDAMPED_COUNT - 1) as f32);
        let mut frequency = UNDAMPED_LOW_HZ;
        for (i, string) in self.undamped.iter_mut().enumerate() {
            let scatter = 1.0 + 0.33 * (hash01(i as u32 * 2_654_435_761) - 0.5) * (step - 1.0) * 2.0;
            let hz = (frequency * scatter).clamp(UNDAMPED_LOW_HZ, UNDAMPED_HIGH_HZ);
            // Shorter lengths ring less: the T60 falls across the bank.
            let t = i as f32 / (UNDAMPED_COUNT - 1) as f32;
            let t60 = UNDAMPED_T60_LOW_S + (UNDAMPED_T60_HIGH_S - UNDAMPED_T60_LOW_S) * t;
            // Alternating sides with a wobble, because they sit along the
            // bridge and are not heard from one point.
            let pan = 0.5 + 0.42 * (hash01(i as u32 * 40_503 + 7) - 0.5) * 2.0;
            *string = BodyMode::tune(hz, t60, pan.clamp(0.05, 0.95), self.sample_rate);
            frequency *= step;
        }
    }

    fn tune_open_strings(&mut self) {
        for (string, (frequency, t60, pan)) in self.open_strings.iter_mut().zip(OPEN_STRINGS) {
            *string = if frequency < 0.45 * self.sample_rate {
                let mut tuned = BodyMode::tune(frequency, t60, pan, self.sample_rate);
                // A long string's bandwidth is under a hertz: at unit peak
                // gain it would catch nothing from a transient. The bridge
                // feeds it far better than that — calibrated against the
                // YDP's sustained 3-8 kHz halo.
                tuned.drive *= 45.0;
                tuned
            } else {
                BodyMode::default()
            };
        }
    }

    /// Sizes the shimmer's delay lines for the current rate.
    fn tune_halo(&mut self) {
        for line in 0..4 {
            self.halo_len[line] =
                ((HALO_DELAYS_S[line] * self.sample_rate) as usize).clamp(1, HALO_BUFFER - 1);
            self.halo_gain[line] = powf(10.0, -3.0 * HALO_DELAYS_S[line] / HALO_RT60_S);
        }
        self.halo_hp_k = 1.0 - expf(-core::f32::consts::TAU * HALO_HP_HZ / self.sample_rate);
    }

    /// Sizes the chamber's delay lines and feedback for the current rate.
    fn tune_room(&mut self) {
        for line in 0..ROOM_LINES {
            let length =
                ((ROOM_DELAYS_S[line] * self.sample_rate) as usize).clamp(1, ROOM_BUFFER - 1);
            self.room_len[line] = length;
            // Per-line gain so every path decays at the same RT60.
            self.room_gain[line] =
                powf(10.0, -3.0 * ROOM_DELAYS_S[line] / ROOM_RT60_S);
        }
        self.room_damp = 1.0 - expf(-core::f32::consts::TAU * ROOM_DAMP_HZ / self.sample_rate);
    }

    /// T60 fitted to published decay ranges: tens of seconds for the lowest
    /// fundamentals, over a second at the top (Valette & Cuesta's losses all
    /// grow with frequency). Every partial reads this at its own frequency.
    /// `string_scale` shifts the loss curve by string weight: a 2 kHz
    /// partial on a massive wound A0 string rings for seconds, the same
    /// 2 kHz as a short treble string's fundamental dies at once. Measured
    /// on the YDP: A0's 1.2-8 kHz band decays ~11 dB/s, which a
    /// frequency-only curve misses by 30+ dB.
    fn t60_seconds(
        &self,
        frequency: f32,
        f0: f32,
        string_scale: f32,
        treble_life: f32,
    ) -> f32 {
        // Which partial this is. The string's losses go with the WAVE NUMBER,
        // not the frequency -- kappa ~ n/L -- so the same 6 kHz is partial 218
        // on A0 and partial 92 on C2, and the bass one is far more heavily
        // damped. Reading the loss off the frequency alone, as a single global
        // rate did, over-damped the tenor to get the bottom octave right: the
        // highs died the instant they were struck, which is a banjo.
        let partial_number: f32 = (frequency / f0.max(1.0)).max(1.0);
        let radiating = frequency;
        let frequency = frequency * string_scale;
        let string = STRING_T60_S / (1.0 + powf(frequency / STRING_KNEE_HZ, STRING_TILT)) + 0.6;
        // Radiation is a second loss channel, in parallel with the string's
        // own, so the two rates add.
        //
        // There used to be an empirical treble rolloff here as well -- a
        // second 1/(1+(f/10400)^1.1) on top -- put there to stop high
        // partials ringing like a guitar's. That is the same job radiation
        // now does from the mechanism, and keeping both counted the loss
        // twice: measured, it took 4-8 kHz at F#1 and C2 to 18 and 34 dB
        // BELOW the real instrument. `treble_life` survives as the control it
        // was, but it now sets how readily the board gives the highs away,
        // which is where the effect actually comes from.
        // Bensa et al. give the string's loss as sigma = b1 + b2*kappa^2, and
        // kappa is proportional to the partial number. Radiation is a second
        // channel in parallel with it, so the rates add.
        let bending = KAPPA_LOSS * partial_number * partial_number;
        let rate = LN_1000 / string
            + (RADIATION_RATE * Self::radiation_efficiency(radiating) + bending)
                / treble_life.max(0.05);
        // There is no register correction here any more, and that is the
        // point. One used to divide the whole note by up to 2.6 because the
        // bass rang too long; but the bass rang too long because the curve
        // above was too steep, and dividing the note flat also shortened the
        // upper partials that were already dying too fast. The curve carries
        // it now.
        (LN_1000 / rate) * (0.5 + 1.5 * self.controls.decay)
    }

    /// How readily the soundboard turns a partial of this frequency into
    /// sound, 0 to 1.
    ///
    /// A plate radiates efficiently only once its bending wavelength exceeds
    /// the wavelength in air. Below that coincidence the board's neighbouring
    /// regions move in antiphase and their near fields cancel: it shoves air
    /// sideways instead of compressing it, and the partial keeps its energy.
    /// A piano soundboard's coincidence sits in the low kilohertz (Ege and
    /// Boutillon put the transition to ribbed-plate behaviour near 1.1 kHz).
    ///
    /// This is one mechanism, not two, and that is the point. The same
    /// inefficiency that makes a bass fundamental quiet is what makes it
    /// last; the same efficiency that makes the upper partials loud is what
    /// kills them. It is why a real bass note *darkens* as it rings.
    ///
    /// Measured on the YDP A0 between 0.08-0.25 s and 1.6-2.2 s: the
    /// fundamental band loses nothing at all while 2-4 kHz loses 22 dB and
    /// 4-8 kHz loses 42 dB. The model, before this, *gained* 4 dB at 2-4 kHz
    /// over the same span -- a bass note growing brighter as it decayed,
    /// which is a plucked string and not a struck one.
    ///
    /// The exponent is steeper than the textbook f^2 because the measured
    /// curve is: A0's loss rate rises a factor of 6 from 750 Hz to 1500 Hz,
    /// then 3 and then 1.9 across the octaves above, which is a power law
    /// bending over into saturation rather than a clean square.
    fn radiation_efficiency(frequency: f32) -> f32 {
        let r = powf(frequency / RADIATION_COINCIDENCE, 2.6);
        r / (1.0 + r)
    }

    /// The soundboard as the filter it is. Measured bridge mobility is ragged
    /// — peaks and dips of ±10 dB and more across the whole compass (Giordano,
    /// "Simple model of a piano soundboard", *JASA* 102, 1997) — and every
    /// partial of every note samples the same fixed curve. Returned as an
    /// amplitude multiplier (~±5 dB) and a decay multiplier: where the board
    /// takes energy readily the partial speaks louder and dies faster.
    ///
    /// Three incommensurate sines in log-frequency stand in for the measured
    /// curve: fixed, smooth at the scale of one partial, uncorrelated at the
    /// scale of a semitone — synthetic, and stated as such.
    fn board_response(frequency: f32) -> (f32, f32) {
        let l = log2f(frequency.max(1.0));
        // Fine scatter, not a few broad humps. Three slow sines across the
        // audio range put a bump every octave or so, and a fixed bump an
        // octave wide is a FORMANT: every note samples the same one, so the
        // instrument speaks with one fixed colour. Measured by averaging every
        // note's spectrum on a log grid -- structure belonging to the note
        // averages away, structure fixed in frequency survives -- the model
        // carried 8.2 dB rms and 47 dB peak to peak of it against the real
        // instrument's 4.7 and 25.
        //
        // It is worst in the bass, and that is the tell: a low note spreads
        // partials across the whole range and so samples the entire pattern,
        // while a treble note touches one small piece of it and sounds fine.
        //
        // A real soundboard's mobility above its ribbed-plate transition
        // (~1.1 kHz, Ege and Boutillon) has high modal density and heavy modal
        // overlap: its raggedness is fine-grained scatter, statistically flat
        // at the scale of an octave. These rates are ten times faster and the
        // depth is halved, which is scatter rather than colour.
        let ragged = sincosf(17.3 * l + 1.3).0
            + sincosf(28.7 * l + 4.1).0
            + sincosf(43.1 * l + 2.2).0
            + sincosf(67.9 * l + 5.7).0
            + sincosf(103.9 * l + 0.4).0;
        let normalized = ragged * (1.0 / 5.0);
        // ±9 dB of level (YDP spectra swing ±15 dB between neighbours; the
        // per-note irregularity supplies the rest), and up to ~×1.5 / ÷1.4 of
        // decay rate, in opposition: a mobile board radiates more and damps
        // the string more.
        // The scatter fades out below a few hundred Hz, because a soundboard
        // is not ragged down there. Raggedness comes from high modal density
        // and heavy modal overlap; at 130 Hz a board has only a handful of
        // modes and its response is smooth. Applying the same +/-6 dB of
        // synthetic scatter that far down is not physics, it is a lottery --
        // and C2 lost it. Measured, its second partial is the STRONGEST in
        // both the real instrument and the reference, and this model had it
        // 9.5 dB down, sitting in a notch that landed there by accident.
        let settled = 1.0 / (1.0 + powf(SCATTER_KNEE_HZ / frequency.max(20.0), 2.0));
        let amplitude = powf(10.0, normalized * 0.30 * settled);
        let decay = 1.0 / (1.0 + 0.35 * normalized * settled);
        (amplitude, decay)
    }

    fn decay_per_sample(&self, t60: f32) -> f32 {
        // Amplitude e-folds T60/6.91 apart; per-sample factor follows.
        expf(-LN_1000 / (t60 * self.sample_rate))
    }

    /// Where the hammer strikes, as a fraction of string length: ~1/8 in the
    /// bass narrowing toward ~1/13 in the treble.
    fn strike_point(note: u8) -> f32 {
        let position = (note - LOW_NOTE) as f32 / (NOTE_COUNT - 1) as f32;
        // Flat at one eighth through the bass, where a real action strikes,
        // and moving toward the bridge only in the upper half.
        //
        // The old law reached 1/8.86 by C2, which puts the comb's null
        // between partials 8 and 9 and takes both. Measured, the instrument
        // notches partial EIGHT sharply -- 26.4 dB down, its weakest -- and
        // leaves the ninth at 11.9 with its neighbours. Ours had the eighth at
        // 30.8 and the ninth at 48.3: the hole in the middle of the harmonics
        // that makes the note sound like a thinner string.
        let upper = (position - 0.35).max(0.0) / 0.65;
        1.0 / (8.0 + 8.0 * upper * upper)
    }

    /// The hammer's contact width as a fraction of string length: a few
    /// percent on the long bass strings, proportionally much wider on the
    /// short treble ones. A point excitation is a pluck — the finite width
    /// is what separates a struck piano string from a classical guitar.
    fn hammer_width(note: u8) -> f32 {
        let position = (note - LOW_NOTE) as f32 / (NOTE_COUNT - 1) as f32;
        // Scaled by real string lengths: an A0 string runs ~2 m under a
        // ~12 mm contact (≈0.6%), a top treble string ~5 cm under the same
        // hammer (≈15%). The earlier 3% bass figure was five times too wide
        // and filtered the top three octaves out of the bass ladder.
        0.006 + 0.14 * position * position
    }

    /// Hammer–string contact time in seconds: longer for soft blows and low
    /// notes, under a millisecond for hard treble blows (Askenfelt & Jansson).
    fn contact_time(&self, note: u8, velocity: f32) -> f32 {
        let position = (note - LOW_NOTE) as f32 / (NOTE_COUNT - 1) as f32;
        // The base is the fortissimo contact — ~2 ms in the bass, under half
        // a millisecond at the top; soft blows stretch it via the swing.
        // With the fourth-order felt this lands the measured cliffs: C4 ff
        // ~2.5 kHz, A4 ff ~3.5 kHz.
        let base = 0.002 - 0.0016 * position;
        // `dynamics` sets how strongly velocity drives the felt; the felt's
        // hardening (force exponent ~2.5) is rendered as this contact-time
        // swing rather than simulated.
        let swing = 1.0 + 1.2 * self.controls.dynamics;
        base * (1.0 + swing - swing * 2.0 * (velocity - 0.5))
    }

    fn start_voice(&mut self, channel: u8, note: u8, velocity: u8) {
        let index = (note.clamp(LOW_NOTE, LOW_NOTE + NOTE_COUNT as u8 - 1) - LOW_NOTE) as usize;
        let mut velocity = velocity as f32 / 127.0;
        // Una corda: the shifted hammer meets the strings with softer felt
        // (the unworn side) and strikes one string fewer.
        if self.soft {
            velocity *= 0.78;
        }

        // A re-struck string keeps ringing: the old voice is eased out over
        // ~250 ms underneath the new strike instead of being damped like a
        // released key.
        let restrike = expf(-1.0 / (0.25 * self.sample_rate));
        let (thud_coefficient, thud_decay) = self.damper_thud();
        for voice in &mut self.voices {
            if voice.active && voice.note == note && voice.channel == channel {
                voice.damp(restrike, thud_coefficient, thud_decay);
            }
        }

        let f0 = self.fundamental[index];
        let b = self.inharmonicity[index];
        let x0 = Self::strike_point(note);
        let width = Self::hammer_width(note);
        let nyquist = 0.47 * self.sample_rate;
        // A piano's ladder is spent long before nyquist: past ~11 kHz the
        // felt cliff has every partial on the noise floor, inaudible but
        // still billing four oscillators a sample. Carrying it that far was
        // most of why one note ate a quarter of the audio call's fuel.
        let audible_top = nyquist.min(11_000.0);

        // Felt low-pass. The cutoff scales with the reciprocal of the contact
        // time; the constant is empirical — a strict 1/(2·t) reading of the
        // pulse width lands far darker than measured piano spectra, because
        // the felt hardens during contact. Floored above the fundamental so
        // the shortest treble strings keep their first partial.
        let position = index as f32 / (NOTE_COUNT - 1) as f32;
        // Tension modulation: a hard blow stretches the string, starting the
        // note sharp; the extra tension relaxes over ~250 ms. Strongest on
        // the heavy bass strings (Askenfelt & Jansson report several cents).
        // Kept small: at ~14 cents the settle reads as an oriental string's
        // bend, not a piano's live blow. Measured piano glides are a few
        // cents at most.
        let glide_cents = if velocity > 0.6 {
            11.5 * self.controls.lab_at(10, position) * velocity * velocity * ((0.35_f32 - position) / 0.35).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let f0 = f0 * powf(2.0, glide_cents / 1200.0);
        let string_scale = powf(f0 / 220.0, 0.55).clamp(0.35, 1.8);
        let treble_life = self.cal(note, 8) * self.controls.lab_at(1, position);
        let contact = self.contact_time(note, velocity);
        // The nonlinear forest keeps the bass ladder open far above what the
        // soft bass hammer alone would give; the felt corner widens with it.
        let bass_top = 1.0 + 2.2 * ((0.35_f32 - position) / 0.35).clamp(0.0, 1.0);
        let cutoff = ((1.9 * self.cal(note, 0) / contact)
            * bass_top
            * self.controls.lab_at(0, position)
            * (0.5 + 1.5 * self.controls.brightness))
            .max(1.5 * f0);

        // Aftersound detune: a fraction of a cent in the bass, over a cent in
        // the treble, scaled by the unison control.
        // Unison detune, in cents. The bass end was 0.3, which at the default
        // unison setting is 0.43 cents -- against a measured 2.9 on the YDP
        // A0, whose 2-4 kHz partials split into clusters about 5 Hz wide.
        //
        // It sits at 0.9 rather than the measured width because the two are
        // in tension and the tension is worth stating. Widening it walks the
        // fit cost the wrong way (19.59 at 0.3, 19.91 here, 20.45 at 1.5)
        // while walking the DENSITY the right way (24, 36 and 41 audible
        // peaks in A0's 2-4 kHz against the instrument's 82). The fit cost
        // scores band levels inside windows and cannot see whether a band's
        // energy sits in 24 components or 82; it also reads the beating as
        // decay error. Where the two disagree this far, neither should be
        // followed alone.
        let detune_cents =
            (0.9 + 0.9 * position) * (self.controls.unison * 2.86) * self.controls.lab_at(13, position);

        // First pass: partial frequencies and unnormalised amplitudes. The
        // comb keeps the sign of sin(n·π·x0): a struck string's partials
        // alternate polarity around each node, and discarding that alternation
        // is part of what makes additive attacks sound synthetic. On top, a
        // deterministic ±1.5 dB irregularity stands in for the bridge
        // admittance the smooth 1/n law ignores — real piano spectra are
        // ragged, and the raggedness is fixed per note, not random per strike.
        let mut frequencies = [0.0_f32; MAX_PARTIALS];
        let mut amplitudes = [0.0_f32; MAX_PARTIALS];
        // Board/radiation/winding colour per partial, and the attack phase
        // states the hammer simulation leaves behind ((q, v/w) unit vector).
        let mut colour = [0.0_f32; MAX_PARTIALS];
        let mut phase_q = [0.0_f32; MAX_PARTIALS];
        let mut phase_o = [1.0_f32; MAX_PARTIALS];
        let mut count = 0;
        let mut peak = 0.0_f32;
        for n in 1..=MAX_PARTIALS {
            let nf = n as f32;
            // Wound strings deviate from Fletcher's formula in the high
            // partials (the winding is not part of the ideal stiff core);
            // the drift grows with n and roughens the bass ladder's texture.
            let winding = 1.0
                + ((0.35 - position) / 0.35).clamp(0.0, 1.0)
                    * 0.0012
                    * (nf / 40.0)
                    * (hash01((note as u32) << 11 | (n as u32) << 4 | 9) - 0.5);
            let frequency = nf * f0 * sqrtf(1.0 + b * nf * nf) * winding;
            if frequency >= audible_top {
                break;
            }
            let (ideal_comb, _) = sincosf(core::f32::consts::PI * nf * x0);
            // The strike-point comb, floored, because the bridge is not a
            // rigid node. It has finite admittance -- that is the whole
            // reason the instrument makes any sound at all -- so the mode
            // shapes are not exact sines with a perfect node at the
            // termination, and the comb never reaches a true null. Measured
            // combs in real pianos are dips of 10-20 dB.
            //
            // In the bass the strike point is almost exactly 1/8, so an ideal
            // comb is exactly periodic and deletes every eighth partial
            // outright. Those are partials the model has already computed and
            // already pays for: A0 places 38 between 2 and 4 kHz and only 19
            // survive to be audible, while the real instrument has 66 audible
            // things in that band -- its densest, and the growl of a concert
            // grand's bottom octave.
            let comb = if ideal_comb < 0.0 { -1.0 } else { 1.0 }
                * sqrtf(ideal_comb * ideal_comb + COMB_FLOOR * COMB_FLOOR);
            // Finite contact width. The felt's force distribution is smooth,
            // so its transform is a Gaussian-like rolloff with no nulls — a
            // sinc (the rectangle's transform) put its first null at partial
            // ~33 on A0 and erased the top three octaves of the bass ladder.
            // Hard blows compress the felt and narrow the contact: the
            // window widens the spectrum with velocity.
            let effective = width * (1.05 - 0.45 * velocity);
            let argument = nf * effective;
            let window = expf(-1.2 * argument * argument);
            // Gaussian felt. The measured sustained spectrum falls off a
            // cliff (C4 ff: partial 9 at -12 dB, partial 11 at -45) — the
            // transform of the felt's smooth force pulse, super-polynomial
            // past the corner. The band energy above the cliff is the
            // attack transient's (noise, clang, phantoms), not the sustained
            // ladder's: a ladder that sustains up there is a guitar. An
            // earlier revision conflated those two measurements.
            let felt_r = frequency / cutoff;
            // Cliff, then floor: past the felt cliff the measured spectrum
            // does not vanish — it sits on a ragged −30…−45 dB shelf out to
            // 8 kHz, the sustained nonlinear forest, growing as the square
            // of velocity. A cliff to silence sounds hollowed out.
            let floor = 0.0455 * velocity * velocity * self.controls.lab_at(6, position) * self.cal(note, 1);
            let felt = expf(-1.2 * felt_r * felt_r).max(floor);
            // The board barely radiates below its first mode: the lowest
            // notes' fundamentals (and even second partials) come out tens of
            // dB down, and the ear reconstructs the pitch from the partial
            // ladder. Radiating them at full strength is a synthesizer's
            // sub bass, not a piano's. Sixth-order: the YDP measurements show
            // -40 dB at 27.5 Hz against ~0 dB by 78 Hz.
            let ratio = frequency / RADIATION_CORNER_HZ;
            let u = {
                let r2 = ratio * ratio;
                r2 * r2 * r2
            };
            let radiation = u / (1.0 + u);
            let (board, _) = Self::board_response(frequency);
            let rough =
                (0.71 + 0.58 * hash01((note as u32) << 8 | n as u32)) * board * radiation;
            colour[count] = rough;
            // Bridge force, not string displacement: the ear hears the force
            // the string exerts on the bridge, proportional to the string's
            // slope at its termination. Modal displacement after a strike
            // falls as sin(n·π·x0)/n, but the slope multiplies each mode by
            // n — the factors cancel, so the radiated spectrum is the comb
            // times the felt filter, with no 1/n law. The 1/n version is a
            // Rhodes: fundamental-heavy, partials 5–30 missing in action.
            let amplitude = comb * window * felt * rough;
            frequencies[count] = frequency;
            amplitudes[count] = amplitude;
            peak = peak.max(amplitude.abs());
            count += 1;
        }
        if count == 0 || peak <= 0.0 {
            return;
        }

        // The hammer as an event, not a formula: integrate the nonlinear
        // felt against the returning waves and keep each mode's amplitude
        // AND phase wherever the simulation speaks louder than the
        // calibrated floor. Comb, contact width and felt filtering are
        // emergent in it; radiation and board colour still apply.
        {
            let mut sim_modes = 0;
            // Above ~8 kHz a mode contributes almost nothing to the contact
            // shape, and the strike runs on the audio thread: modes past it
            // keep the calibrated recipe's amplitude instead.
            while sim_modes < SIM_MODES.min(count)
                && frequencies[sim_modes] < 8_000.0_f32.min(nyquist)
            {
                sim_modes += 1;
            }
            if sim_modes >= 4 && self.strike_budget > 0 {
                self.strike_budget -= 1;
                // The hammer's mass and the felt's stiffness are separate
                // properties, and the contact time is what comes OUT of them:
                // tau ~ pi*sqrt(m/K). Deriving the stiffness from the mass, as
                // this did, made the two cancel exactly -- the integration
                // divides the force by the mass, and a force proportional to
                // the mass leaves the hammer's trajectory untouched. What
                // little survived, the force delivered to the string, was then
                // removed by the renormalisation below. So the mass control
                // was inert by construction, and zero made it divide by zero.
                //
                // The stiffness now comes from the felt alone, so a heavier
                // hammer stays in contact longer and speaks darker, which is
                // why a bass note is dark in the first place. At the control's
                // centre the mass is the nominal one and this is bit for bit
                // the old behaviour.
                let nominal_mass = 0.06 + 0.85 * powf(position, 1.3);
                // The hammer's mass, against the string's.
                //
                // Two corrections, both measured with `how_long_the_hammer_stays`.
                //
                // The first is scale: the hammer was some hundred times too
                // heavy for the string it strikes, so it never decelerated
                // enough to leave. Contact lasted 4.80 ms for every note at
                // every velocity, which is exactly the integration window --
                // the hammer was still pressing when the loop ran out, and a
                // hammer resting on a sounding string damps it.
                //
                // The second is that the string's effective mass here is
                // 1/(2*sum of shape^2), which grows as the mode count SHRINKS.
                // A0 simulates 144 modes and C4 only 45, so the treble string
                // looked three times heavier to the hammer than the bass one.
                // That is an artefact of how many modes we chose to integrate,
                // not a property of the instrument, so it is divided out.
                let mass = (nominal_mass
                    * self.controls.lab_at(8, position)
                    * HAMMER_MASS_SCALE
                    * (sim_modes as f32 / SIM_MODES as f32))
                    .max(1e-6);
                // F0 compensates the hereditary softening (Stulov measures
                // the felt modulus under load): quasi-static stiffness is
                // F0·(1−e), so F0 carries 1/(1−e).
                let stiffness = nominal_mass
                    * powf(core::f32::consts::PI / contact, 2.0)
                    * 34.0
                    * self.controls.lab_at(7, position)
                    * (0.5 + 1.5 * self.controls.brightness);
                let velocity0 = 0.25 + 1.75 * velocity;
                // The felt's hardening, and the panel's voicing controls.
                //
                // Chaigne and Askenfelt use ~2.5 across the compass; the 1.7
                // that stood here for the bass is below anything published,
                // and it was chosen when an analytic recipe set the spectrum
                // and this only modulated it. Now that the integration IS the
                // spectrum, the felt's hardening is what produces the mid
                // harmonics: C2's sixth partial rises from -12.7 to -11.4 dB
                // against the instrument's -6.3 and the fit cost falls from
                // 29.1 to 27.6.
                //
                // `brightness` and the Felt Corner control ride here too,
                // because retiring the recipe left them with nowhere to act.
                // They used to move the recipe's felt cutoff; they also sit in
                // the stiffness, but the integration's length is bounded by
                // the contact time rather than ended by the hammer, so the
                // stiffness no longer decides anything and both controls
                // measured 0.0 dB of authority -- Brightness among them, one
                // of the six on the front panel. Hardness is what a voicer
                // changes when they needle a hammer, so it is where these
                // belong.
                let exponent = ((2.5 + 0.9 * position)
                    * (0.62 + 0.76 * self.controls.brightness)
                    * self.controls.lab_at(0, position))
                    .clamp(1.2, 5.0);
                let (q, over_omega) = simulate_strike(
                    &frequencies,
                    sim_modes,
                    x0,
                    // Hard blows compress the felt and narrow the contact, the
                    // same law the recipe used.
                    width * (1.05 - 0.45 * velocity),
                    mass,
                    stiffness,
                    exponent,
                    velocity0,
                    contact * CONTACT_STRETCH,
                );
                // Scale the simulated strike to the recipe's level.
                //
                // Measured, `peak / sim_peak` runs from about 1,000 to over
                // 4,000,000 depending on note and velocity, and soft blows get
                // a factor twenty-six to forty times larger than hard ones. It
                // is not a unit change: it erases the level the integration
                // just computed and substitutes the drawn law, which is where
                // the touch went and the hammer's mass with it.
                //
                // Scaling the simulation by v^3 to undo that was tried and
                // reverted. It pushes soft strikes BELOW the recipe floor, so
                // quiet notes became pure recipe and loud ones pure
                // simulation, and F#1's brightness ratio inverted from 1.44x
                // to 0.90x -- a soft blow brighter than a hard one.
                //
                // The repair is a per-note reference: normalise by what the
                // strike produces at ONE fixed velocity, cached per note, so
                // the compass stays balanced (the simulation's absolute units
                // are arbitrary and would make the treble hundreds of times
                // quieter) while the strike's own velocity response passes
                // through untouched.
                //
                // `peak / sim_peak` used to do both, and measuring it showed
                // what that cost: the factor runs from about 1,000 to over
                // 4,000,000 depending on note and velocity, and soft blows get
                // a factor twenty-six to forty times larger than hard ones. It
                // was not a unit change. It was erasing the level the
                // integration had just computed and substituting the drawn
                // law -- which is where the touch went, and where the hammer's
                // mass went with it.
                //
                // The compass part is kept, because the simulation's absolute
                // units are arbitrary and letting them set the balance would
                // make the treble hundreds of times quieter than the bass. The
                // velocity part is given back: the measured exponent is close
                // to three across the bass and tenor, and undoing it restores
                // roughly 30 dB of range between a soft blow and a hard one,
                // which is what a piano has and what this did not.
                //
                // Full velocity is the fixed point, so the loudest notes keep
                // the level they were calibrated at and the headroom holds.
                let mut sim_peak = 0.0f32;
                let mut magnitudes = [0.0f32; SIM_MODES];
                for n in 0..sim_modes {
                    let bridge = (n + 1) as f32;
                    magnitudes[n] =
                        bridge * sqrtf(q[n] * q[n] + over_omega[n] * over_omega[n]);
                    sim_peak = sim_peak.max(magnitudes[n] * colour[n]);
                }
                if sim_peak > 0.0 {
                    // The strike SETS the spectrum; the recipe is a floor.
                    //
                    // This used to be a maximum, so the analytic curve won
                    // wherever it was louder and the integration only ever
                    // added brightness on top of it. The result was that the
                    // note's spectrum was drawn rather than generated -- a
                    // product of comb, contact window, felt curve and board
                    // colour, which is a filter, not a force.
                    //
                    // The colour term stays: that is the board and the
                    // radiation, which the strike's output legitimately
                    // passes through on its way out. What is gone is the
                    // recipe competing with the simulation for the amplitude.
                    let normalise = peak / sim_peak;
                    let mut seam = 1.0f32;
                    for n in 0..sim_modes {
                        let candidate = magnitudes[n] * colour[n] * normalise;
                        let magnitude = magnitudes[n].max(1e-12);
                        let recipe = amplitudes[n].abs().max(1e-12);
                        // The last simulated partial says how far the strike's
                        // spectrum sits from the recipe's at the boundary.
                        seam = candidate / recipe;
                        amplitudes[n] = candidate.max(recipe * RECIPE_FLOOR);
                        phase_q[n] = (n + 1) as f32 * q[n] / magnitude;
                        phase_o[n] = (n + 1) as f32 * over_omega[n] / magnitude;
                    }
                    // Above the simulated range the recipe is all there is,
                    // and leaving it at its own level left a seam.
                    //
                    // The integration stops at 8 kHz, so partials past it kept
                    // the amplitude the analytic curve gave them while
                    // everything below became the strike's, which is quieter.
                    // Measured on C2's first 30 ms, the spectrum fell -29,
                    // -39, -51 dB through the upper bands and then JUMPED to
                    // -23 in 6-12 kHz: a 28 dB step upward, exactly at the
                    // boundary. A band of top-octave hash floating above the
                    // note is heard as a click, which is what the user
                    // reported -- "se escucha como un click" -- and no
                    // ingredient could account for it because it was a seam,
                    // not an ingredient.
                    for slot in amplitudes.iter_mut().take(count).skip(sim_modes) {
                        *slot *= seam;
                    }
                }
            }
        }

        // Drop partials the strike already made inaudible, then respect the
        // global budget: a saturated instrument thins new notes, never the
        // audio callback.
        // Measured on the YDP bass (item 2, PIANO_RESEARCH.md): the first
        // partials of a real bass attack carry a smooth progressive phase
        // lag (~-25 deg per partial at A0: 0, 0, -49, -88...) — the
        // dispersive delay of the strike pulse. Impose that order on the
        // lowest partials of wound strings; above them the simulation's
        // phases stand.
        let bass_phase_gate = ((0.35 - position) / 0.35).clamp(0.0, 1.0);
        if bass_phase_gate > 0.3 {
            for n in 0..count.min(6) {
                let theta = -0.44 * n as f32 * bass_phase_gate;
                let (sin_t, cos_t) = sincosf(theta);
                phase_q[n] = sin_t;
                phase_o[n] = cos_t;
            }
        }


        let floor = peak * 1e-3;
        let budget_left = PARTIAL_BUDGET.saturating_sub(self.active_partials);
        // Sixteen slots stay reserved for the nonlinear extras (phantoms and
        // the longitudinal clang): the lowest notes fill the whole array with
        // their transverse ladder otherwise, and the growl never fits.
        let cap = if budget_left < count { budget_left.max(12) } else { count }
            .min(MAX_PARTIALS - 16);

        // Energy normalisation, then the velocity curve: level roughly
        // velocity^1.7 (sound pressure grows faster than hammer speed).
        let mut energy = 0.0;
        for n in 0..count {
            if amplitudes[n].abs() >= floor {
                energy += amplitudes[n] * amplitudes[n];
            }
        }
        let scale = 0.28 * self.cal(note, 7) * powf(velocity.max(0.01), 2.2)
            / sqrtf(energy.max(1e-9));

        // Everything a partial needs, computed before a voice is borrowed:
        // both components draw their decay from the same loss curve, read at
        // the partial's own frequency — the prompt dies ~3× faster, the
        // aftersound lingers past it.
        //
        // The aftersound is not one chorus: each partial gets its own detune
        // (a fixed per-note jitter around the nominal cents) and its level
        // falls with partial number, because bridge coupling feeds the slow,
        // poorly-radiating configurations mostly at low partials. A uniform
        // detune ratio across the whole spectrum beats every partial at a
        // rate proportional to its frequency — precisely the synthesizer
        // "shimmer" a real unison does not have.
        let sample_rate = self.sample_rate;
        let mut partials = [Partial::default(); MAX_PARTIALS];
        let mut placed = 0;
        for n in 0..count {
            if placed >= cap || amplitudes[n].abs() < floor {
                continue;
            }
            let frequency = frequencies[n];
            let amplitude = amplitudes[n] * scale;
            let (_, board_decay) = Self::board_response(frequency);
            // The aftersound sustains much flatter than the prompt: measured
            // A4 holds nearly level from 1 s to 2 s while a shared decay
            // curve kept falling. ×1.8 on the slow stage matches the
            // measured plateau.
            let t60 =
                self.t60_seconds(frequency, f0, string_scale, treble_life) * board_decay * self.cal(note, 4);
            let jitter = 0.55 + 0.9 * hash01((note as u32) << 10 | (n as u32) << 2 | 1);
            let cents = detune_cents * jitter;
            // The strings of the unison, struck together and equal: their
            // subsequent life -- fast coherent decay, dephasing, the long
            // trapped tail, the churn -- is simulated through the bridge
            // coupling below, not scripted here.
            let three = ((index as f32 - 18.0) / 8.0).clamp(0.0, 1.0)
                * if self.soft { 0.25 } else { 1.0 };
            let w3 = if n < 12 { 0.22 * three } else { 0.0 };
            let remainder = 1.0 - w3;
            // The strings of the unison stay separate all the way up.
            //
            // They used to collapse into one oscillator above partial 32, on
            // the reasoning that "the beat between the strings is beyond
            // hearing" up there. The reasoning is backwards: the detune is a
            // constant in CENTS, so the beat rate grows with frequency. At
            // A0's fundamental 3 cents is 0.05 Hz -- a twenty-second beat --
            // but at its eightieth partial, up at 3 kHz, the same 3 cents is
            // 5 Hz, which is not beyond hearing at all. It is roughness, and
            // roughness across a dense band is what a piano's bass sounds
            // like.
            //
            // Measured on the YDP A0 between 2 and 4 kHz: the real instrument
            // shows 82 sharp peaks clustered a few Hz apart, standing 30 dB
            // above the floor. The model showed 26, one per partial, spaced
            // at the full 57 Hz of the ladder and standing 16 dB proud. The
            // collapse was deleting two thirds of what is audible in that
            // band, and it deleted the most in the bass -- an A0 has 112
            // partials above the old threshold and a C6 has none, which is
            // exactly why the treble already reads as a piano and the bottom
            // octave does not.
            //
            // It costs nothing: the second oscillator was allocated either
            // way and simply given zero amplitude.
            let (w1, w2) = (remainder * 0.56, remainder * 0.44);
            let half_ratio = powf(2.0, cents / 2400.0);
            let third_ratio = powf(
                2.0,
                cents * (0.9 + 0.4 * hash01((note as u32) << 9 | (n as u32) << 2 | 3)) / 1200.0,
            );
            // Strings carry only their intrinsic (internal/air) losses in
            // their own rotations; radiation is the bridge's business.
            // The long second stage, and it must NOT be flat across the
            // ladder.
            //
            // A dephased string escapes the bridge drain and then rings at
            // its own intrinsic loss, which this multiplied by a flat 5.3.
            // High partials dephase soonest -- their detune is the same in
            // cents and so far larger in Hz -- so they escaped first and then
            // rang for the longest.
            //
            // Measured on C2, T60 per partial: the real instrument's low
            // partials last 11.4 s and its high ones 9.5 s, a ratio of 1.20
            // with the lows lasting longer, and the reference renderer agrees
            // at 1.07. This model had 25.0 s low against 50.4 s high -- a
            // ratio of 0.50, INVERTED, with the upper partials outlasting the
            // body by two to one and one of them not decaying at all.
            //
            // A wire whose upper harmonics are still sounding after the body
            // has died is not a piano, and "mucha cuerda" is exactly what it
            // sounds like.
            // How much longer the second stage lasts than the first, and
            // nothing else. This is the two-stage decay itself -- the
            // horizontal polarisation outliving the vertical one -- and it is
            // structure, not a correction against a measurement.
            //
            // A `4.05 * f^-0.357` factor used to be multiplied in here,
            // gated to the bass. It was fitted honestly, against C2's
            // partials, but it was fitted on top of a decay curve that was
            // already wrong, so it was measuring that curve's error and not
            // the instrument. With the curve refitted it has nothing left to
            // correct, and keeping it would take a bass note's 500 Hz partial
            // down to two fifths of its proper life for the second time.
            let tail = 1.8 + 3.5 / (1.0 + powf(frequency / TAIL_KNEE_HZ, 1.2));
            let intrinsic =
                self.decay_per_sample(t60 * tail * self.controls.lab_at(12, position));
            let prompt_t60 = t60 * 1.94 * self.controls.lab_at(11, position) / (1.4 + 1.1 * position);
            let step = expf(
                -6.907_755 * (CULL_INTERVAL as f32 / sample_rate) / prompt_t60,
            );
            // How strongly this partial couples to the bridge, and it is not
            // flat with frequency.
            //
            // A fade was first put here to hide the aliasing of a
            // control-rate drain. That reading was wrong -- moving the drain
            // to per-sample changed the measured loss not at all -- but the
            // fade itself kept earning its place, and there is a reason: what
            // sets the coupling is the ratio of the bridge's admittance to
            // the string's characteristic impedance, and that ratio falls
            // away as the partial's frequency rises. A high partial barely
            // moves the bridge.
            let follow = (COUPLING_TOP_HZ / frequency.max(1.0)).clamp(0.0, 1.0);
            let coupling = (1.0 - step) / (2.0 + three) * follow;
            // The partial swells in over many of its own periods, and slowly
            // enough to matter: a bass note does not arrive, it gathers.
            //
            // Measured on C2, energy every 4 ms through the onset: the real
            // instrument is 26 dB down in its first four milliseconds and
            // does not reach full level until 112 ms, seven periods in. This
            // model started 14 dB louder than that -- an audible click where
            // the string has not even completed a quarter period -- and
            // peaked at 44 ms. Stretching the swell puts the peak at 104 ms
            // and takes 7 dB off the click.
            let rise_seconds =
                ((5.0 / frequency) * self.controls.lab_at(9, position)).clamp(0.0008, 0.15);
            let rise = expf(-1.0 / (rise_seconds * sample_rate));
            let (pq, po) = (phase_q[n], phase_o[n]);
            partials[placed] = Partial {
                prompt: Component::start_state(
                    amplitude * w1 * pq,
                    amplitude * w1 * po,
                    (frequency * half_ratio).min(nyquist),
                    intrinsic,
                    sample_rate,
                ),
                aftersound: Component::start_state(
                    amplitude * w2 * pq,
                    amplitude * w2 * po,
                    frequency / half_ratio,
                    intrinsic,
                    sample_rate,
                ),
                bloom: Component::start_state(
                    -amplitude * pq,
                    -amplitude * po,
                    frequency,
                    rise,
                    sample_rate,
                ),
                third: Component::start_state(
                    amplitude * w3 * pq,
                    amplitude * w3 * po,
                    (frequency * third_ratio).min(nyquist),
                    intrinsic,
                    sample_rate,
                ),
                coupling,
            };
            placed += 1;
        }
        if placed == 0 {
            return;
        }

        // Phantom partials: nonlinear transverse→longitudinal mixing puts
        // extra components near twice each low partial's frequency, growing
        // fast with amplitude — the metallic edge of a hard bass note
        // (Conklin 1999; Bank & Sujbert 2005). Rendered for the bottom third
        // of the compass, from the strongest low partials, at a level that
        // scales with the square of velocity.
        // Strongest in the bass but present through the mids: C4 ff carries
        // measurable 3-8 kHz forest energy the gated version lacked entirely.
        let bass_gate = powf((1.0 - 1.1 * position).clamp(0.0, 1.0), 1.5);
        if bass_gate > 0.0 && velocity > 0.4 {
            let phantom_cap = budget_left.max(12).min(MAX_PARTIALS);
            // A third of what it was. Phantom partials are real -- Conklin
            // measured them -- but they are placed BETWEEN the ladder's
            // positions, so their level is what decides how deep the gaps
            // between partials stay, and the gaps are what make a partial
            // read as a pitch instead of as mush.
            //
            // Measured against two references at once: the YDP samples and a
            // licensed reference renderer, on how far the peaks in 2-4 kHz
            // stand above the floor in the sustained part. Both put A0 at
            // 27-30 dB; this model sat at 23.3, and turning the phantoms off
            // entirely accounted for 3.3 dB of the 4 dB gap while clang,
            // chiff and thump accounted for nothing at all. Lowering them
            // improves the fit cost too (19.91 -> 19.61), which is not the
            // usual trade and is worth taking at face value: they were simply
            // too loud.
            let phantom_scale =
                bass_gate * velocity * velocity * 0.21 * self.controls.lab_at(5, position)
                    * self.cal(note, 6);
            let sources = placed.min(6);
            for n in 0..sources {
                if placed >= phantom_cap {
                    break;
                }
                let frequency = frequencies[n] * 2.0;
                if frequency >= nyquist {
                    break;
                }
                let amplitude = amplitudes[n] * scale * phantom_scale;
                if amplitude.abs() < floor * scale {
                    continue;
                }
                // Longitudinal content decays faster than the transverse
                // partial it rides above; no aftersound of its own.
                let t60 = self.t60_seconds(frequency, f0, string_scale, treble_life) * 0.4;
                let decay = self.decay_per_sample(t60);
                partials[placed] = Partial {
                    prompt: Component::start(amplitude, frequency, decay, sample_rate),
                    ..Partial::default()
                };
                placed += 1;
            }

            // Sum-frequency phantoms, f_m + f_n: inharmonicity puts them
            // slightly flat of the real partial they land near, and the slow
            // beat between the two is the growl of a hard bass note —
            // roughness with a rate, not noise (Conklin 1999).
            let pairs: [(usize, usize); 6] = [(0, 1), (0, 2), (1, 2), (1, 3), (2, 3), (0, 4)];
            for (a, b) in pairs {
                if placed >= phantom_cap || b >= sources {
                    break;
                }
                let frequency = frequencies[a] + frequencies[b];
                if frequency >= nyquist {
                    continue;
                }
                let amplitude =
                    sqrtf((amplitudes[a] * amplitudes[b]).abs()) * scale * phantom_scale * 0.7;
                if amplitude < floor * scale {
                    continue;
                }
                let t60 = self.t60_seconds(frequency, f0, string_scale, treble_life) * 0.35;
                let decay = self.decay_per_sample(t60);
                partials[placed] = Partial {
                    prompt: Component::start(amplitude, frequency, decay, sample_rate),
                    ..Partial::default()
                };
                placed += 1;
            }

            // The longitudinal clang: the fast wave along the string sounds
            // the longitudinal modes as a formant near ~17·f0 for wound bass
            // strings (Bank & Sujbert 2005 measure ~1.15 kHz for C2). It is
            // tonal — a wooden-metallic knock with a pitch — short, and
            // nearly absent below forte.
            let clang_level =
                bass_gate * powf(velocity, 2.5) * 0.065 * 0.32 * self.controls.lab_at(4, position)
                    * self.cal(note, 5);
            if clang_level > 1e-4 {
                let formant =
                    f0 * 17.0 * (1.0 + 0.08 * (hash01((note as u32) << 4 | 3) - 0.5));
                for (ratio, level, seed) in [(1.0_f32, 1.0_f32, 7u32), (1.98, 0.45, 13)] {
                    if placed >= phantom_cap {
                        break;
                    }
                    let frequency = formant * ratio;
                    if frequency >= nyquist {
                        continue;
                    }
                    let jitter =
                        1.0 + 0.02 * (hash01((note as u32) << 5 | seed) - 0.5);
                    let t60 = 0.25;
                    let decay = self.decay_per_sample(t60);
                    let rise = expf(-1.0 / (0.001 * sample_rate));
                    let amplitude = clang_level * level;
                    partials[placed] = Partial {
                        prompt: Component::start(
                            amplitude,
                            frequency * jitter,
                            decay,
                            sample_rate,
                        ),
                        bloom: Component::start(
                            -amplitude,
                            frequency * jitter,
                            rise,
                            sample_rate,
                        ),
                        ..Partial::default()
                    };
                    placed += 1;
                }
            }
        }

        // The chiff sits only ~15–20 dB under the tone's peak in a real
        // instrument and lasts longer on the heavy bass hammers.
        // The action's noise is not a click: the key bed, the shank and the
        // damper keep radiating for tens of milliseconds, which is why the
        // reference carries a noise floor through its whole attack. A 12 ms
        // burst is spent before the window the measurement looks at.
        let noise_decay = expf(-1.0 / ((0.060 - 0.042 * position) * sample_rate));
        // The knock starts wide — brighter for harder blows — and its
        // bandwidth contracts with a ~25 ms time constant as it fades.
        let noise_coefficient = 1.0
            - expf(
                -core::f32::consts::TAU * (1200.0 + 2500.0 * position + 6000.0 * velocity)
                    / sample_rate,
            );
        let noise_shrink = expf(-1.0 / (0.070 * sample_rate));
        let noise_body_coefficient = 1.0
            - expf(-core::f32::consts::TAU * (40.0 + 900.0 * position * position) / sample_rate);
        // Constant-power pan by key position, narrowed by the width control.
        let spread = (position - 0.5) * self.controls.width;
        let angle = (0.5 + spread * 0.8) * core::f32::consts::FRAC_PI_2;
        let (pan_right, pan_left) = sincosf(angle);

        // The key-bottom thump: the action landing on the keybed and the
        // board's whole-body motion put a low-frequency thud under every
        // note, treble included — the A/B against the YDP renders shows the
        // real instrument carrying tens of dB more 30–120 Hz energy under
        // mid and treble notes than strings alone can explain. Three short
        // dark components stand in for it.
        {
            let thump_level = powf(velocity, 1.9) * 0.046 * 0.32 * (1.0 - 0.35 * position)
                * self.controls.lab_at(2, position)
                * self.cal(note, 2);
            let rise = expf(-1.0 / (0.004 * sample_rate));
            for (freq, level, seed) in
                [(46.0_f32, 1.0_f32, 17u32), (71.0, 0.7, 23), (103.0, 0.45, 31), (149.0, 0.30, 37), (214.0, 0.20, 41)]
            {
                if placed >= MAX_PARTIALS {
                    break;
                }
                let jitter = 1.0 + 0.10 * (hash01((note as u32) << 7 | seed) - 0.5);
                let amplitude = thump_level * level;
                let decay = self.decay_per_sample(0.30);
                partials[placed] = Partial {
                    prompt: Component::start(amplitude, freq * jitter, decay, sample_rate),
                    bloom: Component::start(-amplitude, freq * jitter, rise, sample_rate),
                    ..Partial::default()
                };
                placed += 1;
            }
        }

        // Duplex scale: the string segments behind the bridge, tuned high,
        // struck only through the bridge, and — crucially — undamped, so a
        // staccato treble note leaves their faint ping ringing. Fitted on the
        // treble half of the compass, where builders fit them.
        let mut duplex = [Component::default(); 2];
        if position > 0.45 {
            // Faint and barely off-harmonic: at −25 dB the duplex reads as
            // shimmer and afterglow; louder or wider it reads as a detuned
            // bell riding every strike. Compressed against velocity — the
            // tone grows with v^1.7 and masks it while held, but after the
            // damper falls the ring stands alone, so a hard strike must not
            // leave proportionally more of it.
            let level = 0.018 * powf(velocity, 1.7) * (1.0 - 0.45 * velocity) * 0.32;
            for (slot, (ratio, seed)) in duplex.iter_mut().zip([(2.015_f32, 11), (4.03, 29)]) {
                let jitter = powf(2.0, (hash01((note as u32) << 6 | seed) - 0.5) * 10.0 / 1200.0);
                let frequency = f0 * ratio * jitter;
                if frequency < nyquist {
                    // Short segments, short ring: undamped is not endless.
                    let t60 = (self.t60_seconds(frequency, f0, 1.0, 1.0) * 0.35).min(0.9);
                    let decay = self.decay_per_sample(t60);
                    *slot = Component::start(level, frequency, decay, sample_rate);
                }
            }
        }

        let chiff_mult = self.controls.lab_at(3, position) * self.cal(note, 3);
        // How hard this string's own stretch pulls it sharp. The bass gate is
        // the amplitude-to-length ratio in disguise: a treble string is short
        // and stiff and barely stretches, a bass string is long and slack and
        // stretches plenty.
        let tension_gain = TENSION_GAIN * bass_gate / (1.0 + 40.0 * position)
            * self.controls.lab_at(10, position);
        let Some(voice) = self.allocate_voice() else { return };
        voice.active = true;
        voice.note = note;
        voice.channel = channel;
        voice.held = true;
        voice.sustained = false;
        voice.partials = partials;
        voice.partial_count = placed;
        voice.duplex = duplex;
        voice.cull_in = CULL_INTERVAL;
        voice.tension_in = TENSION_INTERVAL;
        // The string is tuned at rest, so the stretch it carries once the
        // note has died away must pull it nowhere: the rest value is zero and
        // everything above it is the note sharpening itself.
        voice.tension_rest = 0.0;
        voice.tension_applied = 0.0;
        // The longitudinal modes sit at k*c_L/(2L), and the speaking length
        // follows from the pitch and the transverse speed: L = c/(2*f0) with
        // c = 2*L*f0. So f_L,k = k * f0 * c_L / c, and the ratio c_L/c is what
        // makes them land in the low kilohertz for a bass string and above
        // hearing for a treble one -- which is why this is a bass phenomenon.
        // A fixed ratio to the note's own pitch, not a frequency derived from
        // a guessed string length.
        //
        // Deriving it from length gave a ratio that slid across the compass --
        // 22x the fundamental at A0 but only 10x at C4 -- and ten against
        // twenty is an octave, which is exactly what the user heard: "es como
        // que la octava de eso que agregaste no esta bien". Bank states the
        // figure directly: the longitudinal fundamental sits "around 16 to 20
        // times higher than that of the transverse vibration", and it holds
        // across the instrument because scale design keeps it there.
        let longitudinal_first = LONGITUDINAL_RATIO * f0;
        for (k, mode) in voice.longitudinal.iter_mut().enumerate() {
            let hz = longitudinal_first * (k + 1) as f32;
            if hz < nyquist * 0.9 {
                // Short and lightly damped: the compressional wave loses
                // little in the wire and a lot at the terminations.
                let t60 = (0.9 - 0.15 * k as f32).max(0.25);
                let pan = 0.5 + 0.3 * (hash01((note as u32) << 3 | k as u32) - 0.5);
                *mode = BodyMode::tune(hz, t60, pan, sample_rate);
            } else {
                *mode = BodyMode::default();
            }
        }
        voice.longitudinal_drive = [0.0; LONGITUDINAL_MODES];
        // Scaled so a fortissimo bass strike sharpens by a few cents, which
        // is what the measured glides are, and so it fades with the note
        // rather than on a timer. The bass gate is the amplitude-to-length
        // ratio in disguise: a treble string is short and stiff and barely
        // stretches, a bass string is long and slack and stretches plenty.
        voice.tension_gain = tension_gain;
        voice.energy = 1.0;
        // The hammer/soundboard thump: heavier and darker in the bass.
        // Flat through the bass and the tenor, where the measurement wanted
        // far more knock than the model had, and eased above it: a treble
        // hammer is a fraction of a bass hammer's mass and its action moves
        // less. Part of what reads as too much noise up there is the treble's
        // own tone measuring 6-22 dB under the reference in 1-4 kHz, since
        // noisiness is a share of the total — fixing that is the real repair,
        // and this taper is not a substitute for it.
        let action = 1.0 - 0.85 * ((position - 0.5) / 0.5).max(0.0);
        voice.noise_amp = velocity * velocity * KNOCK_LEVEL * action * chiff_mult;
        voice.noise_decay = noise_decay;
        voice.noise_coefficient = noise_coefficient;
        voice.noise_body = 0.0;
        voice.noise_body_coefficient = noise_body_coefficient;
        voice.noise_shrink = noise_shrink;
        voice.noise_lp = 0.0;
        voice.noise_seed = 0x9E37_79B9 ^ (note as u32).wrapping_mul(2_654_435_761);
        voice.pan_left = pan_left;
        voice.pan_right = pan_right;
        // The glide is no longer scripted. It used to be a 28-step ramp of a
        // hand-set size; it now falls out of the tension law above, which
        // sharpens the string while it is displaced and lets it settle as the
        // note decays -- the same curve, but produced rather than drawn, and
        // by the mechanism that also couples the modes to each other.
        voice.glide_rate = 0.0;
        voice.glide_steps = 0;
        self.active_partials += placed;

        // Sympathetic resonance, the pedal's halo: with the dampers up, the
        // other strings' coinciding partials pick the struck note's energy up
        // through the bridge and ring on slowly. Rendered as a shadow voice —
        // the same partial ladder ~24 dB down, each component detuned by its
        // own few cents (many strings, none exactly aligned), single-stage
        // slow decay, released by the pedal like any sustained string.
        if self.pedal && placed > 0 {
            let halo_count = placed.min(24);
            let mut halo = [Partial::default(); MAX_PARTIALS];
            let rise = expf(-1.0 / (0.030 * sample_rate));
            for n in 0..halo_count {
                let frequency = frequencies[n];
                let spread =
                    powf(2.0, (hash01((note as u32) << 12 | (n as u32) << 3 | 5) - 0.5) * 5.0 / 1200.0);
                let detuned = (frequency * spread).min(nyquist);
                let amplitude = amplitudes[n] * scale * 0.063;
                let t60 = self.t60_seconds(frequency, f0, string_scale, treble_life) * 1.5;
                let slow = self.decay_per_sample(t60);
                halo[n] = Partial {
                    prompt: Component::start(amplitude, detuned, slow, sample_rate),
                    bloom: Component::start(-amplitude, detuned, rise, sample_rate),
                    ..Partial::default()
                };
            }
            if let Some(shadow) = self.allocate_voice() {
                *shadow = Voice::default();
                shadow.active = true;
                shadow.note = note;
                shadow.channel = channel;
                shadow.held = false;
                shadow.sustained = true;
                shadow.partials = halo;
                shadow.partial_count = halo_count;
                shadow.pan_left = pan_left;
                shadow.pan_right = pan_right;
                shadow.energy = 0.01;
                self.active_partials += halo_count;
            }
        }
    }

    fn allocate_voice(&mut self) -> Option<&mut Voice> {
        if let Some(index) = self.voices.iter().position(|voice| !voice.active) {
            return Some(&mut self.voices[index]);
        }
        // All busy: steal the quietest, refunding its partials to the budget.
        let index = self
            .voices
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.energy.total_cmp(&b.energy))
            .map(|(index, _)| index)?;
        self.active_partials = self
            .active_partials
            .saturating_sub(self.voices[index].partial_count);
        Some(&mut self.voices[index])
    }

    /// Per-sample decay multiplier a falling damper applies: the note dies in
    /// tens of milliseconds instead of seconds.
    fn damper_factor(&self, note: u8) -> f32 {
        Self::damper_for(note, self.sample_rate)
    }

    /// Dampers are not equally effective across the compass: a treble damper
    /// stops its short light string almost at once, while a wound bass string
    /// carries far too much energy to be stopped that fast. A single 60 ms
    /// constant for the whole keyboard left every release ringing ~230 ms
    /// down to −34 dB, which smears into a wash as soon as playing gets fast.
    fn damper_for(note: u8, sample_rate: f32) -> f32 {
        let position = (note.clamp(LOW_NOTE, LOW_NOTE + NOTE_COUNT as u8 - 1) - LOW_NOTE) as f32
            / (NOTE_COUNT - 1) as f32;
        let seconds = 0.075 - 0.055 * position;
        expf(-1.0 / (seconds * sample_rate))
    }

    /// The damper thud's colour and length: dark and short.
    fn damper_thud(&self) -> (f32, f32) {
        let coefficient =
            1.0 - expf(-core::f32::consts::TAU * 260.0 / self.sample_rate);
        let decay = expf(-1.0 / (0.010 * self.sample_rate));
        (coefficient, decay)
    }

    fn release(&mut self, channel: u8, note: u8) {
        let damper = self.damper_factor(note);
        let (thud_coefficient, thud_decay) = self.damper_thud();
        for voice in &mut self.voices {
            if voice.active && voice.note == note && voice.channel == channel && voice.held {
                if self.pedal {
                    voice.held = false;
                    voice.sustained = true;
                } else {
                    voice.damp(damper, thud_coefficient, thud_decay);
                }
            }
        }
    }

    fn set_pedal(&mut self, down: bool) {
        self.pedal = down;
        if !down {
            let (thud_coefficient, thud_decay) = self.damper_thud();
            let rate = self.sample_rate;
            for voice in &mut self.voices {
                if voice.active && voice.sustained {
                    let damper = Self::damper_for(voice.note, rate);
                    voice.damp(damper, thud_coefficient, thud_decay);
                }
            }
        }
    }

    fn all_notes_off(&mut self) {
        let (thud_coefficient, thud_decay) = self.damper_thud();
        let rate = self.sample_rate;
        for voice in &mut self.voices {
            if voice.active {
                let damper = Self::damper_for(voice.note, rate);
                voice.damp(damper, thud_coefficient, thud_decay);
            }
        }
        self.pedal = false;
    }

    fn handle_midi(&mut self, event: &MidiEvent) {
        let data = event.data;
        let channel = data[0] & 0x0f;
        match data[0] & 0xf0 {
            0x90 if data[2] > 0 => self.start_voice(channel, data[1] & 0x7f, data[2] & 0x7f),
            0x80 | 0x90 => self.release(channel, data[1] & 0x7f),
            0xb0 => match data[1] {
                64 => self.set_pedal(data[2] >= 64),
                67 => self.soft = data[2] >= 64,
                120 | 123 => self.all_notes_off(),
                _ => {}
            },
            _ => {}
        }
    }

    /// Cubic soft clip with unity gain at small levels; only a pedalled
    /// fortissimo cluster ever reaches it.
    fn soften(sample: f32) -> f32 {
        let x = sample.clamp(-1.5, 1.5);
        x * (1.0 - x * x / 6.75)
    }
}

impl Processor for ConcertGrand {
    fn prepare(
        &mut self,
        sample_rate: f64,
        _maximum_frames: u32,
        _input_channels: u32,
        _output_channels: u32,
    ) -> bool {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return false;
        }
        self.sample_rate = sample_rate as f32;
        self.tune_board();
        self.tune_open_strings();
        self.tune_undamped();
        self.tune_halo();
        self.tune_lid();
        self.tune_room();
        self.reset();
        true
    }

    fn reset(&mut self) {
        self.voices = [Voice::default(); MAX_VOICES];
        self.pedal = false;
        self.active_partials = 0;
        for string in &mut self.undamped {
            string.y1 = 0.0;
            string.y2 = 0.0;
        }
        for string in &mut self.open_strings {
            string.y1 = 0.0;
            string.y2 = 0.0;
        }
        for mode in &mut self.board {
            mode.y1 = 0.0;
            mode.y2 = 0.0;
        }
        self.halo = [[0.0; HALO_BUFFER]; 4];
        self.halo_lp = 0.0;
        self.halo_write = 0;
        self.lid = [0.0; LID_BUFFER];
        self.lid_write = 0;
        self.room = [[0.0; ROOM_BUFFER]; ROOM_LINES];
        self.room_lp = [0.0; ROOM_LINES];
        self.room_write = 0;
    }

    fn set_parameter(&mut self, index: u32, value: f64) -> bool {
        self.controls.set(index, value)
    }

    fn get_parameter(&self, index: u32) -> Option<f64> {
        self.controls.get(index)
    }

    fn load_preset(&mut self, id: &str) -> bool {
        self.controls = match id {
            "concert" => Controls::default(),
            "mellow" => Controls {
                brightness: 0.28,
                dynamics: 0.5,
                unison: 0.55,
                decay: 0.55,
                width: 0.6,
                level: 0.7,
                lab: [0.5; LAB_COUNT],
            lab_tilt: [0.5; LAB_TILT_COUNT],
            },
            "bright" => Controls {
                brightness: 0.8,
                dynamics: 0.75,
                unison: 0.45,
                decay: 0.45,
                width: 0.75,
                level: 0.68,
                lab: [0.5; LAB_COUNT],
            lab_tilt: [0.5; LAB_TILT_COUNT],
            },
            "intimate" => Controls {
                brightness: 0.4,
                dynamics: 0.45,
                unison: 0.65,
                decay: 0.35,
                width: 0.35,
                level: 0.72,
                lab: [0.5; LAB_COUNT],
            lab_tilt: [0.5; LAB_TILT_COUNT],
            },
            _ => return false,
        };
        true
    }

    fn save_state(&self, destination: &mut [u8]) -> Option<usize> {
        let mut values = [0.0f32; PARAM_COUNT];
        values[..6].copy_from_slice(&[
            self.controls.brightness,
            self.controls.dynamics,
            self.controls.unison,
            self.controls.decay,
            self.controls.width,
            self.controls.level,
        ]);
        values[6..6 + LAB_COUNT].copy_from_slice(&self.controls.lab);
        values[6 + LAB_COUNT..].copy_from_slice(&self.controls.lab_tilt);
        let target = destination.get_mut(..values.len() * 4)?;
        for (chunk, value) in target.chunks_exact_mut(4).zip(values) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        Some(values.len() * 4)
    }

    fn load_state(&mut self, state: &[u8]) -> bool {
        // A state saved by an older build is shorter, because controls have
        // only ever been added to the end. Read what it has and leave the
        // rest at its default, rather than rejecting the whole thing: every
        // control the user had dialled in is still in there, and throwing
        // them away to avoid guessing at three new ones is the worse trade.
        if state.len() % 4 != 0 || state.len() > PARAM_COUNT * 4 {
            return false;
        }
        let mut values = [0.5_f32; PARAM_COUNT];
        for (value, chunk) in values.iter_mut().zip(state.chunks_exact(4)) {
            let Ok(bytes) = <[u8; 4]>::try_from(chunk) else {
                return false;
            };
            let decoded = f32::from_le_bytes(bytes);
            if !decoded.is_finite() || !(0.0..=1.0).contains(&decoded) {
                return false;
            }
            *value = decoded;
        }
        let mut lab = [0.5f32; LAB_COUNT];
        lab.copy_from_slice(&values[6..6 + LAB_COUNT]);
        let mut lab_tilt = [0.5f32; LAB_TILT_COUNT];
        lab_tilt.copy_from_slice(&values[6 + LAB_COUNT..]);
        self.controls = Controls {
            brightness: values[0],
            dynamics: values[1],
            unison: values[2],
            decay: values[3],
            width: values[4],
            level: values[5],
            lab,
            lab_tilt,
        };
        true
    }

    fn process(
        &mut self,
        _input: &[f32],
        output: &mut [f32],
        midi: &[MidiEvent],
        parameters: &[ParameterEvent],
        frames: u32,
        _input_channels: u32,
        output_channels: u32,
    ) {
        let channels = output_channels as usize;
        // Three full strikes per buffer: measured at ~0.5 ms each natively,
        // which leaves the rest of the callback to the voices already ringing.
        self.strike_budget = 3;
        let level = self.controls.level * self.controls.level;
        let mut midi_index = 0;
        let mut parameter_index = 0;

        for frame in 0..frames as usize {
            while let Some(event) = midi.get(midi_index) {
                if event.frame as usize != frame {
                    break;
                }
                self.handle_midi(event);
                midi_index += 1;
            }
            while let Some(event) = parameters.get(parameter_index) {
                if event.frame as usize != frame {
                    break;
                }
                let _ = self.controls.set(event.index, event.value);
                parameter_index += 1;
            }

            let mut left = 0.0;
            let mut right = 0.0;
            for voice in &mut self.voices {
                if !voice.active {
                    continue;
                }
                let sample = voice.tick();
                left += sample * voice.pan_left;
                right += sample * voice.pan_right;

                voice.tension_in -= 1;
                if voice.tension_in == 0 {
                    voice.tension_in = TENSION_INTERVAL;
                    voice.tension_step();
                }
                voice.cull_in -= 1;
                if voice.cull_in == 0 {
                    voice.cull_in = CULL_INTERVAL;
                    let removed = voice.cull();
                    self.active_partials = self.active_partials.saturating_sub(removed);
                }
            }

            // Everything the strings produce radiates through the board.
            let excitation = left + right;
            let mut board_left = 0.0;
            let mut board_right = 0.0;
            for mode in self.board.iter_mut().take(self.board_count) {
                let y = mode.tick(excitation);
                board_left += y * mode.pan_left;
                board_right += y * mode.pan_right;
            }
            // The open top octave listens to the bridge and rings on.
            let mut open_left = 0.0;
            let mut open_right = 0.0;
            for string in &mut self.open_strings {
                let y = string.tick(excitation);
                open_left += y * string.pan_left;
                open_right += y * string.pan_right;
            }
            // Every other string's undamped length, listening to the bridge.
            let mut undamped_left = 0.0;
            let mut undamped_right = 0.0;
            for string in &mut self.undamped {
                let y = string.tick(excitation);
                undamped_left += y * string.pan_left;
                undamped_right += y * string.pan_right;
            }
            let undamped_gain = UNDAMPED_MIX * self.controls.lab(15);

            // The shimmer: everything above ~1.8 kHz feeds the undamped
            // open register and rings on.
            self.halo_lp += self.halo_hp_k * (excitation - self.halo_lp);
            let bright = excitation - self.halo_lp;
            let mut halo_outs = [0.0f32; 4];
            let mut halo_sum = 0.0;
            for line in 0..4 {
                halo_outs[line] = self.halo[line][self.halo_write % self.halo_len[line]];
                halo_sum += halo_outs[line];
            }
            let halo_householder = halo_sum * 0.5;
            for line in 0..4 {
                let feedback = (halo_outs[line] - halo_householder) * self.halo_gain[line];
                self.halo[line][self.halo_write % self.halo_len[line]] =
                    bright * 0.5 + feedback;
            }
            self.halo_write = self.halo_write.wrapping_add(1);
            let sympathy = self.controls.lab(15);
            let halo_left = (halo_outs[0] - halo_outs[1]) * HALO_MIX * sympathy;
            let halo_right = (halo_outs[2] - halo_outs[3]) * HALO_MIX * sympathy;

            // The lid and rim reflect the near field back a few dozen
            // milliseconds late, differently per side.
            let staged = excitation
                + (board_left + board_right) * BOARD_MIX
                + (undamped_left + undamped_right) * undamped_gain
                + (open_left + open_right) * OPEN_MIX * sympathy
                + halo_left
                + halo_right;
            self.lid[self.lid_write] = staged;
            let mut lid_left = 0.0;
            for (offset, gain) in self.lid_left {
                lid_left += self.lid[(self.lid_write + LID_BUFFER - offset) % LID_BUFFER] * gain;
            }
            let mut lid_right = 0.0;
            for (offset, gain) in self.lid_right {
                lid_right += self.lid[(self.lid_write + LID_BUFFER - offset) % LID_BUFFER] * gain;
            }
            self.lid_write = (self.lid_write + 1) % LID_BUFFER;

            // The chamber: read every line, mix through the Householder
            // matrix, damp the highs in the feedback, write back with the
            // input. The recorded instrument the model is measured against
            // lives in a room; the tail is part of the piano the ear knows.
            let mut outs = [0.0f32; ROOM_LINES];
            let mut outs_sum = 0.0;
            for line in 0..ROOM_LINES {
                outs[line] = self.room[line][self.room_write % self.room_len[line]];
                outs_sum += outs[line];
            }
            let householder = outs_sum * (2.0 / ROOM_LINES as f32);
            for line in 0..ROOM_LINES {
                let feedback = outs[line] - householder;
                self.room_lp[line] += self.room_damp * (feedback - self.room_lp[line]);
                self.room[line][self.room_write % self.room_len[line]] =
                    staged * 0.25 + self.room_lp[line] * self.room_gain[line];
            }
            self.room_write = self.room_write.wrapping_add(1);
            let air = self.controls.lab(16);
            let room_left = (outs[0] - outs[1] + outs[2]) * ROOM_MIX * air;
            let room_right = (outs[3] - outs[4] + outs[5]) * ROOM_MIX * air;

            // There is one board and it is the through path, so there is one
            // gain. The pair it replaced was mixed 25 dB apart in the wrong
            // direction, which let a sparse parallel comb own 78-87% of the
            // output below 3 kHz.
            // Headroom, so the instrument stops living inside its own
            // limiter.
            //
            // Measured at what reaches `soften`, which clamps at 1.5: a
            // single fortissimo bass note arrived at 1.60, a bass octave at
            // 2.53, a five-note chord at 5.49 and a ten-note chord at 6.58 --
            // more than four times into a brick wall. Everything above
            // mezzoforte came out flat-topped, so a ten-note chord peaked at
            // exactly the same 0.462 as one note.
            //
            // A piano whose dynamics stop at mezzoforte, whose attacks are
            // decapitated because the attack IS the peak, and where raising
            // any level control just pushes further into the clamp and
            // returns the same flattened shape, is not a piano. It is an
            // electric piano, which is what this has been called for forty
            // versions -- and it also explains, at last, why moving the panel
            // seemed to do so little.
            //
            // Sized so the loudest thing the instrument can be asked for, a
            // ten-note fortissimo chord, lands near 1.2 and stays out of the
            // clamp. That costs about 11 dB of output, which belongs in the
            // host's gain and not in a saturator: the desktop already runs
            // +6 dB and allows +12.
            let board_gain = BOARD_MIX * self.controls.lab(14) * HEADROOM;
            let left = Self::soften(
                board_left * board_gain
                    + undamped_left * undamped_gain * HEADROOM
                    + open_left * OPEN_MIX * sympathy * HEADROOM
                    + halo_left * HEADROOM
                    + lid_left * air * HEADROOM
                    + room_left * HEADROOM,
            ) * level;
            let right = Self::soften(
                board_right * board_gain
                    + undamped_right * undamped_gain * HEADROOM
                    + open_right * OPEN_MIX * sympathy * HEADROOM
                    + halo_right * HEADROOM
                    + lid_right * air * HEADROOM
                    + room_right * HEADROOM,
            ) * level;
            match channels {
                0 => {}
                1 => output[frame] = Self::soften(left + right),
                _ => {
                    output[frame * channels] = left;
                    output[frame * channels + 1] = right;
                    for channel in 2..channels {
                        output[frame * channels + channel] = 0.0;
                    }
                }
            }
        }
    }
}

export_processor!(
    ConcertGrand,
    max_frames = 4096,
    max_input_channels = 0,
    max_output_channels = 2,
    max_midi_events = 256,
    max_parameter_events = 256,
    max_transfer_bytes = 4096
);

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests run at a low rate to keep them fast; the model is rate-agnostic.
    const FS: f64 = 16_000.0;

    fn prepared() -> Box<ConcertGrand> {
        let mut piano = Box::new(ConcertGrand::default());
        assert!(piano.prepare(FS, 512, 0, 2));
        piano
    }

    fn note_on(note: u8, velocity: u8) -> MidiEvent {
        MidiEvent { frame: 0, data: [0x90, note, velocity], length: 3 }
    }

    fn note_off(note: u8) -> MidiEvent {
        MidiEvent { frame: 0, data: [0x80, note, 0], length: 3 }
    }

    fn render(piano: &mut ConcertGrand, frames: usize, midi: &[MidiEvent]) -> Vec<f32> {
        let mut output = vec![0.0; frames * 2];
        piano.process(&[], &mut output, midi, &[], frames as u32, 0, 2);
        output
    }

    fn energy(samples: &[f32]) -> f32 {
        samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32
    }

    #[test]
    fn a4_is_the_tuning_anchor_and_octaves_stretch_outward() {
        let piano = Box::new(ConcertGrand::default());
        let a4 = piano.fundamental[69 - LOW_NOTE as usize];
        assert!((a4 - 440.0).abs() < 0.01, "A4 is {a4}");
        // Above A4 the octaves run sharp of 2:1; below, flat — Railsback's
        // curve, derived from the inharmonicity rather than drawn.
        let a5 = piano.fundamental[81 - LOW_NOTE as usize];
        assert!(a5 > 2.0 * a4, "A5 {a5} is not stretched above {}", 2.0 * a4);
        let a3 = piano.fundamental[57 - LOW_NOTE as usize];
        assert!(a3 < a4 / 2.0, "A3 {a3} is not stretched below {}", a4 / 2.0);
        let a0 = piano.fundamental[0];
        assert!(a0 < 27.5, "A0 {a0} should sit flat of equal temperament");
    }

    #[test]
    fn inharmonicity_fits_the_published_shape() {
        // Smallest in the tenor, largest at the top: the ranges reported in
        // Fletcher & Rossing ch. 12.
        let tenor = ConcertGrand::inharmonicity_for(45);
        let bass = ConcertGrand::inharmonicity_for(21);
        let top = ConcertGrand::inharmonicity_for(108);
        assert!(tenor < bass && bass < top);
        assert!((5e-5..5e-4).contains(&tenor), "tenor B {tenor}");
        assert!((1e-3..5e-2).contains(&top), "treble B {top}");
    }

    #[test]
    fn the_strike_point_comb_suppresses_its_partial() {
        // The hammer strikes near 1/8 in the bass, so partials with a node
        // there — around n=8 — must come out well below their neighbours.
        let x0 = ConcertGrand::strike_point(21);
        let comb = |n: f32| sincosf(core::f32::consts::PI * n * x0).0.abs();
        let null = (1.0 / x0).round();
        assert!(comb(null) < 0.25 * comb(null - 2.0));
        assert!(comb(null) < 0.25 * comb(null + 2.0));
    }

    #[test]
    fn harder_blows_are_brighter_not_just_louder() {
        // Spectral centroid of the initial partial amplitudes must rise with
        // velocity: the felt's contact time shortens, the cutoff rises.
        let piano = prepared();
        let centroid = |velocity: f32| {
            let note = 60;
            let index = (note - LOW_NOTE) as usize;
            let f0 = piano.fundamental[index];
            let b = piano.inharmonicity[index];
            let x0 = ConcertGrand::strike_point(note);
            // The same felt low-pass the model applies at note-on.
            let cutoff =
                ((2.4 / piano.contact_time(note, velocity)) * 1.25).max(1.5 * f0);
            let mut weighted = 0.0;
            let mut total = 0.0;
            for n in 1..=40 {
                let nf = n as f32;
                let frequency = nf * f0 * sqrtf(1.0 + b * nf * nf);
                let r = frequency / cutoff;
                let amp = sincosf(core::f32::consts::PI * nf * x0).0.abs()
                    * expf(-1.2 * r * r);
                weighted += amp * frequency;
                total += amp;
            }
            weighted / total
        };
        assert!(
            centroid(1.0) > centroid(0.15) * 1.3,
            "ff centroid {} vs pp {}",
            centroid(1.0),
            centroid(0.15)
        );
    }

    #[test]
    fn a_held_note_decays_in_two_stages() {
        let mut piano = prepared();
        render(&mut piano, 64, &[note_on(57, 110)]);
        // dB per second early vs late: the prompt components dominate first,
        // the aftersound carries the tail, so the early slope must be steeper.
        let early_a = energy(&render(&mut piano, 3200, &[]));
        let early_b = energy(&render(&mut piano, 3200, &[]));
        // Past the knee: with string-scaled damping the A2 handover from
        // prompt to aftersound sits near 2.3 s.
        for _ in 0..24 {
            render(&mut piano, 3200, &[]);
        }
        let late_a = energy(&render(&mut piano, 3200, &[]));
        let late_b = energy(&render(&mut piano, 3200, &[]));
        let early_slope = (early_a / early_b.max(1e-20)).ln();
        let late_slope = (late_a / late_b.max(1e-20)).ln();
        assert!(late_b > 0.0, "the tail went silent too soon");
        assert!(
            early_slope > late_slope * 1.5,
            "early {early_slope} vs late {late_slope}"
        );
    }

    #[test]
    fn the_damper_falls_unless_the_pedal_holds_it() {
        let hold = (FS * 0.5) as usize;
        // Without the pedal: release, then almost nothing half a second on.
        let mut dry = prepared();
        render(&mut dry, hold, &[note_on(60, 100)]);
        render(&mut dry, hold, &[note_off(60)]);
        let damped = energy(&render(&mut dry, 1600, &[]));

        // With the pedal down the same release changes nothing audible.
        let mut pedalled = prepared();
        let pedal = MidiEvent { frame: 0, data: [0xb0, 64, 127], length: 3 };
        render(&mut pedalled, hold, &[pedal, note_on(60, 100)]);
        render(&mut pedalled, hold, &[note_off(60)]);
        let sustained = energy(&render(&mut pedalled, 1600, &[]));

        assert!(
            sustained > damped * 100.0,
            "pedal {sustained} vs damped {damped}"
        );
    }

    #[test]
    fn a_fortissimo_cluster_stays_inside_the_output_range() {
        let mut piano = prepared();
        let chord: Vec<MidiEvent> = [24u8, 28, 31, 36, 43, 48, 55, 60, 64, 67]
            .iter()
            .map(|note| note_on(*note, 127))
            .collect();
        let rendered = render(&mut piano, (FS * 1.0) as usize, &chord);
        assert!(rendered.iter().all(|sample| sample.is_finite()));
        assert!(rendered.iter().all(|sample| sample.abs() <= 1.0));
    }

    #[test]
    fn silence_before_a_note_and_sound_after_one() {
        let mut piano = prepared();
        assert!(render(&mut piano, 512, &[]).iter().all(|sample| *sample == 0.0));
        let sounding = render(&mut piano, 4000, &[note_on(69, 100)]);
        assert!(sounding.iter().any(|sample| sample.abs() > 0.01));
    }

    #[test]
    fn presets_and_state_round_trip() {
        let mut piano = Box::new(ConcertGrand::default());
        assert!(piano.load_preset("mellow"));
        let mut state = [0u8; PARAM_COUNT * 4];
        assert_eq!(piano.save_state(&mut state), Some(PARAM_COUNT * 4));
        assert!(piano.load_preset("bright"));
        assert!(piano.load_state(&state));
        assert_eq!(piano.get_parameter(PARAM_BRIGHTNESS), Some(0.28_f32 as f64));
        assert!(!piano.load_preset("unknown"));
    }

    #[test]
    fn a_weight_leans_a_control_into_the_bass_and_leaves_the_treble_alone() {
        // The whole point: a control set once can mean something different at
        // each end of the keyboard, so a value that suits the treble no longer
        // has to be the value the bass gets.
        let mut controls = Controls { lab: [0.5; LAB_COUNT], ..Default::default() };
        let flat = controls.lab_at(0, 0.0);
        assert!(
            (flat - controls.lab_at(0, 1.0)).abs() < 1e-6,
            "an unweighted control is not flat across the compass"
        );

        controls.lab_tilt[0] = 1.0;
        assert!(
            controls.lab_at(0, 0.0) > flat * 3.0,
            "weighting to the bass did not lift the bass"
        );
        assert!(
            (controls.lab_at(0, 1.0) - flat).abs() < 1e-5,
            "the treble is the anchor and must not move"
        );

        // The regression this shape exists for: with the pivot in the middle
        // of the compass every one of these controls was inert around middle
        // C, which is where anyone dialling by ear plays first.
        let middle = 0.45;
        assert!(
            controls.lab_at(0, middle) > flat * 1.8,
            "the weight does nothing near middle C: {} vs {flat}",
            controls.lab_at(0, middle)
        );

        // Off means off at every pitch: a weight must not resurrect a control
        // the user has turned all the way down.
        controls.lab[0] = 0.0;
        assert_eq!(controls.lab_at(0, 0.0), 0.0);
        assert_eq!(controls.lab_at(0, 1.0), 0.0);
    }

    #[test]
    fn the_unweighted_tail_of_the_lab_is_read_as_flat() {
        // Board, sympathy and air are applied after the voices are summed and
        // so have no weight stored. Asking for one must answer "flat".
        let controls = Controls { lab: [0.5; LAB_COUNT], ..Default::default() };
        for i in LAB_TILT_COUNT..LAB_COUNT {
            assert_eq!(controls.lab_at(i, 0.0), controls.lab_at(i, 1.0));
            assert_eq!(controls.lab_at(i, 0.0), controls.lab(i));
        }
    }

    #[test]
    fn a_state_from_an_older_build_keeps_the_controls_it_does_have() {
        // Controls are only ever appended, so a shorter state is an older one.
        // Rejecting it would throw away every setting the user had dialled in.
        let mut piano = Box::new(ConcertGrand::default());
        piano.set_parameter(PARAM_BRIGHTNESS, 0.9);
        let mut state = [0u8; PARAM_COUNT * 4];
        piano.save_state(&mut state).unwrap();

        let mut older = Box::new(ConcertGrand::default());
        assert!(older.load_state(&state[..24]));
        assert_eq!(older.get_parameter(PARAM_BRIGHTNESS), Some(0.9_f32 as f64));
        assert_eq!(older.get_parameter(LAB_TILT_BASE as u32), Some(0.5));

        // A state longer than the model has room for is not an older one.
        assert!(!older.load_state(&[0u8; PARAM_COUNT * 4 + 4]));
    }

    /// Which parameters actually change the sound, and by how much. Not a
    /// pass/fail test: run it to see the whole panel at once.
    ///
    /// Total energy is the wrong measure on its own -- most of these controls
    /// are timbral and move energy between bands without changing how much
    /// there is -- so this reports the largest per-band change as well, and
    /// the change in the attack, which is where several of them live.
    #[test]
    #[ignore]
    fn sweep_every_parameter() {
        /// Energy at one frequency, by Goertzel. Enough to see a band move.
        fn bin(samples: &[f32], hz: f32) -> f32 {
            let w = 2.0 * core::f32::consts::PI * hz / FS as f32;
            let (sine, cosine) = sincosf(w);
            let coefficient = 2.0 * cosine;
            let (mut s1, mut s2) = (0.0f32, 0.0f32);
            for frame in samples.chunks_exact(2) {
                let s0 = frame[0] + coefficient * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            let real = s1 - s2 * cosine;
            let imaginary = s2 * sine;
            ((real * real + imaginary * imaginary) / samples.len() as f32).max(1e-20)
        }

        const BANDS: [f32; 8] = [60.0, 120.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 6000.0];

        fn render_at(index: u32, value: f64, note: u8) -> Vec<f32> {
            let mut piano = prepared();
            assert!(piano.set_parameter(index, value), "param {index} rejected");
            render(&mut piano, 8000, &[note_on(note, 100)])
        }

        let db = |high: f32, low: f32| 10.0 * (high / low).log10();
        println!("{:>5}  {:>28}  {:>28}  {:>28}", "param", "n33", "n60", "n88");
        for index in 0..PARAM_COUNT as u32 {
            let mut line = format!("{index:>5}");
            for note in [33u8, 60, 88] {
                let low = render_at(index, 0.0, note);
                let high = render_at(index, 1.0, note);
                let level = db(energy(&high) + 1e-20, energy(&low) + 1e-20);
                // The attack is the first 30 ms, where the hammer controls act.
                let head = (0.030 * FS) as usize * 2;
                let attack = db(
                    energy(&high[..head]) + 1e-20,
                    energy(&low[..head]) + 1e-20,
                );
                // Weight each band by how much energy is actually in it. The
                // unweighted maximum was worthless: 30 dB of change in a band
                // that holds nothing at all reads as a powerful control and
                // is inaudible.
                let (mut moved, mut weight) = (0.0f32, 0.0f32);
                let (mut centroid_high, mut centroid_low) = (0.0f32, 0.0f32);
                let (mut sum_high, mut sum_low) = (0.0f32, 0.0f32);
                for hz in BANDS {
                    let (a, b) = (bin(&high, hz), bin(&low, hz));
                    let w = a.max(b);
                    moved += w * db(a, b).abs();
                    weight += w;
                    centroid_high += a * hz;
                    centroid_low += b * hz;
                    sum_high += a;
                    sum_low += b;
                }
                let shape = if weight > 0.0 { moved / weight } else { 0.0 };
                let centroid = if sum_high > 0.0 && sum_low > 0.0 {
                    12.0 * ((centroid_high / sum_high) / (centroid_low / sum_low)).log2()
                } else {
                    0.0
                };
                line.push_str(&format!(
                    "  lvl{level:>6.1} atk{attack:>6.1} spec{shape:>5.1} cent{centroid:>6.1}"
                ));
            }
            println!("{line}");
        }
    }

    /// Does the bass's high end come from the strings or from the staging?
    /// The model's 2-8 kHz RISES over two seconds where the real instrument's
    /// collapses by 22-42 dB, and damping partials cannot fix a rise that the
    /// room is feeding.
    #[test]
    #[ignore]
    fn where_the_bass_high_end_comes_from() {
        fn decay(board: f64, sympathy: f64, air: f64, lo: f32, hi: f32) -> f32 {
            let mut piano = prepared();
            piano.set_parameter(20, board);
            piano.set_parameter(21, sympathy);
            piano.set_parameter(22, air);
            let out = render(&mut piano, (FS * 2.4) as usize, &[note_on(21, 125)]);
            // Band energy early (0.08-0.25 s) against late (1.6-2.2 s).
            let win = |a: f32, b: f32| {
                let (i, j) = ((a * FS as f32) as usize * 2, (b * FS as f32) as usize * 2);
                let slice = &out[i..j.min(out.len())];
                let mut total = 0.0f32;
                let mut hz = lo;
                while hz < hi {
                    let w = 2.0 * core::f32::consts::PI * hz / FS as f32;
                    let (sine, cosine) = sincosf(w);
                    let (mut s1, mut s2) = (0.0f32, 0.0f32);
                    for frame in slice.chunks_exact(2) {
                        let s0 = frame[0] + 2.0 * cosine * s1 - s2;
                        s2 = s1;
                        s1 = s0;
                    }
                    let (re, im) = (s1 - s2 * cosine, s2 * sine);
                    total += (re * re + im * im) / slice.len() as f32;
                    hz *= 1.2;
                }
                total.max(1e-20)
            };
            10.0 * (win(1.6, 2.2) / win(0.08, 0.25)).log10()
        }
        for (name, lo, hi) in [("1-2k", 1000.0, 2000.0), ("2-4k", 2000.0, 4000.0)] {
            println!(
                "{name}: full {:+.1} dB | no air {:+.1} | no sympathy {:+.1} | strings only {:+.1}  (real -7.5 / -22.4)",
                decay(0.5, 0.5, 0.5, lo, hi),
                decay(0.5, 0.5, 0.0, lo, hi),
                decay(0.5, 0.0, 0.5, lo, hi),
                decay(0.5, 0.0, 0.0, lo, hi),
            );
        }
    }

    /// What the tension law actually sees: the stretch, and the pitch shift
    /// in cents it asks for. Sizing a nonlinearity by guesswork is how you
    /// ship one that does nothing or one that ruins the tuning.
    #[test]
    #[ignore]
    fn tension_scale() {
        for note in [21u8, 36, 60] {
            let mut piano = prepared();
            render(&mut piano, 64, &[note_on(note, 125)]);
            for block in 0..6 {
                render(&mut piano, (FS * 0.12) as usize, &[]);
                let Some(voice) = piano.voices.iter().find(|v| v.active) else { break };
                let mut stretch = 0.0f32;
                for partial in &voice.partials[..voice.partial_count] {
                    let a = partial.prompt.s + partial.aftersound.s + partial.third.s;
                    stretch += a * a;
                }
                // The rate is applied every TENSION_INTERVAL samples and is a
                // fractional frequency shift per step, so cents follow.
                let rate = voice.tension_gain * (stretch - voice.tension_rest);
                let per_second = rate * (FS as f32 / TENSION_INTERVAL as f32);
                println!(
                    "note {note} t={:.2}s  stretch={stretch:9.3}  rate={rate:.3e}  ~{:+.2} cents of extra advance per second",
                    (block + 1) as f32 * 0.12,
                    1200.0 * per_second / core::f32::consts::TAU / 1.0
                );
            }
            println!();
        }
    }

    /// How close the instrument comes to the desktop's brick wall.
    ///
    /// The desktop applies +6 dB and then hard-clips at full scale. Anything
    /// the plugin sends above 0.5 comes back flat-topped -- and the loudest
    /// thing in a piano note is its attack, which is the one part whose shape
    /// separates a struck string from a plucked one.
    #[test]
    #[ignore]
    fn headroom_against_the_desktop_output() {
        let ceiling = 1.0 / powf(10.0, 6.0 / 20.0);
        for (label, notes) in [
            ("single ff bass", vec![21u8]),
            ("bass octave ff", vec![21, 33]),
            ("five-note chord ff", vec![28, 35, 40, 44, 47]),
            ("ten-note chord ff", vec![28, 33, 40, 45, 47, 52, 57, 59, 64, 69]),
        ] {
            let mut piano = prepared();
            let events: Vec<MidiEvent> = notes.iter().map(|n| note_on(*n, 127)).collect();
            let out = render(&mut piano, (FS * 0.6) as usize, &events);
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let over = out.iter().filter(|s| s.abs() > ceiling).count();
            println!(
                "{label:>20}: peak {peak:.3}  (the desktop clips above {ceiling:.3})  {over} of {} samples clipped = {:.2}%",
                out.len(),
                100.0 * over as f32 / out.len() as f32
            );
        }
    }

    /// How much of a note is generated by the strike, and how much is drawn.
    ///
    /// `simulate_strike` integrates the felt against the string modes and
    /// produces real modal amplitudes and phases. Then that result is
    /// renormalised to the recipe's peak and kept only where it EXCEEDS the
    /// recipe. This counts what survives.
    #[test]
    #[ignore]
    fn how_much_of_the_note_the_strike_actually_sets() {
        for note in [21u8, 36, 48, 60, 72] {
            let mut piano = prepared();
            render(&mut piano, 128, &[note_on(note, 125)]);
            let Some(voice) = piano.voices.iter().find(|v| v.active) else { continue };
            let count = voice.partial_count;
            // SIM_MODES is the ceiling on how many partials the simulation can
            // even reach; everything above it is recipe by construction.
            let reachable = SIM_MODES.min(count);
            println!(
                "note {note:>3}: {count:>3} partials placed, the strike can reach at most {reachable:>3} of them ({:>3}%) -- the rest is the drawn curve, unconditionally",
                100 * reachable / count.max(1)
            );
        }
    }

    /// How long the simulated hammer actually stays on the string, against
    /// the contact time the model asks for. A hammer that leaves in a
    /// fraction of the intended time is a small hard one, whatever its
    /// nominal mass says.
    #[test]
    #[ignore]
    fn how_long_the_hammer_stays() {
        println!("{:>5} {:>4} {:>12} {:>12} {:>8}", "note", "vel", "simulado", "pedido", "razon");
        for note in [21u8, 36, 48, 60] {
            for velocity in [60u8, 127] {
                CONTACT_STEPS.store(0, core::sync::atomic::Ordering::Relaxed);
                let mut piano = prepared();
                render(&mut piano, 128, &[note_on(note, velocity)]);
                let steps = CONTACT_STEPS.load(core::sync::atomic::Ordering::Relaxed);
                if steps == 0 {
                    continue;
                }
                let simulated = steps as f32 * 4.0e-6 * 1000.0;
                let asked = piano.contact_time(note, velocity as f32 / 127.0) * 1000.0;
                println!(
                    "{note:>5} {velocity:>4} {simulated:>9.2} ms {asked:>9.2} ms {:>8.2}",
                    simulated / asked
                );
            }
        }
    }

    /// Does the strike leave the string with VELOCITY or with DISPLACEMENT?
    ///
    /// A finger releases a string from a static shape: position, no velocity.
    /// A hammer hands it momentum: velocity concentrated near the contact,
    /// little displacement. The two sound completely different, and the model
    /// stores both -- `q` and `v/omega` -- so the balance can be read off.
    #[test]
    #[ignore]
    fn strike_or_pluck() {
        println!("{:>5} {:>4} {:>26}", "note", "vel", "energia: posicion / velocidad");
        for note in [21u8, 36, 48, 60] {
            for velocity in [60u8, 127] {
                let mut piano = prepared();
                render(&mut piano, 64, &[note_on(note, velocity)]);
                let Some(voice) = piano.voices.iter().find(|v| v.active) else { continue };
                // At note-on the components hold (s, c) = amplitude * (pq, po),
                // which is the strike's (position, velocity/omega) direction.
                let (mut pos, mut vel) = (0.0f32, 0.0f32);
                for partial in &voice.partials[..voice.partial_count] {
                    let p = &partial.prompt;
                    pos += p.s * p.s;
                    vel += p.c * p.c;
                }
                let total = pos + vel;
                if total <= 0.0 {
                    continue;
                }
                println!(
                    "{note:>5} {velocity:>4} {:>12.0}% {:>12.0}%",
                    100.0 * pos / total,
                    100.0 * vel / total
                );
            }
        }
    }

    #[test]
    fn hard_bass_notes_grow_phantom_partials() {
        // Nonlinear mixing is a large-amplitude effect: a fortissimo A0 must
        // carry components a pianissimo A0 does not have at all.
        let mut soft = prepared();
        render(&mut soft, 64, &[note_on(21, 30)]);
        let soft_count = soft.voices.iter().find(|v| v.active).unwrap().partial_count;
        let mut hard = prepared();
        render(&mut hard, 64, &[note_on(21, 127)]);
        let hard_count = hard.voices.iter().find(|v| v.active).unwrap().partial_count;
        assert!(
            hard_count > soft_count,
            "ff placed {hard_count} partials vs pp {soft_count}"
        );
    }

    #[test]
    fn the_aftersound_beats_hardest_low_in_the_spectrum() {
        // Bridge coupling feeds the slow configurations mostly at low
        // partials: the aftersound-to-prompt ratio must fall with partial
        // number instead of staying a uniform chorus.
        let mut piano = prepared();
        render(&mut piano, 1, &[note_on(45, 100)]);
        let voice = piano.voices.iter().find(|v| v.active).unwrap();
        let ratio = |p: &Partial| {
            (p.aftersound.magnitude_squared() / p.prompt.magnitude_squared().max(1e-20)).sqrt()
        };
        let low = ratio(&voice.partials[0]);
        let high = ratio(&voice.partials[voice.partial_count.saturating_sub(3)]);
        assert!(
            low > high * 1.5,
            "aftersound ratio low {low} vs high {high}"
        );
    }

    /// Not a test: dumps the C4 ff partial ladder to hunt the HF-floor bug.
    #[test]
    #[ignore]
    fn dump_c4_ladder() {
        let mut piano = prepared();
        render(&mut piano, 8, &[note_on(60, 125)]);
        let voice = piano.voices.iter().find(|v| v.active).unwrap();
        let f0 = piano.fundamental[60 - LOW_NOTE as usize];
        for n in 0..voice.partial_count {
            let p = &voice.partials[n];
            let amp = (p.prompt.magnitude_squared()
                + p.aftersound.magnitude_squared()
                + p.third.magnitude_squared())
            .sqrt();
            std::println!("n={} ~f={:.0} amp={:.6}", n + 1, (n + 1) as f32 * f0, amp);
        }
    }

    /// Not a test: renders reference notes to WAV for calibration against
    /// the YDP samples. Run with
    /// `cargo test -p rackforge-concert-grand render_reference -- --ignored`.
    #[test]
    #[ignore]
    fn render_reference_wavs() {
        let out = std::env::var("CG_RENDER_DIR").unwrap_or_else(|_| ".".into());
        // Optional calibration table: 10 lines x 8 space-separated floats.
        let cal = std::env::var("CG_CAL").ok().map(|path| {
            let text = std::fs::read_to_string(path).unwrap();
            let mut table = [[1.0f32; CAL_PARAMS]; 10];
            for (row, line) in table.iter_mut().zip(text.lines()) {
                for (slot, tok) in row.iter_mut().zip(line.split_whitespace()) {
                    *slot = tok.parse().unwrap();
                }
            }
            table
        });
        // Optional parameter overrides, "index=value,index=value". Lets the
        // measurement scripts isolate one stage of the instrument -- render
        // with the room off, or the board alone -- without a rebuild.
        let overrides: Vec<(u32, f64)> = std::env::var("CG_PARAMS")
            .ok()
            .map(|spec| {
                spec.split(',')
                    .filter(|part| !part.is_empty())
                    .map(|part| {
                        let (index, value) = part.split_once('=').expect("index=value");
                        (index.trim().parse().unwrap(), value.trim().parse().unwrap())
                    })
                    .collect()
            })
            .unwrap_or_default();
        // A chromatic sweep, for measurements that average across the compass.
        let chromatic = std::env::var("CG_CHROMATIC").is_ok();
        let notes: Vec<(u8, u8)> = if chromatic {
            (0..30).map(|i| (21 + 3 * i as u8, 110u8)).collect()
        } else if cal.is_some() {
            (0..30).map(|i| (21 + 3 * i as u8, 125u8)).collect()
        } else {
            vec![(21u8, 123u8), (36, 120), (48, 125), (60, 125), (69, 125), (30, 85), (30, 124), (48, 105), (60, 70)]
        };
        for (note, velocity) in notes {
            let mut piano = Box::new(ConcertGrand::default());
            if let Some(table) = cal {
                piano.cal = table;
            }
            for (index, value) in &overrides {
                assert!(piano.set_parameter(*index, *value), "param {index} rejected");
            }
            assert!(piano.prepare(44_100.0, 512, 0, 2));
            let frames = 44_100 * 5;
            let mut output = vec![0.0f32; frames * 2];
            piano.process(
                &[],
                &mut output,
                &[note_on(note, velocity)],
                &[],
                frames as u32,
                0,
                2,
            );
            // Mono mix, 16-bit PCM WAV.
            let mono: Vec<i16> = output
                .chunks_exact(2)
                .map(|f| (((f[0] + f[1]) * 0.5).clamp(-1.0, 1.0) * 32_767.0) as i16)
                .collect();
            let mut bytes = Vec::with_capacity(44 + mono.len() * 2);
            let data_len = (mono.len() * 2) as u32;
            bytes.extend(b"RIFF");
            bytes.extend((36 + data_len).to_le_bytes());
            bytes.extend(b"WAVEfmt ");
            bytes.extend(16u32.to_le_bytes());
            bytes.extend(1u16.to_le_bytes());
            bytes.extend(1u16.to_le_bytes());
            bytes.extend(44_100u32.to_le_bytes());
            bytes.extend((44_100u32 * 2).to_le_bytes());
            bytes.extend(2u16.to_le_bytes());
            bytes.extend(16u16.to_le_bytes());
            bytes.extend(b"data");
            bytes.extend(data_len.to_le_bytes());
            for sample in mono {
                bytes.extend(sample.to_le_bytes());
            }
            std::fs::write(format!("{out}/model{note:03}v{velocity}.wav"), bytes).unwrap();
        }
    }

    #[test]
    fn the_unison_dephases_and_traps_its_energy() {
        // Weinreich's signature, now simulated: the hammer leaves the
        // strings in phase (coherence ~1); the detune spreads them, the
        // bridge drains only the coherent part, and the ratio of radiated
        // (|sum|^2) to stored (sum of |z|^2) energy must fall over the
        // sustain while the stored energy itself survives.
        //
        // Read at ONE instant this proves nothing, and for a while it was
        // read that way. A detuned unison beats: the strings dephase and
        // then rephase, over and over, so the coherence cycles rather than
        // decaying. Traced every 100 ms it runs 0.70, 0.28, 0.02, ... 0.71,
        // 0.68, ... 0.82, 0.57 -- and whether a single late reading looks
        // like dephasing depends only on where in that cycle it lands. This
        // test used to sample 2.5 s in and pass on a beat minimum; a change
        // to the decay curve moved the beat, not the physics, and it failed.
        //
        // So it compares the strike against the whole sustain instead. Right
        // after the hammer the strings are together and the coherence is
        // high; averaged over seconds it must sit well below that, and it
        // cannot be faked by a beat phase because it spans many beats.
        let mut piano = prepared();
        render(&mut piano, 64, &[note_on(60, 100)]);
        let coherence = |piano: &ConcertGrand| {
            let voice = piano.voices.iter().find(|v| v.active).unwrap();
            // A low partial, because this test is about dephasing and needs a
            // partial that is still alive to watch. Radiation damping now
            // kills C4's eighth by two seconds, which is what the real
            // instrument does and what this test used to sit on.
            let p = &voice.partials[3.min(voice.partial_count - 1)];
            let sum_s = p.prompt.s + p.aftersound.s + p.third.s;
            let sum_c = p.prompt.c + p.aftersound.c + p.third.c;
            let radiated = sum_s * sum_s + sum_c * sum_c;
            let stored = p.prompt.magnitude_squared()
                + p.aftersound.magnitude_squared()
                + p.third.magnitude_squared();
            (radiated / (3.0 * stored).max(1e-24), stored)
        };
        let (struck, _) = coherence(&piano);
        assert!(
            struck > 0.7,
            "the hammer must leave the strings in phase: {struck}"
        );

        // Many beats' worth, so no single phase can carry the result.
        let mut total = 0.0;
        let mut samples = 0;
        let mut last_stored = 0.0;
        for _ in 0..30 {
            render(&mut piano, (FS * 0.1) as usize, &[]);
            let (c, stored) = coherence(&piano);
            total += c;
            samples += 1;
            last_stored = stored;
        }
        let sustained = total / samples as f32;
        assert!(last_stored > 0.0, "the strings died entirely");
        assert!(
            sustained < struck * 0.7,
            "no dephasing: struck {struck} -> sustained {sustained}"
        );
    }

    #[test]
    fn the_staging_leaves_a_tail_that_dies() {
        // Staging used to be pinned at zero and this test held it there, on
        // the reasoning that the naked instrument should convince first. It
        // never could: a piano with no undamped strings, no lid and no room
        // is a direct-injected tone, which is what an electric keyboard is.
        // What the test has to hold now is the other failure — that the tail
        // is a tail and not a drone.
        let mut piano = prepared();
        let chord: Vec<MidiEvent> =
            [48u8, 55, 64].iter().map(|n| note_on(*n, 120)).collect();
        render(&mut piano, (FS * 0.3) as usize, &chord);
        let offs: Vec<MidiEvent> = [48u8, 55, 64].iter().map(|n| note_off(*n)).collect();
        render(&mut piano, 64, &offs);
        // Just after the dampers land the room is still speaking.
        render(&mut piano, (FS * 0.15) as usize, &[]);
        let early = energy(&render(&mut piano, 1600, &[]));
        assert!(early > 0.0, "the dampers took the room with them");
        // Two seconds on it has to be gone, or the instrument never stops.
        render(&mut piano, (FS * 2.0) as usize, &[]);
        let late = energy(&render(&mut piano, 1600, &[]));
        assert!(
            late < early * 1e-3,
            "staging drones after damping: {early} -> {late}"
        );
    }

    #[test]
    fn sympathy_and_air_can_be_turned_off() {
        // Both staging knobs have to reach silence, so the dry instrument is
        // still available to measure against.
        let mut piano = prepared();
        for index in [15u32, 16] {
            assert!(piano.set_parameter(6 + index, 0.0));
        }
        let wet = {
            let mut on = prepared();
            render(&mut on, (FS * 0.3) as usize, &[note_on(60, 110)]);
            render(&mut on, 64, &[note_off(60)]);
            render(&mut on, (FS * 0.2) as usize, &[]);
            energy(&render(&mut on, 1600, &[]))
        };
        render(&mut piano, (FS * 0.3) as usize, &[note_on(60, 110)]);
        render(&mut piano, 64, &[note_off(60)]);
        render(&mut piano, (FS * 0.2) as usize, &[]);
        let dry = energy(&render(&mut piano, 1600, &[]));
        assert!(dry < wet, "staging at zero is not drier: {dry} vs {wet}");
    }
    #[test]
    fn the_pedal_wraps_a_note_in_a_sympathetic_halo() {
        // With the dampers up, a struck note must carry more live partials
        // than the same strike without the pedal: the shadow voice is there.
        let mut dry = prepared();
        render(&mut dry, 64, &[note_on(60, 100)]);
        let without = dry.active_partials;
        let mut pedalled = prepared();
        let pedal = MidiEvent { frame: 0, data: [0xb0, 64, 127], length: 3 };
        render(&mut pedalled, 64, &[pedal, note_on(60, 100)]);
        assert!(
            pedalled.active_partials > without,
            "pedalled {} vs dry {without}",
            pedalled.active_partials
        );
        // And lifting the pedal releases the halo with everything else.
        let lift = MidiEvent { frame: 0, data: [0xb0, 64, 0], length: 3 };
        render(&mut pedalled, 16, &[note_off(60), lift]);
        render(&mut pedalled, (FS * 1.0) as usize, &[]);
        let late = energy(&render(&mut pedalled, 512, &[]));
        assert!(late < 1e-6, "halo survived the pedal lift: {late}");
    }

    #[test]
    fn duplex_segments_ignore_the_damper() {
        // The back segments have no dampers: releasing the key must speed up
        // every string component's decay while leaving the duplex rotation
        // untouched.
        let mut piano = prepared();
        render(&mut piano, 256, &[note_on(84, 120)]);
        let decay_squared = |c: &Component| c.rc * c.rc + c.rs * c.rs;
        let voice = piano.voices.iter().find(|v| v.active).unwrap();
        assert!(
            decay_squared(&voice.duplex[0]) > 0.0,
            "treble note grew no duplex"
        );
        let string_before = decay_squared(&voice.partials[0].prompt);
        let duplex_before = decay_squared(&voice.duplex[0]);
        render(&mut piano, 8, &[note_off(84)]);
        let voice = piano.voices.iter().find(|v| v.active).unwrap();
        let string_after = decay_squared(&voice.partials[0].prompt);
        let duplex_after = decay_squared(&voice.duplex[0]);
        assert!(
            string_after < string_before * 0.9999,
            "the damper did not touch the strings: {string_before} -> {string_after}"
        );
        assert!(
            (duplex_after - duplex_before).abs() < duplex_before * 1e-5,
            "the damper touched the duplex: {duplex_before} -> {duplex_after}"
        );
    }
    #[test]
    fn una_corda_softens_and_darkens() {
        let strike = |soft: bool| {
            let mut piano = prepared();
            if soft {
                let cc = MidiEvent { frame: 0, data: [0xb0, 67, 127], length: 3 };
                render(&mut piano, 8, &[cc]);
            }
            energy(&render(&mut piano, 2048, &[note_on(60, 110)]))
        };
        let plain = strike(false);
        let soft = strike(true);
        assert!(soft < plain * 0.8, "una corda {soft} vs plain {plain}");
    }

    /// Not a test: measures what a strike costs the audio callback.
    #[test]
    #[ignore]
    fn measure_strike_cost() {
        let mut piano = Box::new(ConcertGrand::default());
        assert!(piano.prepare(48_000.0, 512, 0, 2));
        let mut output = vec![0.0f32; 512 * 2];
        // One buffer at 48 kHz / 512 frames is 10.67 ms of budget.
        for (label, notes) in [("single", 1usize), ("five-note chord", 5), ("ten-note chord", 10)] {
            let events: Vec<MidiEvent> = (0..notes)
                .map(|i| MidiEvent { frame: 0, data: [0x90, 40 + 4 * i as u8, 110], length: 3 })
                .collect();
            let start = std::time::Instant::now();
            const ROUNDS: u32 = 50;
            for _ in 0..ROUNDS {
                piano.reset();
                piano.process(&[], &mut output, &events, &[], 512, 0, 2);
            }
            let per = start.elapsed().as_secs_f64() * 1000.0 / ROUNDS as f64;
            std::println!("{label}: {per:.2} ms per buffer ({:.0}% of budget)", per / 10.67 * 100.0);
        }
        // Steady state: what a held chord costs once the strikes are over.
        // This is what fast playing accumulates, and what the callback pays
        // on every buffer until the voices die.
        for held in [5usize, 10, 20] {
            piano.reset();
            let events: Vec<MidiEvent> = (0..held)
                .map(|i| MidiEvent { frame: 0, data: [0x90, 33 + 3 * i as u8, 115], length: 3 })
                .collect();
            let pedal = MidiEvent { frame: 0, data: [0xb0, 64, 127], length: 3 };
            piano.process(&[], &mut output, &[pedal], &[], 512, 0, 2);
            for chunk in events.chunks(3) {
                piano.process(&[], &mut output, chunk, &[], 512, 0, 2);
            }
            let start = std::time::Instant::now();
            const ROUNDS: u32 = 100;
            for _ in 0..ROUNDS {
                piano.process(&[], &mut output, &[], &[], 512, 0, 2);
            }
            let per = start.elapsed().as_secs_f64() * 1000.0 / ROUNDS as f64;
            let live: usize = piano.voices.iter().filter(|v| v.active).count();
            std::println!(
                "{held} pedalled notes ({live} voices, {} partials): {per:.2} ms ({:.0}% of budget)",
                piano.active_partials, per / 10.67 * 100.0
            );
        }
    }

    /// Hammers the model the way a fast player does: dense chords, repeats,
    /// pedal, the whole compass, every velocity. A panic here is a trap in
    /// the packaged wasm, which takes the audio stream down with it.
    /// Not a test: how long sound persists after the key is released, which
    /// is what "the notes hang behind" would show up as.
    #[test]
    #[ignore]
    fn measure_release_tail() {
        for (label, note, pedal) in [
            ("bass C2", 36u8, false),
            ("mid C4", 60, false),
            ("treble C6", 84, false),
            ("treble C6 + pedal", 84, true),
            ("glissando 24 treble notes", 0, false),
        ] {
            let mut piano = Box::new(ConcertGrand::default());
            assert!(piano.prepare(48_000.0, 512, 0, 2));
            let mut out = vec![0.0f32; 512 * 2];
            if pedal {
                let cc = [MidiEvent { frame: 0, data: [0xb0, 64, 127], length: 3 }];
                piano.process(&[], &mut out, &cc, &[], 512, 0, 2);
            }
            if note == 0 {
                for n in 0..24u8 {
                    let on = [MidiEvent { frame: 0, data: [0x90, 72 + n % 24, 110], length: 3 }];
                    piano.process(&[], &mut out, &on, &[], 512, 0, 2);
                }
                for n in 0..24u8 {
                    let off = [MidiEvent { frame: 0, data: [0x80, 72 + n % 24, 0], length: 3 }];
                    piano.process(&[], &mut out, &off, &[], 512, 0, 2);
                }
            } else {
                let on = [MidiEvent { frame: 0, data: [0x90, note, 110], length: 3 }];
                piano.process(&[], &mut out, &on, &[], 512, 0, 2);
                for _ in 0..40 {
                    piano.process(&[], &mut out, &[], &[], 512, 0, 2);
                }
                let off = [MidiEvent { frame: 0, data: [0x80, note, 0], length: 3 }];
                piano.process(&[], &mut out, &off, &[], 512, 0, 2);
            }
            let loud = |o: &[f32]| o.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let mut peak_at_release = 0.0f32;
            let mut ms_to_silence = None;
            for block in 0..280 {
                piano.process(&[], &mut out, &[], &[], 512, 0, 2);
                let level = loud(&out);
                if block == 0 {
                    peak_at_release = level.max(1e-9);
                }
                if ms_to_silence.is_none() && level < peak_at_release * 0.02 {
                    ms_to_silence = Some(block as f32 * 512.0 / 48.0);
                }
            }
            let voices = piano.voices.iter().filter(|v| v.active).count();
            std::println!(
                "{label}: -34 dB after {:?} ms, {voices} voices still live",
                ms_to_silence.map(|v| v as u32)
            );
        }
    }

    #[test]
    fn survives_dense_fast_playing() {
        // At the host's rate, not the tests' 16 kHz: a bass note has far more
        // partials under nyquist there, which is what fills the arrays.
        let mut piano = Box::new(ConcertGrand::default());
        assert!(piano.prepare(48_000.0, 512, 0, 2));
        let mut seed = 0x1234_5678u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 16) as usize
        };
        let mut output = vec![0.0f32; 512 * 2];
        for round in 0..4000 {
            let mut events = Vec::new();
            if round % 37 == 0 {
                events.push(MidiEvent { frame: 0, data: [0xb0, 64, 127], length: 3 });
            }
            if round % 53 == 0 {
                events.push(MidiEvent { frame: 0, data: [0xb0, 64, 0], length: 3 });
            }
            for _ in 0..(next() % 12) {
                let note = (21 + next() % 88) as u8;
                let velocity = (1 + next() % 127) as u8;
                let on = next() % 4 != 0;
                events.push(MidiEvent {
                    frame: (next() % 128) as u32,
                    data: [if on { 0x90 } else { 0x80 }, note, velocity],
                    length: 3,
                });
            }
            events.sort_by_key(|e| e.frame);
            piano.process(&[], &mut output, &events, &[], 128, 0, 2);
            assert!(output.iter().all(|s| s.is_finite()), "non-finite output at round {round}");
        }
    }

    #[test]
    fn a_bass_note_blooms_instead_of_switching_on() {
        // Measured on the partial itself, not on rendered energy: the hammer
        // chiff is immediate by design and swamped the old energy-window
        // version of this test every time the chiff level moved.
        let mut piano = prepared();
        render(&mut piano, 1, &[note_on(24, 100)]);
        // The partial's actual phasor: all four components summed. The
        // bloom starts as the exact negative of the other three, so a
        // struck partial leaves here at ~zero and swells into the tone.
        let magnitude = |piano: &ConcertGrand| {
            let voice = piano.voices.iter().find(|v| v.active).unwrap();
            let p = &voice.partials[0];
            let s = p.prompt.s + p.aftersound.s + p.third.s + p.bloom.s;
            let c = p.prompt.c + p.aftersound.c + p.third.c + p.bloom.c;
            (s * s + c * c).sqrt()
        };
        let at_strike = magnitude(&piano);
        render(&mut piano, (FS * 0.06) as usize, &[]);
        let developed = magnitude(&piano);
        assert!(
            developed > at_strike * 2.0,
            "the partial did not swell in: {at_strike} -> {developed}"
        );
    }
    #[test]
    fn the_lowest_notes_speak_through_upper_partials_not_the_fundamental() {
        // Measured C1 spectra put the strongest partial around n=4–6 and the
        // fundamental tens of dB down: the board cannot radiate below its
        // first mode. The model's initial amplitudes must show the same shape.
        let mut piano = prepared();
        render(&mut piano, 1, &[note_on(24, 100)]);
        let voice = piano.voices.iter().find(|v| v.active).unwrap();
        let amp = |i: usize| voice.partials[i].prompt.magnitude_squared();
        // Scan the transverse ladder only: nonlinear extras and the
        // mechanism thump are appended after it.
        let ladder = voice.partial_count.min(40);
        let strongest = (0..ladder)
            .max_by(|a, b| amp(*a).total_cmp(&amp(*b)))
            .unwrap();
        assert!(
            (1..=26).contains(&strongest),
            "strongest partial is n={}",
            strongest + 1
        );
        assert!(
            amp(0) < amp(strongest) * 0.25,
            "fundamental {} vs strongest {}",
            amp(0),
            amp(strongest)
        );
    }

    #[test]
    fn the_board_response_is_fixed_ragged_and_bounded() {
        // Every partial of every note samples the same curve: the response
        // at a frequency is one value, its level swing stays within ±6 dB,
        // its decay pull within a factor ~1.6, and it actually varies.
        let mut minimum = f32::MAX;
        let mut maximum = f32::MIN;
        for step in 0..400 {
            let frequency = 30.0 * powf(2.0, step as f32 / 50.0).min(12_000.0);
            let (amplitude, decay) = ConcertGrand::board_response(frequency);
            let again = ConcertGrand::board_response(frequency);
            assert_eq!((amplitude, decay), again);
            assert!((0.3..3.4).contains(&amplitude), "level {amplitude} at {frequency} Hz");
            assert!((0.6..1.8).contains(&decay), "decay {decay} at {frequency} Hz");
            minimum = minimum.min(amplitude);
            maximum = maximum.max(amplitude);
        }
        assert!(maximum > minimum * 2.0, "the board is flat: {minimum}..{maximum}");
    }

    #[test]
    fn releasing_a_ringing_string_thuds() {
        // The damper lands on a moving string: release must re-arm the noise
        // burst, scaled by the energy still in the voice.
        let mut piano = prepared();
        render(&mut piano, 2048, &[note_on(48, 110)]);
        // Let the attack chiff die out first, then release.
        render(&mut piano, (FS * 0.5) as usize, &[]);
        let before = piano.voices.iter().find(|v| v.active).unwrap().noise_amp;
        render(&mut piano, 8, &[note_off(48)]);
        let after = piano.voices.iter().find(|v| v.active).unwrap().noise_amp;
        assert!(
            after > before && after > 1e-4,
            "release thud {after} (was {before})"
        );
    }

    #[test]
    fn the_hammer_width_separates_a_strike_from_a_pluck() {
        // The sinc window of the distributed contact must attenuate high
        // partials well below what a point excitation would give: at C4
        // (width ~5%), partial 20 sits past the window's first null region.
        let width = ConcertGrand::hammer_width(60);
        let window = |n: f32| {
            let argument = n * width;
            expf(-1.2 * argument * argument)
        };
        assert!(window(2.0) > 0.9, "low partials pass ({})", window(2.0));
        assert!(window(40.0) < 0.35, "partial 40 unattenuated ({})", window(40.0));
    }

    #[test]
    fn body_modes_have_unit_order_gain_at_every_frequency() {
        // Drive each mode with a unit sine at its own resonance: the settled
        // output must stay O(1) for the lowest and highest modes alike. The
        // peak gain of a two-pole is ≈ 1/((1-r)·2·sin ω0); normalising by
        // (1-r) alone once left the 62 Hz mode ~60× hotter than the 818 Hz
        // one — an accidental bass boost, not a soundboard.
        let sample_rate = FS as f32;
        for frequency in [BOARD_BOTTOM_HZ, 1000.0, BOARD_TOP_HZ] {
            let mut mode =
                BodyMode::tune(frequency, board_t60(frequency), 0.5, sample_rate);
            let omega = core::f32::consts::TAU * frequency / sample_rate;
            let mut peak = 0.0_f32;
            for n in 0..(sample_rate as usize) {
                let y = mode.tick(sincosf(omega * n as f32).0);
                if n > sample_rate as usize / 2 {
                    peak = peak.max(y.abs());
                }
            }
            assert!(
                (0.2..3.0).contains(&peak),
                "mode at {frequency} Hz settles at gain {peak}"
            );
        }
    }

    #[test]
    fn the_body_rings_briefly_after_the_strings_are_damped() {
        // Strike hard and damp at once: the strings die in tens of
        // milliseconds, but the soundboard modes keep a short wooden tail.
        let mut piano = prepared();
        render(&mut piano, 256, &[note_on(48, 127)]);
        render(&mut piano, 64, &[note_off(48)]);
        // 100 ms on: strings are ~gone, the body should not be silent yet.
        render(&mut piano, (FS * 0.1) as usize, &[]);
        let tail = energy(&render(&mut piano, 800, &[]));
        assert!(tail > 0.0, "no body tail after damping");
        // And it is a tail, not a drone: another 400 ms and it has faded
        // by an order of magnitude.
        render(&mut piano, (FS * 0.4) as usize, &[]);
        let later = energy(&render(&mut piano, 800, &[]));
        assert!(
            later < tail * 0.5,
            "body tail {tail} did not fade (later {later})"
        );
    }

    #[test]
    fn dead_partials_are_retired_from_the_budget() {
        let mut piano = prepared();
        render(&mut piano, 64, &[note_on(96, 90)]);
        let at_start = piano.active_partials;
        assert!(at_start > 0);
        render(&mut piano, 64, &[note_off(96)]);
        // Damped treble partials die in tens of milliseconds; a second later
        // the budget must have been refunded.
        render(&mut piano, FS as usize, &[]);
        assert!(
            piano.active_partials < at_start,
            "budget still {at_start} after the note died"
        );
    }
}
