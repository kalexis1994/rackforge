//! Research acoustic paths for compact action and pedal sources.
//!
//! These signals are synthetic source amplitudes, not measured forces in N.
//! The emission gain and case bandwidth require acoustic calibration. Geometry
//! determines receiver gain and extra travel time relative to the board centre.
//! This is a reduced radiation model, not a reciprocal model of case vibration.

use crate::math::{expf, sqrtf};

const HISTORY: usize = 1024;

pub fn receiver_geometry(
    board_range: f32,
    board_response: f32,
    pattern: f32,
    reference: f32,
    preamp: f32,
    extra_range_squared: f32,
    sample_rate: f32,
    sound_speed: f32,
) -> (f32, f32) {
    let range = sqrtf(board_range * board_range + extra_range_squared.max(0.0));
    let response = (1.0 - pattern) + (board_response - (1.0 - pattern)) * board_range / range;
    let gain = reference / range * preamp * response;
    let delay = (range - board_range) * sample_rate / sound_speed;
    (gain, delay)
}

pub struct Frame {
    pub direct: [f32; 2],
    pub radiated: [f32; 2],
}

pub struct Path {
    history: [[f32; HISTORY]; 2],
    cursor: usize,
    low: [f32; 2],
    coefficient: f32,
    emission: f32,
    gain: [f32; 2],
    delay: [f32; 2],
}

impl Default for Path {
    fn default() -> Self {
        Self {
            history: [[0.0; HISTORY]; 2],
            cursor: 0,
            low: [0.0; 2],
            coefficient: 1.0,
            emission: 1.0,
            gain: [1.0; 2],
            delay: [0.0; 2],
        }
    }
}

impl Path {
    pub fn configure(&mut self, emission: f32, corner: f32, rate: f32) {
        self.emission = emission.clamp(0.0, 1.0);
        self.coefficient = 1.0 - expf(-core::f32::consts::TAU * corner / rate);
    }

    pub fn receiver(&mut self, side: usize, gain: f32, delay: f32) {
        self.gain[side] = gain;
        self.delay[side] = delay.clamp(0.0, (HISTORY - 2) as f32);
    }

    pub fn tick(&mut self, source: [f32; 2]) -> Frame {
        let mut frame = Frame {
            direct: [0.0; 2],
            radiated: [0.0; 2],
        };
        for side in 0..2 {
            self.low[side] += self.coefficient * (source[side] - self.low[side]);
            let pressure = self.low[side] * self.emission;
            frame.radiated[side] = pressure;
            self.history[side][self.cursor] = pressure;
            let whole = self.delay[side] as usize;
            let fraction = self.delay[side] - whole as f32;
            let at = (self.cursor + HISTORY - whole) % HISTORY;
            let before = (at + HISTORY - 1) % HISTORY;
            let heard =
                self.history[side][at] * (1.0 - fraction) + self.history[side][before] * fraction;
            frame.direct[side] = heard * self.gain[side];
        }
        self.cursor = (self.cursor + 1) % HISTORY;
        frame
    }

    pub fn clear(&mut self) {
        self.history = [[0.0; HISTORY]; 2];
        self.cursor = 0;
        self.low = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_action_component_does_not_drive_the_string_bridge() {
        let saved = crate::MECHANICAL_RADIATION.get();
        crate::MECHANICAL_RADIATION.set(1.0);
        let mut voice = crate::Voice::<1>::default();
        voice.onset = 1.0;
        voice.partial_count = 1;
        voice.partials[0].mechanical_source = true;
        voice.partials[0].set_lane(0, crate::Component::start(1.0, 1000.0, 0.99, 44100.0));
        let mut heard = 0.0;
        for _ in 0..100 {
            assert_eq!(voice.tick(0.0), 0.0);
            heard += voice.keybed_out * voice.keybed_out;
        }
        crate::MECHANICAL_RADIATION.set(saved);
        assert!(heard > 0.0);
    }

    #[test]
    fn remote_mechanism_arrives_later_and_is_quieter() {
        for rate in [44100.0, 48000.0, 96000.0, 192000.0] {
            let (close, a) = receiver_geometry(0.5, 1.0, 0.5, 2.0, 1.57, 0.81, rate, 343.0);
            let (far, b) = receiver_geometry(4.0, 1.0, 0.5, 2.0, 1.57, 0.81, rate, 343.0);
            assert!(close > far && a > 0.0 && b > 0.0 && a < 1022.0);
            let (pedal, d) = receiver_geometry(0.5, 1.0, 0.5, 2.0, 1.57, 2.41, rate, 343.0);
            assert!(pedal < close && d > a && d < 1022.0);
        }
    }

    #[test]
    fn fractional_propagation_and_case_filter_do_not_create_energy() {
        let mut path = Path::default();
        path.configure(0.25, 1500.0, 44100.0);
        path.receiver(0, 1.0, 3.5);
        path.receiver(1, -1.0, 3.5);
        let mut energy = 0.0;
        for n in 0..10000 {
            let frame = path.tick(if n == 0 { [1.0; 2] } else { [0.0; 2] });
            assert!((frame.direct[0] + frame.direct[1]).abs() < 1e-8);
            if n < 3 {
                assert_eq!(frame.direct, [0.0; 2]);
            }
            energy += frame.direct[0] * frame.direct[0];
        }
        assert!(energy > 0.0 && energy < 0.25 * 0.25);
        path.clear();
        assert_eq!(path.tick([0.0; 2]).direct, [0.0; 2]);
    }
}
