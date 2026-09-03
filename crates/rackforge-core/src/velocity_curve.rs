//! How hard a key was struck, as this machine reads it.
//!
//! Every controller has its own idea of velocity: one keybed needs to be
//! hammered to reach 127, the next reaches it by accident. A player cannot
//! change the keybed, so the host lets them change the reading — a curve
//! from what arrives to what the instruments are told, drawn by three points
//! a hand can drag: the floor, a bend in the middle, and the ceiling.
//!
//! Three properties matter more than the shape, and they are what the tests
//! below hold on to:
//!
//! * it never inverts — a harder strike is never quieter,
//! * the middle point is ON the curve, so the hand that drags it sees where
//!   it went rather than a hint of it,
//! * the identity curve is exactly the identity, byte for byte, so a player
//!   who never opens this screen is playing an unmapped keyboard.
//!
//! The interpolation is monotone cubic Hermite with the Fritsch–Carlson
//! tangents: the standard way to draw a smooth curve through points without
//! the overshoot a plain spline gives, which here would read as a keybed
//! that gets quieter as you press harder.

use serde::{Deserialize, Serialize};

/// The velocity reading, as four numbers on the MIDI scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VelocityCurve {
    /// What the softest strike becomes: the floor, at input 0.
    pub low: u8,
    /// Where the bend sits, and what it becomes.
    pub mid_input: u8,
    pub mid_output: u8,
    /// What the hardest strike becomes: the ceiling, at input 127.
    pub high: u8,
}

impl Default for VelocityCurve {
    /// The identity: what a host that has never been told otherwise does.
    fn default() -> Self {
        Self {
            low: 0,
            mid_input: 64,
            mid_output: 64,
            high: 127,
        }
    }
}

impl VelocityCurve {
    /// The curve as the rest of the host may rely on it: the bend inside the
    /// range, and the outputs in the order the axis runs. A curve read from a
    /// file someone has edited by hand is corrected rather than rejected —
    /// there is no reading of "the ceiling is below the floor" worth honouring,
    /// and refusing to start over it would be worse.
    pub fn sanitised(self) -> Self {
        let low = self.low.min(127);
        let high = self.high.min(127);
        let mid_input = self.mid_input.clamp(1, 126);
        let (floor, ceiling) = if low <= high {
            (low, high)
        } else {
            (high, low)
        };
        Self {
            low: floor,
            mid_input,
            mid_output: self.mid_output.clamp(floor, ceiling),
            high: ceiling,
        }
    }

    /// Whether this curve leaves every velocity exactly as it arrived.
    pub fn is_identity(&self) -> bool {
        let curve = self.sanitised();
        curve.low == 0
            && curve.high == 127
            && u16::from(curve.mid_output) == u16::from(curve.mid_input)
    }

    /// What a strike of `velocity` becomes.
    ///
    /// Zero is left alone: on the wire a note-on of zero velocity is a note
    /// OFF, and a curve with a raised floor would turn every release into a
    /// note that never stops. For the same reason a real strike never becomes
    /// zero.
    pub fn map(&self, velocity: u8) -> u8 {
        if velocity == 0 {
            return 0;
        }
        if self.is_identity() {
            return velocity.min(127);
        }
        let mapped = self.evaluate(f32::from(velocity.min(127)) / 127.0);
        let scaled = (mapped * 127.0 + 0.5) as i32;
        scaled.clamp(1, 127) as u8
    }

    /// The same curve at MIDI 2.0 width. The endpoints mean the same loudness
    /// on both scales, so a sixteen-bit strike rides the shape its byte would
    /// have ridden.
    pub fn map_wide(&self, velocity: u16) -> u16 {
        if velocity == 0 {
            return 0;
        }
        if self.is_identity() {
            return velocity;
        }
        let mapped = self.evaluate(f32::from(velocity) / f32::from(u16::MAX));
        let scaled = (mapped * f32::from(u16::MAX) + 0.5) as i32;
        scaled.clamp(1, i32::from(u16::MAX)) as u16
    }

    /// The curve on the unit square, which is where it is drawn and where
    /// both widths above meet it.
    pub fn evaluate(&self, x: f32) -> f32 {
        let curve = self.sanitised();
        let xs = [0.0, f32::from(curve.mid_input) / 127.0, 1.0];
        let ys = [
            f32::from(curve.low) / 127.0,
            f32::from(curve.mid_output) / 127.0,
            f32::from(curve.high) / 127.0,
        ];
        monotone_hermite(&xs, &ys, x.clamp(0.0, 1.0))
    }
}

/// Monotone cubic Hermite through three ascending points.
///
/// The secants set the tangents (Fritsch–Carlson): a tangent is the average
/// of its neighbouring secants, except where a secant is flat, and there the
/// tangent is flat too — which is what stops the curve from bulging above a
/// plateau and coming back down.
fn monotone_hermite(xs: &[f32; 3], ys: &[f32; 3], x: f32) -> f32 {
    let mut secant = [0.0f32; 2];
    for i in 0..2 {
        let run = xs[i + 1] - xs[i];
        secant[i] = if run > 1e-6 {
            (ys[i + 1] - ys[i]) / run
        } else {
            0.0
        };
    }
    let mut tangent = [0.0f32; 3];
    tangent[0] = secant[0];
    tangent[2] = secant[1];
    tangent[1] = if secant[0] * secant[1] <= 0.0 {
        // A turn, or a plateau on one side: flat here, so neither segment
        // overshoots into the other.
        0.0
    } else {
        0.5 * (secant[0] + secant[1])
    };
    // Fritsch–Carlson: keep each tangent inside three times its secants.
    for i in 0..2 {
        if secant[i].abs() <= 1e-9 {
            tangent[i] = 0.0;
            tangent[i + 1] = 0.0;
            continue;
        }
        let a = tangent[i] / secant[i];
        let b = tangent[i + 1] / secant[i];
        let magnitude = (a * a + b * b).sqrt();
        if magnitude > 3.0 {
            let scale = 3.0 / magnitude;
            tangent[i] = scale * a * secant[i];
            tangent[i + 1] = scale * b * secant[i];
        }
    }
    let segment = if x <= xs[1] { 0 } else { 1 };
    let run = xs[segment + 1] - xs[segment];
    if run <= 1e-6 {
        return ys[segment + 1];
    }
    let t = ((x - xs[segment]) / run).clamp(0.0, 1.0);
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    (h00 * ys[segment]
        + h10 * run * tangent[segment]
        + h01 * ys[segment + 1]
        + h11 * run * tangent[segment + 1])
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_curve_is_the_identity_byte_for_byte() {
        let curve = VelocityCurve::default();
        assert!(curve.is_identity());
        for velocity in 0..=127u8 {
            assert_eq!(curve.map(velocity), velocity, "velocity {velocity}");
        }
        for value in [0u16, 1, 1234, 32_768, u16::MAX] {
            assert_eq!(curve.map_wide(value), value);
        }
    }

    #[test]
    fn a_note_off_stays_a_note_off_and_a_strike_never_becomes_one() {
        // Zero velocity on a note-on IS a note off; a raised floor must not
        // turn every release into a note that never stops.
        let curve = VelocityCurve {
            low: 40,
            mid_input: 64,
            mid_output: 80,
            high: 127,
        };
        assert_eq!(curve.map(0), 0);
        assert_eq!(curve.map_wide(0), 0);
        for velocity in 1..=127u8 {
            assert!(curve.map(velocity) >= 1, "velocity {velocity} vanished");
        }
        // And a ceiling of zero cannot silence the keyboard either.
        let silent = VelocityCurve {
            low: 0,
            mid_input: 64,
            mid_output: 0,
            high: 0,
        };
        assert!(silent.map(100) >= 1);
    }

    #[test]
    fn a_harder_strike_is_never_quieter() {
        let curves = [
            VelocityCurve {
                low: 0,
                mid_input: 20,
                mid_output: 100,
                high: 127,
            },
            VelocityCurve {
                low: 0,
                mid_input: 110,
                mid_output: 20,
                high: 127,
            },
            VelocityCurve {
                low: 30,
                mid_input: 64,
                mid_output: 35,
                high: 90,
            },
            VelocityCurve {
                low: 60,
                mid_input: 30,
                mid_output: 60,
                high: 60,
            },
            VelocityCurve {
                low: 0,
                mid_input: 1,
                mid_output: 127,
                high: 127,
            },
            VelocityCurve {
                low: 0,
                mid_input: 126,
                mid_output: 0,
                high: 127,
            },
        ];
        for curve in curves {
            let mut previous = 0;
            for velocity in 1..=127u8 {
                let mapped = curve.map(velocity);
                assert!(
                    mapped >= previous,
                    "{curve:?} fell from {previous} to {mapped} at {velocity}"
                );
                previous = mapped;
            }
        }
    }

    #[test]
    fn the_curve_passes_through_the_point_the_hand_dragged() {
        for (mid_input, mid_output) in [(20u8, 90u8), (64, 30), (100, 110), (40, 40)] {
            let curve = VelocityCurve {
                low: 0,
                mid_input,
                mid_output,
                high: 127,
            };
            let mapped = curve.map(mid_input);
            assert!(
                mapped.abs_diff(mid_output) <= 1,
                "{curve:?} put its own middle point at {mapped}"
            );
        }
    }

    #[test]
    fn the_endpoints_are_the_floor_and_the_ceiling() {
        let curve = VelocityCurve {
            low: 25,
            mid_input: 64,
            mid_output: 70,
            high: 110,
        };
        assert!(
            curve.map(1).abs_diff(25) <= 2,
            "the floor read {}",
            curve.map(1)
        );
        assert_eq!(curve.map(127), 110);
    }

    #[test]
    fn a_curve_edited_by_hand_into_nonsense_is_corrected_not_obeyed() {
        // The ceiling below the floor, the bend outside the range and its
        // output outside both: there is no reading of this worth honouring,
        // and refusing to start over it would be worse.
        let curve = VelocityCurve {
            low: 120,
            mid_input: 200,
            mid_output: 3,
            high: 10,
        };
        let sane = curve.sanitised();
        assert_eq!(sane.low, 10);
        assert_eq!(sane.high, 120);
        assert_eq!(sane.mid_input, 126);
        assert!((sane.low..=sane.high).contains(&sane.mid_output));
        let mut previous = 0;
        for velocity in 1..=127u8 {
            let mapped = curve.map(velocity);
            assert!(mapped >= previous);
            previous = mapped;
        }
    }

    #[test]
    fn the_wide_scale_rides_the_same_shape_as_the_byte() {
        let curve = VelocityCurve {
            low: 10,
            mid_input: 40,
            mid_output: 90,
            high: 120,
        };
        for velocity in 1..=127u8 {
            let byte = f32::from(curve.map(velocity)) / 127.0;
            let wide = f32::from(
                curve.map_wide(((u32::from(velocity) * u32::from(u16::MAX)) / 127) as u16),
            ) / f32::from(u16::MAX);
            assert!(
                (byte - wide).abs() < 0.02,
                "velocity {velocity}: byte {byte} against wide {wide}"
            );
        }
    }
}
