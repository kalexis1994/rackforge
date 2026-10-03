//! Passive modal velocity admittance, discretized by the trapezoidal rule.
//! Radiation is a separate observation of these same mechanical modes.
use crate::math::{expf, powf, roundf, sincosf};

/// Positive, smooth modal-mass calibration across plate frequency regimes.
/// The same resulting mass must be used for admittance and radiation. This
/// reduced calibration does not identify rib geometry from microphone audio.
pub(crate) fn regional_mobility(frequency: f32, mean: f32, bass: f32, treble: f32) -> f32 {
    if bass == 1.0 && treble == 1.0 {
        return mean; // Preserve the accepted calibration bit for bit.
    }
    let low = frequency.max(0.0) / 200.0;
    let high = frequency.max(0.0) / 1100.0;
    let low4 = low * low * low * low;
    let high4 = high * high * high * high;
    mean * powf(bass.clamp(0.125, 8.0), 1.0 / (1.0 + low4))
        * powf(treble.clamp(0.125, 8.0), high4 / (1.0 + high4))
}

pub(crate) fn warped_omega(frequency: f32, rate: f32) -> f32 {
    let (s, c) = sincosf(core::f32::consts::PI * frequency.clamp(0.01, 0.45 * rate) / rate);
    2.0 * rate * s / c
}

/// Exact rank-one contraction for a per-string amplitude loss rate.
/// `norm` is the squared mechanical coupling vector, not strike amplitudes.
pub(crate) fn drain_coefficient(rate: f32, norm: f32, sample_rate: f32) -> f32 {
    let x = rate.max(0.0) * norm / sample_rate;
    let lost = if x < 0.001 {
        x * (1.0 - 0.5 * x + x * x / 6.0)
    } else {
        1.0 - expf(-x)
    };
    lost / norm.max(1e-12)
}

/// Frozen-frequency, weak-coupling approximation to complex bridge loading.
/// With z=s+i*c rotating as exp(-i*omega*t), positive Im(Y) lowers frequency.
/// The coherent subspace contracts by exp(-gamma*norm*dt) and rotates;
/// the orthogonal subspace is unchanged. This does not add board memory.
pub(crate) fn complex_loading(gamma: f32, beta: f32, norm: f32, rate: f32) -> (f32, f32) {
    let norm = norm.max(1e-12);
    let drain = drain_coefficient(gamma, norm, rate);
    let radius = 1.0 - drain * norm;
    let (sin, cos) = loading_sincos(beta * norm / rate);
    // Retain accuracy when the phase increment is much smaller than one.
    let phase_loss = if cos > 0.5 {
        sin * sin / (1.0 + cos)
    } else {
        1.0 - cos
    };
    (drain + radius * phase_loss / norm, radius * sin / norm)
}

pub(crate) fn loading_sincos(angle: f32) -> (f32, f32) {
    // Higher-order coefficients here avoid accumulated phase error in a
    // conservative projection. Computed once per partial, not per sample.
    let quadrant = roundf(angle * core::f32::consts::FRAC_2_PI);
    let r = angle - quadrant * core::f32::consts::FRAC_PI_2;
    let r2 = r * r;
    let sin = r * (1.0 - r2 / 6.0 * (1.0 - r2 / 20.0 * (1.0 - r2 / 42.0 * (1.0 - r2 / 72.0))));
    let cos = 1.0
        - r2 / 2.0 * (1.0 - r2 / 12.0 * (1.0 - r2 / 30.0 * (1.0 - r2 / 56.0 * (1.0 - r2 / 90.0))));
    match (quadrant as i32).rem_euclid(4) {
        0 => (sin, cos),
        1 => (cos, -sin),
        2 => (-sin, -cos),
        _ => (-cos, sin),
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct MobilityMode {
    pub omega2: f32,
    pub sigma: f32,
    pub inverse_mass: f32,
    pub q: u16,
    pub a: f32,
    pub b: f32,
    pub c: f32,
}

impl MobilityMode {
    pub fn conductance(self, omega: f32, projection: f32) -> f32 {
        let detuning = self.omega2 - omega * omega;
        let loss = 2.0 * self.sigma * omega;
        self.inverse_mass * 2.0 * self.sigma * omega * omega * projection * projection
            / (detuning * detuning + loss * loss).max(1e-20)
    }

    pub fn susceptance(self, omega: f32, projection: f32) -> f32 {
        let detuning = self.omega2 - omega * omega;
        let loss = 2.0 * self.sigma * omega;
        self.inverse_mass * omega * detuning * projection * projection
            / (detuning * detuning + loss * loss).max(1e-20)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Coefficients {
    pub av: f32,
    pub aq: f32,
    pub force: f32,
    pub half_step: f32,
    pub mobility: MobilityMode,
}

impl Coefficients {
    pub fn new(frequency: f32, loss: f32, mass: f32, rate: f32) -> Self {
        let omega = warped_omega(frequency, rate);
        let sigma = 0.5 * omega * loss;
        let dt = 1.0 / rate;
        let beta = 0.25 * omega * omega * dt * dt;
        let denominator = 1.0 + sigma * dt + beta;
        Self {
            av: (1.0 - sigma * dt - beta) / denominator,
            aq: -omega * omega * dt / denominator,
            force: dt / (2.0 * mass * denominator),
            half_step: 0.5 * dt,
            mobility: MobilityMode {
                omega2: omega * omega,
                sigma,
                inverse_mass: 1.0 / mass,
                ..MobilityMode::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regional_modal_masses_preserve_loading_and_passivity() {
        for frequency in [50.0, 200.0, 500.0, 1100.0, 8000.0] {
            let mean = 0.0005;
            assert_eq!(regional_mobility(frequency, mean, 1.0, 1.0), mean);
            for (bass, treble) in [(0.125, 8.0), (2.0, 4.0), (8.0, 0.125)] {
                let mobility = regional_mobility(frequency, mean, bass, treble);
                assert!(mobility.is_finite() && mobility > 0.0);
                let mass = 1.0 / (4.0 * 20.0 * mobility);
                let c = Coefficients::new(frequency, 0.04, mass, 44100.0);
                // Changing a modal mass must scale both the time-domain force
                // response and the collocated load by precisely that factor.
                let original = Coefficients::new(frequency, 0.04, 25.0, 44100.0);
                let ratio = 25.0 / mass;
                assert!((c.force / original.force - ratio).abs() < 1e-5 * ratio);
                for f in [70.0, 370.0, 2000.0, 9000.0] {
                    let omega = warped_omega(f, 44100.0);
                    let y = c.mobility.conductance(omega, 0.7);
                    let old = original.mobility.conductance(omega, 0.7);
                    assert!(y.is_finite() && y >= 0.0);
                    assert!((y / old - ratio).abs() < 1e-5 * ratio);
                }
            }
        }
    }

    #[test]
    fn reactive_mobility_matches_velocity_phase() {
        for rate in [44100.0, 96000.0] {
            let coeff = Coefficients::new(440.0, 0.04, 12.0, rate);
            for frequency in [220.0, 370.0, 510.0, 900.0] {
                let (mut q, mut v, mut previous) = (0.0, 0.0, 0.0);
                let (mut cross, mut sine2) = (0.0f64, 0.0f64);
                for n in 0..192000 {
                    let (sin, cos) = sincosf(core::f32::consts::TAU * frequency * n as f32 / rate);
                    let next = coeff.av * v + coeff.aq * q + coeff.force * (cos + previous);
                    q += coeff.half_step * (v + next);
                    v = next;
                    previous = cos;
                    if n >= 96000 {
                        cross += v as f64 * sin as f64;
                        sine2 += (sin as f64).powi(2);
                    }
                }
                let predicted = coeff
                    .mobility
                    .susceptance(warped_omega(frequency, rate), 1.0)
                    as f64;
                assert!(((-cross / sine2) / predicted - 1.0).abs() < 0.01);
            }
        }
    }

    #[test]
    fn loading_matches_the_time_domain_velocity_response() {
        for rate in [44100.0, 96000.0] {
            let c = Coefficients::new(440.0, 0.04, 12.0, rate);
            let (mut q, mut v, mut previous) = (0.0, 0.0, 0.0);
            let (mut power, mut force2) = (0.0f64, 0.0f64);
            for n in 0..96000 {
                let force = sincosf(core::f32::consts::TAU * 370.0 * n as f32 / rate).1;
                let next = c.av * v + c.aq * q + c.force * (force + previous);
                q += c.half_step * (v + next);
                v = next;
                previous = force;
                if n >= 48000 {
                    power += force as f64 * v as f64;
                    force2 += (force as f64).powi(2);
                }
            }
            let predicted = c.mobility.conductance(warped_omega(370.0, rate), 1.0) as f64;
            assert!((power / force2 / predicted - 1.0).abs() < 0.01);
        }
    }

    #[test]
    fn supplied_work_equals_stored_energy_and_dissipation() {
        // This checks a physical invariant against the discrete recurrence,
        // including an unforced tail, rather than merely checking bounded output.
        for rate in [44100.0, 48000.0, 96000.0] {
            for frequency in [50.0, 370.0, 8000.0] {
                let c = Coefficients::new(frequency, 0.035, 12.0, rate);
                let (mut q, mut v, mut previous_force) = (0.0f32, 0.0f32, 0.0f32);
                let (mut work, mut dissipated) = (0.0f64, 0.0f64);
                let mut max_energy = 0.0f64;
                for n in 0..6000 {
                    let force = if n < 800 {
                        sincosf(0.17 * n as f32).0 * 5.0
                    } else {
                        0.0
                    };
                    let next_v = c.av * v + c.aq * q + c.force * (force + previous_force);
                    let next_q = q + c.half_step * (v + next_v);
                    let average_v = 0.5 * (v as f64 + next_v as f64);
                    work += 0.5 * (force as f64 + previous_force as f64) * average_v / rate as f64;
                    dissipated +=
                        24.0 * c.mobility.sigma as f64 * average_v * average_v / rate as f64;
                    let stored = 6.0
                        * ((next_v as f64).powi(2)
                            + c.mobility.omega2 as f64 * (next_q as f64).powi(2));
                    max_energy = max_energy.max(stored);
                    assert!(
                        (work - dissipated - stored).abs() < 2e-4 * max_energy + 1e-10,
                        "energy mismatch at {frequency} Hz / {rate} Hz, frame {n}"
                    );
                    (q, v, previous_force) = (next_q, next_v, force);
                }
            }
        }
    }

    #[test]
    fn collocated_conductance_is_nonnegative_and_shape_sign_invariant() {
        let mode = Coefficients::new(370.0, 0.025, 12.0, 44100.0).mobility;
        for i in 1..1000 {
            let w = warped_omega(i as f32 * 10.0, 44100.0);
            let a = mode.conductance(w, 0.7);
            assert!(a.is_finite() && a >= 0.0);
            assert_eq!(a, mode.conductance(w, -0.7));
            assert_eq!(mode.conductance(w, 0.0), 0.0);
        }
    }
}
