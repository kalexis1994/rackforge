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
        let mapped = self.evaluate(f64::from(velocity.min(127)) / 127.0);
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
        let mapped = self.evaluate(f64::from(velocity) / f64::from(u16::MAX));
        let scaled = (mapped * f64::from(u16::MAX) + 0.5) as i32;
        scaled.clamp(1, i32::from(u16::MAX)) as u16
    }

    /// The curve on the unit square, which is where it is drawn and where
    /// both widths above meet it.
    ///
    /// Double precision, and not because the shape needs it. The browser
    /// draws this same curve in JavaScript, where every number is a double,
    /// and single precision here made the two answers differ by one step on
    /// about one reading in eight thousand — a byte the square promised and
    /// the keyboard did not play. In double they are the same arithmetic on
    /// the same widths, so they agree because they cannot do otherwise.
    /// `conformance_vectors` below is where that is held to.
    pub fn evaluate(&self, x: f64) -> f64 {
        let curve = self.sanitised();
        let xs = [0.0, f64::from(curve.mid_input) / 127.0, 1.0];
        let ys = [
            f64::from(curve.low) / 127.0,
            f64::from(curve.mid_output) / 127.0,
            f64::from(curve.high) / 127.0,
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
fn monotone_hermite(xs: &[f64; 3], ys: &[f64; 3], x: f64) -> f64 {
    let mut secant = [0.0f64; 2];
    for i in 0..2 {
        let run = xs[i + 1] - xs[i];
        secant[i] = if run > 1e-6 {
            (ys[i + 1] - ys[i]) / run
        } else {
            0.0
        };
    }
    let mut tangent = [0.0f64; 3];
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

/// The curves the conformance vectors cover.
///
/// Each is a shape the editor can actually produce or a file on disk can
/// actually carry: the identity, a raised floor, a bend either way, a narrow
/// band, a span of nothing, both extremes of the bend, and a curve edited by
/// hand into nonsense. Adding one here and regenerating the fixture is how a
/// newly found disagreement between the two implementations becomes a
/// standing test rather than a bug someone remembers.
const CONFORMANCE_CURVES: [(&str, VelocityCurve); 15] = [
    (
        "identity",
        VelocityCurve {
            low: 0,
            mid_input: 64,
            mid_output: 64,
            high: 127,
        },
    ),
    (
        "raised floor",
        VelocityCurve {
            low: 40,
            mid_input: 64,
            mid_output: 80,
            high: 127,
        },
    ),
    (
        "bend towards loud",
        VelocityCurve {
            low: 0,
            mid_input: 20,
            mid_output: 100,
            high: 127,
        },
    ),
    (
        "bend towards soft",
        VelocityCurve {
            low: 0,
            mid_input: 110,
            mid_output: 20,
            high: 127,
        },
    ),
    (
        "narrow band",
        VelocityCurve {
            low: 30,
            mid_input: 64,
            mid_output: 35,
            high: 90,
        },
    ),
    (
        "span of nothing",
        VelocityCurve {
            low: 60,
            mid_input: 30,
            mid_output: 60,
            high: 60,
        },
    ),
    (
        "bend hard against the floor",
        VelocityCurve {
            low: 0,
            mid_input: 1,
            mid_output: 127,
            high: 127,
        },
    ),
    (
        "bend hard against the ceiling",
        VelocityCurve {
            low: 0,
            mid_input: 126,
            mid_output: 0,
            high: 127,
        },
    ),
    (
        "silent ceiling",
        VelocityCurve {
            low: 0,
            mid_input: 64,
            mid_output: 0,
            high: 0,
        },
    ),
    (
        "floor and ceiling both moved",
        VelocityCurve {
            low: 25,
            mid_input: 64,
            mid_output: 70,
            high: 110,
        },
    ),
    (
        "the wide-scale curve",
        VelocityCurve {
            low: 10,
            mid_input: 40,
            mid_output: 90,
            high: 120,
        },
    ),
    (
        "nonsense, to be corrected",
        VelocityCurve {
            low: 120,
            mid_input: 200,
            mid_output: 3,
            high: 10,
        },
    ),
    // The three below are not shapes anyone would choose. They are the
    // readings that sat closest to a rounding boundary in a sweep of five
    // and a half million, back when this side read in single precision and
    // the browser read in double: each one landed a step apart. They are
    // here so that the arithmetic the two sides share is pinned where it is
    // thinnest, rather than only where it is comfortable.
    (
        "knife edge, against the floor",
        VelocityCurve {
            low: 0,
            mid_input: 1,
            mid_output: 0,
            high: 28,
        },
    ),
    (
        "knife edge, past the bend",
        VelocityCurve {
            low: 0,
            mid_input: 45,
            mid_output: 0,
            high: 28,
        },
    ),
    (
        "knife edge, over the ceiling",
        VelocityCurve {
            low: 0,
            mid_input: 12,
            mid_output: 91,
            high: 84,
        },
    ),
];

/// How many samples of the unit square each curve is recorded at.
const CONFORMANCE_EVALUATE_STEPS: u8 = 64;

fn conformance_curve_json(curve: &VelocityCurve) -> String {
    format!(
        "{{ \"low\": {}, \"mid_input\": {}, \"mid_output\": {}, \"high\": {} }}",
        curve.low, curve.mid_input, curve.mid_output, curve.high
    )
}

/// The velocity reading written down as numbers, for the hosts that
/// implement it twice.
///
/// RackForge reads velocity here, on the audio thread, and again in
/// TypeScript, where the square in Settings draws what this thread is about
/// to do. Two implementations of one curve drift, and the drift is silent:
/// the drawing goes on looking right while the keyboard plays something
/// else. This is the arithmetic itself — the corrected curve, whether it is
/// the identity, all 128 readings, and the shape on the unit square — so
/// that both sides can be held to the same numbers instead of each to its
/// own.
///
/// `crates/rackforge-midi-api/fixtures/velocity-curve-v1.json` is this
/// function's output. The test below compares them and
/// `web/src/velocityCurve.conformance.test.ts` reads the same file, so the
/// fixture is a view of this implementation rather than a third copy of it.
///
/// The inputs are all in the range a `u8` can hold, because this side cannot
/// express any other kind. The readings the editor invents mid-drag — a
/// fraction, a negative — belong to the TypeScript tests alone.
pub fn conformance_vectors() -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"contract\": \"velocity-curve-v1\",\n");
    out.push_str(
        "  \"generated_by\": \"UPDATE_VELOCITY_VECTORS=1 cargo test -p rackforge-midi-api\",\n",
    );
    out.push_str(&format!(
        "  \"evaluate_steps\": {CONFORMANCE_EVALUATE_STEPS},\n"
    ));
    out.push_str("  \"curves\": [\n");
    for (index, (name, curve)) in CONFORMANCE_CURVES.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!("      \"name\": \"{name}\",\n"));
        out.push_str(&format!(
            "      \"input\": {},\n",
            conformance_curve_json(curve)
        ));
        out.push_str(&format!(
            "      \"sanitised\": {},\n",
            conformance_curve_json(&curve.sanitised())
        ));
        out.push_str(&format!("      \"identity\": {},\n", curve.is_identity()));
        out.push_str("      \"map\": [\n");
        for velocity in 0..=127u8 {
            if velocity % 16 == 0 {
                out.push_str("        ");
            }
            out.push_str(&curve.map(velocity).to_string());
            if velocity < 127 {
                out.push(',');
            }
            if velocity % 16 == 15 {
                out.push('\n');
            } else {
                out.push(' ');
            }
        }
        out.push_str("      ],\n");
        out.push_str("      \"evaluate\": [\n");
        for step in 0..=CONFORMANCE_EVALUATE_STEPS {
            if step % 8 == 0 {
                out.push_str("        ");
            }
            let x = f64::from(step) / f64::from(CONFORMANCE_EVALUATE_STEPS);
            out.push_str(&curve.evaluate(x).to_string());
            if step < CONFORMANCE_EVALUATE_STEPS {
                out.push(',');
            }
            if step % 8 == 7 || step == CONFORMANCE_EVALUATE_STEPS {
                out.push('\n');
            } else {
                out.push(' ');
            }
        }
        out.push_str("      ]\n");
        if index + 1 == CONFORMANCE_CURVES.len() {
            out.push_str("    }\n");
        } else {
            out.push_str("    },\n");
        }
    }
    out.push_str("  ]\n");
    out.push_str("}\n");
    out
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

    #[test]
    fn the_conformance_vectors_match_this_implementation() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/velocity-curve-v1.json"
        );
        let expected = conformance_vectors();
        if std::env::var("UPDATE_VELOCITY_VECTORS").is_ok() {
            std::fs::write(path, &expected).expect("writing the conformance vectors");
            return;
        }
        // Compared without regard to line endings. The record is stored with
        // newlines, and `text=auto` hands a Windows checkout the same bytes
        // with carriage returns in them — so a contributor there would be
        // told the record is out of date by a difference nobody made and
        // regenerating cannot fix.
        let actual = std::fs::read_to_string(path)
            .expect("reading fixtures/velocity-curve-v1.json")
            .replace('\r', "");
        assert_eq!(
            actual, expected,
            "fixtures/velocity-curve-v1.json is out of date; run \
             UPDATE_VELOCITY_VECTORS=1 cargo test -p rackforge-midi-api"
        );
    }
}
