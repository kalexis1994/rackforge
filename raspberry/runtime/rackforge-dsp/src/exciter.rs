use crate::{PrepareError, StereoFrame, StereoProcessor};
use std::f32::consts::TAU;

const MIN_SAMPLE_RATE: f32 = 8_000.0;
const MAX_SAMPLE_RATE: f32 = 384_000.0;
const OVERSAMPLING: f32 = 2.0;
const SMOOTHING_SECONDS: f32 = 0.020;
const EQ_LOW_FREQUENCY_HZ: f32 = 240.0;
const EQ_HIGH_FREQUENCY_HZ: f32 = 4_000.0;
const HARMONIC_DC_BLOCK_HZ: f32 = 12.0;
// The excited branch must remain an accent, not become a second saturated
// instrument. These conservative gains keep the low-order harmonic residual
// below the emphasized fundamental even at the fully-wet setting.
const PRESENCE_GAIN: f32 = 0.20;
const HARMONIC_GAIN: f32 = 0.85;
const HALF_WAVE_DRIVE: f32 = 1.50;
const EMPHATIC_FREQUENCIES_HZ: [f32; 10] = [
    600.0, 800.0, 1_100.0, 1_500.0, 2_200.0, 3_000.0, 4_200.0, 5_800.0, 8_000.0, 11_000.0,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExciterParameters {
    pub enabled: bool,
    /// Internal balance from unprocessed (-1.0) to excited (+1.0).
    pub blend: f32,
    /// Classic discrete emphasis selector, from 1 through 10.
    pub emphatic_point: u8,
    pub eq_low_db: f32,
    pub eq_high_db: f32,
    /// Outer dry/effect balance, from DRY (0.0) to WET (1.0).
    pub mix: f32,
}

impl Default for ExciterParameters {
    fn default() -> Self {
        Self {
            enabled: false,
            blend: 1.0,
            emphatic_point: 5,
            eq_low_db: 0.0,
            eq_high_db: 0.0,
            mix: 1.0,
        }
    }
}

impl ExciterParameters {
    pub fn validate(self) -> Result<Self, &'static str> {
        validate_range(self.blend, -1.0, 1.0, "exciter blend")?;
        if !(1..=10).contains(&self.emphatic_point) {
            return Err("exciter emphatic point");
        }
        validate_range(self.eq_low_db, -12.0, 12.0, "exciter low EQ")?;
        validate_range(self.eq_high_db, -12.0, 12.0, "exciter high EQ")?;
        validate_range(self.mix, 0.0, 1.0, "exciter dry/wet")?;
        Ok(self)
    }
}

fn validate_range(
    value: f32,
    minimum: f32,
    maximum: f32,
    name: &'static str,
) -> Result<(), &'static str> {
    if value.is_finite() && (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(name)
    }
}

/// Classic workstation-style parallel exciter.
///
/// A two-band EQ feeds both an unprocessed branch and an excitation branch.
/// The selected emphatic point controls the band that produces presence plus
/// even and odd harmonics. `blend` crossfades those branches and `mix`
/// crossfades the complete effect against the original input. The nonlinear
/// branch runs at 2x; the dry path is zero-latency and bit-exact while disabled.
pub struct Exciter {
    sample_rate: f32,
    left: Channel,
    right: Channel,
    eq_low_coefficient: f32,
    eq_high_coefficient: f32,
    harmonic_dc_block_coefficient: f32,
    emphasis_highpass_coefficient: Smoothed,
    emphasis_lowpass_coefficient: Smoothed,
    low_gain: Smoothed,
    high_gain: Smoothed,
    excited_weight: Smoothed,
    wet: Smoothed,
}

impl Exciter {
    pub fn new(sample_rate: f32) -> Result<Self, PrepareError> {
        if !sample_rate.is_finite() || !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
            return Err(PrepareError);
        }
        let defaults = ExciterParameters::default();
        let smoothing = smoothing_coefficient(sample_rate);
        let (emphasis_highpass, emphasis_lowpass) =
            emphasis_coefficients(defaults.emphatic_point, sample_rate);
        Ok(Self {
            sample_rate,
            left: Channel::default(),
            right: Channel::default(),
            eq_low_coefficient: one_pole_coefficient(EQ_LOW_FREQUENCY_HZ, sample_rate),
            eq_high_coefficient: one_pole_coefficient(
                EQ_HIGH_FREQUENCY_HZ.min(sample_rate * 0.45),
                sample_rate,
            ),
            harmonic_dc_block_coefficient: dc_block_coefficient(
                HARMONIC_DC_BLOCK_HZ,
                sample_rate * OVERSAMPLING,
            ),
            emphasis_highpass_coefficient: Smoothed::new(emphasis_highpass, smoothing),
            emphasis_lowpass_coefficient: Smoothed::new(emphasis_lowpass, smoothing),
            low_gain: Smoothed::new(db_to_gain(defaults.eq_low_db), smoothing),
            high_gain: Smoothed::new(db_to_gain(defaults.eq_high_db), smoothing),
            excited_weight: Smoothed::new(blend_weight(defaults.blend), smoothing),
            wet: Smoothed::new(0.0, smoothing),
        })
    }

    pub fn set_parameters(&mut self, parameters: ExciterParameters) -> Result<(), &'static str> {
        let parameters = parameters.validate()?;
        let (emphasis_highpass, emphasis_lowpass) =
            emphasis_coefficients(parameters.emphatic_point, self.sample_rate);
        self.emphasis_highpass_coefficient
            .set_target(emphasis_highpass);
        self.emphasis_lowpass_coefficient
            .set_target(emphasis_lowpass);
        self.low_gain.set_target(db_to_gain(parameters.eq_low_db));
        self.high_gain.set_target(db_to_gain(parameters.eq_high_db));
        self.excited_weight
            .set_target(blend_weight(parameters.blend));
        self.wet.set_target(if parameters.enabled {
            parameters.mix
        } else {
            0.0
        });
        Ok(())
    }

    pub fn latency_samples(&self) -> u32 {
        0
    }

    pub fn process(&mut self, input: StereoFrame) -> StereoFrame {
        <Self as StereoProcessor>::process(self, input)
    }

    pub fn reset(&mut self) {
        <Self as StereoProcessor>::reset(self);
    }
}

impl StereoProcessor for Exciter {
    fn reset(&mut self) {
        self.left = Channel::default();
        self.right = Channel::default();
        self.emphasis_highpass_coefficient.snap();
        self.emphasis_lowpass_coefficient.snap();
        self.low_gain.snap();
        self.high_gain.snap();
        self.excited_weight.snap();
        self.wet.snap();
    }

    fn process(&mut self, input: StereoFrame) -> StereoFrame {
        let emphasis_highpass = self.emphasis_highpass_coefficient.next();
        let emphasis_lowpass = self.emphasis_lowpass_coefficient.next();
        let low_gain = self.low_gain.next();
        let high_gain = self.high_gain.next();
        let excited_weight = self.excited_weight.next();
        let wet = self.wet.next();
        let parameters = FrameParameters {
            eq_low_coefficient: self.eq_low_coefficient,
            eq_high_coefficient: self.eq_high_coefficient,
            emphasis_highpass,
            emphasis_lowpass,
            low_gain,
            high_gain,
            excited_weight,
            wet,
            harmonic_dc_block_coefficient: self.harmonic_dc_block_coefficient,
        };
        StereoFrame::new(
            self.left.process(input.left, parameters),
            self.right.process(input.right, parameters),
        )
    }
}

#[derive(Clone, Copy)]
struct FrameParameters {
    eq_low_coefficient: f32,
    eq_high_coefficient: f32,
    emphasis_highpass: f32,
    emphasis_lowpass: f32,
    low_gain: f32,
    high_gain: f32,
    excited_weight: f32,
    wet: f32,
    harmonic_dc_block_coefficient: f32,
}

#[derive(Default)]
struct Channel {
    eq_lowpass: f32,
    eq_highpass_lowpass: f32,
    previous_equalized: f32,
    emphasis_highpass_lowpass: f32,
    emphasis_band_lowpass: f32,
    previous_harmonics: f32,
    harmonic_dc_block_output: f32,
}

impl Channel {
    fn process(&mut self, input: f32, parameters: FrameParameters) -> f32 {
        let equalized = self.equalize(input, parameters);
        let midpoint = (self.previous_equalized + equalized) * 0.5;
        let first = self.excitation(
            midpoint,
            parameters.emphasis_highpass,
            parameters.emphasis_lowpass,
            parameters.harmonic_dc_block_coefficient,
        );
        let second = self.excitation(
            equalized,
            parameters.emphasis_highpass,
            parameters.emphasis_lowpass,
            parameters.harmonic_dc_block_coefficient,
        );
        self.previous_equalized = equalized;

        let excited = equalized + (first + second) * 0.5;
        let internally_blended = linear_mix(equalized, excited, parameters.excited_weight);
        denormal_guard(linear_mix(input, internally_blended, parameters.wet))
    }

    fn equalize(&mut self, input: f32, parameters: FrameParameters) -> f32 {
        self.eq_lowpass += (input - self.eq_lowpass) * parameters.eq_low_coefficient;
        self.eq_highpass_lowpass +=
            (input - self.eq_highpass_lowpass) * parameters.eq_high_coefficient;
        let high = input - self.eq_highpass_lowpass;
        denormal_guard(
            input
                + self.eq_lowpass * (parameters.low_gain - 1.0)
                + high * (parameters.high_gain - 1.0),
        )
    }

    fn excitation(
        &mut self,
        input: f32,
        highpass_coefficient: f32,
        lowpass_coefficient: f32,
        harmonic_dc_block_coefficient: f32,
    ) -> f32 {
        self.emphasis_highpass_lowpass +=
            (input - self.emphasis_highpass_lowpass) * highpass_coefficient;
        let above_lower_edge = input - self.emphasis_highpass_lowpass;
        self.emphasis_band_lowpass +=
            (above_lower_edge - self.emphasis_band_lowpass) * lowpass_coefficient;
        let emphasized = self.emphasis_band_lowpass;

        // Compress only the negative half-wave, then remove the original band.
        // This creates predominantly low-order even harmonics without driving
        // the complete signal into saturation. Asymmetric shaping creates DC,
        // so the residual is explicitly DC-blocked before it is mixed back.
        let shaped = if emphasized < 0.0 {
            emphasized / (1.0 + HALF_WAVE_DRIVE * -emphasized)
        } else {
            emphasized
        };
        let raw_harmonics = shaped - emphasized;
        let harmonics = raw_harmonics - self.previous_harmonics
            + harmonic_dc_block_coefficient * self.harmonic_dc_block_output;
        self.previous_harmonics = raw_harmonics;
        self.harmonic_dc_block_output = denormal_guard(harmonics);
        denormal_guard(
            emphasized * PRESENCE_GAIN + self.harmonic_dc_block_output * HARMONIC_GAIN,
        )
    }
}

fn emphasis_coefficients(point: u8, sample_rate: f32) -> (f32, f32) {
    let center =
        EMPHATIC_FREQUENCIES_HZ[usize::from(point.saturating_sub(1))].min(sample_rate * 0.40);
    let lower_edge = (center / 1.7).max(80.0);
    let upper_edge = (center * 1.7).min(sample_rate * 0.45);
    (
        one_pole_coefficient(lower_edge, sample_rate * OVERSAMPLING),
        one_pole_coefficient(upper_edge, sample_rate * OVERSAMPLING),
    )
}

fn one_pole_coefficient(frequency_hz: f32, sample_rate: f32) -> f32 {
    1.0 - (-TAU * frequency_hz / sample_rate).exp()
}

fn dc_block_coefficient(frequency_hz: f32, sample_rate: f32) -> f32 {
    (-TAU * frequency_hz / sample_rate).exp()
}

fn db_to_gain(decibels: f32) -> f32 {
    10.0_f32.powf(decibels / 20.0)
}

fn blend_weight(blend: f32) -> f32 {
    (blend + 1.0) * 0.5
}

fn linear_mix(dry: f32, wet: f32, mix: f32) -> f32 {
    let mix = mix.clamp(0.0, 1.0);
    dry * (1.0 - mix) + wet * mix
}

fn smoothing_coefficient(sample_rate: f32) -> f32 {
    1.0 - (-1.0 / (SMOOTHING_SECONDS * sample_rate)).exp()
}

fn denormal_guard(sample: f32) -> f32 {
    if sample.abs() < 1.0e-20 { 0.0 } else { sample }
}

struct Smoothed {
    current: f32,
    target: f32,
    coefficient: f32,
}

impl Smoothed {
    fn new(value: f32, coefficient: f32) -> Self {
        Self {
            current: value,
            target: value,
            coefficient,
        }
    }

    fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    fn next(&mut self) -> f32 {
        self.current += (self.target - self.current) * self.coefficient;
        self.current
    }

    fn snap(&mut self) {
        self.current = self.target;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_sample_rate_and_classic_parameter_ranges() {
        assert!(Exciter::new(7_999.0).is_err());
        assert!(Exciter::new(f32::NAN).is_err());
        let mut exciter = Exciter::new(48_000.0).unwrap();
        assert!(
            exciter
                .set_parameters(ExciterParameters {
                    emphatic_point: 0,
                    ..ExciterParameters::default()
                })
                .is_err()
        );
        assert!(
            exciter
                .set_parameters(ExciterParameters {
                    eq_high_db: 12.1,
                    ..ExciterParameters::default()
                })
                .is_err()
        );
    }

    #[test]
    fn disabled_exciter_is_bit_exact_after_reset() {
        let mut exciter = Exciter::new(48_000.0).unwrap();
        exciter
            .set_parameters(ExciterParameters::default())
            .unwrap();
        exciter.reset();
        for index in 0..96_000 {
            let input = StereoFrame::new(
                (index as f32 * 0.019).sin() * 0.4,
                (index as f32 * 0.023).sin() * 0.3,
            );
            assert_eq!(exciter.process(input), input);
        }
    }

    #[test]
    fn m1_style_defaults_are_clearly_non_neutral_when_enabled() {
        let mut exciter = Exciter::new(48_000.0).unwrap();
        exciter
            .set_parameters(ExciterParameters {
                enabled: true,
                ..ExciterParameters::default()
            })
            .unwrap();
        exciter.reset();
        let mut dry_energy = 0.0_f64;
        let mut difference_energy = 0.0_f64;
        for index in 0..96_000 {
            let input = (TAU * 2_200.0 * index as f32 / 48_000.0).sin() * 0.3;
            let output = exciter.process(StereoFrame::splat(input)).left;
            assert!(output.is_finite());
            assert!(output.abs() < 2.0);
            dry_energy += f64::from(input * input);
            difference_energy += f64::from((output - input) * (output - input));
        }
        let relative_difference = (difference_energy / dry_energy).sqrt();
        assert!(
            relative_difference > 0.05,
            "relative difference {relative_difference}"
        );
    }

    #[test]
    fn negative_blend_is_drier_than_positive_blend() {
        fn difference(blend: f32) -> f64 {
            let mut exciter = Exciter::new(48_000.0).unwrap();
            exciter
                .set_parameters(ExciterParameters {
                    enabled: true,
                    blend,
                    ..ExciterParameters::default()
                })
                .unwrap();
            exciter.reset();
            (0..48_000)
                .map(|index| {
                    let input = (TAU * 2_200.0 * index as f32 / 48_000.0).sin() * 0.3;
                    let output = exciter.process(StereoFrame::splat(input)).left;
                    f64::from((output - input).abs())
                })
                .sum()
        }
        assert!(difference(1.0) > difference(-1.0) * 10.0);
    }

    #[test]
    fn low_dry_wet_cannot_turn_into_heavy_distortion() {
        let mut exciter = Exciter::new(48_000.0).unwrap();
        exciter
            .set_parameters(ExciterParameters {
                enabled: true,
                blend: 1.0,
                mix: 0.10,
                ..ExciterParameters::default()
            })
            .unwrap();
        exciter.reset();

        let mut dry_energy = 0.0_f64;
        let mut difference_energy = 0.0_f64;
        let mut output_peak = 0.0_f32;
        for index in 0..96_000 {
            let phase = TAU * index as f32 / 48_000.0;
            let input = phase.sin() * 0.32
                + (phase * 7.0).sin() * 0.18
                + (phase * 31.0).sin() * 0.08;
            let output = exciter.process(StereoFrame::splat(input)).left;
            assert!(output.is_finite());
            dry_energy += f64::from(input * input);
            difference_energy += f64::from((output - input) * (output - input));
            output_peak = output_peak.max(output.abs());
        }

        let relative_difference = (difference_energy / dry_energy).sqrt();
        assert!(
            relative_difference < 0.08,
            "low-mix relative difference {relative_difference}"
        );
        assert!(output_peak < 0.75, "low-mix output peak {output_peak}");
    }

    #[test]
    fn asymmetric_harmonics_do_not_leak_dc() {
        let mut exciter = Exciter::new(48_000.0).unwrap();
        exciter
            .set_parameters(ExciterParameters {
                enabled: true,
                ..ExciterParameters::default()
            })
            .unwrap();
        exciter.reset();

        let mut sum = 0.0_f64;
        let mut measured = 0_u32;
        for index in 0..144_000 {
            let input = (TAU * 1_500.0 * index as f32 / 48_000.0).sin() * 0.5;
            let output = exciter.process(StereoFrame::splat(input)).left;
            if index >= 48_000 {
                sum += f64::from(output);
                measured += 1;
            }
        }
        let mean = sum / f64::from(measured);
        assert!(mean.abs() < 1.0e-4, "DC mean {mean}");
    }

    #[test]
    fn low_and_high_eq_are_independent() {
        fn difference(frequency_hz: f32, low_db: f32, high_db: f32) -> f64 {
            let mut exciter = Exciter::new(48_000.0).unwrap();
            exciter
                .set_parameters(ExciterParameters {
                    enabled: true,
                    blend: -1.0,
                    eq_low_db: low_db,
                    eq_high_db: high_db,
                    ..ExciterParameters::default()
                })
                .unwrap();
            exciter.reset();
            (0..48_000)
                .map(|index| {
                    let input = (TAU * frequency_hz * index as f32 / 48_000.0).sin() * 0.2;
                    let output = exciter.process(StereoFrame::splat(input)).left;
                    f64::from((output - input).abs())
                })
                .sum()
        }
        assert!(difference(100.0, 12.0, 0.0) > difference(100.0, 0.0, 12.0));
        assert!(difference(8_000.0, 0.0, 12.0) > difference(8_000.0, 12.0, 0.0));
    }

    #[test]
    fn reset_is_deterministic() {
        let parameters = ExciterParameters {
            enabled: true,
            ..ExciterParameters::default()
        };
        let mut first = Exciter::new(48_000.0).unwrap();
        let mut second = Exciter::new(48_000.0).unwrap();
        first.set_parameters(parameters).unwrap();
        second.set_parameters(parameters).unwrap();
        for _ in 0..1_000 {
            first.process(StereoFrame::new(0.7, -0.4));
        }
        first.reset();
        second.reset();
        for index in 0..10_000 {
            let input = StereoFrame::splat((index as f32 * 0.013).sin() * 0.5);
            assert_eq!(first.process(input), second.process(input));
        }
    }
}
