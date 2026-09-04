#![forbid(unsafe_code)]

mod chorus;
mod exciter;
mod reverb;

pub use chorus::{Chorus, ChorusParameters, PrepareError};
pub use exciter::{Exciter, ExciterParameters};
pub use reverb::{Reverb, ReverbParameters};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StereoFrame {
    pub left: f32,
    pub right: f32,
}

impl StereoFrame {
    pub const fn new(left: f32, right: f32) -> Self {
        Self { left, right }
    }

    pub const fn splat(sample: f32) -> Self {
        Self::new(sample, sample)
    }

    pub fn mono(self) -> f32 {
        (self.left + self.right) * 0.5
    }
}

/// Contract for DSP nodes that can run on RackForge's audio thread.
///
/// Implementations must not allocate, lock, log or perform I/O from
/// `process`. Construction and `reset` happen outside the hot path.
pub trait StereoProcessor {
    fn reset(&mut self);
    fn process(&mut self, input: StereoFrame) -> StereoFrame;
}

/// Where the output stops being exactly itself. Below this nothing is touched
/// at all, and every instrument RackForge ships peaks beneath it at unity.
pub const OUTPUT_KNEE: f32 = 0.95;

/// The last thing between a mix and a converter.
///
/// Something has to stop a sample leaving a host above full scale, because the
/// converter cannot take it. The obvious way is `clamp(-1.0, 1.0)`, and it is
/// the worst available one: exactly linear and then instantly not, which is a
/// high-order nonlinearity, and those make intermodulation products rather
/// than harmonics. On a chord they land BELOW the lowest note being played,
/// where nothing is meant to be at all -- measured on the Concert Grand, a
/// desktop clipping one sample in six of a ten-note fortissimo put six
/// decibels of energy under 40 Hz on a chord whose lowest note is 41.
///
/// So it bends. Exactly the identity up to the knee -- an ordinary mix never
/// touches this -- and above it `u/(1+u)` toward a ceiling of one, whose slope
/// is one where it joins, so there is no corner to hear. It is the same curve
/// the Concert Grand's own preamplifier uses above its knee, for the same
/// reason.
///
/// A safety net, not a sound: anything arriving far past the knee is still
/// compressed hard, and the fix for that is gain, not this curve. A sample
/// that is not a number leaves as silence rather than as a scream.
///
/// It lives here rather than in a host because there are three hosts, and the
/// two that had this wall had written it out separately.
#[must_use]
pub fn output_ceiling(sample: f32) -> f32 {
    if !sample.is_finite() {
        return 0.0;
    }
    let magnitude = sample.abs();
    if magnitude <= OUTPUT_KNEE {
        return sample;
    }
    let over = (magnitude - OUTPUT_KNEE) / (1.0 - OUTPUT_KNEE);
    let shaped = OUTPUT_KNEE + (1.0 - OUTPUT_KNEE) * over / (1.0 + over);
    if sample < 0.0 { -shaped } else { shaped }
}

#[cfg(test)]
mod output_ceiling_tests {
    use super::{OUTPUT_KNEE, output_ceiling};

    #[test]
    fn it_is_transparent_until_it_is_needed() {
        // The loudest instrument in the store peaks at 0.925 on a ten-note
        // fortissimo chord, so this path has to be the identity for it.
        for sample in [0.0f32, 0.001, 0.5, 0.788, 0.925, OUTPUT_KNEE] {
            assert_eq!(output_ceiling(sample), sample, "{sample} was altered");
            assert_eq!(output_ceiling(-sample), -sample, "-{sample} was altered");
        }
    }

    #[test]
    fn it_bends_rather_than_squaring_off() {
        let mut previous = OUTPUT_KNEE;
        let mut sample = OUTPUT_KNEE;
        // Four is a full-scale signal through the loudest gain a host offers,
        // +12 dB, and so the most that can actually arrive. It must still be
        // rising there. Far beyond it the curve is asymptotic and stops rising
        // in `f32` long before it stops rising in arithmetic.
        while sample < 40.0 {
            sample += 0.01;
            let shaped = output_ceiling(sample);
            assert!(shaped <= 1.0, "{sample} left as {shaped}, over full scale");
            if sample <= 4.0 {
                assert!(
                    shaped > previous,
                    "{sample} did not rise: the curve flattened"
                );
            } else {
                assert!(shaped >= previous, "{sample} went backwards");
            }
            previous = shaped;
        }
    }

    #[test]
    fn it_joins_without_a_corner() {
        let step = 1e-4;
        let below = (output_ceiling(OUTPUT_KNEE) - output_ceiling(OUTPUT_KNEE - step)) / step;
        let above = (output_ceiling(OUTPUT_KNEE + step) - output_ceiling(OUTPUT_KNEE)) / step;
        assert!(
            (below - above).abs() < 0.01,
            "a corner at the knee: {below} then {above}"
        );
    }

    #[test]
    fn a_sample_that_is_not_a_number_leaves_as_silence() {
        assert_eq!(output_ceiling(f32::NAN), 0.0);
        assert_eq!(output_ceiling(f32::INFINITY), 0.0);
        assert_eq!(output_ceiling(f32::NEG_INFINITY), 0.0);
    }
}
