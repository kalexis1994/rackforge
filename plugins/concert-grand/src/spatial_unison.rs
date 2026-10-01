//! First-order local spatial mobility of the existing bridge interpolation.
//! Two orthogonal coordinates: common force and the first moment of pin forces.
//! Not a full reciprocal time-domain string/soundboard solver.
#[cfg(test)]
use crate::math::expf;
use crate::math::sqrtf;

#[derive(Clone, Copy, Default)]
pub(crate) struct Loading {
    // Real deltas from identity and imaginary parts, symmetric 2x2 matrices.
    re: [f32; 3],
    im: [f32; 3],
    inv_norm: f32,
    weights: [f32; 3],
    pub radius: f32,
}

impl Loading {
    pub fn active(&self) -> bool {
        self.inv_norm > 0.0
    }

    pub fn new(
        g: [f32; 3],
        h: [f32; 3],
        norm: f32,
        weights: [f32; 3],
        radius: f32,
        rate: f32,
    ) -> Self {
        // Strang splitting D*U*D: each D is positive/contractive, U unitary.
        // Frozen-frequency approximation; splitting error is O(dt^3).
        let e = real_decay(g, 0.5 / rate);
        let d = [1.0 + e[0], e[1], 1.0 + e[2]];
        let (u, v) = unitary(h, 1.0 / rate);
        let f = multiply_symmetric(d, u);
        // D*U*D-I = 2E+E² + D*(U-I)*D. Avoid subtracting nearly equal
        // values around one: weak losses must survive f32 coefficient storage.
        let re = [
            2.0 * e[0] + e[0] * e[0] + e[1] * e[1] + f[0],
            2.0 * e[1] + e[1] * (e[0] + e[2]) + f[1],
            2.0 * e[2] + e[1] * e[1] + e[2] * e[2] + f[2],
        ];
        let im = multiply_symmetric(d, v);
        Self {
            re,
            im,
            inv_norm: 1.0 / sqrtf(norm),
            weights,
            radius,
        }
    }

    pub fn moment(&self, s: &[f32; 5]) -> f32 {
        self.radius * (self.weights[0] * s[0] + self.weights[1] * s[1] + self.weights[2] * s[2])
    }

    pub fn apply(&self, s: &mut [f32; 5], c: &mut [f32; 5], mask: u8, horizontal: f32) {
        let mut a = [horizontal * s[3], horizontal * c[3]];
        let mut b = [0.0, 0.0];
        for lane in 0..3 {
            if mask & (1 << lane) != 0 {
                a[0] += s[lane];
                a[1] += c[lane];
            }
            b[0] += self.weights[lane] * s[lane];
            b[1] += self.weights[lane] * c[lane];
        }
        a[0] *= self.inv_norm;
        a[1] *= self.inv_norm;
        let da = [
            self.re[0] * a[0] + self.re[1] * b[0] - self.im[0] * a[1] - self.im[1] * b[1],
            self.re[0] * a[1] + self.re[1] * b[1] + self.im[0] * a[0] + self.im[1] * b[0],
        ];
        let db = [
            self.re[1] * a[0] + self.re[2] * b[0] - self.im[1] * a[1] - self.im[2] * b[1],
            self.re[1] * a[1] + self.re[2] * b[1] + self.im[1] * a[0] + self.im[2] * b[0],
        ];
        for lane in 0..3 {
            if mask & (1 << lane) != 0 {
                s[lane] += self.inv_norm * da[0] + self.weights[lane] * db[0];
                c[lane] += self.inv_norm * da[1] + self.weights[lane] * db[1];
            }
        }
        s[3] += horizontal * self.inv_norm * da[0];
        c[3] += horizontal * self.inv_norm * da[1];
    }
}

/// Delta from identity of exp(-G*dt), computed from accurate small losses.
fn real_decay(g: [f32; 3], dt: f32) -> [f32; 3] {
    let mean = 0.5 * (g[0] + g[2]);
    let diff = 0.5 * (g[0] - g[2]);
    let radius = sqrtf(diff * diff + g[1] * g[1]);
    if radius < 1e-20 {
        let x = -crate::bridge::drain_coefficient(mean, 1.0, 1.0 / dt);
        return [x, 0.0, x];
    }
    // A sum of collocated conductances is PSD; clamp roundoff at zero.
    let lo = -crate::bridge::drain_coefficient((mean - radius).max(0.0), 1.0, 1.0 / dt);
    let hi = -crate::bridge::drain_coefficient((mean + radius).max(0.0), 1.0, 1.0 / dt);
    let a = 0.5 * (hi + lo);
    let b = 0.5 * (hi - lo) / radius;
    [a + b * diff, b * g[1], a - b * diff]
}

fn unitary(h: [f32; 3], dt: f32) -> ([f32; 3], [f32; 3]) {
    let mean = 0.5 * (h[0] + h[2]);
    let diff = 0.5 * (h[0] - h[2]);
    let radius = sqrtf(diff * diff + h[1] * h[1]);
    let (sin, cos) = crate::bridge::loading_sincos(mean * dt);
    let (sr, cr) = crate::bridge::loading_sincos(radius * dt);
    let scale = if radius > 1e-20 { sr / radius } else { dt };
    let b = [scale * diff, scale * h[1], -scale * diff];
    let loss = |s: f32, c: f32| if c > 0.5 { s * s / (1.0 + c) } else { 1.0 - c };
    let lm = loss(sin, cos);
    let lr = loss(sr, cr);
    let delta = -lm - lr + lm * lr;
    (
        [delta - sin * b[0], -sin * b[1], delta - sin * b[2]],
        [sin * cr + cos * b[0], cos * b[1], sin * cr + cos * b[2]],
    )
}

/// D*U*D, preserving symmetry without a general matrix allocation.
fn multiply_symmetric(d: [f32; 3], u: [f32; 3]) -> [f32; 3] {
    [
        d[0] * d[0] * u[0] + 2.0 * d[0] * d[1] * u[1] + d[1] * d[1] * u[2],
        d[0] * d[1] * u[0] + (d[0] * d[2] + d[1] * d[1]) * u[1] + d[1] * d[2] * u[2],
        d[1] * d[1] * u[0] + 2.0 * d[1] * d[2] * u[1] + d[2] * d[2] * u[2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_loading_is_contractive_for_arbitrary_complex_states() {
        for rate in [16000.0, 44100.0, 96000.0] {
            for g in [
                [0.0, 0.0, 0.0],
                [500.0, 50.0, 20.0],
                [5.0, -2.0, 1.0],
                [10000.0, 1000.0, 200.0],
            ] {
                for h in [
                    [0.0, 0.0, 0.0],
                    [-1000.0, 20.0, 80.0],
                    [5000.0, -1700.0, -500.0],
                ] {
                    let l = Loading::new(
                        g,
                        h,
                        3.0144,
                        [
                            -core::f32::consts::FRAC_1_SQRT_2,
                            0.0,
                            core::f32::consts::FRAC_1_SQRT_2,
                        ],
                        0.01,
                        rate,
                    );
                    for seed in 0..32 {
                        let mut s =
                            core::array::from_fn(|i| crate::hash01(seed * 31 + i as u32) - 0.5);
                        let mut c =
                            core::array::from_fn(|i| crate::hash01(seed * 79 + i as u32) - 0.5);
                        let before: f32 = s.iter().chain(c.iter()).map(|x| x * x).sum();
                        l.apply(&mut s, &mut c, 7, 0.12);
                        let after: f32 = s.iter().chain(c.iter()).map(|x| x * x).sum();
                        assert!(
                            after <= before + 2e-6,
                            "{g:?}/{h:?}/{rate}: {before} -> {after}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn common_coordinate_matches_existing_loading_without_spatial_gradient() {
        let norm = 3.0144;
        let l = Loading::new(
            [5.0 * norm, 0.0, 0.0],
            [40.0 * norm, 0.0, 0.0],
            norm,
            [
                -core::f32::consts::FRAC_1_SQRT_2,
                0.0,
                core::f32::consts::FRAC_1_SQRT_2,
            ],
            0.0,
            44100.0,
        );
        let mut s = [1.0, 1.0, 1.0, 0.12, 0.0];
        let mut c = [0.0; 5];
        for _ in 0..256 {
            l.apply(&mut s, &mut c, 7, 0.12);
        }
        let radius = expf(-5.0 * norm * 256.0 / 44100.0);
        let phase = 40.0 * norm * 256.0 / 44100.0;
        assert!(
            (s[0] - radius * phase.cos()).abs() < 3e-5,
            "{} expected {}",
            s[0],
            radius * phase.cos()
        );
        assert!(
            (c[0] - radius * phase.sin()).abs() < 3e-5,
            "{} expected {}",
            c[0],
            radius * phase.sin()
        );
    }

    #[test]
    fn force_moment_is_work_conjugate_to_modal_gradient() {
        let gap = 0.006 / 1.4;
        let w = [-1.0, 0.0, 1.0];
        let s = [0.21, -0.4, 0.72, 0.0, 0.0];
        let weights = w.map(|v| v * core::f32::consts::FRAC_1_SQRT_2);
        let l = Loading::new(
            [1.0, 0.0, 1.0],
            [0.0; 3],
            3.0,
            weights,
            gap * sqrtf(2.0),
            44100.0,
        );
        for phi in [-0.3, 0.0, 1.4] {
            for derivative in [-20.0, 0.0, 20.0] {
                let separate: f32 = (0..3).map(|i| (phi + gap * w[i] * derivative) * s[i]).sum();
                let combined = phi * (s[0] + s[1] + s[2]) + derivative * l.moment(&s);
                assert!((separate - combined).abs() < 2e-7);
            }
        }
    }
}
