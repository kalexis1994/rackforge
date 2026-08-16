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
const MAX_PARTIALS: usize = 80;
/// Ceiling on simultaneously active partials across all voices, so a pedalled
/// fortissimo run degrades new notes' partial counts instead of the callback.
const PARTIAL_BUDGET: usize = 1400;
/// How often a voice retires inaudible components, in samples.
const CULL_INTERVAL: u32 = 256;
/// A component whose squared magnitude falls below this is inaudible even
/// summed eighty times: kill it and spend the arithmetic elsewhere.
const DEAD_MAGNITUDE_SQUARED: f32 = 1e-10;

/// Parameter indices, matching the packaged schema.
const PARAM_BRIGHTNESS: u32 = 0;
const PARAM_DYNAMICS: u32 = 1;
const PARAM_UNISON: u32 = 2;
const PARAM_DECAY: u32 = 3;
const PARAM_WIDTH: u32 = 4;
const PARAM_LEVEL: u32 = 5;
const PARAM_COUNT: usize = 6;

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

/// One partial: the prompt component and its detuned, slower aftersound.
/// Their superposition is the two-stage decay and the unison beat.
#[derive(Clone, Copy, Default)]
struct Partial {
    prompt: Component,
    aftersound: Component,
}

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
    /// Hammer/soundboard thump: a decaying low-passed noise burst.
    noise_amp: f32,
    noise_decay: f32,
    noise_lp: f32,
    noise_coefficient: f32,
    noise_seed: u32,
    pan_left: f32,
    pan_right: f32,
    /// Samples until the next audibility cull.
    cull_in: u32,
    /// Rough loudness, refreshed at cull time; used to steal the quietest.
    energy: f32,
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
            noise_seed: 1,
            pan_left: 0.0,
            pan_right: 0.0,
            cull_in: CULL_INTERVAL,
            energy: 0.0,
        }
    }
}

impl Voice {
    /// Renders one mono sample and advances every live component.
    #[inline(always)]
    fn tick(&mut self) -> f32 {
        let mut sum = 0.0;
        for partial in &mut self.partials[..self.partial_count] {
            sum += partial.prompt.tick() + partial.aftersound.tick();
        }
        if self.noise_amp > 1e-7 {
            // Park–Miller-style LCG: white noise costs one multiply-add.
            self.noise_seed = self.noise_seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let white = (self.noise_seed >> 9) as f32 * (1.0 / 4_194_304.0) - 1.0;
            self.noise_lp += self.noise_coefficient * (white - self.noise_lp);
            sum += self.noise_lp * self.noise_amp;
            self.noise_amp *= self.noise_decay;
        }
        sum
    }

    /// Retires components that have decayed below audibility and refreshes the
    /// loudness estimate. Runs at block cadence, never per sample.
    fn cull(&mut self) -> usize {
        let mut removed = 0;
        let mut energy = 0.0;
        let mut index = 0;
        while index < self.partial_count {
            let partial = &self.partials[index];
            let magnitude = partial.prompt.magnitude_squared()
                + partial.aftersound.magnitude_squared();
            if magnitude < DEAD_MAGNITUDE_SQUARED {
                self.partial_count -= 1;
                self.partials[index] = self.partials[self.partial_count];
                removed += 1;
            } else {
                energy += magnitude;
                index += 1;
            }
        }
        self.energy = energy;
        if self.partial_count == 0 && self.noise_amp <= 1e-7 {
            self.active = false;
        }
        removed
    }

    fn damp(&mut self, factor: f32) {
        for partial in &mut self.partials[..self.partial_count] {
            partial.prompt.damp(factor);
            partial.aftersound.damp(factor);
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
}

impl Default for Controls {
    fn default() -> Self {
        // The "Concert" preset.
        Self {
            brightness: 0.5,
            dynamics: 0.6,
            unison: 0.5,
            decay: 0.5,
            width: 0.7,
            level: 0.7,
        }
    }
}

impl Controls {
    fn get(&self, index: u32) -> Option<f64> {
        let value = match index {
            PARAM_BRIGHTNESS => self.brightness,
            PARAM_DYNAMICS => self.dynamics,
            PARAM_UNISON => self.unison,
            PARAM_DECAY => self.decay,
            PARAM_WIDTH => self.width,
            PARAM_LEVEL => self.level,
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
            _ => return false,
        }
        true
    }
}

pub struct ConcertGrand {
    controls: Controls,
    sample_rate: f32,
    /// Fundamental of each note after the derived octave stretch.
    fundamental: [f32; NOTE_COUNT],
    /// Fletcher inharmonicity coefficient per note.
    inharmonicity: [f32; NOTE_COUNT],
    voices: [Voice; MAX_VOICES],
    pedal: bool,
    /// Live count of active partials, the budget the callback answers to.
    active_partials: usize,
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
            active_partials: 0,
        };
        piano.tune();
        piano
    }
}

/// `sqrt((1 + 4B) / (1 + B))`: how sharp a stiff string's second partial is,
/// relative to twice its fundamental. The stretch derives from this.
fn octave_stretch_ratio(b: f32) -> f32 {
    sqrtf((1.0 + 4.0 * b) / (1.0 + b))
}

impl ConcertGrand {
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

    /// T60 fitted to published decay ranges: tens of seconds for the lowest
    /// fundamentals, under a second at the top (Valette & Cuesta's losses all
    /// grow with frequency). Every partial reads this at its own frequency.
    fn t60_seconds(&self, frequency: f32) -> f32 {
        let base = 22.0 / (1.0 + powf(frequency / 220.0, 1.4)) + 0.3;
        base * (0.5 + 1.5 * self.controls.decay)
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

    /// Hammer–string contact time in seconds: longer for soft blows and low
    /// notes, under a millisecond for hard treble blows (Askenfelt & Jansson).
    fn contact_time(&self, note: u8, velocity: f32) -> f32 {
        let position = (note - LOW_NOTE) as f32 / (NOTE_COUNT - 1) as f32;
        let base = 0.0035 - 0.0027 * position;
        // `dynamics` sets how strongly velocity drives the felt; the felt's
        // hardening (force exponent ~2.5) is rendered as this contact-time
        // swing rather than simulated.
        let swing = 0.5 + 0.6 * self.controls.dynamics;
        base * (1.0 + swing - swing * 2.0 * (velocity - 0.5))
    }

    fn start_voice(&mut self, channel: u8, note: u8, velocity: u8) {
        let index = (note.clamp(LOW_NOTE, LOW_NOTE + NOTE_COUNT as u8 - 1) - LOW_NOTE) as usize;
        let velocity = velocity as f32 / 127.0;

        // A re-struck string keeps ringing in reality; the model damps the old
        // voice quickly instead (documented simplification).
        let damper = self.damper_factor();
        for voice in &mut self.voices {
            if voice.active && voice.note == note && voice.channel == channel {
                voice.damp(damper);
            }
        }

        let f0 = self.fundamental[index];
        let b = self.inharmonicity[index];
        let x0 = Self::strike_point(note);
        let nyquist = 0.47 * self.sample_rate;

        // Felt low-pass. The cutoff scales with the reciprocal of the contact
        // time; the constant is empirical — a strict 1/(2·t) reading of the
        // pulse width lands far darker than measured piano spectra, because
        // the felt hardens during contact. Floored above the fundamental so
        // the shortest treble strings keep their first partial.
        let contact = self.contact_time(note, velocity);
        let cutoff =
            ((2.0 / contact) * (0.5 + 1.5 * self.controls.brightness)).max(1.5 * f0);

        // Aftersound detune: a fraction of a cent in the bass, over a cent in
        // the treble, scaled by the unison control.
        let position = index as f32 / (NOTE_COUNT - 1) as f32;
        let detune_cents = (0.3 + 0.9 * position) * (self.controls.unison * 2.0);

        // First pass: partial frequencies and unnormalised amplitudes.
        let mut frequencies = [0.0_f32; MAX_PARTIALS];
        let mut amplitudes = [0.0_f32; MAX_PARTIALS];
        let mut count = 0;
        let mut peak = 0.0_f32;
        for n in 1..=MAX_PARTIALS {
            let nf = n as f32;
            let frequency = nf * f0 * sqrtf(1.0 + b * nf * nf);
            if frequency >= nyquist {
                break;
            }
            let (comb, _) = sincosf(core::f32::consts::PI * nf * x0);
            let felt = 1.0 / (1.0 + (frequency / cutoff) * (frequency / cutoff));
            let amplitude = (comb.abs() / nf) * felt;
            frequencies[count] = frequency;
            amplitudes[count] = amplitude;
            peak = peak.max(amplitude);
            count += 1;
        }
        if count == 0 || peak <= 0.0 {
            return;
        }

        // Drop partials the strike already made inaudible, then respect the
        // global budget: a saturated instrument thins new notes, never the
        // audio callback.
        let floor = peak * 1e-3;
        let budget_left = PARTIAL_BUDGET.saturating_sub(self.active_partials);
        let cap = if budget_left < count { budget_left.max(12) } else { count };

        // Energy normalisation, then the velocity curve: level roughly
        // velocity^1.7 (sound pressure grows faster than hammer speed).
        let mut energy = 0.0;
        for n in 0..count {
            if amplitudes[n] >= floor {
                energy += amplitudes[n] * amplitudes[n];
            }
        }
        let scale = 0.32 * powf(velocity.max(0.01), 1.7) / sqrtf(energy.max(1e-9));

        // Everything a partial needs, computed before a voice is borrowed:
        // both components draw their decay from the same loss curve, read at
        // the partial's own frequency — the prompt dies ~3× faster, the
        // aftersound lingers past it.
        let sample_rate = self.sample_rate;
        let detune_ratio = powf(2.0, detune_cents / 1200.0);
        let mut partials = [Partial::default(); MAX_PARTIALS];
        let mut placed = 0;
        for n in 0..count {
            if placed >= cap || amplitudes[n] < floor {
                continue;
            }
            let frequency = frequencies[n];
            let amplitude = amplitudes[n] * scale;
            let t60 = self.t60_seconds(frequency);
            let natural = self.decay_per_sample(t60);
            let prompt = self.decay_per_sample(t60 / 3.0);
            partials[placed] = Partial {
                prompt: Component::start(amplitude * 0.85, frequency, prompt, sample_rate),
                aftersound: Component::start(
                    amplitude * 0.30,
                    (frequency * detune_ratio).min(nyquist),
                    natural,
                    sample_rate,
                ),
            };
            placed += 1;
        }
        if placed == 0 {
            return;
        }

        let noise_decay = expf(-1.0 / (0.015 * sample_rate));
        let noise_coefficient =
            1.0 - expf(-core::f32::consts::TAU * (80.0 + 260.0 * position) / sample_rate);
        // Constant-power pan by key position, narrowed by the width control.
        let spread = (position - 0.5) * self.controls.width;
        let angle = (0.5 + spread * 0.8) * core::f32::consts::FRAC_PI_2;
        let (pan_right, pan_left) = sincosf(angle);

        let Some(voice) = self.allocate_voice() else { return };
        voice.active = true;
        voice.note = note;
        voice.channel = channel;
        voice.held = true;
        voice.sustained = false;
        voice.partials = partials;
        voice.partial_count = placed;
        voice.cull_in = CULL_INTERVAL;
        voice.energy = 1.0;
        // The hammer/soundboard thump: heavier and darker in the bass.
        voice.noise_amp = velocity * velocity * (0.045 - 0.03 * position);
        voice.noise_decay = noise_decay;
        voice.noise_coefficient = noise_coefficient;
        voice.noise_lp = 0.0;
        voice.noise_seed = 0x9E37_79B9 ^ (note as u32).wrapping_mul(2_654_435_761);
        voice.pan_left = pan_left;
        voice.pan_right = pan_right;
        self.active_partials += placed;
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

    fn release(&mut self, channel: u8, note: u8) {
        let damper = self.damper_factor();
        for voice in &mut self.voices {
            if voice.active && voice.note == note && voice.channel == channel && voice.held {
                if self.pedal {
                    voice.held = false;
                    voice.sustained = true;
                } else {
                    voice.damp(damper);
                }
            }
        }
    }

    fn set_pedal(&mut self, down: bool) {
        self.pedal = down;
        if !down {
            let damper = self.damper_factor();
            for voice in &mut self.voices {
                if voice.active && voice.sustained {
                    voice.damp(damper);
                }
            }
        }
    }

    fn all_notes_off(&mut self) {
        let damper = self.damper_factor();
        for voice in &mut self.voices {
            if voice.active {
                voice.damp(damper);
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
        self.reset();
        true
    }

    fn reset(&mut self) {
        self.voices = [Voice::default(); MAX_VOICES];
        self.pedal = false;
        self.active_partials = 0;
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
            },
            "bright" => Controls {
                brightness: 0.8,
                dynamics: 0.75,
                unison: 0.45,
                decay: 0.45,
                width: 0.75,
                level: 0.68,
            },
            "intimate" => Controls {
                brightness: 0.4,
                dynamics: 0.45,
                unison: 0.65,
                decay: 0.35,
                width: 0.35,
                level: 0.72,
            },
            _ => return false,
        };
        true
    }

    fn save_state(&self, destination: &mut [u8]) -> Option<usize> {
        let values = [
            self.controls.brightness,
            self.controls.dynamics,
            self.controls.unison,
            self.controls.decay,
            self.controls.width,
            self.controls.level,
        ];
        let target = destination.get_mut(..values.len() * 4)?;
        for (chunk, value) in target.chunks_exact_mut(4).zip(values) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        Some(values.len() * 4)
    }

    fn load_state(&mut self, state: &[u8]) -> bool {
        if state.len() != PARAM_COUNT * 4 {
            return false;
        }
        let mut values = [0.0_f32; PARAM_COUNT];
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
        self.controls = Controls {
            brightness: values[0],
            dynamics: values[1],
            unison: values[2],
            decay: values[3],
            width: values[4],
            level: values[5],
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

            let left = Self::soften(left) * level;
            let right = Self::soften(right) * level;
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

    fn prepared() -> ConcertGrand {
        let mut piano = ConcertGrand::default();
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
        let piano = ConcertGrand::default();
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
        let comb = |n: f32| sincosf(core::f32::consts::PI * n * x0).0.abs() / n;
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
                ((2.0 / piano.contact_time(note, velocity)) * 1.25).max(1.5 * f0);
            let mut weighted = 0.0;
            let mut total = 0.0;
            for n in 1..=40 {
                let nf = n as f32;
                let frequency = nf * f0 * sqrtf(1.0 + b * nf * nf);
                let amp = (sincosf(core::f32::consts::PI * nf * x0).0.abs() / nf)
                    / (1.0 + (frequency / cutoff) * (frequency / cutoff));
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
        for _ in 0..12 {
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
        let mut piano = ConcertGrand::default();
        assert!(piano.load_preset("mellow"));
        let mut state = [0u8; 32];
        assert_eq!(piano.save_state(&mut state), Some(24));
        assert!(piano.load_preset("bright"));
        assert!(piano.load_state(&state[..24]));
        assert_eq!(piano.get_parameter(PARAM_BRIGHTNESS), Some(0.28_f32 as f64));
        assert!(!piano.load_preset("unknown"));
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
