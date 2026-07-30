use crate::{StereoFrame, StereoProcessor};
use std::f32::consts::TAU;
use std::fmt;

const MIN_SAMPLE_RATE: f32 = 8_000.0;
const MAX_SAMPLE_RATE: f32 = 384_000.0;
const MAX_DELAY_SECONDS: f32 = 0.060;
const SMOOTHING_SECONDS: f32 = 0.020;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChorusParameters {
    pub enabled: bool,
    pub rate_hz: f32,
    pub depth: f32,
    pub delay_ms: f32,
    pub feedback: f32,
    pub width: f32,
    pub mix: f32,
}

impl Default for ChorusParameters {
    fn default() -> Self {
        Self {
            enabled: false,
            rate_hz: 0.8,
            depth: 0.55,
            delay_ms: 14.0,
            feedback: 0.10,
            width: 0.75,
            mix: 0.30,
        }
    }
}

impl ChorusParameters {
    pub fn validate(self) -> Result<Self, &'static str> {
        validate_range(self.rate_hz, 0.05, 10.0, "chorus rate")?;
        validate_range(self.depth, 0.0, 1.0, "chorus depth")?;
        validate_range(self.delay_ms, 2.0, 30.0, "chorus delay")?;
        validate_range(self.feedback, -0.90, 0.90, "chorus feedback")?;
        validate_range(self.width, 0.0, 1.0, "chorus width")?;
        validate_range(self.mix, 0.0, 1.0, "chorus mix")?;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrepareError;

impl fmt::Display for PrepareError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sample rate must be finite and between 8 kHz and 384 kHz")
    }
}

impl std::error::Error for PrepareError {}

pub struct Chorus {
    sample_rate: f32,
    left: DelayLine,
    right: DelayLine,
    phase: f32,
    rate_hz: Smoothed,
    depth: Smoothed,
    delay_ms: Smoothed,
    feedback: Smoothed,
    width: Smoothed,
    wet: Smoothed,
}

impl Chorus {
    pub fn new(sample_rate: f32) -> Result<Self, PrepareError> {
        if !sample_rate.is_finite() || !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
            return Err(PrepareError);
        }
        let delay_capacity = (sample_rate * MAX_DELAY_SECONDS).ceil() as usize + 4;
        let parameters = ChorusParameters::default();
        let smoothing = smoothing_coefficient(sample_rate);
        Ok(Self {
            sample_rate,
            left: DelayLine::new(delay_capacity),
            right: DelayLine::new(delay_capacity),
            phase: 0.0,
            rate_hz: Smoothed::new(parameters.rate_hz, smoothing),
            depth: Smoothed::new(parameters.depth, smoothing),
            delay_ms: Smoothed::new(parameters.delay_ms, smoothing),
            feedback: Smoothed::new(parameters.feedback, smoothing),
            width: Smoothed::new(parameters.width, smoothing),
            wet: Smoothed::new(0.0, smoothing),
        })
    }

    pub fn set_parameters(&mut self, parameters: ChorusParameters) -> Result<(), &'static str> {
        let parameters = parameters.validate()?;
        self.rate_hz.set_target(parameters.rate_hz);
        self.depth.set_target(parameters.depth);
        self.delay_ms.set_target(parameters.delay_ms);
        self.feedback.set_target(parameters.feedback);
        self.width.set_target(parameters.width);
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

impl StereoProcessor for Chorus {
    fn reset(&mut self) {
        self.left.clear();
        self.right.clear();
        self.phase = 0.0;
        self.rate_hz.snap();
        self.depth.snap();
        self.delay_ms.snap();
        self.feedback.snap();
        self.width.snap();
        self.wet.snap();
    }

    fn process(&mut self, input: StereoFrame) -> StereoFrame {
        let rate_hz = self.rate_hz.next();
        let depth = self.depth.next();
        let delay_ms = self.delay_ms.next();
        let feedback = self.feedback.next();
        let width = self.width.next();
        let wet_mix = self.wet.next();

        let phase_left = self.phase;
        let phase_right = wrap_phase(self.phase + 0.25 * width);
        let excursion_ms = 8.0 * depth;
        let left_delay = delay_samples(
            delay_ms + excursion_ms * phase_left.mul_add(TAU, 0.0).sin(),
            self.sample_rate,
            self.left.capacity(),
        );
        let right_delay = delay_samples(
            delay_ms + excursion_ms * phase_right.mul_add(TAU, 0.0).sin(),
            self.sample_rate,
            self.right.capacity(),
        );

        let wet_left = self.left.read(left_delay);
        let wet_right = self.right.read(right_delay);
        let injection = 1.0 - feedback.abs();
        self.left
            .write(denormal_guard(input.left * injection + wet_left * feedback));
        self.right.write(denormal_guard(
            input.right * injection + wet_right * feedback,
        ));

        self.phase = wrap_phase(self.phase + rate_hz / self.sample_rate);
        StereoFrame::new(
            bounded_mix(input.left, wet_left, wet_mix),
            bounded_mix(input.right, wet_right, wet_mix),
        )
    }
}

fn smoothing_coefficient(sample_rate: f32) -> f32 {
    1.0 - (-1.0 / (SMOOTHING_SECONDS * sample_rate)).exp()
}

fn wrap_phase(phase: f32) -> f32 {
    phase - phase.floor()
}

fn delay_samples(delay_ms: f32, sample_rate: f32, capacity: usize) -> f32 {
    let maximum = capacity.saturating_sub(3) as f32;
    (delay_ms * 0.001 * sample_rate).clamp(1.0, maximum)
}

fn bounded_mix(dry: f32, wet: f32, mix: f32) -> f32 {
    let mix = mix.clamp(0.0, 1.0);
    dry * (1.0 - mix) + wet * mix
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

struct DelayLine {
    samples: Box<[f32]>,
    write_index: usize,
}

impl DelayLine {
    fn new(capacity: usize) -> Self {
        Self {
            samples: vec![0.0; capacity].into_boxed_slice(),
            write_index: 0,
        }
    }

    fn capacity(&self) -> usize {
        self.samples.len()
    }

    fn clear(&mut self) {
        self.samples.fill(0.0);
        self.write_index = 0;
    }

    fn read(&self, delay_samples: f32) -> f32 {
        let capacity = self.samples.len() as isize;
        let position = self.write_index as f32 - delay_samples;
        let base = position.floor();
        let fraction = position - base;
        let first = (base as isize).rem_euclid(capacity) as usize;
        let second = (base as isize + 1).rem_euclid(capacity) as usize;
        self.samples[first] + (self.samples[second] - self.samples[first]) * fraction
    }

    fn write(&mut self, sample: f32) {
        self.samples[self.write_index] = sample;
        self.write_index += 1;
        if self.write_index == self.samples.len() {
            self.write_index = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_sample_rates_and_parameters() {
        assert!(Chorus::new(f32::NAN).is_err());
        assert!(Chorus::new(7_999.0).is_err());
        let mut chorus = Chorus::new(48_000.0).unwrap();
        assert!(
            chorus
                .set_parameters(ChorusParameters {
                    mix: 1.1,
                    ..ChorusParameters::default()
                })
                .is_err()
        );
    }

    #[test]
    fn disabled_chorus_is_bit_exact_after_settling() {
        let mut chorus = Chorus::new(48_000.0).unwrap();
        chorus.set_parameters(ChorusParameters::default()).unwrap();
        chorus.reset();
        for index in 0..4_096 {
            let sample = (index as f32 * 0.031).sin() * 0.25;
            assert_eq!(
                chorus.process(StereoFrame::splat(sample)),
                StereoFrame::splat(sample)
            );
        }
    }

    #[test]
    fn enabled_chorus_is_finite_and_creates_stereo_motion() {
        let mut chorus = Chorus::new(48_000.0).unwrap();
        chorus
            .set_parameters(ChorusParameters {
                enabled: true,
                mix: 0.5,
                ..ChorusParameters::default()
            })
            .unwrap();
        chorus.reset();
        let mut stereo_difference = 0.0_f32;
        let mut peak = 0.0_f32;
        for index in 0..96_000 {
            let input = if index == 0 { 1.0 } else { 0.0 };
            let output = chorus.process(StereoFrame::splat(input));
            assert!(output.left.is_finite() && output.right.is_finite());
            stereo_difference += (output.left - output.right).abs();
            peak = peak.max(output.left.abs()).max(output.right.abs());
        }
        assert!(stereo_difference > 0.01);
        assert!(peak <= 1.0);
    }

    #[test]
    fn reset_removes_delay_history() {
        let mut chorus = Chorus::new(48_000.0).unwrap();
        chorus
            .set_parameters(ChorusParameters {
                enabled: true,
                mix: 1.0,
                ..ChorusParameters::default()
            })
            .unwrap();
        chorus.reset();
        chorus.process(StereoFrame::splat(1.0));
        for _ in 0..4_000 {
            chorus.process(StereoFrame::default());
        }
        chorus.reset();
        for _ in 0..4_000 {
            assert_eq!(
                chorus.process(StereoFrame::default()),
                StereoFrame::default()
            );
        }
    }

    #[test]
    fn extreme_valid_feedback_does_not_create_unbounded_gain() {
        let mut chorus = Chorus::new(48_000.0).unwrap();
        chorus
            .set_parameters(ChorusParameters {
                enabled: true,
                feedback: 0.90,
                mix: 1.0,
                ..ChorusParameters::default()
            })
            .unwrap();
        chorus.reset();
        let mut peak = 0.0_f32;
        for index in 0..480_000 {
            let input = (index as f32 * 0.017).sin() * 0.8;
            let output = chorus.process(StereoFrame::splat(input));
            peak = peak.max(output.left.abs()).max(output.right.abs());
        }
        assert!(peak <= 0.800_001, "unexpected peak {peak}");
    }
}
