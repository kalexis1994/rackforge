//! ISO 9613-1 absorption and a causal minimum-phase FIR approximation.
//! Atmospheric loss belongs to travelled metres, not to the age of a note.
use crate::math::{expf, powf, sincosf};

pub const MAX_TAPS: usize = 32;
const MAX_STAGES: usize = 16;
const GRID: usize = 128;

#[derive(Clone, Copy)]
pub struct Atmosphere {
    classical: f32,
    oxygen: f32,
    nitrogen: f32,
    oxygen_relaxation: f32,
    nitrogen_relaxation: f32,
}

impl Atmosphere {
    /// Temperature °C, relative humidity %, absolute pressure kPa.
    /// Inputs are bounded to the reference model's supported conditions.
    pub fn new(temperature: f32, humidity: f32, pressure: f32) -> Self {
        let kelvin = temperature.clamp(-20.0, 50.0) + 273.15;
        let relative_temperature = kelvin / 293.15;
        let relative_pressure = pressure.clamp(50.0, 200.0) / 101.325;
        let saturation = powf(10.0, -6.8346 * powf(273.16 / kelvin, 1.261) + 4.6151);
        let h = humidity.clamp(10.0, 100.0) * saturation / relative_pressure;
        Self {
            classical: 1.84e-11 * powf(relative_temperature, 0.5) / relative_pressure,
            oxygen: powf(relative_temperature, -2.5) * 0.01275 * expf(-2239.1 / kelvin),
            nitrogen: powf(relative_temperature, -2.5) * 0.1068 * expf(-3352.0 / kelvin),
            oxygen_relaxation: relative_pressure * (24.0 + 4.04e4 * h * (0.02 + h) / (0.391 + h)),
            nitrogen_relaxation: relative_pressure
                * powf(relative_temperature, -0.5)
                * (9.0 + 280.0 * h * expf(-4.170 * (powf(relative_temperature, -1.0 / 3.0) - 1.0))),
        }
    }

    /// Pressure-amplitude attenuation in Nepers/m. ISO's dB/m is 8.686×this.
    pub fn nepers_per_m(&self, frequency: f32) -> f32 {
        let square = frequency * frequency;
        square
            * (self.classical
                + self.oxygen / (self.oxygen_relaxation + square / self.oxygen_relaxation)
                + self.nitrogen / (self.nitrogen_relaxation + square / self.nitrogen_relaxation))
    }

    /// Minimum-phase log impulse, per metre. Calculated once per room retune,
    /// shared by all paths; each path exponentiates only its distance.
    pub fn kernel(&self, rate: f32, strength: f32) -> Kernel {
        let mut loss = [0.0; GRID / 2 + 1];
        for (k, value) in loss.iter_mut().enumerate() {
            *value = -self.nepers_per_m(k as f32 * rate / GRID as f32) * strength.max(0.0);
        }
        let mut cepstrum = [0.0; MAX_TAPS];
        for (n, value) in cepstrum.iter_mut().enumerate() {
            let mut sum = loss[0];
            for (k, loss) in loss.iter().copied().enumerate().skip(1) {
                // Reduce before sincosf: its documented argument range is
                // small, while n*k can span several revolutions.
                let angle = core::f32::consts::TAU * ((n * k) % GRID) as f32 / GRID as f32;
                let weight = if k == GRID / 2 { 1.0 } else { 2.0 };
                sum += weight * loss * sincosf(angle).1;
            }
            *value = sum / GRID as f32 * if n == 0 { 1.0 } else { 2.0 };
        }
        Kernel {
            cepstrum,
            strength: strength.max(0.0),
        }
    }
}

pub struct Kernel {
    cepstrum: [f32; MAX_TAPS],
    strength: f32,
}

#[derive(Clone, Copy)]
pub struct Path {
    coefficients: [f32; MAX_TAPS],
    history: [[f32; MAX_TAPS]; MAX_STAGES],
    cursor: usize,
    taps: usize,
    stages: usize,
    remaining: usize,
}

impl Default for Path {
    fn default() -> Self {
        Self {
            coefficients: [0.0; MAX_TAPS],
            history: [[0.0; MAX_TAPS]; MAX_STAGES],
            cursor: 0,
            taps: 0,
            stages: 0,
            remaining: 0,
        }
    }
}

impl Path {
    pub fn configure(&mut self, kernel: &Kernel, metres: f32, enabled: bool) {
        if !enabled || metres <= 0.0 {
            self.taps = 0;
            self.clear();
            return;
        }
        // Large absorption amounts use several identical shorter paths.
        // This avoids underflow when the existing advanced amount reaches
        // 16× its default. Ordinary concert paths need only one stage.
        let previous_stages = self.stages;
        self.stages = 1;
        while metres * kernel.strength / self.stages as f32 > 100.0 && self.stages < MAX_STAGES {
            self.stages += 1;
        }
        let metres = metres / self.stages as f32;
        for state in &mut self.history[previous_stages.min(self.stages)..self.stages] {
            *state = [0.0; MAX_TAPS];
        }
        // Existing input history may still matter after a path edit.
        if self.remaining > 0 {
            self.remaining = self.taps.max(MAX_TAPS) * self.stages;
        }
        self.taps = if metres * kernel.strength <= 32.0 {
            16
        } else {
            MAX_TAPS
        };
        self.coefficients = [0.0; MAX_TAPS];
        self.coefficients[0] = expf(kernel.cepstrum[0] * metres);
        // Exponential power-series recurrence, not an FFT in the audio loop.
        for n in 1..self.taps {
            let mut sum = 0.0;
            for k in 1..=n {
                sum += k as f32 * kernel.cepstrum[k] * metres * self.coefficients[n - k];
            }
            self.coefficients[n] = sum / n as f32;
        }
        // A finite prefix slightly misses DC. Air loss is zero at DC;
        // enforce that boundary, then validate passivity over frequency.
        let dc: f32 = self.coefficients[..self.taps].iter().sum();
        for value in &mut self.coefficients[..self.taps] {
            *value /= dc;
        }
    }

    pub fn clear(&mut self) {
        self.history = [[0.0; MAX_TAPS]; MAX_STAGES];
        self.cursor = 0;
        self.remaining = 0;
    }

    #[inline]
    pub fn tick(&mut self, input: f32) -> f32 {
        if self.taps == 0 {
            return input;
        }
        if input == 0.0 && self.remaining == 0 {
            return 0.0;
        }
        if input != 0.0 {
            self.remaining = self.taps * self.stages;
        } else {
            self.remaining -= 1;
        }
        let mut output = input;
        for stage in &mut self.history[..self.stages] {
            stage[self.cursor] = output;
            output = 0.0;
            for tap in 0..self.taps {
                output +=
                    self.coefficients[tap] * stage[(self.cursor + MAX_TAPS - tap) & (MAX_TAPS - 1)];
            }
        }
        self.cursor = (self.cursor + 1) & (MAX_TAPS - 1);
        if self.remaining == 0 {
            self.clear();
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_reference_atmosphere_has_the_expected_units_and_values() {
        let atmosphere = Atmosphere::new(20.0, 50.0, 101.325);
        for (frequency, db_m) in [
            (1000.0, 0.00466473),
            (4000.0, 0.02966553),
            (8000.0, 0.10529093),
        ] {
            let actual = 8.686 * atmosphere.nepers_per_m(frequency);
            assert!(
                (actual / db_m - 1.0).abs() < 0.0001,
                "{frequency}: {actual}"
            );
        }
        assert_eq!(atmosphere.nepers_per_m(0.0), 0.0);
        assert!(
            Atmosphere::new(20.0, 10.0, 101.325).nepers_per_m(4000.0)
                > atmosphere.nepers_per_m(4000.0)
        );
    }

    #[test]
    fn advanced_absorption_extreme_stays_finite_and_clears_all_stages() {
        let kernel = Atmosphere::new(20.0, 50.0, 101.325).kernel(96_000.0, 16.0);
        let mut path = Path::default();
        path.configure(&kernel, 100.0, true);
        assert_eq!(path.stages, 16);
        for n in 0..4000 {
            let input = (n as f32 * 0.1).sin();
            let output = path.tick(input);
            assert!(output.is_finite() && output.abs() <= 1.01);
        }
        path.clear();
        assert_eq!(path.tick(0.0), 0.0);
    }

    #[test]
    fn causal_fir_matches_iso_loss_and_never_boosts_the_supported_paths() {
        let atmosphere = Atmosphere::new(20.0, 50.0, 101.325);
        for rate in [44100.0, 48000.0, 96000.0] {
            let kernel = atmosphere.kernel(rate, 1.0);
            for distance in [0.5, 2.64, 16.0, 32.0, 100.0] {
                let mut path = Path::default();
                path.configure(&kernel, distance, true);
                let mut impulse = [0.0; MAX_TAPS];
                for (n, sample) in impulse.iter_mut().enumerate() {
                    *sample = path.tick(if n == 0 { 1.0 } else { 0.0 });
                }
                assert!((impulse.iter().sum::<f32>() - 1.0).abs() < 2e-6);
                let mut maximum_error = 0.0f64;
                for frequency in (20..=16000).step_by(20) {
                    let omega = std::f64::consts::TAU * frequency as f64 / rate as f64;
                    let mut real = 0.0;
                    let mut imaginary = 0.0;
                    for (n, h) in impulse.iter().enumerate() {
                        real += *h as f64 * (omega * n as f64).cos();
                        imaginary -= *h as f64 * (omega * n as f64).sin();
                    }
                    let magnitude = (real * real + imaginary * imaginary).sqrt();
                    assert!(
                        magnitude <= 1.00001,
                        "active air filter: {rate}/{distance}/{frequency}"
                    );
                    let actual_db = 20.0 * magnitude.log10();
                    let expected_db = -8.686 * atmosphere.nepers_per_m(frequency as f32) * distance;
                    maximum_error = maximum_error.max((actual_db - expected_db as f64).abs());
                }
                let tolerance = if distance <= 32.0 { 0.11 } else { 0.5 };
                assert!(
                    maximum_error < tolerance,
                    "{rate}/{distance}: error {maximum_error}"
                );
                std::println!(
                    "air_fir rate={rate} metres={distance} max_error_db={maximum_error:.6}"
                );
                path.clear();
                assert_eq!(path.tick(0.0), 0.0);
                path.configure(&kernel, distance, false);
                assert_eq!(path.tick(0.25), 0.25);
            }
        }
    }
}
