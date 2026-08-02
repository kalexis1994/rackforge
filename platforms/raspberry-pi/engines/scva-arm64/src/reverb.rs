const REFERENCE_RATE: f32 = 44_100.0;
const COMB_TUNINGS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNINGS: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;

struct Comb {
    buffer: Vec<f32>,
    cursor: usize,
    filter: f32,
}

impl Comb {
    fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length.max(1)],
            cursor: 0,
            filter: 0.0,
        }
    }

    fn process(&mut self, input: f32, feedback: f32, damping: f32) -> f32 {
        let output = self.buffer[self.cursor];
        self.filter = output * (1.0 - damping) + self.filter * damping;
        self.buffer[self.cursor] = input + self.filter * feedback;
        self.cursor += 1;
        if self.cursor == self.buffer.len() {
            self.cursor = 0;
        }
        output
    }
}

struct AllPass {
    buffer: Vec<f32>,
    cursor: usize,
}

impl AllPass {
    fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length.max(1)],
            cursor: 0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let delayed = self.buffer[self.cursor];
        let output = delayed - input;
        self.buffer[self.cursor] = input + delayed * 0.5;
        self.cursor += 1;
        if self.cursor == self.buffer.len() {
            self.cursor = 0;
        }
        output
    }
}

struct ReverbChannel {
    combs: Vec<Comb>,
    allpasses: Vec<AllPass>,
}

impl ReverbChannel {
    fn new(sample_rate: u32, spread: usize) -> Self {
        let scale = sample_rate as f32 / REFERENCE_RATE;
        let scaled = |length: usize| ((length + spread) as f32 * scale).round() as usize;
        Self {
            combs: COMB_TUNINGS
                .into_iter()
                .map(|length| Comb::new(scaled(length)))
                .collect(),
            allpasses: ALLPASS_TUNINGS
                .into_iter()
                .map(|length| AllPass::new(scaled(length)))
                .collect(),
        }
    }

    fn process(&mut self, input: f32, feedback: f32, damping: f32) -> f32 {
        let mut output = self
            .combs
            .iter_mut()
            .map(|comb| comb.process(input, feedback, damping))
            .sum::<f32>();
        for allpass in &mut self.allpasses {
            output = allpass.process(output);
        }
        output
    }
}

/// A bounded stereo reverberator used while the SCCore room algorithm is
/// reconstructed. Its topology and level match the measured role of the
/// default SCVA reverb: a quiet, decorrelated return mixed behind the dry tone.
pub struct StereoReverb {
    left: ReverbChannel,
    right: ReverbChannel,
}

impl StereoReverb {
    const INPUT_GAIN: f32 = 0.015;
    const FEEDBACK: f32 = 0.82;
    const DAMPING: f32 = 0.25;
    const RETURN_GAIN: f32 = 0.34;

    pub fn new(sample_rate: u32) -> Self {
        Self {
            left: ReverbChannel::new(sample_rate, 0),
            right: ReverbChannel::new(sample_rate, STEREO_SPREAD),
        }
    }

    pub fn process(&mut self, mono: f32) -> (f32, f32) {
        let input = mono * Self::INPUT_GAIN;
        (
            self.left.process(input, Self::FEEDBACK, Self::DAMPING) * Self::RETURN_GAIN,
            self.right.process(input, Self::FEEDBACK, Self::DAMPING) * Self::RETURN_GAIN,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::StereoReverb;

    #[test]
    fn silence_stays_silent() {
        let mut reverb = StereoReverb::new(48_000);
        for _ in 0..48_000 {
            assert_eq!(reverb.process(0.0), (0.0, 0.0));
        }
    }

    #[test]
    fn impulse_produces_a_bounded_decorrelated_tail() {
        let mut reverb = StereoReverb::new(48_000);
        let mut peak = 0.0_f32;
        let mut stereo_difference = 0.0_f32;
        for frame in 0..96_000 {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            let (left, right) = reverb.process(input);
            assert!(left.is_finite() && right.is_finite());
            peak = peak.max(left.abs()).max(right.abs());
            stereo_difference += (left - right).abs();
        }
        assert!(peak > 0.0 && peak < 0.2);
        assert!(stereo_difference > 0.01);
    }
}
