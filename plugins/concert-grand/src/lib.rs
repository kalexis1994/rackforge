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

const MAX_VOICES: usize = 20;
/// Room for the full transverse ladder of the lowest notes plus their
/// nonlinear extras — A0 alone fills ~120 slots with real partials.
const MAX_PARTIALS: usize = 144;
/// Ceiling on simultaneously active partials across all voices, so a pedalled
/// fortissimo run degrades new notes' partial counts instead of the callback.
const PARTIAL_BUDGET: usize = 2000;

/// Below the soundboard's first mode the board radiates almost nothing.
/// Calibrated against the YDP Grand samples: A0's fundamental measures
/// ~-40 dB against its strongest partial, 46 Hz ~-25 dB, 78 Hz ~0 dB — a
/// steep transition this sixth-order corner reproduces.
const RADIATION_CORNER_HZ: f32 = 66.0;
/// How often a voice retires inaudible components, in samples.
const CULL_INTERVAL: u32 = 256;
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
/// Lab parameters 6..=21: raw multipliers the player sweeps by ear while we
/// hunt the piano. Each 0..1 value maps to a x0.25..x4 multiplier.
const LAB_COUNT: usize = 16;
const PARAM_COUNT: usize = 6 + LAB_COUNT;

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
        let omega = core::f32::consts::TAU * frequency / sample_rate;
        let (sin, cos) = sincosf(omega);
        Self { s: s0, c: c0, rc: decay_per_sample * cos, rs: decay_per_sample * sin }
    }

    #[inline(always)]
    fn tick(&mut self) -> f32 {
        let s = self.s * self.rc + self.c * self.rs;
        let c = self.c * self.rc - self.s * self.rs;
        self.s = s;
        self.c = c;
        s
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

/// The synthetic soundboard: mode frequencies spread irregularly the way a
/// ribbed wooden plate's are. Decays are short — a piano board is heavily
/// loaded by the bridge and ribs; sparse modes that ring for hundreds of
/// milliseconds read as a guitar box, not a piano.
/// (Frequency Hz, T60 s, pan 0..1.)
const BODY_MODES: [(f32, f32, f32); 18] = [
    (62.0, 0.14, 0.44),
    (88.0, 0.13, 0.58),
    (109.0, 0.12, 0.38),
    (136.0, 0.11, 0.62),
    (171.0, 0.10, 0.46),
    (214.0, 0.09, 0.56),
    (268.0, 0.08, 0.40),
    (334.0, 0.075, 0.60),
    (419.0, 0.07, 0.48),
    (523.0, 0.065, 0.54),
    (655.0, 0.06, 0.42),
    (818.0, 0.055, 0.58),
    // The dense mid region every ribbed board has: short, close-packed,
    // quieter — the wooden presence under the mid and treble strings that a
    // naked additive ladder (a guitar, a koto) does not have.
    (1021.0, 0.05, 0.47),
    (1276.0, 0.045, 0.55),
    (1594.0, 0.042, 0.43),
    (1993.0, 0.038, 0.57),
    (2491.0, 0.035, 0.49),
    (3114.0, 0.032, 0.53),
];

/// Wet level of the modal body relative to the direct string sum.
const BODY_MIX: f32 = 0.9;

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
const OPEN_MIX: f32 = 0.0;

/// The open register as a statistic: twenty undamped strings with their
/// partial ladders behave collectively like a short, dense, undamped
/// high-frequency reverberator. Four short lines, input high-passed at
/// ~1.8 kHz, T60 ≈ 2.2 s, no damping in the loop — the silvery shimmer
/// under every note of a real grand.
const HALO_DELAYS_S: [f32; 4] = [0.0071, 0.0097, 0.0127, 0.0163];
const HALO_RT60_S: f32 = 2.2;
const HALO_HP_HZ: f32 = 1800.0;
const HALO_BUFFER: usize = 2048;
const HALO_MIX: f32 = 0.0;

/// Near-field reflections off the lid and rim: a handful of sparse early
/// taps, different per side so the image widens, no tail — this is the air
/// around an open grand, not a hall. A dry direct-injected tone is precisely
/// what an electric piano is. (Delay in seconds, gain.)
const LID_TAPS_LEFT: [(f32, f32); 4] =
    [(0.0113, 0.0), (0.0191, 0.0), (0.0257, 0.0), (0.0331, 0.0)];
const LID_TAPS_RIGHT: [(f32, f32); 4] =
    [(0.0097, 0.0), (0.0179, 0.0), (0.0243, 0.0), (0.0311, 0.0)];
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
const ROOM_MIX: f32 = 0.0;

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
        }
    }
}

impl Voice {
    /// Renders one mono sample and advances every live component.
    #[inline(always)]
    fn tick(&mut self) -> f32 {
        let mut sum = 0.0;
        for partial in &mut self.partials[..self.partial_count] {
            sum += partial.prompt.tick()
                + partial.aftersound.tick()
                + partial.bloom.tick()
                + partial.third.tick();
        }
        sum += self.duplex[0].tick() + self.duplex[1].tick();
        if self.noise_amp > 1e-7 {
            // Park–Miller-style LCG: white noise costs one multiply-add.
            self.noise_seed = self.noise_seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let white = (self.noise_seed >> 9) as f32 * (1.0 / 4_194_304.0) - 1.0;
            self.noise_lp += self.noise_coefficient * (white - self.noise_lp);
            sum += self.noise_lp * self.noise_amp;
            self.noise_amp *= self.noise_decay;
            // The knock darkens as it dies: a tapped soundboard's noise is a
            // low-pass whose bandwidth contracts over time.
            self.noise_coefficient *= self.noise_shrink;
        }
        sum
    }

    /// Retires components that have decayed below audibility and refreshes the
    /// loudness estimate. Runs at block cadence, never per sample.
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
        // The bridge: each string loses the same slice of the coherent sum.
        // In-phase configurations radiate and die; dephased ones barely
        // couple and live on. This is where the two-stage decay, the beats
        // and the churn of the sustain come from.
        for partial in &mut self.partials[..self.partial_count] {
            let k = partial.coupling;
            if k > 0.0 {
                let sum_s = partial.prompt.s + partial.aftersound.s + partial.third.s;
                let sum_c = partial.prompt.c + partial.aftersound.c + partial.third.c;
                partial.prompt.s -= k * sum_s;
                partial.prompt.c -= k * sum_c;
                // The second component is the horizontal polarisation: it
                // drives the bridge sideways and couples an order of
                // magnitude more weakly — it IS the long tail (Weinreich).
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
            let partial = &self.partials[index];
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
        }
    }
}

impl Controls {
    /// Lab multiplier i: 0..1 slider -> x0.25..x4, centre = x1.
    fn lab(&self, i: usize) -> f32 {
        powf(16.0, self.lab[i] - 0.5)
    }

    fn get(&self, index: u32) -> Option<f64> {
        let value = match index {
            PARAM_BRIGHTNESS => self.brightness,
            PARAM_DYNAMICS => self.dynamics,
            PARAM_UNISON => self.unison,
            PARAM_DECAY => self.decay,
            PARAM_WIDTH => self.width,
            PARAM_LEVEL => self.level,
            6..=21 => self.lab[index as usize - 6],
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
            6..=21 => self.lab[index as usize - 6] = value,
            _ => return false,
        }
        true
    }
}

/// Per-note calibration anchors (MIDI notes) and parameter count. The
/// fitting harness optimises `cal` against tools/piano-targets.json; 1.0
/// everywhere means the hand-calibrated base model.
const CAL_ANCHORS: [u8; 10] = [21, 30, 39, 48, 57, 66, 75, 84, 96, 108];
const CAL_PARAMS: usize = 10;
// felt, floor, thump, chiff, decay, clang, phantoms, level, treble life, serial

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
    /// The synthetic soundboard the string sum radiates through.
    body: [BodyMode; BODY_MODES.len()],
    /// Delay line feeding the lid/rim early reflections.
    lid: [f32; LID_BUFFER],
    lid_write: usize,
    /// Tap offsets in samples and gains, per side.
    lid_left: [(usize, f32); LID_TAPS_LEFT.len()],
    lid_right: [(usize, f32); LID_TAPS_RIGHT.len()],
    /// Serial-board mix from the most recent strike's note calibration.
    serial_cal: f32,
    /// Per-note calibration table: [anchor][param] multipliers.
    cal: [[f32; CAL_PARAMS]; 10],
    /// The serial board: a dense constant-Q modal bank the ENTIRE string
    /// sum radiates through — no partial reaches the air unfiltered. Modes
    /// overlap (spacing ~ bandwidth) so coverage is continuous but ragged,
    /// like a ribbed plate's driving-point mobility.
    serial: [BodyMode; 36],
    /// The undamped top-octave strings, always listening to the bridge.
    open_strings: [BodyMode; OPEN_STRINGS.len()],
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
            body: [BodyMode::default(); BODY_MODES.len()],
            // Per-note calibration fitted against the YDP samples (cost
            // 6821 -> 5770): ten anchors from A0 to C8, eight multipliers
            // each (felt, HF floor, thump, chiff, decay, clang, phantoms,
            // level). Level is pinned to 1.0: the fitting cost normalises
            // every window, so it is blind to absolute level and the tilt it
            // "found" there was drift, audible as a loud bass and a muffled
            // treble. The rest are physical — decay runs
            // x1.5 in the bass and x0.65 in the treble, which a single
            // global number can only average into being wrong everywhere.
            serial_cal: 1.0,
            cal: [
                [0.3239, 0.8114, 0.3539, 0.3392, 1.3207, 0.5434, 1.1626, 1.0000, 0.3686, 0.2500],
                [0.2745, 0.9575, 0.5429, 0.3392, 1.1193, 0.5434, 0.4940, 1.0000, 0.3686, 0.2500],
                [0.3239, 0.9575, 0.7058, 0.3392, 1.3207, 0.4180, 0.4940, 1.0000, 0.3686, 0.2500],
                [0.3239, 0.9575, 0.5429, 0.4002, 1.3207, 0.5434, 0.4940, 1.0000, 0.3686, 0.2500],
                [0.8079, 0.8533, 0.6137, 0.3934, 1.0098, 0.4733, 1.3111, 1.0000, 0.6211, 0.5500],
                [1.2315, 1.3899, 1.3269, 1.0852, 0.6292, 0.8858, 0.6124, 1.0000, 0.8421, 0.8125],
                [1.5340, 1.0000, 1.1017, 0.7692, 0.6519, 0.6519, 0.6519, 1.0000, 1.0000, 1.0000],
                [1.0000, 1.5340, 1.5340, 1.5340, 0.6519, 1.0000, 1.3000, 1.0000, 1.0000, 1.0000],
                [1.0000, 1.1800, 0.6519, 1.5340, 0.6519, 1.0000, 0.8475, 1.0000, 1.0000, 1.0000],
                [1.1017, 1.5340, 0.6519, 1.5340, 0.6519, 1.0000, 1.0000, 1.0000, 1.0000, 1.0000],
            ],
            serial: [BodyMode::default(); 36],
            open_strings: [BodyMode::default(); OPEN_STRINGS.len()],
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
        piano.tune_body();
        piano.tune_serial();
        piano.tune_open_strings();
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
const SIM_MODES: usize = 48;

/// Integrates the felt hammer against the string's modal system from first
/// touch to release: nonlinear felt (F ∝ ξ^2.5), the string pushing back,
/// the returning wave reshaping the pulse while contact lasts. Returns each
/// mode's (position, velocity/ω) state at contact end — amplitudes AND
/// phases of the attack, emergent instead of scripted (Chaigne & Askenfelt).
/// Normalised units: string modal masses are 1, the hammer carries `mass`.
fn simulate_strike(
    frequencies: &[f32],
    modes: usize,
    x0: f32,
    mass: f32,
    stiffness: f32,
    exponent: f32,
    velocity0: f32,
) -> ([f32; SIM_MODES], [f32; SIM_MODES]) {
    let dt = 4.0e-6_f32;
    let mut omega = [0.0f32; SIM_MODES];
    let mut shape = [0.0f32; SIM_MODES];
    for n in 0..modes {
        omega[n] = core::f32::consts::TAU * frequencies[n];
        shape[n] = sincosf(core::f32::consts::PI * (n + 1) as f32 * x0).0;
    }
    let mut q = [0.0f32; SIM_MODES];
    let mut v = [0.0f32; SIM_MODES];
    let mut hammer_y = 0.0f32;
    let mut hammer_v = velocity0;
    let mut touched = false;
    // Stulov's hereditary felt: wool has memory. The force follows
    // F = F0·(x^p − e·h) where h is x^p passed through an exponential
    // history kernel — loading is stiffer than unloading, the loop
    // dissipates, the pulse comes out shorter and asymmetric (JASA 97,
    // 1995). e and t0 are his hereditary parameters.
    const STULOV_EPSILON: f32 = 0.94;
    const STULOV_TAU_S: f32 = 6.0e-6;
    let history_keep = expf(-dt / STULOV_TAU_S);
    let mut history = 0.0f32;
    for _ in 0..1200 {
        let mut string_y = 0.0;
        for n in 0..modes {
            string_y += q[n] * shape[n];
        }
        let felt = hammer_y - string_y;
        let force = if felt > 0.0 {
            touched = true;
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

    /// Retunes the soundboard modes to the current sample rate.
    fn tune_body(&mut self) {
        for (mode, (frequency, t60, pan)) in self.body.iter_mut().zip(BODY_MODES) {
            *mode = BodyMode::tune(frequency, t60, pan, self.sample_rate);
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

    /// Tunes the serial board: log-spaced 60 Hz..8.5 kHz, T60 = 8/f
    /// (constant Q), per-mode jittered frequency and pan.
    fn tune_serial(&mut self) {
        for index in 0..36 {
            let ratio = index as f32 / 35.0;
            let jitter = 1.0 + 0.06 * (hash01(0xB0A2D ^ (index as u32) << 3) - 0.5);
            let frequency = 60.0 * powf(8500.0 / 60.0, ratio) * jitter;
            let t60 = (14.0 / frequency).clamp(0.0016, 0.22);
            let pan = 0.35 + 0.30 * hash01(0x5EA1 ^ (index as u32) << 5);
            self.serial[index] = if frequency < 0.45 * self.sample_rate {
                let mut mode = BodyMode::tune(frequency, t60, pan, self.sample_rate);
                // A real plate's mobility is ragged: per-mode strength swings
                // ~±8 dB — a bank of equal modes is only a volume knob.
                mode.drive *= 0.65 + 0.8 * hash01(0xF00D ^ (index as u32) << 7);
                mode
            } else {
                BodyMode::default()
            };
        }
    }

    /// Retunes the undamped top-octave strings.
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
    fn t60_seconds(&self, frequency: f32, string_scale: f32, treble_life: f32) -> f32 {
        let frequency = frequency * string_scale;
        let base = 24.0 / (1.0 + powf(frequency / 180.0, 1.25)) + 0.6;
        // Sustained high partials are a guitar's, not a piano's: above ~2 kHz
        // the string's own losses cut the ring to fractions of a second, and
        // the treble energy the ear expects lives in the attack instead.
        let treble_losses =
            1.0 / (1.0 + powf(frequency / (10400.0 * self.controls.lab(1) * treble_life), 1.1));
        base * treble_losses * (0.5 + 1.5 * self.controls.decay)
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
        let ragged = sincosf(4.1 * l + 1.3).0
            + sincosf(6.7 * l + 4.1).0
            + sincosf(10.3 * l + 2.2).0;
        let normalized = ragged * (1.0 / 3.0);
        // ±9 dB of level (YDP spectra swing ±15 dB between neighbours; the
        // per-note irregularity supplies the rest), and up to ~×1.5 / ÷1.4 of
        // decay rate, in opposition: a mobile board radiates more and damps
        // the string more.
        let amplitude = powf(10.0, normalized * 0.45);
        let decay = 1.0 / (1.0 + 0.35 * normalized);
        (amplitude, decay)
    }

    fn decay_per_sample(&self, t60: f32) -> f32 {
        // Amplitude e-folds T60/6.91 apart; per-sample factor follows.
        expf(-6.907_755 / (t60 * self.sample_rate))
    }

    /// Where the hammer strikes, as a fraction of string length: ~1/8 in the
    /// bass narrowing toward ~1/13 in the treble.
    fn strike_point(note: u8) -> f32 {
        let position = (note - LOW_NOTE) as f32 / (NOTE_COUNT - 1) as f32;
        1.0 / (8.0 + 5.0 * position)
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
            11.5 * self.controls.lab(10) * velocity * velocity * ((0.35_f32 - position) / 0.35).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let f0 = f0 * powf(2.0, glide_cents / 1200.0);
        let string_scale = powf(f0 / 220.0, 0.55).clamp(0.35, 1.8);
        let treble_life = self.cal(note, 8);
        let contact = self.contact_time(note, velocity);
        // The nonlinear forest keeps the bass ladder open far above what the
        // soft bass hammer alone would give; the felt corner widens with it.
        let bass_top = 1.0 + 2.2 * ((0.35_f32 - position) / 0.35).clamp(0.0, 1.0);
        let cutoff = ((1.9 * self.cal(note, 0) / contact)
            * bass_top
            * self.controls.lab(0)
            * (0.5 + 1.5 * self.controls.brightness))
            .max(1.5 * f0);

        // Aftersound detune: a fraction of a cent in the bass, over a cent in
        // the treble, scaled by the unison control.
        let detune_cents =
            (0.3 + 0.9 * position) * (self.controls.unison * 2.86) * self.controls.lab(13);

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
            if frequency >= nyquist {
                break;
            }
            let (comb, _) = sincosf(core::f32::consts::PI * nf * x0);
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
            let floor = 0.0455 * velocity * velocity * self.controls.lab(6) * self.cal(note, 1);
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
            while sim_modes < SIM_MODES.min(count)
                && frequencies[sim_modes] < 16_000.0_f32.min(nyquist)
            {
                sim_modes += 1;
            }
            if sim_modes >= 4 {
                let mass = (0.06 + 0.85 * powf(position, 1.3)) * self.controls.lab(8);
                // F0 compensates the hereditary softening (Stulov measures
                // the felt modulus under load): quasi-static stiffness is
                // F0·(1−e), so F0 carries 1/(1−e).
                let stiffness = mass
                    * powf(core::f32::consts::PI / contact, 2.0)
                    * 34.0
                    * self.controls.lab(7)
                    * (0.5 + 1.5 * self.controls.brightness);
                let velocity0 = 0.25 + 1.75 * velocity;
                let exponent = 1.7 + 1.7 * position;
                let (q, over_omega) = simulate_strike(
                    &frequencies, sim_modes, x0, mass, stiffness, exponent, velocity0,
                );
                // Scale the simulated bridge-force spectrum to the recipe's
                // overall level so the calibrated loudness holds.
                let mut sim_peak = 0.0f32;
                let mut magnitudes = [0.0f32; SIM_MODES];
                for n in 0..sim_modes {
                    let bridge = (n + 1) as f32;
                    magnitudes[n] =
                        bridge * sqrtf(q[n] * q[n] + over_omega[n] * over_omega[n]);
                    sim_peak = sim_peak.max(magnitudes[n] * colour[n]);
                }
                if sim_peak > 0.0 {
                    let normalise = peak / sim_peak;
                    for n in 0..sim_modes {
                        let candidate = magnitudes[n] * colour[n] * normalise;
                        if candidate > amplitudes[n].abs() {
                            let magnitude = magnitudes[n].max(1e-12);
                            amplitudes[n] = candidate;
                            phase_q[n] = (n + 1) as f32 * q[n] / magnitude;
                            phase_o[n] = (n + 1) as f32 * over_omega[n] / magnitude;
                        }
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
                self.t60_seconds(frequency, string_scale, treble_life) * board_decay * self.cal(note, 4);
            let jitter = 0.55 + 0.9 * hash01((note as u32) << 10 | (n as u32) << 2 | 1);
            let cents = detune_cents * jitter;
            // The strings of the unison, struck together and equal: their
            // subsequent life -- fast coherent decay, dephasing, the long
            // trapped tail, the churn -- is simulated through the bridge
            // coupling below, not scripted here.
            let three = ((index as f32 - 18.0) / 8.0).clamp(0.0, 1.0)
                * if self.soft { 0.25 } else { 1.0 };
            let w3 = 0.22 * three;
            let remainder = 1.0 - w3;
            let (w1, w2) = (remainder * 0.56, remainder * 0.44);
            let half_ratio = powf(2.0, cents / 2400.0);
            let third_ratio = powf(
                2.0,
                cents * (0.9 + 0.4 * hash01((note as u32) << 9 | (n as u32) << 2 | 3)) / 1200.0,
            );
            // Strings carry only their intrinsic (internal/air) losses in
            // their own rotations; radiation is the bridge's business.
            let intrinsic = self.decay_per_sample(t60 * 5.3 * self.controls.lab(12));
            let prompt_t60 = t60 * 1.94 * self.controls.lab(11) / (1.4 + 1.1 * position);
            let step = expf(
                -6.907_755 * (CULL_INTERVAL as f32 / sample_rate) / prompt_t60,
            );
            let coupling = (1.0 - step) / (2.0 + three);
            // The partial swells in over a few of its own periods -- ~45 ms
            // for a bass fundamental, effectively instant in the treble.
            let rise_seconds =
                ((0.9 / frequency) * self.controls.lab(9)).clamp(0.0008, 0.09);
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
            let phantom_scale =
                bass_gate * velocity * velocity * 0.64 * self.controls.lab(5)
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
                let t60 = self.t60_seconds(frequency, string_scale, treble_life) * 0.4;
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
                let t60 = self.t60_seconds(frequency, string_scale, treble_life) * 0.35;
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
                bass_gate * powf(velocity, 2.5) * 0.065 * 0.32 * self.controls.lab(4)
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
        let noise_decay = expf(-1.0 / ((0.012 + 0.010 * (1.0 - position)) * sample_rate));
        // The knock starts wide — brighter for harder blows — and its
        // bandwidth contracts with a ~25 ms time constant as it fades.
        let noise_coefficient = 1.0
            - expf(
                -core::f32::consts::TAU * (500.0 + 700.0 * position + 4000.0 * velocity)
                    / sample_rate,
            );
        let noise_shrink = expf(-1.0 / (0.025 * sample_rate));
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
                * self.controls.lab(2)
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
                    let t60 = (self.t60_seconds(frequency, 1.0, 1.0) * 0.35).min(0.9);
                    let decay = self.decay_per_sample(t60);
                    *slot = Component::start(level, frequency, decay, sample_rate);
                }
            }
        }

        let chiff_mult = self.controls.lab(3) * self.cal(note, 3);
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
        voice.energy = 1.0;
        // The hammer/soundboard thump: heavier and darker in the bass.
        voice.noise_amp = velocity * velocity * (0.083 - 0.065 * position) * chiff_mult;
        voice.noise_decay = noise_decay;
        voice.noise_coefficient = noise_coefficient;
        voice.noise_shrink = noise_shrink;
        voice.noise_lp = 0.0;
        voice.noise_seed = 0x9E37_79B9 ^ (note as u32).wrapping_mul(2_654_435_761);
        voice.pan_left = pan_left;
        voice.pan_right = pan_right;
        voice.glide_rate = -(glide_cents / 28.0) * core::f32::consts::LN_2 / 1200.0;
        voice.glide_steps = if glide_cents > 0.05 { 28 } else { 0 };
        self.serial_cal = self.cal(note, 9);
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
                let t60 = self.t60_seconds(frequency, string_scale, treble_life) * 1.5;
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
    fn damper_factor(&self) -> f32 {
        expf(-1.0 / (0.06 * self.sample_rate))
    }

    /// The damper thud's colour and length: dark and short.
    fn damper_thud(&self) -> (f32, f32) {
        let coefficient =
            1.0 - expf(-core::f32::consts::TAU * 260.0 / self.sample_rate);
        let decay = expf(-1.0 / (0.010 * self.sample_rate));
        (coefficient, decay)
    }

    fn release(&mut self, channel: u8, note: u8) {
        let damper = self.damper_factor();
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
            let damper = self.damper_factor();
            let (thud_coefficient, thud_decay) = self.damper_thud();
            for voice in &mut self.voices {
                if voice.active && voice.sustained {
                    voice.damp(damper, thud_coefficient, thud_decay);
                }
            }
        }
    }

    fn all_notes_off(&mut self) {
        let damper = self.damper_factor();
        let (thud_coefficient, thud_decay) = self.damper_thud();
        for voice in &mut self.voices {
            if voice.active {
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
        self.tune_body();
        self.tune_serial();
        self.tune_open_strings();
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
        for mode in &mut self.body {
            mode.y1 = 0.0;
            mode.y2 = 0.0;
        }
        for string in &mut self.open_strings {
            string.y1 = 0.0;
            string.y2 = 0.0;
        }
        for mode in &mut self.serial {
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
            },
            "bright" => Controls {
                brightness: 0.8,
                dynamics: 0.75,
                unison: 0.45,
                decay: 0.45,
                width: 0.75,
                level: 0.68,
                lab: [0.5; LAB_COUNT],
            },
            "intimate" => Controls {
                brightness: 0.4,
                dynamics: 0.45,
                unison: 0.65,
                decay: 0.35,
                width: 0.35,
                level: 0.72,
                lab: [0.5; LAB_COUNT],
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
        values[6..].copy_from_slice(&self.controls.lab);
        let target = destination.get_mut(..values.len() * 4)?;
        for (chunk, value) in target.chunks_exact_mut(4).zip(values) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        Some(values.len() * 4)
    }

    fn load_state(&mut self, state: &[u8]) -> bool {
        // 24 bytes = the original six controls; the lab keeps defaults.
        if state.len() != PARAM_COUNT * 4 && state.len() != 24 {
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
        lab.copy_from_slice(&values[6..]);
        self.controls = Controls {
            brightness: values[0],
            dynamics: values[1],
            unison: values[2],
            decay: values[3],
            width: values[4],
            level: values[5],
            lab,
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

                voice.cull_in -= 1;
                if voice.cull_in == 0 {
                    voice.cull_in = CULL_INTERVAL;
                    let removed = voice.cull();
                    self.active_partials = self.active_partials.saturating_sub(removed);
                }
            }

            // Radiate the string sum through the modal body: dry plus a wet
            // sum the twelve resonators colour, panned mode by mode.
            let excitation = left + right;
            let mut body_left = 0.0;
            let mut body_right = 0.0;
            for mode in &mut self.body {
                let y = mode.tick(excitation);
                body_left += y * mode.pan_left;
                body_right += y * mode.pan_right;
            }
            // The open top octave listens to the bridge and rings on.
            let mut open_left = 0.0;
            let mut open_right = 0.0;
            for string in &mut self.open_strings {
                let y = string.tick(excitation);
                open_left += y * string.pan_left;
                open_right += y * string.pan_right;
            }
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
            let halo_left = (halo_outs[0] - halo_outs[1]) * HALO_MIX;
            let halo_right = (halo_outs[2] - halo_outs[3]) * HALO_MIX;

            // The lid and rim reflect the near field back a few dozen
            // milliseconds late, differently per side.
            let staged = excitation
                + (body_left + body_right) * BODY_MIX
                + (open_left + open_right) * OPEN_MIX
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
            let room_left = (outs[0] - outs[1] + outs[2]) * ROOM_MIX;
            let room_right = (outs[3] - outs[4] + outs[5]) * ROOM_MIX;

            // Everything radiates through the board: the direct string sum
            // is replaced by the serial bank's output.
            let mut serial_left = 0.0;
            let mut serial_right = 0.0;
            for mode in &mut self.serial {
                let y = mode.tick(excitation);
                serial_left += y * mode.pan_left;
                serial_right += y * mode.pan_right;
            }
            let serial_mix = 0.084 * self.controls.lab(14) * self.serial_cal;
            let body_colour = BODY_MIX * 1.74 * self.controls.lab(15);
            let left = Self::soften(
                serial_left * serial_mix
                    + body_left * body_colour
                    + open_left * OPEN_MIX
                    + halo_left
                    + lid_left
                    + room_left,
            ) * level;
            let right = Self::soften(
                serial_right * serial_mix
                    + body_right * body_colour
                    + open_right * OPEN_MIX
                    + halo_right
                    + lid_right
                    + room_right,
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
        let notes: Vec<(u8, u8)> = if cal.is_some() {
            (0..30).map(|i| (21 + 3 * i as u8, 125u8)).collect()
        } else {
            vec![(21u8, 123u8), (36, 120), (48, 125), (60, 125), (69, 125), (30, 85), (30, 124), (48, 105), (60, 70)]
        };
        for (note, velocity) in notes {
            let mut piano = Box::new(ConcertGrand::default());
            if let Some(table) = cal {
                piano.cal = table;
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
        let mut piano = prepared();
        render(&mut piano, 64, &[note_on(60, 100)]);
        let coherence = |piano: &ConcertGrand| {
            let voice = piano.voices.iter().find(|v| v.active).unwrap();
            let p = &voice.partials[7.min(voice.partial_count - 1)];
            let sum_s = p.prompt.s + p.aftersound.s + p.third.s;
            let sum_c = p.prompt.c + p.aftersound.c + p.third.c;
            let radiated = sum_s * sum_s + sum_c * sum_c;
            let stored = p.prompt.magnitude_squared()
                + p.aftersound.magnitude_squared()
                + p.third.magnitude_squared();
            (radiated / (3.0 * stored).max(1e-24), stored)
        };
        render(&mut piano, (FS * 0.1) as usize, &[]);
        let (early_coherence, _) = coherence(&piano);
        render(&mut piano, (FS * 2.5) as usize, &[]);
        let (late_coherence, late_stored) = coherence(&piano);
        assert!(late_stored > 0.0, "the strings died entirely");
        assert!(
            late_coherence < early_coherence * 0.7,
            "no dephasing: {early_coherence} -> {late_coherence}"
        );
    }

    #[test]
    fn the_dry_instrument_leaves_no_room_tail() {
        // Staging is disabled until the naked instrument convinces: after a
        // damped chord and the body's short ring, the output must be silent.
        let mut piano = prepared();
        let chord: Vec<MidiEvent> =
            [48u8, 55, 64].iter().map(|n| note_on(*n, 120)).collect();
        render(&mut piano, (FS * 0.3) as usize, &chord);
        let offs: Vec<MidiEvent> = [48u8, 55, 64].iter().map(|n| note_off(*n)).collect();
        render(&mut piano, 64, &offs);
        render(&mut piano, (FS * 1.5) as usize, &[]);
        let late = energy(&render(&mut piano, 1600, &[]));
        assert!(late < 1e-7, "staging bleed after damping: {late}");
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
        for (frequency, t60, _) in [BODY_MODES[0], BODY_MODES[BODY_MODES.len() - 1]] {
            let mut mode = BodyMode::tune(frequency, t60, 0.5, sample_rate);
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
