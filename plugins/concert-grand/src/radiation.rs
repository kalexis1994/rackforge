//! Shared experimental mode shape for bridge excitation and spatial radiation.
//! This is a reduced plate basis, not a measured soundboard or an acoustic solver.
use crate::math::{sincosf, sqrtf};

/// Retarded Rayleigh pressure from a modal surface velocity. The common
/// height/c travel time is removed equally from both capsules and every mode.
/// This research observer has no acoustic feedback into the mechanical bank.
pub(crate) struct RetardedPressure {
    kernel: [[f32; 256]; 2],
    history: [f32; 256],
    previous_velocity: f32,
    cursor: usize,
    end: usize,
}

impl Default for RetardedPressure {
    fn default() -> Self {
        Self {
            kernel: [[0.0; 256]; 2],
            history: [0.0; 256],
            previous_velocity: 0.0,
            cursor: 0,
            end: 0,
        }
    }
}

impl RetardedPressure {
    pub fn clear_kernel(&mut self) {
        self.kernel = [[0.0; 256]; 2];
        self.end = 0;
    }
    pub fn deposit(&mut self, side: usize, delay: f32, weight: f32) {
        let index = delay as usize;
        assert!(
            index + 1 < 256,
            "retarded pressure support exceeds 256 samples"
        );
        let frac = delay - index as f32;
        self.kernel[side][index] += weight * (1.0 - frac);
        self.kernel[side][index + 1] += weight * frac;
        self.end = self.end.max(index + 2);
    }
    pub fn tick(&mut self, velocity: f32, rate: f32) -> (f32, f32) {
        self.history[self.cursor] = (velocity - self.previous_velocity) * rate;
        self.previous_velocity = velocity;
        let mut out = [0.0; 2];
        for delay in 0..self.end {
            let value = self.history[(self.cursor + 256 - delay) % 256];
            out[0] += value * self.kernel[0][delay];
            out[1] += value * self.kernel[1][delay];
        }
        self.cursor = (self.cursor + 1) % 256;
        (out[0], out[1])
    }
}

#[cfg(test)]
mod retarded_tests {
    use super::*;

    #[test]
    fn pressure_matches_discrete_acceleration_and_retarded_phase() {
        for rate in [44100.0f32, 48000.0, 96000.0] {
            for frequency in [130.0f32, 1000.0, 8000.0, 16000.0] {
                let mut observer = RetardedPressure::default();
                observer.deposit(0, 3.25, 0.7);
                observer.deposit(1, 6.5, -0.3);
                let omega = core::f64::consts::TAU * frequency as f64 / rate as f64;
                let mut squared = 0.0;
                let mut reference = 0.0;
                for n in 0..4096 {
                    let velocity = (omega * n as f64).sin() as f32;
                    let actual = observer.tick(velocity, rate);
                    if n > 300 {
                        let acceleration = |d: usize| {
                            rate as f64
                                * ((omega * (n - d) as f64).sin()
                                    - (omega * (n - d - 1) as f64).sin())
                        };
                        let expected = [
                            0.7 * (0.75 * acceleration(3) + 0.25 * acceleration(4)),
                            -0.3 * (0.5 * acceleration(6) + 0.5 * acceleration(7)),
                        ];
                        squared += (actual.0 as f64 - expected[0]).powi(2)
                            + (actual.1 as f64 - expected[1]).powi(2);
                        reference += expected[0].powi(2) + expected[1].powi(2);
                    }
                }
                assert!(
                    (squared / reference).sqrt() < 1e-5,
                    "{frequency} Hz / {rate} Hz"
                );
            }
        }
    }

    #[test]
    fn finite_pressure_has_no_energy_or_dc_without_motion() {
        let mut observer = RetardedPressure::default();
        observer.deposit(0, 2.5, 1.0);
        observer.deposit(1, 9.3, 1.0);
        for n in 0..2048 {
            let (l, r) = observer.tick(0.2, 44100.0);
            if n > 256 {
                assert_eq!((l, r), (0.0, 0.0));
            }
        }
    }
}

/// Real two-tap filter matching G at omega, using the e^(j omega n)
/// convention: G = a + b exp(-j omega), hence b = -Im(G)/sin(omega).
/// The caller's modes are strictly between DC and Nyquist.
pub(crate) fn output_coefficients(gain: (f32, f32), omega: f32) -> (f32, f32) {
    let (sin, cos) = sincosf(omega);
    let denominator = if sin.abs() < 1e-6 { 1e-6 } else { sin };
    let b = -gain.1 / denominator;
    (gain.0 - b * cos, b)
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Shape {
    pub q: f32,
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub qy: f32,
    pub theta_y: f32,
}

impl Shape {
    pub fn at(self, x: f32, y: f32) -> f32 {
        let (s, c) = sincosf(core::f32::consts::PI * self.q * x);
        (self.a + self.b * c + self.c * s) * self.across(y)
    }

    pub fn across(self, y: f32) -> f32 {
        sincosf(core::f32::consts::PI * self.qy * y + self.theta_y).1
    }

    // Mass normalization on the unit rectangle. Unlike microphone-power
    // normalization this cannot undo cancellation at a particular receiver.
    pub fn normalized(mut self) -> Self {
        let mut along_energy = 0.0;
        let mut across_energy = 0.0;
        for x in 0..40 {
            let (s, c) = sincosf(core::f32::consts::PI * self.q * (x as f32 + 0.5) / 40.0);
            let value = self.a + self.b * c + self.c * s;
            along_energy += value * value / 40.0;
        }
        for y in 0..24 {
            let value = self.across((y as f32 + 0.5) / 24.0 - 0.5);
            across_energy += value * value / 24.0;
        }
        let energy = along_energy * across_energy;
        let scale = if energy > 1e-8 {
            1.0 / sqrtf(energy)
        } else {
            0.0
        };
        self.a *= scale;
        self.b *= scale;
        self.c *= scale;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_filter_preserves_propagation_phase_not_its_conjugate() {
        for frequency in [50.0, 261.626, 349.228, 391.995, 4000.0, 8500.0] {
            let omega = core::f32::consts::TAU * frequency / 44100.0;
            let (im, re) = sincosf(-core::f32::consts::TAU * frequency * 0.7 / 343.0);
            let (a, b) = output_coefficients((re, im), omega);
            let (s, c) = sincosf(omega);
            assert!((a + b * c - re).abs() < 2e-5);
            assert!((-b * s - im).abs() < 2e-5, "phase sign at {frequency}");
        }
    }

    #[test]
    fn mass_normalization_preserves_receiver_cancellation() {
        let odd = Shape {
            q: 1.0,
            b: 1.0,
            ..Shape::default()
        }
        .normalized();
        let mut sum = 0.0;
        let mut energy = 0.0;
        for x in 0..400 {
            let value = odd.at((x as f32 + 0.5) / 400.0, 0.0);
            sum += value / 400.0;
            energy += value * value / 400.0;
        }
        assert!(sum.abs() < 1e-5, "a cancelling mode must stay cancelling");
        assert!((energy - 1.0).abs() < 1e-4);
    }

    #[test]
    fn bridge_projection_matches_the_radiating_surface() {
        for q in [0.0, 1.0, 7.0, 15.0] {
            let shape = Shape {
                q,
                a: 0.2,
                b: 0.8,
                c: -0.4,
                qy: 3.1,
                theta_y: 0.7,
            }
            .normalized();
            for i in 0..16 {
                let x = i as f32 / 15.0;
                let (s, c) = sincosf(core::f32::consts::PI * q * x);
                let bridge = shape.across(0.0) * (shape.a + shape.b * c + shape.c * s);
                assert!((bridge - shape.at(x, 0.0)).abs() < 1e-6);
            }
        }
    }
}
