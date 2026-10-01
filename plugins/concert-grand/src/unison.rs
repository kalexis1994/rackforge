//! Experimental per-string tuning and stiffness, fixed across the partial ladder.
//! These are effective parameters, not measured properties of the reference piano.
use crate::math::{exp2f, sqrtf};

/// Smooth retention of tuning width in the treble. The legacy taper stays
/// bit-identical at zero; a nonzero floor prevents coincident string ladders.
pub fn register_width(top_octave: f32, treble_ratio: f32) -> f32 {
    (1.0 - top_octave) + treble_ratio * top_octave
}

#[derive(Clone, Copy)]
pub struct Unison {
    pub pitch: [f32; 3],
    pub stiffness: [f32; 3],
    base: f32,
}

impl Unison {
    pub fn new(note: u8, base: f32, cents: f32, spread: f32) -> Self {
        Self::with_center(note, base, cents, spread, 0.5)
    }

    pub fn with_center(note: u8, base: f32, cents: f32, spread: f32, center: f32) -> Self {
        let seed = (note as u32).wrapping_mul(0x9e3779b9);
        // One instrument-wide tuning pattern with the same small per-key
        // imperfection. Center 0.5 preserves the previous renderer exactly.
        let middle = (center - 0.5 + 0.25 * (crate::hash01(seed ^ 0x41) - 0.5)).clamp(-0.5, 0.5);
        let mut offsets = [-0.5, middle, 0.5];
        let mean = offsets.iter().sum::<f32>() / 3.0;
        for value in &mut offsets {
            *value -= mean;
        }
        let mut variations = core::array::from_fn::<_, 3, _>(|i| {
            2.0 * crate::hash01(seed ^ (i as u32 + 1).wrapping_mul(0x85ebca6b)) - 1.0
        });
        let mean = variations.iter().sum::<f32>() / 3.0;
        for value in &mut variations {
            *value -= mean;
        }
        Self {
            pitch: offsets.map(|x| exp2f(x * cents / 1200.0)),
            stiffness: variations.map(|x| base * (1.0 + x * spread)),
            base,
        }
    }

    pub fn ratios(&self, harmonic: usize) -> [f32; 3] {
        let h2 = (harmonic * harmonic) as f32;
        core::array::from_fn(|i| {
            if self.stiffness[i] == self.base {
                return self.pitch[i];
            }
            // Hold each fundamental fixed while changing its inharmonicity.
            self.pitch[i]
                * sqrtf(
                    ((1.0 + self.stiffness[i] * h2) / (1.0 + self.base * h2))
                        * ((1.0 + self.base) / (1.0 + self.stiffness[i])),
                )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_treble_width_is_continuous_and_keeps_three_distinct_strings() {
        for i in 0..=100 {
            let top = i as f32 / 100.0;
            assert_eq!(register_width(top, 0.0), 1.0 - top);
            let width = register_width(top, 0.25);
            assert!((0.25..=1.0).contains(&width));
            let u = Unison::new(96, 0.001, 0.5 * width, 0.0);
            assert!(u.pitch[0] < u.pitch[1] && u.pitch[1] < u.pitch[2]);
            if i > 0 {
                assert!((width - register_width((i - 1) as f32 / 100.0, 0.25)).abs() < 0.008);
            }
        }
    }

    #[test]
    fn one_tuning_per_string_without_stiffness_spread() {
        let u = Unison::new(60, 0.0002, 0.7, 0.0);
        for h in 1..=144 {
            assert_eq!(u.ratios(h), u.pitch);
        }
        assert!((u.pitch.iter().product::<f32>() - 1.0).abs() < 2e-6);
    }

    #[test]
    fn each_ladder_obeys_its_own_stiff_string_law() {
        let u = Unison::new(72, 0.0005, 0.5, 0.02);
        for h in 1..=100 {
            let ratios = u.ratios(h);
            for i in 0..3 {
                let actual = (1.0 + u.base * (h * h) as f32) / (1.0 + u.base)
                    * (ratios[i] / u.pitch[i]).powi(2);
                let expected = (1.0 + u.stiffness[i] * (h * h) as f32) / (1.0 + u.stiffness[i]);
                assert!((actual / expected - 1.0).abs() < 5e-6);
            }
        }
    }

    #[test]
    fn profiles_are_positive_and_repeatable_across_keyboard() {
        for note in 21..=108 {
            let a = Unison::new(note, 0.001, 2.0, 0.25);
            let b = Unison::new(note, 0.001, 2.0, 0.25);
            assert_eq!(a.pitch, b.pitch);
            assert_eq!(a.stiffness, b.stiffness);
            for h in 1..=144 {
                assert!(a.ratios(h).iter().all(|r| r.is_finite() && *r > 0.0));
            }
        }
    }

    #[test]
    fn skewed_tunings_preserve_center_pitch_and_outer_span() {
        for note in 21..=108 {
            for center in [0.0, 0.15, 0.35, 0.5, 0.65, 0.85, 1.0] {
                let a = Unison::with_center(note, 0.001, 2.0, 0.0, center);
                assert!((a.pitch.iter().product::<f32>() - 1.0).abs() < 2e-6);
                assert!((a.pitch[2] / a.pitch[0] - exp2f(2.0 / 1200.0)).abs() < 2e-6);
                assert!(a.pitch[0] <= a.pitch[1] && a.pitch[1] <= a.pitch[2]);
                if center == 0.5 {
                    assert_eq!(a.pitch, Unison::new(note, 0.001, 2.0, 0.0).pitch);
                }
                for harmonic in 1..=144 {
                    assert_eq!(a.ratios(harmonic), a.pitch);
                }
            }
        }
    }
}
