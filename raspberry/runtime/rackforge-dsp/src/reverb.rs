use crate::{PrepareError, StereoFrame, StereoProcessor};
use std::array;
use std::f32::consts::TAU;

const LINE_COUNT: usize = 8;
const MIN_SAMPLE_RATE: f32 = 8_000.0;
const MAX_SAMPLE_RATE: f32 = 384_000.0;
const MAX_PRE_DELAY_SECONDS: f32 = 0.200;
const MAX_TANK_DELAY_SECONDS: f32 = 0.125;
const SMOOTHING_SECONDS: f32 = 0.030;
const INJECTION_GAIN: f32 = 0.18;
const OUTPUT_GAIN: f32 = 0.32;
const BASE_DELAYS_MS: [f32; LINE_COUNT] = [29.7, 37.1, 41.1, 43.7, 53.1, 59.3, 61.7, 71.9];
const INPUT_SIGNS: [f32; LINE_COUNT] = [1.0, 1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0];
const SIDE_SIGNS: [f32; LINE_COUNT] = [1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0, 1.0];
const LEFT_SIGNS: [f32; LINE_COUNT] = [1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0];
const RIGHT_SIGNS: [f32; LINE_COUNT] = [-1.0, 1.0, 1.0, -1.0, 1.0, 1.0, -1.0, -1.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbParameters {
    pub enabled: bool,
    pub size: f32,
    pub decay_seconds: f32,
    pub pre_delay_ms: f32,
    pub damping: f32,
    pub width: f32,
    pub mix: f32,
}

impl Default for ReverbParameters {
    fn default() -> Self {
        Self {
            enabled: false,
            size: 0.85,
            decay_seconds: 2.4,
            pre_delay_ms: 18.0,
            damping: 0.45,
            width: 0.80,
            mix: 0.25,
        }
    }
}

impl ReverbParameters {
    pub fn validate(self) -> Result<Self, &'static str> {
        validate_range(self.size, 0.50, 1.50, "reverb size")?;
        validate_range(self.decay_seconds, 0.20, 12.0, "reverb decay")?;
        validate_range(
            self.pre_delay_ms,
            0.0,
            MAX_PRE_DELAY_SECONDS * 1_000.0,
            "reverb pre-delay",
        )?;
        validate_range(self.damping, 0.0, 1.0, "reverb damping")?;
        validate_range(self.width, 0.0, 1.0, "reverb width")?;
        validate_range(self.mix, 0.0, 1.0, "reverb mix")?;
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

pub struct Reverb {
    sample_rate: f32,
    pre_delay_left: DelayLine,
    pre_delay_right: DelayLine,
    tank: [DelayLine; LINE_COUNT],
    damping_state: [f32; LINE_COUNT],
    size: Smoothed,
    pre_delay_ms: Smoothed,
    damping_coefficient: Smoothed,
    feedback: [Smoothed; LINE_COUNT],
    width: Smoothed,
    wet: Smoothed,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Result<Self, PrepareError> {
        if !sample_rate.is_finite() || !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
            return Err(PrepareError);
        }
        let smoothing = smoothing_coefficient(sample_rate);
        let defaults = ReverbParameters::default();
        let feedback = feedback_targets(defaults.size, defaults.decay_seconds);
        let pre_delay_capacity = (sample_rate * MAX_PRE_DELAY_SECONDS).ceil() as usize + 4;
        let tank_capacity = (sample_rate * MAX_TANK_DELAY_SECONDS).ceil() as usize + 4;
        Ok(Self {
            sample_rate,
            pre_delay_left: DelayLine::new(pre_delay_capacity),
            pre_delay_right: DelayLine::new(pre_delay_capacity),
            tank: array::from_fn(|_| DelayLine::new(tank_capacity)),
            damping_state: [0.0; LINE_COUNT],
            size: Smoothed::new(defaults.size, smoothing),
            pre_delay_ms: Smoothed::new(defaults.pre_delay_ms, smoothing),
            damping_coefficient: Smoothed::new(
                damping_coefficient(defaults.damping, sample_rate),
                smoothing,
            ),
            feedback: feedback.map(|value| Smoothed::new(value, smoothing)),
            width: Smoothed::new(defaults.width, smoothing),
            wet: Smoothed::new(0.0, smoothing),
        })
    }

    pub fn set_parameters(&mut self, parameters: ReverbParameters) -> Result<(), &'static str> {
        let parameters = parameters.validate()?;
        self.size.set_target(parameters.size);
        self.pre_delay_ms.set_target(parameters.pre_delay_ms);
        self.damping_coefficient
            .set_target(damping_coefficient(parameters.damping, self.sample_rate));
        for (smoother, target) in self
            .feedback
            .iter_mut()
            .zip(feedback_targets(parameters.size, parameters.decay_seconds))
        {
            smoother.set_target(target);
        }
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

impl StereoProcessor for Reverb {
    fn reset(&mut self) {
        self.pre_delay_left.clear();
        self.pre_delay_right.clear();
        for line in &mut self.tank {
            line.clear();
        }
        self.damping_state.fill(0.0);
        self.size.snap();
        self.pre_delay_ms.snap();
        self.damping_coefficient.snap();
        for feedback in &mut self.feedback {
            feedback.snap();
        }
        self.width.snap();
        self.wet.snap();
    }

    fn process(&mut self, input: StereoFrame) -> StereoFrame {
        let pre_delay_samples = self.pre_delay_ms.next() * 0.001 * self.sample_rate;
        let delayed_input = if pre_delay_samples < 1.0 {
            input
        } else {
            StereoFrame::new(
                self.pre_delay_left.read(pre_delay_samples),
                self.pre_delay_right.read(pre_delay_samples),
            )
        };
        self.pre_delay_left.write(input.left);
        self.pre_delay_right.write(input.right);

        let size = self.size.next();
        let damping = self.damping_coefficient.next();
        let mut delayed = [0.0_f32; LINE_COUNT];
        for (index, line) in self.tank.iter().enumerate() {
            let samples = BASE_DELAYS_MS[index] * 0.001 * size * self.sample_rate;
            let value = line.read(samples);
            self.damping_state[index] += (value - self.damping_state[index]) * damping;
            delayed[index] = denormal_guard(self.damping_state[index]);
        }

        let sum = delayed.iter().sum::<f32>();
        let householder_scale = 2.0 / LINE_COUNT as f32;
        let mono_input = (delayed_input.left + delayed_input.right) * 0.5;
        let side_input = (delayed_input.left - delayed_input.right) * 0.5;
        for index in 0..LINE_COUNT {
            let reflected = delayed[index] - householder_scale * sum;
            let injected =
                (mono_input * INPUT_SIGNS[index] + side_input * SIDE_SIGNS[index]) * INJECTION_GAIN;
            let feedback = self.feedback[index].next();
            self.tank[index].write(denormal_guard(injected + reflected * feedback));
        }

        let mut wet_left = 0.0_f32;
        let mut wet_right = 0.0_f32;
        for index in 0..LINE_COUNT {
            wet_left += delayed[index] * LEFT_SIGNS[index];
            wet_right += delayed[index] * RIGHT_SIGNS[index];
        }
        wet_left *= OUTPUT_GAIN;
        wet_right *= OUTPUT_GAIN;

        let width = self.width.next();
        let wet_mono = (wet_left + wet_right) * 0.5;
        wet_left = wet_mono + (wet_left - wet_mono) * width;
        wet_right = wet_mono + (wet_right - wet_mono) * width;
        let mix = self.wet.next();
        StereoFrame::new(
            linear_mix(input.left, wet_left, mix),
            linear_mix(input.right, wet_right, mix),
        )
    }
}

fn smoothing_coefficient(sample_rate: f32) -> f32 {
    1.0 - (-1.0 / (SMOOTHING_SECONDS * sample_rate)).exp()
}

fn feedback_targets(size: f32, decay_seconds: f32) -> [f32; LINE_COUNT] {
    BASE_DELAYS_MS.map(|delay_ms| {
        let delay_seconds = delay_ms * 0.001 * size;
        10.0_f32.powf(-3.0 * delay_seconds / decay_seconds)
    })
}

fn damping_coefficient(damping: f32, sample_rate: f32) -> f32 {
    let openness = 1.0 - damping;
    let cutoff_hz = 600.0 + 17_400.0 * openness * openness;
    1.0 - (-TAU * cutoff_hz / sample_rate).exp()
}

fn linear_mix(dry: f32, wet: f32, mix: f32) -> f32 {
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

    fn clear(&mut self) {
        self.samples.fill(0.0);
        self.write_index = 0;
    }

    fn read(&self, delay_samples: f32) -> f32 {
        let maximum = self.samples.len().saturating_sub(3) as f32;
        let delay_samples = delay_samples.clamp(1.0, maximum);
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
    fn validates_sample_rate_and_parameters() {
        assert!(Reverb::new(7_999.0).is_err());
        assert!(Reverb::new(f32::INFINITY).is_err());
        let mut reverb = Reverb::new(48_000.0).unwrap();
        assert!(
            reverb
                .set_parameters(ReverbParameters {
                    decay_seconds: 12.1,
                    ..ReverbParameters::default()
                })
                .is_err()
        );
    }

    #[test]
    fn disabled_reverb_is_bit_exact_after_reset() {
        let mut reverb = Reverb::new(48_000.0).unwrap();
        reverb.set_parameters(ReverbParameters::default()).unwrap();
        reverb.reset();
        for index in 0..96_000 {
            let input = StereoFrame::new(
                (index as f32 * 0.019).sin() * 0.4,
                (index as f32 * 0.023).sin() * 0.3,
            );
            assert_eq!(reverb.process(input), input);
        }
    }

    #[test]
    fn impulse_creates_a_finite_stereo_tail() {
        let mut reverb = Reverb::new(48_000.0).unwrap();
        reverb
            .set_parameters(ReverbParameters {
                enabled: true,
                mix: 1.0,
                ..ReverbParameters::default()
            })
            .unwrap();
        reverb.reset();
        let mut energy = 0.0_f64;
        let mut stereo_difference = 0.0_f64;
        for index in 0..480_000 {
            let input = if index == 0 {
                StereoFrame::splat(1.0)
            } else {
                StereoFrame::default()
            };
            let output = reverb.process(input);
            assert!(output.left.is_finite() && output.right.is_finite());
            energy += f64::from(output.left * output.left + output.right * output.right);
            stereo_difference += f64::from((output.left - output.right).abs());
        }
        assert!(energy > 0.01);
        assert!(energy < 100.0);
        assert!(stereo_difference > 0.01);
    }

    #[test]
    fn extreme_decay_remains_finite_and_eventually_fades() {
        let mut reverb = Reverb::new(48_000.0).unwrap();
        reverb
            .set_parameters(ReverbParameters {
                enabled: true,
                size: 1.5,
                decay_seconds: 12.0,
                damping: 0.0,
                mix: 1.0,
                ..ReverbParameters::default()
            })
            .unwrap();
        reverb.reset();
        let mut late_peak = 0.0_f32;
        for index in 0..1_440_000 {
            let input = if index == 0 {
                StereoFrame::splat(1.0)
            } else {
                StereoFrame::default()
            };
            let output = reverb.process(input);
            assert!(output.left.is_finite() && output.right.is_finite());
            if index > 1_200_000 {
                late_peak = late_peak.max(output.left.abs()).max(output.right.abs());
            }
        }
        assert!(late_peak < 0.001, "late peak {late_peak}");
    }

    #[test]
    fn reset_clears_the_entire_tail() {
        let mut reverb = Reverb::new(48_000.0).unwrap();
        reverb
            .set_parameters(ReverbParameters {
                enabled: true,
                mix: 1.0,
                ..ReverbParameters::default()
            })
            .unwrap();
        reverb.reset();
        reverb.process(StereoFrame::splat(1.0));
        for _ in 0..20_000 {
            reverb.process(StereoFrame::default());
        }
        reverb.reset();
        for _ in 0..20_000 {
            assert_eq!(
                reverb.process(StereoFrame::default()),
                StereoFrame::default()
            );
        }
    }
}
