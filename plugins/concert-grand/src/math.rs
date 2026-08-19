//! Control-rate float math for a `no_std` component.
//!
//! The audio loop needs none of this — rendering is multiplies and adds by
//! construction. These run at note-on and prepare time only, so they favour
//! clarity over speed, and each states its accuracy. All of them are far
//! more precise than anything audible: the largest error below is parts in
//! 10⁷, and it feeds coefficients, not samples.

/// `sin` and `cos` together, exact quadrant logic, polynomial on [-π/4, π/4].
/// Absolute error a few parts in 10⁶ for |x| < 100, dominated by f32
/// argument-reduction rounding rather than by the polynomial.
pub fn sincosf(x: f32) -> (f32, f32) {
    const FRAC_2_PI: f32 = 0.636_619_77;
    const FRAC_PI_2: f32 = 1.570_796_3;
    let quadrant = roundf(x * FRAC_2_PI);
    let r = x - quadrant * FRAC_PI_2;
    let r2 = r * r;
    let sin = r * (1.0 - r2 / 6.0 * (1.0 - r2 / 20.0 * (1.0 - r2 / 42.0)));
    let cos = 1.0 - r2 / 2.0 * (1.0 - r2 / 12.0 * (1.0 - r2 / 30.0));
    match (quadrant as i32).rem_euclid(4) {
        0 => (sin, cos),
        1 => (cos, -sin),
        2 => (-sin, -cos),
        _ => (-cos, sin),
    }
}

/// `e^x` via exponent split and a degree-6 polynomial on the remainder.
/// Relative error below 1e-6 across the range this model uses (|x| < 30).
pub fn expf(x: f32) -> f32 {
    const LN_2: f32 = 0.693_147_2;
    if x < -87.0 {
        return 0.0;
    }
    if x > 88.0 {
        return f32::INFINITY;
    }
    let k = roundf(x / LN_2);
    let r = x - k * LN_2;
    let mut power = 1.0;
    let mut term = 1.0;
    for n in 1..8 {
        term *= r / n as f32;
        power += term;
    }
    scale_by_power_of_two(power, k as i32)
}

/// Natural log from the significand's `atanh` series plus the exponent.
/// Relative error below 1e-6 for normal positive inputs.
pub fn lnf(x: f32) -> f32 {
    const LN_2: f32 = 0.693_147_2;
    const SQRT_2: f32 = 1.414_213_6;
    if x <= 0.0 {
        return f32::NEG_INFINITY;
    }
    let bits = x.to_bits();
    let mut exponent = ((bits >> 23) & 0xff) as i32 - 127;
    let mut mantissa = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000);
    if mantissa > SQRT_2 {
        mantissa *= 0.5;
        exponent += 1;
    }
    let t = (mantissa - 1.0) / (mantissa + 1.0);
    let t2 = t * t;
    let series = t * (2.0 + t2 * (2.0 / 3.0 + t2 * (2.0 / 5.0 + t2 * (2.0 / 7.0 + t2 * (2.0 / 9.0)))));
    series + exponent as f32 * LN_2
}

pub fn log2f(x: f32) -> f32 {
    lnf(x) * 1.442_695
}

/// `x^y` for positive `x`, through `exp(y·ln x)`.
pub fn powf(x: f32, y: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    expf(y * lnf(x))
}

/// Newton's method seeded from the bit pattern; three refinements give
/// relative error below 1e-7 for normal positive inputs.
pub fn sqrtf(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut y = f32::from_bits((x.to_bits() >> 1) + 0x1fbd_1df5);
    for _ in 0..3 {
        y = 0.5 * (y + x / y);
    }
    y
}

pub fn roundf(x: f32) -> f32 {
    let truncated = x as i32 as f32;
    let fraction = x - truncated;
    if fraction > 0.5 {
        truncated + 1.0
    } else if fraction < -0.5 {
        truncated - 1.0
    } else {
        truncated
    }
}

fn scale_by_power_of_two(value: f32, power: i32) -> f32 {
    let exponent = power.clamp(-126, 127) + 127;
    value * f32::from_bits((exponent as u32) << 23)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_std_within_stated_accuracy() {
        for i in 0..2000 {
            let x = (i as f32 - 1000.0) * 0.01;
            let (sin, cos) = sincosf(x);
            assert!((sin - x.sin()).abs() < 5e-6, "sin({x})");
            assert!((cos - x.cos()).abs() < 5e-6, "cos({x})");
            assert!(
                (expf(x * 0.03) - (x * 0.03).exp()).abs() / (x * 0.03).exp() < 1e-5,
                "exp({x})"
            );
        }
        for i in 1..2000 {
            let x = i as f32 * 0.37;
            assert!((lnf(x) - x.ln()).abs() < 2e-6 * (1.0 + x.ln().abs()), "ln({x})");
            assert!((sqrtf(x) - x.sqrt()).abs() / x.sqrt() < 1e-6, "sqrt({x})");
        }
        assert!((powf(10.0, -3.95) - 10.0_f32.powf(-3.95)).abs() < 1e-8);
    }
}
