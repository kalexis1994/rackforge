#![cfg_attr(target_arch = "wasm32", no_std)]

//! A five-voice instrument that exists to demonstrate — and test — the
//! `parallel_render_v1` contract end to end. It is the worked example for a
//! synthesizer like RF-5: the coordinator owns everything global (MIDI
//! parsing, voice allocation, the shared vibrato LFO, the master stage) and
//! each of the five voices is one independent unit a host may render on any
//! of its own worker threads.
//!
//! The division of labor is the entire point:
//!
//! * `begin_block` runs once per block on the coordinator. It consumes MIDI
//!   and automation, advances the vibrato LFO exactly once, allocates
//!   voices, and writes each active voice a dispatch payload carrying every
//!   per-block dynamic value the voice needs (note events with their exact
//!   frames, the LFO value, a settings snapshot, the sample rate).
//! * `render_unit` runs per voice, possibly concurrently, from the payload
//!   and the voice's own persistent state only. It cannot see the
//!   coordinator: on multi-core hosts it executes inside an isolated
//!   instance where the coordinator state is simply absent.
//! * `end_block` runs once, summing the voice slots in ascending unit order
//!   and applying the master level — a deterministic combine no matter which
//!   worker finished first.

use rackforge_plugin_sdk::{
    BlockContext, MidiEvent, ParallelProcessor, PlanWriter, UnitContext, UnitMix,
    export_parallel_processor,
};

const MAX_UNITS: usize = 5;
const DISPATCH_STRIDE: usize = 48;
const MAX_OUTPUT_CHANNELS: usize = 2;
/// Parameter indices, in the order the packaged schema declares them.
const PARAM_BRIGHTNESS: u32 = 0;
const PARAM_ATTACK: u32 = 1;
const PARAM_RELEASE: u32 = 2;
const PARAM_SHAPE: u32 = 3;
const PARAM_LEVEL: u32 = 4;
/// Depth of the block-rate vibrato applied by every voice from the shared
/// LFO value. Small on purpose; its role is to prove that global modulation
/// is computed once and distributed, never duplicated per unit.
const VIBRATO_DEPTH: f32 = 0.003;

/// Payload flags.
const FLAG_START: u32 = 1 << 0;
const FLAG_RELEASE: u32 = 1 << 1;

/// One voice's dispatch payload: fixed little-endian layout, 44 bytes used.
struct Payload {
    flags: u32,
    start_frame: u32,
    release_frame: u32,
    note: u32,
    velocity: u32,
    lfo: f32,
    brightness: f32,
    attack: f32,
    release: f32,
    shape: f32,
    sample_rate: f32,
}

impl Payload {
    fn write(&self, destination: &mut [u8; DISPATCH_STRIDE]) {
        let words = [
            self.flags,
            self.start_frame,
            self.release_frame,
            self.note,
            self.velocity,
            self.lfo.to_bits(),
            self.brightness.to_bits(),
            self.attack.to_bits(),
            self.release.to_bits(),
            self.shape.to_bits(),
            self.sample_rate.to_bits(),
        ];
        for (chunk, word) in destination.as_chunks_mut::<4>().0.iter_mut().zip(words) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
    }

    fn read(source: &[u8]) -> Option<Self> {
        if source.len() < 44 {
            return None;
        }
        let mut words = [0_u32; 11];
        for (word, chunk) in words.iter_mut().zip(source.as_chunks::<4>().0) {
            *word = u32::from_le_bytes(*chunk);
        }
        Some(Self {
            flags: words[0],
            start_frame: words[1],
            release_frame: words[2],
            note: words[3],
            velocity: words[4],
            lfo: f32::from_bits(words[5]),
            brightness: f32::from_bits(words[6]),
            attack: f32::from_bits(words[7]),
            release: f32::from_bits(words[8]),
            shape: f32::from_bits(words[9]),
            sample_rate: f32::from_bits(words[10]),
        })
    }
}

#[derive(Clone, Copy)]
struct Settings {
    brightness: f32,
    attack: f32,
    release: f32,
    shape: f32,
    level: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            brightness: 0.6,
            attack: 0.05,
            release: 0.3,
            shape: 0.25,
            level: 0.7,
        }
    }
}

impl Settings {
    fn set(&mut self, index: u32, value: f64) -> bool {
        if !(0.0..=1.0).contains(&value) {
            return false;
        }
        let value = value as f32;
        match index {
            PARAM_BRIGHTNESS => self.brightness = value,
            PARAM_ATTACK => self.attack = value,
            PARAM_RELEASE => self.release = value,
            PARAM_SHAPE => self.shape = value,
            PARAM_LEVEL => self.level = value,
            _ => return false,
        }
        true
    }

    fn get(&self, index: u32) -> Option<f64> {
        let value = match index {
            PARAM_BRIGHTNESS => self.brightness,
            PARAM_ATTACK => self.attack,
            PARAM_RELEASE => self.release,
            PARAM_SHAPE => self.shape,
            PARAM_LEVEL => self.level,
            _ => return None,
        };
        Some(f64::from(value))
    }

    /// Frames the release tail can keep sounding after a note-off.
    fn release_frames(&self, sample_rate: f32) -> u64 {
        let seconds = 0.02 + self.release * self.release * 4.0;
        (seconds * sample_rate) as u64 + 1
    }
}

/// Coordinator-side bookkeeping for one unit: which note it plays and for
/// how many more frames it may keep sounding. The audible voice state lives
/// in [`VoiceUnit`], possibly in a different instance entirely.
#[derive(Clone, Copy, Default)]
struct Assignment {
    note: u8,
    channel: u8,
    gate: bool,
    /// Frames of audible life left once the gate closed. `None` while held.
    remaining: Option<u64>,
    /// This block's note events for the unit.
    start: Option<(u32, u8, u8)>,
    release: Option<u32>,
}

impl Assignment {
    fn sounding(&self) -> bool {
        self.gate || self.remaining.is_some_and(|remaining| remaining > 0)
    }
}

/// Global state: the RF-5-shaped half that must run exactly once per block.
pub struct ParallelDemoSynth {
    settings: Settings,
    assignments: [Assignment; MAX_UNITS],
    next_steal: usize,
    lfo_phase: f32,
    sample_rate: f32,
}

impl Default for ParallelDemoSynth {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            assignments: [Assignment::default(); MAX_UNITS],
            next_steal: 0,
            lfo_phase: 0.0,
            sample_rate: 48_000.0,
        }
    }
}

impl ParallelDemoSynth {
    fn note_on(&mut self, frame: u32, channel: u8, note: u8, velocity: u8) {
        if velocity == 0 {
            self.note_off(frame, channel, note);
            return;
        }
        let unit = self
            .assignments
            .iter()
            .position(|assignment| !assignment.sounding())
            .unwrap_or_else(|| {
                let stolen = self.next_steal;
                self.next_steal = (self.next_steal + 1) % MAX_UNITS;
                stolen
            });
        let assignment = &mut self.assignments[unit];
        assignment.note = note;
        assignment.channel = channel;
        assignment.gate = true;
        assignment.remaining = None;
        assignment.start = Some((frame, note, velocity));
        assignment.release = None;
    }

    fn note_off(&mut self, frame: u32, channel: u8, note: u8) {
        for assignment in &mut self.assignments {
            if assignment.gate && assignment.note == note && assignment.channel == channel {
                assignment.gate = false;
                assignment.remaining = Some(self.settings.release_frames(self.sample_rate));
                assignment.release = Some(frame);
            }
        }
    }

    fn all_notes_off(&mut self, frame: u32) {
        for assignment in &mut self.assignments {
            if assignment.gate {
                assignment.gate = false;
                assignment.remaining = Some(self.settings.release_frames(self.sample_rate));
                assignment.release = Some(frame);
            }
        }
    }

    fn handle_midi(&mut self, event: &MidiEvent) {
        let data = event.data;
        let channel = data[0] & 0x0f;
        match data[0] & 0xf0 {
            0x90 => self.note_on(event.frame, channel, data[1] & 0x7f, data[2] & 0x7f),
            0x80 => self.note_off(event.frame, channel, data[1] & 0x7f),
            0xb0 if data[1] == 120 || data[1] == 123 => self.all_notes_off(event.frame),
            _ => {}
        }
    }
}

/// One voice: everything that evolves sample by sample, owned by its unit.
#[derive(Clone, Copy, Default)]
pub struct VoiceUnit {
    active: bool,
    releasing: bool,
    note: u8,
    sine: f32,
    cosine: f32,
    rotation_sine: f32,
    rotation_cosine: f32,
    envelope: f32,
    attack_step: f32,
    release_step: f32,
    velocity: f32,
    filter: f32,
    filter_coefficient: f32,
}

impl VoiceUnit {
    fn start(&mut self, payload: &Payload) {
        let note = payload.note as u8;
        let frequency = note_frequency(note);
        self.note = note;
        self.active = true;
        self.releasing = false;
        self.sine = 0.0;
        self.cosine = 1.0;
        self.envelope = 0.0;
        self.velocity = payload.velocity as f32 / 127.0;
        let sample_rate = payload.sample_rate.max(1.0);
        let attack_seconds = 0.002 + payload.attack * payload.attack * 2.0;
        self.attack_step = 1.0 / (attack_seconds * sample_rate).max(1.0);
        let release_seconds = 0.02 + payload.release * payload.release * 4.0;
        self.release_step = 1.0 / (release_seconds * sample_rate).max(1.0);
        self.filter = 0.0;
        let cutoff = frequency * (1.0 + payload.brightness * 12.0);
        let normalized = (core::f32::consts::TAU * cutoff / sample_rate).min(1.0);
        self.filter_coefficient = normalized.clamp(0.005, 0.999);
    }

    /// Applies the block's shared vibrato: the rotation is derived from the
    /// note and the LFO value the coordinator distributed, so every host
    /// path computes the identical pitch for the identical block.
    fn tune(&mut self, payload: &Payload) {
        if !self.active {
            return;
        }
        let frequency = note_frequency(self.note) * (1.0 + payload.lfo * VIBRATO_DEPTH);
        let increment = core::f32::consts::TAU * frequency / payload.sample_rate.max(1.0);
        let (rotation_sine, rotation_cosine) = rotation(increment);
        self.rotation_sine = rotation_sine;
        self.rotation_cosine = rotation_cosine;
    }

    fn next(&mut self, shape: f32) -> f32 {
        let sine = self.sine * self.rotation_cosine + self.cosine * self.rotation_sine;
        let cosine = self.cosine * self.rotation_cosine - self.sine * self.rotation_sine;
        let magnitude = sine * sine + cosine * cosine;
        let correction = 1.5 - 0.5 * magnitude;
        self.sine = sine * correction;
        self.cosine = cosine * correction;

        if self.releasing {
            self.envelope -= self.release_step;
            if self.envelope <= 0.0 {
                self.envelope = 0.0;
                self.active = false;
            }
        } else if self.envelope < 1.0 {
            self.envelope = (self.envelope + self.attack_step).min(1.0);
        }

        let third = self.sine * (3.0 - 4.0 * self.sine * self.sine);
        let raw = self.sine * (1.0 - shape) + third * shape * 0.6;
        self.filter += self.filter_coefficient * (raw - self.filter);
        self.filter * self.envelope * self.velocity
    }
}

impl ParallelProcessor for ParallelDemoSynth {
    type Unit = VoiceUnit;

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
        self.assignments = [Assignment::default(); MAX_UNITS];
        self.lfo_phase = 0.0;
        true
    }

    fn set_parameter(&mut self, index: u32, value: f64) -> bool {
        self.settings.set(index, value)
    }

    fn get_parameter(&self, index: u32) -> Option<f64> {
        self.settings.get(index)
    }

    fn reset(&mut self) {
        self.assignments = [Assignment::default(); MAX_UNITS];
        self.lfo_phase = 0.0;
    }

    fn load_preset(&mut self, id: &str) -> bool {
        self.settings = match id {
            "keys" => Settings {
                brightness: 0.6,
                attack: 0.02,
                release: 0.35,
                shape: 0.2,
                level: 0.7,
            },
            "pad" => Settings {
                brightness: 0.35,
                attack: 0.55,
                release: 0.75,
                shape: 0.1,
                level: 0.6,
            },
            "lead" => Settings {
                brightness: 0.85,
                attack: 0.01,
                release: 0.25,
                shape: 0.7,
                level: 0.65,
            },
            _ => return false,
        };
        true
    }

    fn save_state(&self, destination: &mut [u8]) -> Option<usize> {
        let values = [
            self.settings.brightness,
            self.settings.attack,
            self.settings.release,
            self.settings.shape,
            self.settings.level,
        ];
        let target = destination.get_mut(..values.len() * 4)?;
        for (chunk, value) in target.as_chunks_mut::<4>().0.iter_mut().zip(values) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        Some(values.len() * 4)
    }

    fn load_state(&mut self, state: &[u8]) -> bool {
        if state.len() != 20 {
            return false;
        }
        let mut values = [0.0_f32; 5];
        for (value, chunk) in values.iter_mut().zip(state.as_chunks::<4>().0) {
            let decoded = f32::from_le_bytes(*chunk);
            if !decoded.is_finite() || !(0.0..=1.0).contains(&decoded) {
                return false;
            }
            *value = decoded;
        }
        self.settings = Settings {
            brightness: values[0],
            attack: values[1],
            release: values[2],
            shape: values[3],
            level: values[4],
        };
        true
    }

    fn begin_block(&mut self, context: &BlockContext<'_>, plan: &mut PlanWriter<'_>) {
        // Sample-accurate automation arrives here with exact frames; this
        // instrument applies settings changes at their event order, which is
        // identical in every host path because only the coordinator sees
        // them.
        for event in context.parameters {
            let _ = self.settings.set(event.index, event.value);
        }
        for event in context.midi {
            self.handle_midi(event);
        }
        // The shared LFO advances exactly once per block, here and only
        // here. Units receive its value through their payloads — never by
        // running their own copy, which would drift the moment host paths
        // differ.
        self.lfo_phase += 6.0 * context.frames as f32 / self.sample_rate.max(1.0);
        if self.lfo_phase > core::f32::consts::TAU {
            self.lfo_phase -= core::f32::consts::TAU;
        }
        let (lfo, _) = rotation_at(self.lfo_phase);

        for unit in 0..MAX_UNITS {
            let assignment = &mut self.assignments[unit];
            let starting = assignment.start.is_some();
            if !assignment.sounding() && !starting {
                assignment.release = None;
                continue;
            }
            let (flags, start_frame, velocity, note) = match assignment.start.take() {
                Some((frame, note, velocity)) => {
                    (FLAG_START, frame, u32::from(velocity), u32::from(note))
                }
                None => (0, 0, 0, u32::from(assignment.note)),
            };
            let (flags, release_frame) = match assignment.release.take() {
                Some(frame) => (flags | FLAG_RELEASE, frame),
                None => (flags, 0),
            };
            if let Some(remaining) = assignment.remaining.as_mut() {
                *remaining = remaining.saturating_sub(u64::from(context.frames));
            }
            let payload = Payload {
                flags,
                start_frame,
                release_frame,
                note,
                velocity,
                lfo,
                brightness: self.settings.brightness,
                attack: self.settings.attack,
                release: self.settings.release,
                shape: self.settings.shape,
                sample_rate: self.sample_rate,
            };
            let mut bytes = [0_u8; DISPATCH_STRIDE];
            payload.write(&mut bytes);
            let _ = plan.activate(unit as u32, &bytes);
        }
    }

    fn render_unit(
        _unit_index: u32,
        unit: &mut VoiceUnit,
        payload: &[u8],
        context: &UnitContext<'_>,
        output: &mut [f32],
    ) {
        let channels = (context.output_channels as usize).clamp(1, MAX_OUTPUT_CHANNELS);
        let Some(payload) = Payload::read(payload) else {
            output.fill(0.0);
            return;
        };
        unit.tune(&payload);
        for frame in 0..context.frames as usize {
            if payload.flags & FLAG_START != 0 && frame as u32 == payload.start_frame {
                unit.start(&payload);
                unit.tune(&payload);
            }
            if payload.flags & FLAG_RELEASE != 0 && frame as u32 == payload.release_frame {
                unit.releasing = true;
            }
            let sample = if unit.active {
                unit.next(payload.shape)
            } else {
                0.0
            };
            for channel in 0..context.output_channels as usize {
                output[frame * context.output_channels as usize + channel] =
                    if channel < channels { sample } else { 0.0 };
            }
        }
    }

    fn end_block(
        &mut self,
        mix: &UnitMix<'_>,
        output: &mut [f32],
        frames: u32,
        output_channels: u32,
    ) {
        let samples = frames as usize * output_channels as usize;
        output[..samples].fill(0.0);
        // Ascending unit order: the deterministic float summation the
        // contract requires, independent of completion order.
        for unit in mix.active_units() {
            let slot = mix.slot(unit);
            for (target, sample) in output[..samples].iter_mut().zip(slot) {
                *target += *sample;
            }
        }
        // A full five-voice chord stays inside the output range.
        let master = self.settings.level * 0.25;
        for sample in &mut output[..samples] {
            *sample *= master;
        }
    }
}

/// Equal temperament from A440, computed with repeated multiplication so the
/// component needs no `exp`.
fn note_frequency(note: u8) -> f32 {
    const SEMITONE: f32 = 1.059_463_1;
    let mut frequency = 440.0;
    let mut remaining = i32::from(note) - 69;
    while remaining > 0 {
        frequency *= SEMITONE;
        remaining -= 1;
    }
    while remaining < 0 {
        frequency /= SEMITONE;
        remaining += 1;
    }
    frequency
}

/// Sine and cosine of one small increment, from their Taylor series.
fn rotation(increment: f32) -> (f32, f32) {
    let x = increment;
    let x2 = x * x;
    let sine = x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0)));
    let cosine = 1.0 - x2 / 2.0 * (1.0 - x2 / 12.0 * (1.0 - x2 / 30.0));
    (sine, cosine)
}

/// Sine of an arbitrary phase in `0..TAU`, folded into the small-angle
/// series with quadrant symmetry — accurate enough for a slow vibrato LFO.
fn rotation_at(phase: f32) -> (f32, f32) {
    let quarter = core::f32::consts::FRAC_PI_2;
    let (folded, sign) = if phase < quarter {
        (phase, 1.0)
    } else if phase < 2.0 * quarter {
        (2.0 * quarter - phase, 1.0)
    } else if phase < 3.0 * quarter {
        (phase - 2.0 * quarter, -1.0)
    } else {
        (4.0 * quarter - phase, -1.0)
    };
    let (sine, cosine) = rotation(folded);
    (sine * sign, cosine)
}

export_parallel_processor!(
    ParallelDemoSynth,
    max_units = 5,
    dispatch_stride = 48,
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
    use rackforge_plugin_sdk::Processor;

    const FRAMES: u32 = 128;
    const SAMPLES: usize = FRAMES as usize * 2;

    /// The exported component is single-threaded by contract; its statics
    /// are shared, so tests that go through the composed export path must
    /// not interleave.
    static EXPORT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn note_on(frame: u32, note: u8, velocity: u8) -> MidiEvent {
        MidiEvent {
            frame,
            data: [0x90, note, velocity],
            length: 3,
        }
    }

    fn note_off(frame: u32, note: u8) -> MidiEvent {
        MidiEvent {
            frame,
            data: [0x80, note, 0],
            length: 3,
        }
    }

    /// Renders a block through the composed classic export path.
    fn render(adapter: &mut RackForgeParallelExport, midi: &[MidiEvent]) -> [f32; SAMPLES] {
        let mut output = [0.0_f32; SAMPLES];
        Processor::process(adapter, &[], &mut output, midi, &[], FRAMES, 0, 2);
        output
    }

    /// Renders the same block by driving the stage functions directly with
    /// caller-owned buffers, exactly as a scheduling host would (minus the
    /// instance isolation, which the host-side tests cover).
    fn render_staged(
        synth: &mut ParallelDemoSynth,
        units: &mut [VoiceUnit; MAX_UNITS],
        midi: &[MidiEvent],
    ) -> [f32; SAMPLES] {
        let mut plan = [0_u32; MAX_UNITS * 2];
        let mut dispatch = [0_u8; MAX_UNITS * DISPATCH_STRIDE];
        let mut writer = PlanWriter::new(&mut plan, &mut dispatch, DISPATCH_STRIDE, MAX_UNITS);
        let context = BlockContext {
            input: &[],
            midi,
            parameters: &[],
            frames: FRAMES,
            input_channels: 0,
            output_channels: 2,
        };
        synth.begin_block(&context, &mut writer);
        let count = writer.activated();

        let mut mix = [0.0_f32; MAX_UNITS * SAMPLES];
        // Reverse order on purpose: completion order must not matter.
        for index in (0..count).rev() {
            let unit = plan[index * 2];
            let payload_len = plan[index * 2 + 1] as usize;
            let payload = &dispatch[unit as usize * DISPATCH_STRIDE..][..payload_len];
            let unit_context = UnitContext {
                input: &[],
                frames: FRAMES,
                output_channels: 2,
            };
            let mut voice_output = [0.0_f32; SAMPLES];
            ParallelDemoSynth::render_unit(
                unit,
                &mut units[unit as usize],
                payload,
                &unit_context,
                &mut voice_output,
            );
            mix[unit as usize * SAMPLES..][..SAMPLES].copy_from_slice(&voice_output);
        }

        let unit_mix = UnitMix::new(&mix, SAMPLES, &plan, count, SAMPLES);
        let mut output = [0.0_f32; SAMPLES];
        synth.end_block(&unit_mix, &mut output, FRAMES, 2);
        output
    }

    #[test]
    fn the_composed_process_matches_manual_stage_execution_exactly() {
        let _guard = EXPORT_LOCK.lock().unwrap();
        let mut composed = RackForgeParallelExport::default();
        assert!(Processor::prepare(&mut composed, 48_000.0, FRAMES, 0, 2));
        let mut synth = ParallelDemoSynth::default();
        assert!(ParallelProcessor::prepare(
            &mut synth, 48_000.0, FRAMES, 0, 2
        ));
        let mut units = [VoiceUnit::default(); MAX_UNITS];

        let script: [&[MidiEvent]; 6] = [
            &[note_on(3, 60, 100), note_on(3, 64, 90)],
            &[],
            &[note_on(0, 67, 110)],
            &[note_off(10, 60)],
            &[],
            &[],
        ];
        for (block, midi) in script.iter().enumerate() {
            let expected = render(&mut composed, midi);
            let produced = render_staged(&mut synth, &mut units, midi);
            assert_eq!(expected, produced, "block {block} diverged");
        }
    }

    #[test]
    fn silence_until_a_note_arrives_and_chords_stay_bounded() {
        let _guard = EXPORT_LOCK.lock().unwrap();
        let mut adapter = RackForgeParallelExport::default();
        assert!(Processor::prepare(&mut adapter, 48_000.0, FRAMES, 0, 2));
        let silent = render(&mut adapter, &[]);
        assert!(silent.iter().all(|sample| *sample == 0.0));

        let chord: [MidiEvent; 7] =
            core::array::from_fn(|index| note_on(0, 60 + index as u8 * 2, 127));
        let mut sounded = false;
        for _ in 0..16 {
            let block = render(&mut adapter, &chord[..0]);
            let _ = block;
        }
        let first = render(&mut adapter, &chord);
        sounded |= first.iter().any(|sample| sample.abs() > 0.0);
        for _ in 0..8 {
            let block = render(&mut adapter, &[]);
            assert!(block.iter().all(|sample| sample.abs() <= 1.0));
            sounded |= block.iter().any(|sample| sample.abs() > 0.01);
        }
        assert!(sounded);
    }

    #[test]
    fn seven_notes_share_five_units_deterministically() {
        let mut synth = ParallelDemoSynth::default();
        assert!(ParallelProcessor::prepare(
            &mut synth, 48_000.0, FRAMES, 0, 2
        ));
        let chord: [MidiEvent; 7] = core::array::from_fn(|index| note_on(0, 60 + index as u8, 100));
        let mut plan = [0_u32; MAX_UNITS * 2];
        let mut dispatch = [0_u8; MAX_UNITS * DISPATCH_STRIDE];
        let mut writer = PlanWriter::new(&mut plan, &mut dispatch, DISPATCH_STRIDE, MAX_UNITS);
        synth.begin_block(
            &BlockContext {
                input: &[],
                midi: &chord,
                parameters: &[],
                frames: FRAMES,
                input_channels: 0,
                output_channels: 2,
            },
            &mut writer,
        );
        // All five units are active; the two stolen ones carry the newest
        // notes deterministically (round-robin steal from unit 0).
        assert_eq!(writer.activated(), MAX_UNITS);
        let notes: Vec<u32> = (0..MAX_UNITS)
            .map(|unit| {
                Payload::read(&dispatch[unit * DISPATCH_STRIDE..][..DISPATCH_STRIDE])
                    .unwrap()
                    .note
            })
            .collect();
        assert_eq!(notes, vec![65, 66, 62, 63, 64]);
    }

    #[test]
    fn a_released_note_frees_its_unit_after_the_tail() {
        let mut synth = ParallelDemoSynth::default();
        assert!(ParallelProcessor::prepare(
            &mut synth, 48_000.0, FRAMES, 0, 2
        ));
        let mut units = [VoiceUnit::default(); MAX_UNITS];
        render_staged(&mut synth, &mut units, &[note_on(0, 60, 100)]);
        render_staged(&mut synth, &mut units, &[note_off(0, 60)]);
        // The default release is under half a second; a second of blocks is
        // more than enough for the coordinator to retire the unit.
        for _ in 0..400 {
            render_staged(&mut synth, &mut units, &[]);
        }
        assert!(!synth.assignments[0].sounding());
        let quiet = render_staged(&mut synth, &mut units, &[]);
        assert!(quiet.iter().all(|sample| sample.abs() < 1e-6));
    }

    #[test]
    fn presets_and_state_round_trip() {
        let mut synth = ParallelDemoSynth::default();
        assert!(ParallelProcessor::load_preset(&mut synth, "pad"));
        let mut state = [0_u8; 32];
        assert_eq!(ParallelProcessor::save_state(&synth, &mut state), Some(20));
        assert!(ParallelProcessor::load_preset(&mut synth, "lead"));
        assert!(ParallelProcessor::load_state(&mut synth, &state[..20]));
        assert_eq!(
            ParallelProcessor::get_parameter(&synth, PARAM_ATTACK),
            Some(0.55_f32 as f64)
        );
        assert_eq!(
            ParallelProcessor::get_parameter(&synth, PARAM_LEVEL),
            Some(0.6_f32 as f64)
        );
    }
}
