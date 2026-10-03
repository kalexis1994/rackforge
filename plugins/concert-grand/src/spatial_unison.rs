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

    pub fn moment(&self, s: &[f32]) -> f32 {
        self.radius * (self.weights[0] * s[0] + self.weights[1] * s[1] + self.weights[2] * s[2])
    }

    /// Deltas in the two orthonormal mechanical coordinates. Reused by the
    /// native directional diagnostic; pin moments and polarizations are
    /// different choices of coordinates, not the same physical hypothesis.
    fn coordinate_delta(&self, a: [f32; 2], b: [f32; 2]) -> ([f32; 2], [f32; 2]) {
        let da = [
            self.re[0] * a[0] + self.re[1] * b[0] - self.im[0] * a[1] - self.im[1] * b[1],
            self.re[0] * a[1] + self.re[1] * b[1] + self.im[0] * a[0] + self.im[1] * b[0],
        ];
        let db = [
            self.re[1] * a[0] + self.re[2] * b[0] - self.im[1] * a[1] - self.im[2] * b[1],
            self.re[1] * a[1] + self.re[2] * b[1] + self.im[1] * a[0] + self.im[2] * b[0],
        ];
        (da, db)
    }

    pub fn apply(&self, s: &mut [f32], c: &mut [f32], mask: u8, horizontal: f32) {
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
        let (da, db) = self.coordinate_delta(a, b);
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

/// Native P1a diagnostic: collective vertical motion and the inherited single
/// horizontal state. It does NOT provide two polarizations per physical string.
/// `lever` is height / assumed bridge-line length, not measured geometry.
#[cfg(test)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Directional {
    matrix: Loading,
    inv_root: f32,
    lever: f32,
}

#[cfg(test)]
impl Directional {
    pub fn new(g: [f32; 3], h: [f32; 3], count: f32, lever: f32, rate: f32) -> Self {
        Self {
            matrix: Loading::new(g, h, 1.0, [0.0; 3], 0.0, rate),
            inv_root: 1.0 / sqrtf(count),
            lever,
        }
    }
    pub fn active(&self) -> bool {
        self.inv_root > 0.0
    }
    pub fn moment(&self, s: &[f32]) -> f32 {
        self.lever * s[3]
    }
    pub fn apply(&self, s: &mut [f32], c: &mut [f32], mask: u8) {
        let mut a = [0.0; 2];
        for lane in 0..3 {
            if mask & (1 << lane) != 0 {
                a[0] += s[lane];
                a[1] += c[lane];
            }
        }
        a[0] *= self.inv_root;
        a[1] *= self.inv_root;
        let (da, db) = self.matrix.coordinate_delta(a, [s[3], c[3]]);
        for lane in 0..3 {
            if mask & (1 << lane) != 0 {
                s[lane] += self.inv_root * da[0];
                c[lane] += self.inv_root * da[1];
            }
        }
        s[3] += db[0];
        c[3] += db[1];
    }

    /// P1b: the same two-coordinate matrix, lifted isometrically into the
    /// common horizontal subspace. Horizontal differences remain dark unless
    /// unequal rotations or another spatial projection mix them into it.
    pub fn apply_split(&self, s: &mut [f32], c: &mut [f32], mask: u8) {
        let mut a = [0.0; 2];
        for lane in 0..3 {
            if mask & (1 << lane) != 0 {
                a[0] += s[lane];
                a[1] += c[lane];
            }
        }
        a[0] *= self.inv_root;
        a[1] *= self.inv_root;
        let b = [
            crate::polarization::common(s, mask),
            crate::polarization::common(c, mask),
        ];
        let (da, db) = self.matrix.coordinate_delta(a, b);
        for lane in 0..3 {
            if mask & (1 << lane) != 0 {
                s[lane] += self.inv_root * da[0];
                c[lane] += self.inv_root * da[1];
                let h = crate::polarization::HORIZONTAL_LANES[lane];
                s[h] += self.inv_root * db[0];
                c[h] += self.inv_root * db[1];
            }
        }
    }

    pub fn split_moment(&self, s: &[f32], mask: u8) -> f32 {
        self.lever * crate::polarization::common(s, mask)
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
    fn directional_zero_lever_matches_scalar_loading() {
        for count in 1..=3 {
            let mask = (1 << count) - 1;
            let n = count as f32;
            let horizontal = 0.12;
            let norm = n + horizontal * horizontal;
            for (gamma, beta) in [(0.0, 20.0), (5.0, -300.0), (200.0, 70.0)] {
                let g = [
                    gamma * n,
                    gamma * sqrtf(n) * horizontal,
                    gamma * horizontal * horizontal,
                ];
                let h = [
                    beta * n,
                    beta * sqrtf(n) * horizontal,
                    beta * horizontal * horizontal,
                ];
                let directional = Directional::new(g, h, n, 0.0, 44100.0);
                let (drain, phase) = crate::bridge::complex_loading(gamma, beta, norm, 44100.0);
                let mut s = [0.3, -0.2, 0.1, 0.7, -0.5];
                let mut c = [-0.4, 0.6, -0.1, 0.2, 0.8];
                let (mut expected_s, mut expected_c) = (s, c);
                let mut sum = [horizontal * s[3], horizontal * c[3]];
                for lane in 0..count {
                    sum[0] += s[lane];
                    sum[1] += c[lane];
                }
                let take = [
                    drain * sum[0] + phase * sum[1],
                    drain * sum[1] - phase * sum[0],
                ];
                for lane in 0..count {
                    expected_s[lane] -= take[0];
                    expected_c[lane] -= take[1];
                }
                expected_s[3] -= horizontal * take[0];
                expected_c[3] -= horizontal * take[1];
                directional.apply(&mut s, &mut c, mask);
                for lane in 0..5 {
                    assert!((s[lane] - expected_s[lane]).abs() < 2e-6);
                    assert!((c[lane] - expected_c[lane]).abs() < 2e-6);
                }
                assert_eq!(directional.moment(&s), 0.0);
            }
        }
    }

    #[test]
    fn directional_loading_is_passive_and_preserves_dark_vertical_motion() {
        // G and H are sums of modal outer products: a dissipative PSD matrix
        // and an arbitrary symmetric reactive matrix, with different axes.
        for rate in [16000.0, 44100.0, 96000.0] {
            for count in 1..=3 {
                let mask = (1 << count) - 1;
                for seed in 0..32 {
                    let d = Directional::new(
                        [20.0, -7.0, 5.0],
                        [-300.0, 40.0, 50.0],
                        count as f32,
                        0.02,
                        rate,
                    );
                    let mut s: [f32; 5] =
                        core::array::from_fn(|i| crate::hash01(seed * 31 + i as u32) - 0.5);
                    let mut c: [f32; 5] =
                        core::array::from_fn(|i| crate::hash01(seed * 79 + i as u32) - 0.5);
                    let before: f32 = s.iter().chain(c.iter()).map(|x| x * x).sum();
                    let differences = [s[0] - s[1], c[0] - c[1]];
                    let bloom = [s[4], c[4]];
                    d.apply(&mut s, &mut c, mask);
                    let after: f32 = s.iter().chain(c.iter()).map(|x| x * x).sum();
                    assert!(after <= before + 2e-6);
                    assert_eq!([s[4], c[4]], bloom);
                    if count >= 2 {
                        assert!((s[0] - s[1] - differences[0]).abs() < 1e-6);
                        assert!((c[0] - c[1] - differences[1]).abs() < 1e-6);
                    }
                }
            }
        }
    }

    #[test]
    fn directional_moment_and_loading_share_the_lever_projection() {
        let lever = 0.025 / 1.4;
        let d = Directional::new([1.0, 0.0, 1.0], [0.0; 3], 3.0, lever, 44100.0);
        let state = [0.3, -0.2, 0.1, 0.8, 0.0];
        let (projection, gradient, horizontal) = (0.6, -3.2, 0.12);
        let source = projection * (state[0] + state[1] + state[2] + horizontal * state[3])
            + gradient * d.moment(&state);
        let local = projection * (state[0] + state[1] + state[2])
            + (horizontal * projection + lever * gradient) * state[3];
        assert!((source - local).abs() < 1e-6);
    }

    #[test]
    #[ignore = "offline comparison with independent scipy matrix exponentials"]
    fn export_directional_matrix_probes() {
        use std::io::Write;
        let out = std::env::var("CG_DIRECTIONAL_MATRIX_CSV").expect("output CSV");
        let mut file = std::fs::File::create(out).unwrap();
        writeln!(
            file,
            "rate,g00,g01,g11,h00,h01,h11,re00,re01,re11,im00,im01,im11"
        )
        .unwrap();
        for rate in [16000.0, 44100.0, 96000.0] {
            for (g, h) in [
                ([20.0, -7.0, 5.0], [-300.0, 40.0, 50.0]),
                ([0.0; 3], [5000.0, -1700.0, -500.0]),
                ([10000.0, 1000.0, 200.0], [5000.0, -1700.0, -500.0]),
            ] {
                let m = Loading::new(g, h, 1.0, [0.0; 3], 0.0, rate);
                writeln!(
                    file,
                    "{rate},{},{},{},{},{},{},{},{},{},{},{},{}",
                    g[0],
                    g[1],
                    g[2],
                    h[0],
                    h[1],
                    h[2],
                    m.re[0],
                    m.re[1],
                    m.re[2],
                    m.im[0],
                    m.im[1],
                    m.im[2]
                )
                .unwrap();
            }
        }
    }

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
                        let mut s: [f32; 5] =
                            core::array::from_fn(|i| crate::hash01(seed * 31 + i as u32) - 0.5);
                        let mut c: [f32; 5] =
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
