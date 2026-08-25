#![cfg_attr(target_arch = "wasm32", no_std)]

//! A small polyphonic instrument that exists so RackForge has something to
//! play everywhere, including in a browser where no sound library is
//! installed.
//!
//! It is deliberately self-contained: no resources, no sample data, and no
//! floating-point library beyond the arithmetic WebAssembly itself provides.
//! Everything it needs — a sine, an envelope, a filter — is computed from
//! recurrences rather than from `sin` or `exp`, so the component builds as
//! `no_std` and sounds identical on every host.

use rackforge_plugin_sdk::{MidiEvent, ParameterEvent, Processor, export_processor};

const MAX_VOICES: usize = 16;
const MAX_OUTPUT_CHANNELS: usize = 2;
/// Parameter indices, in the order the packaged schema declares them.
const PARAM_BRIGHTNESS: u32 = 0;
const PARAM_ATTACK: u32 = 1;
const PARAM_RELEASE: u32 = 2;
const PARAM_SHAPE: u32 = 3;
const PARAM_LEVEL: u32 = 4;

/// One sounding note.
///
/// The oscillator is a quadrature pair advanced by a rotation each frame: two
/// multiplies and two adds per sample, exact enough for an audio-rate sine and
/// free of any transcendental call.
#[derive(Clone, Copy, Default)]
struct Voice {
    note: u8,
    channel: u8,
    active: bool,
    releasing: bool,
    sine: f32,
    cosine: f32,
    rotation_sine: f32,
    rotation_cosine: f32,
    envelope: f32,
    attack_step: f32,
    release_step: f32,
    velocity: f32,
    /// One-pole low-pass state, so brightness follows the note rather than
    /// being a fixed timbre.
    filter: f32,
    filter_coefficient: f32,
}

impl Voice {
    fn start(
        &mut self,
        note: u8,
        channel: u8,
        velocity: u8,
        settings: &Settings,
        sample_rate: f32,
    ) {
        let frequency = note_frequency(note);
        let increment = core::f32::consts::TAU * frequency / sample_rate.max(1.0);
        let (rotation_sine, rotation_cosine) = rotation(increment);
        self.note = note;
        self.channel = channel;
        self.active = true;
        self.releasing = false;
        self.sine = 0.0;
        self.cosine = 1.0;
        self.rotation_sine = rotation_sine;
        self.rotation_cosine = rotation_cosine;
        self.envelope = 0.0;
        self.velocity = f32::from(velocity) / 127.0;
        self.attack_step = settings.attack_step(sample_rate);
        self.release_step = settings.release_step(sample_rate);
        self.filter = 0.0;
        self.filter_coefficient = settings.filter_coefficient(frequency, sample_rate);
    }

    fn release(&mut self) {
        self.releasing = true;
    }

    fn next(&mut self, shape: f32) -> f32 {
        // Rotate the quadrature pair. Renormalising keeps rounding from
        // shrinking or growing the amplitude over a long-held note.
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

        // `shape` folds in the third harmonic, which turns the sine into
        // something closer to a soft square as it rises.
        let third = self.sine * (3.0 - 4.0 * self.sine * self.sine);
        let raw = self.sine * (1.0 - shape) + third * shape * 0.6;
        self.filter += self.filter_coefficient * (raw - self.filter);
        self.filter * self.envelope * self.velocity
    }
}

/// The sound-shaping parameters, all normalised to 0..=1 as the schema
/// declares them.
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
    fn attack_step(&self, sample_rate: f32) -> f32 {
        let seconds = 0.002 + self.attack * self.attack * 2.0;
        1.0 / (seconds * sample_rate).max(1.0)
    }

    fn release_step(&self, sample_rate: f32) -> f32 {
        let seconds = 0.02 + self.release * self.release * 4.0;
        1.0 / (seconds * sample_rate).max(1.0)
    }

    /// Tracks the note: a bright setting opens the filter well above the
    /// fundamental, a dark one closes it near it.
    fn filter_coefficient(&self, frequency: f32, sample_rate: f32) -> f32 {
        let cutoff = frequency * (1.0 + self.brightness * 12.0);
        let normalized = (core::f32::consts::TAU * cutoff / sample_rate.max(1.0)).min(1.0);
        normalized.clamp(0.005, 0.999)
    }

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
}

pub struct DemoSynth {
    settings: Settings,
    voices: [Voice; MAX_VOICES],
    sample_rate: f32,
    /// Rotates which voice is stolen, so a busy passage does not always cut
    /// the same note short.
    next_voice: usize,
}

impl Default for DemoSynth {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            voices: [Voice::default(); MAX_VOICES],
            sample_rate: 48_000.0,
            next_voice: 0,
        }
    }
}

impl DemoSynth {
    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        if velocity == 0 {
            self.note_off(channel, note);
            return;
        }
        let index = self
            .voices
            .iter()
            .position(|voice| !voice.active)
            .unwrap_or_else(|| {
                let stolen = self.next_voice;
                self.next_voice = (self.next_voice + 1) % MAX_VOICES;
                stolen
            });
        let settings = self.settings;
        let sample_rate = self.sample_rate;
        self.voices[index].start(note, channel, velocity, &settings, sample_rate);
    }

    fn note_off(&mut self, channel: u8, note: u8) {
        for voice in &mut self.voices {
            if voice.active && voice.note == note && voice.channel == channel {
                voice.release();
            }
        }
    }

    fn all_notes_off(&mut self) {
        for voice in &mut self.voices {
            if voice.active {
                voice.release();
            }
        }
    }

    fn handle_midi(&mut self, event: &MidiEvent) {
        let data = event.data;
        let channel = data[0] & 0x0f;
        match data[0] & 0xf0 {
            0x90 => self.note_on(channel, data[1] & 0x7f, data[2] & 0x7f),
            0x80 => self.note_off(channel, data[1] & 0x7f),
            0xb0 if data[1] == 120 || data[1] == 123 => self.all_notes_off(),
            _ => {}
        }
    }
}

impl Processor for DemoSynth {
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
        self.voices = [Voice::default(); MAX_VOICES];
        true
    }

    fn reset(&mut self) {
        self.voices = [Voice::default(); MAX_VOICES];
    }

    fn set_parameter(&mut self, index: u32, value: f64) -> bool {
        self.settings.set(index, value)
    }

    fn get_parameter(&self, index: u32) -> Option<f64> {
        self.settings.get(index)
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
            "bass" => Settings {
                brightness: 0.2,
                attack: 0.01,
                release: 0.2,
                shape: 0.5,
                level: 0.8,
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
        for (chunk, value) in target.chunks_exact_mut(4).zip(values) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        Some(values.len() * 4)
    }

    fn load_state(&mut self, state: &[u8]) -> bool {
        if state.len() != 20 {
            return false;
        }
        let mut values = [0.0_f32; 5];
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
        self.settings = Settings {
            brightness: values[0],
            attack: values[1],
            release: values[2],
            shape: values[3],
            level: values[4],
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
        let channels = (output_channels as usize).clamp(1, MAX_OUTPUT_CHANNELS);
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
                let _ = self.settings.set(event.index, event.value);
                parameter_index += 1;
            }

            let shape = self.settings.shape;
            let mut mixed = 0.0;
            for voice in &mut self.voices {
                if voice.active {
                    mixed += voice.next(shape);
                }
            }
            // Voices are summed, so the mix is scaled to keep a full chord
            // inside the output range instead of clipping it.
            let sample = mixed * self.settings.level * 0.25;
            for channel in 0..channels {
                output[frame * output_channels as usize + channel] = sample;
            }
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

/// Sine and cosine of one phase increment, from their Taylor series. The
/// increment is well under a radian at audio rates, where five terms are far
/// more accurate than single precision can represent.
fn rotation(increment: f32) -> (f32, f32) {
    let x = increment;
    let x2 = x * x;
    let sine = x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0)));
    let cosine = 1.0 - x2 / 2.0 * (1.0 - x2 / 12.0 * (1.0 - x2 / 30.0));
    (sine, cosine)
}

export_processor!(
    DemoSynth,
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

    fn render(synth: &mut DemoSynth, frames: u32, midi: &[MidiEvent]) -> Vec<f32> {
        let mut output = vec![0.0; frames as usize * 2];
        synth.process(&[], &mut output, midi, &[], frames, 0, 2);
        output
    }

    #[test]
    fn tunes_a_to_440() {
        assert!((note_frequency(69) - 440.0).abs() < 0.01);
        assert!((note_frequency(81) - 880.0).abs() < 0.5);
        assert!((note_frequency(57) - 220.0).abs() < 0.2);
    }

    #[test]
    fn rotation_matches_the_trigonometric_pair() {
        let increment = core::f32::consts::TAU * 440.0 / 48_000.0;
        let (sine, cosine) = rotation(increment);
        assert!((sine - increment.sin()).abs() < 1e-6);
        assert!((cosine - increment.cos()).abs() < 1e-6);
    }

    #[test]
    fn silence_until_a_note_arrives() {
        let mut synth = DemoSynth::default();
        assert!(synth.prepare(48_000.0, 128, 0, 2));
        assert!(
            render(&mut synth, 64, &[])
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }

    #[test]
    fn a_held_note_sounds_and_a_released_one_fades_out() {
        let mut synth = DemoSynth::default();
        assert!(synth.prepare(48_000.0, 4096, 0, 2));
        let note_on = MidiEvent {
            frame: 0,
            data: [0x90, 69, 100],
            length: 3,
        };
        let sounding = render(&mut synth, 2048, &[note_on]);
        assert!(sounding.iter().any(|sample| sample.abs() > 0.01));

        let note_off = MidiEvent {
            frame: 0,
            data: [0x80, 69, 0],
            length: 3,
        };
        render(&mut synth, 4096, &[note_off]);
        // The default release runs for a little under half a second, so give
        // it a second of audio before expecting silence.
        for _ in 0..12 {
            render(&mut synth, 4096, &[]);
        }
        let after = render(&mut synth, 512, &[]);
        assert!(after.iter().all(|sample| sample.abs() < 1e-6));
    }

    #[test]
    fn every_output_sample_stays_inside_the_audio_range() {
        let mut synth = DemoSynth::default();
        assert!(synth.prepare(48_000.0, 4096, 0, 2));
        let chord: Vec<MidiEvent> = (60..76)
            .map(|note| MidiEvent {
                frame: 0,
                data: [0x90, note, 127],
                length: 3,
            })
            .collect();
        let rendered = render(&mut synth, 4096, &chord);
        assert!(rendered.iter().all(|sample| sample.abs() <= 1.0));
    }

    #[test]
    fn presets_and_state_round_trip() {
        let mut synth = DemoSynth::default();
        assert!(synth.load_preset("pad"));
        let mut state = [0; 32];
        assert_eq!(synth.save_state(&mut state), Some(20));
        assert!(synth.load_preset("bass"));
        assert!(synth.load_state(&state[..20]));
        assert_eq!(synth.get_parameter(PARAM_ATTACK), Some(0.55_f32 as f64));
    }
}
