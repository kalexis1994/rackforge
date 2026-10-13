//! SI extensible three-wire choir and live shared hammer, fixed bridge pilot.
//! Exact finite cosine quadrature and AAVF carriers; no samples or state resets.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    env, fs,
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
struct CountAlloc;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Complex {
    r: f64,
    i: f64,
}
type FftKernel = fn(&mut [Complex], &[Complex], bool);
fn fft_butterflies_portable(data: &mut [Complex], roots: &[Complex], inverse: bool) {
    let mut width = 2;
    while width <= data.len() {
        let half = width / 2;
        let twiddles = &roots[half - 1..2 * half - 1];
        for chunk in data.chunks_exact_mut(width) {
            let (left, right) = chunk.split_at_mut(half);
            for ((u, v), root) in left.iter_mut().zip(right).zip(twiddles) {
                let wi = if inverse { -root.i } else { root.i };
                let r = v.r * root.r - v.i * wi;
                let i = v.r * wi + v.i * root.r;
                *v = Complex {
                    r: u.r - r,
                    i: u.i - i,
                };
                *u = Complex {
                    r: u.r + r,
                    i: u.i + i,
                };
            }
        }
        width *= 2;
    }
}
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn fft_butterflies_avx2(data: &mut [Complex], roots: &[Complex], inverse: bool) {
    use std::arch::x86_64::*;
    // Complex has repr(C); each load covers exactly two adjacent complex
    // values. Power-of-two halves >=2 are even, with no overread.
    unsafe {
        let ptr = data.as_mut_ptr();
        let mut width = 2;
        while width <= data.len() {
            let half = width / 2;
            let twiddle = roots.as_ptr().add(half - 1);
            for first in (0..data.len()).step_by(width) {
                if half == 1 {
                    let u = *ptr.add(first);
                    let v = *ptr.add(first + half);
                    let w = *twiddle;
                    let wi = if inverse { -w.i } else { w.i };
                    let r = v.r * w.r - v.i * wi;
                    let i = v.r * wi + v.i * w.r;
                    *ptr.add(first) = Complex {
                        r: u.r + r,
                        i: u.i + i,
                    };
                    *ptr.add(first + half) = Complex {
                        r: u.r - r,
                        i: u.i - i,
                    };
                } else {
                    for j in (0..half).step_by(2) {
                        let u = _mm256_loadu_pd(ptr.add(first + j).cast());
                        let v = _mm256_loadu_pd(ptr.add(first + j + half).cast());
                        let mut w = _mm256_loadu_pd(twiddle.add(j).cast());
                        if inverse {
                            w = _mm256_xor_pd(w, _mm256_set_pd(-0., 0., -0., 0.));
                        }
                        let wr = _mm256_movedup_pd(w);
                        let wi = _mm256_permute_pd(w, 15);
                        let swapped = _mm256_permute_pd(v, 5);
                        let product =
                            _mm256_addsub_pd(_mm256_mul_pd(v, wr), _mm256_mul_pd(swapped, wi));
                        _mm256_storeu_pd(ptr.add(first + j).cast(), _mm256_add_pd(u, product));
                        _mm256_storeu_pd(
                            ptr.add(first + j + half).cast(),
                            _mm256_sub_pd(u, product),
                        );
                    }
                }
            }
            width *= 2;
        }
    }
}
#[cfg(target_arch = "x86_64")]
fn fft_simd(data: &mut [Complex], roots: &[Complex], inverse: bool) {
    unsafe {
        fft_butterflies_avx2(data, roots, inverse);
    }
}
fn fft_dispatch() -> (FftKernel, bool) {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        return (fft_simd, true);
    }
    (fft_butterflies_portable, false)
}
struct CosinePlan {
    nodes: usize,
    fft: Vec<Complex>,
    roots: Vec<Complex>,
    phase: Vec<Complex>,
    reverse: Vec<usize>,
    kernel: FftKernel,
    simd_fft: bool,
    real_roots: Vec<Complex>,
    zero_transforms: usize,
}
impl CosinePlan {
    fn new(nodes: usize) -> Self {
        assert!(nodes.is_power_of_two());
        let n = nodes / 2;
        let bits = n.trailing_zeros();
        let (kernel, simd_fft) = fft_dispatch();
        Self {
            kernel,
            simd_fft,
            nodes,
            real_roots: (0..nodes)
                .map(|k| {
                    let angle = -2. * std::f64::consts::PI * k as f64 / nodes as f64;
                    Complex {
                        r: angle.cos(),
                        i: angle.sin(),
                    }
                })
                .collect(),
            zero_transforms: 0,
            fft: vec![Complex::default(); n],
            roots: (1..=bits)
                .flat_map(|stage| {
                    let half = 1usize << (stage - 1);
                    let stride = n / (2 * half);
                    (0..half).map(move |k| {
                        let a = -2. * std::f64::consts::PI * (k * stride) as f64 / n as f64;
                        Complex {
                            r: a.cos(),
                            i: a.sin(),
                        }
                    })
                })
                .collect(),
            phase: (0..nodes)
                .map(|k| {
                    let a = std::f64::consts::PI * k as f64 / (2 * nodes) as f64;
                    Complex {
                        r: a.cos(),
                        i: a.sin(),
                    }
                })
                .collect(),
            reverse: (0..n)
                .map(|k| k.reverse_bits() >> (usize::BITS - bits))
                .collect(),
        }
    }
    fn transform(&mut self, inverse: bool) {
        let n = self.fft.len();
        for k in 0..n {
            if self.reverse[k] > k {
                self.fft.swap(k, self.reverse[k]);
            }
        }
        (self.kernel)(&mut self.fft, &self.roots, inverse);
        if inverse {
            for x in &mut self.fft {
                x.r /= n as f64;
                x.i /= n as f64;
            }
        }
    }
    fn spectrum(&self, q: &[f64], scales: &[f64], k: usize) -> Complex {
        let a = if k > 0 && k <= q.len() {
            self.nodes as f64 * 0.5 * q[k - 1] * scales[k - 1]
        } else {
            0.
        };
        let mirror = self.nodes - k;
        let b = if k > 0 && mirror <= q.len() {
            self.nodes as f64 * 0.5 * q[mirror - 1] * scales[mirror - 1]
        } else {
            0.
        };
        let p = self.phase[k];
        Complex {
            r: a * p.r + b * p.i,
            i: a * p.i - b * p.r,
        }
    }
    fn synthesize(&mut self, q: &[f64], scales: &[f64], out: &mut [f64]) {
        assert_eq!(q.len(), scales.len());
        assert_eq!(out.len(), self.nodes);
        if q.iter().all(|x| *x == 0.) {
            out.fill(0.);
            self.zero_transforms += 1;
            return;
        }
        // Recover the FFT of packed even/odd real values: Z=E+iO,
        // E=(X_k+X_{k+N/2})/2, O=conj(W_k)*(X_k-X_{k+N/2})/2.
        let m = self.nodes / 2;
        for k in 0..m {
            let x = self.spectrum(q, scales, k);
            let y = self.spectrum(q, scales, k + m);
            let w = self.real_roots[k];
            let dr = x.r - y.r;
            let di = x.i - y.i;
            self.fft[k] = Complex {
                r: 0.5 * (x.r + y.r + w.i * dr - w.r * di),
                i: 0.5 * (x.i + y.i + w.r * dr + w.i * di),
            };
        }
        self.transform(true);
        for j in 0..m {
            let a = 2 * j;
            let b = a + 1;
            let ia = if a < m {
                2 * a
            } else {
                2 * (self.nodes - 1 - a) + 1
            };
            let ib = if b < m {
                2 * b
            } else {
                2 * (self.nodes - 1 - b) + 1
            };
            out[ia] = self.fft[j].r;
            out[ib] = self.fft[j].i;
        }
    }
    fn analyze(&mut self, values: &[f64], scales: &[f64], factor: f64, out: &mut [f64]) {
        assert_eq!(values.len(), self.nodes);
        assert_eq!(scales.len(), out.len());
        if values.iter().all(|x| *x == 0.) {
            out.fill(0.);
            self.zero_transforms += 1;
            return;
        }
        let m = self.nodes / 2;
        for j in 0..m {
            let a = 2 * j;
            let b = a + 1;
            let ia = if a < m {
                2 * a
            } else {
                2 * (self.nodes - 1 - a) + 1
            };
            let ib = if b < m {
                2 * b
            } else {
                2 * (self.nodes - 1 - b) + 1
            };
            self.fft[j] = Complex {
                r: values[ia],
                i: values[ib],
            };
        }
        self.transform(false);
        for (index, (value, scale)) in out.iter_mut().zip(scales).enumerate() {
            let k = index + 1;
            let a = self.fft[k % m];
            let b = self.fft[(m - k % m) % m];
            let w = self.real_roots[k];
            let xr = 0.5 * (a.r + b.r + w.r * (a.i + b.i) + w.i * (a.r - b.r));
            let xi = 0.5 * (a.i - b.i + w.r * (b.r - a.r) + w.i * (a.i + b.i));
            let p = self.phase[k];
            *value = 2. * factor * *scale * (xr * p.r + xi * p.i);
        }
    }
}

// Data for the axial endpoint elimination. The dynamic modal carriers
// remain live: only their nonlinear endpoint forces are solved directly.
struct AxialEndpoint<'a> {
    frames: usize,
    micro_reference: bool,
    q: &'a mut [f64],
    v: &'a mut [f64],
    q0: &'a [f64],
    v0: &'a [f64],
    start_force: &'a [f64],
    end_force: &'a mut [f64],
    correction: &'a mut [f64],
    scalar_correction: Option<f64>,
    weighted_displacement: &'a mut [f64],
    coordinate_work: &'a mut [f64],
    transfer: &'a [[f64; 10]],
    c: &'a [f64],
    s: &'a [f64],
    gain: &'a [f64],
}
pub struct Wire {
    nt: usize,
    nl: usize,
    nodes: usize,
    length: f64,
    ea: f64,
    omega2: Vec<f64>,
    ku: Vec<f64>,
    kv: Vec<f64>,
    cross: Vec<f64>,
    lift_mass: f64,
    plan: CosinePlan,
    fields: Vec<f64>,
    products: Vec<f64>,
}
impl Wire {
    fn new(
        nt: usize,
        nl: usize,
        nodes: usize,
        length: f64,
        mu: f64,
        tension: f64,
        ea: f64,
        ei: f64,
    ) -> Self {
        assert!(
            nt > 0
                && nl > 0
                && nt <= 256
                && nl <= 512
                && nodes > nl.max(2 * nt)
                && nodes.is_power_of_two()
                && nodes <= 2048
        );
        assert!(
            [length, mu, tension, ea, ei].iter().all(|v| v.is_finite())
                && length > 0.
                && mu > 0.
                && tension > 0.
                && ea > 0.
                && ei >= 0.
        );
        let mass = mu * length / 2.;
        let root = mass.sqrt();
        let ku = (1..=nl)
            .map(|n| std::f64::consts::PI * n as f64 / length / root)
            .collect::<Vec<_>>();
        let kv = (1..=nt)
            .map(|n| std::f64::consts::PI * n as f64 / length / root)
            .collect::<Vec<_>>();
        let axial = ku.iter().map(|k| ea * length / 2. * k * k);
        let transverse = kv
            .iter()
            .enumerate()
            .map(|(j, k)| {
                tension * length / 2. * k * k
                    + ei * length / 2. * (std::f64::consts::PI * (j + 1) as f64 / length).powi(4)
                        / mass
            })
            .collect::<Vec<_>>();
        let omega2 = axial
            .chain(transverse.iter().copied())
            .chain(transverse.iter().copied())
            .collect();
        let cross = (1..=nt)
            .map(|n| {
                mu * length * (if n % 2 == 1 { 1. } else { -1. })
                    / (std::f64::consts::PI * n as f64 * root)
            })
            .collect();
        Self {
            nt,
            nl,
            nodes,
            length,
            ea,
            omega2,
            ku,
            kv,
            cross,
            lift_mass: mu * length / 3.,
            plan: CosinePlan::new(nodes),
            fields: vec![0.; 6 * nodes],
            products: vec![0.; 3 * nodes],
        }
    }
    fn size(&self) -> usize {
        self.nl + 2 * self.nt
    }
    fn fields(&mut self, q: &[f64], bridge: f64, slot: usize) {
        let n = self.nodes;
        let start = slot * 3 * n;
        self.plan
            .synthesize(&q[..self.nl], &self.ku, &mut self.fields[start..start + n]);
        self.plan.synthesize(
            &q[self.nl..self.nl + self.nt],
            &self.kv,
            &mut self.fields[start + n..start + 2 * n],
        );
        self.plan.synthesize(
            &q[self.nl + self.nt..],
            &self.kv,
            &mut self.fields[start + 2 * n..start + 3 * n],
        );
        for x in &mut self.fields[start + n..start + 2 * n] {
            *x += bridge / self.length;
        }
    }
    pub fn nonlinear_potential(&mut self, q: &[f64], bridge: f64) -> f64 {
        self.fields(q, bridge, 0);
        let n = self.nodes;
        let mut sum = 0.;
        for j in 0..n {
            let u = self.fields[j];
            let v = self.fields[n + j];
            let w = self.fields[2 * n + j];
            let eta = 0.5 * (v * v + w * w);
            sum += (u + 0.5 * eta) * eta;
        }
        self.ea * self.length / n as f64 * sum
    }
    fn point_nonlinearity(&mut self, q: &[f64], out: &mut [f64]) -> f64 {
        self.fields(q, 0., 0);
        self.point_from_fields(out, false)
    }
    fn point_from_fields(&mut self, out: &mut [f64], axial_ready: bool) -> f64 {
        let n = self.nodes;
        let mut energy = 0.;
        for j in 0..n {
            let u = self.fields[j];
            let v = self.fields[n + j];
            let w = self.fields[2 * n + j];
            let eta = 0.5 * (v * v + w * w);
            self.products[j] = eta;
            self.products[n + j] = (u + eta) * v;
            self.products[2 * n + j] = (u + eta) * w;
            energy += (u + 0.5 * eta) * eta;
        }
        let factor = self.ea * self.length / (2 * n) as f64;
        if !axial_ready {
            self.plan
                .analyze(&self.products[..n], &self.ku, factor, &mut out[..self.nl]);
        }
        self.plan.analyze(
            &self.products[n..2 * n],
            &self.kv,
            factor,
            &mut out[self.nl..self.nl + self.nt],
        );
        self.plan.analyze(
            &self.products[2 * n..],
            &self.kv,
            factor,
            &mut out[self.nl + self.nt..],
        );
        self.ea * self.length / n as f64 * energy
    }
    /// g_u = EA D_u^T eta(v,w) is independent of u. Evaluate it first,
    /// then compose every axial carrier with this exact endpoint force.
    /// The force is not static: q0/v0 and all fast modal phases are retained.
    fn point_condensed_axial(
        &mut self,
        endpoint: AxialEndpoint<'_>,
        out: &mut [f64],
    ) -> (f64, f64) {
        let n = self.nodes;
        let nl = self.nl;
        let nt = self.nt;
        self.plan.synthesize(
            &endpoint.q[nl..nl + nt],
            &self.kv,
            &mut self.fields[n..2 * n],
        );
        self.plan.synthesize(
            &endpoint.q[nl + nt..],
            &self.kv,
            &mut self.fields[2 * n..3 * n],
        );
        for node in 0..n {
            let v = self.fields[n + node];
            let w = self.fields[2 * n + node];
            self.products[node] = 0.5 * (v * v + w * w);
        }
        let factor = self.ea * self.length / (2 * n) as f64;
        self.plan
            .analyze(&self.products[..n], &self.ku, factor, &mut out[..nl]);
        let mut work_change = 0.;
        for j in 0..nl {
            let force = out[j];
            let slope = (force - endpoint.start_force[j]) / endpoint.frames as f64;
            let base_first = endpoint.start_force[j] + 0.5 * slope;
            if let Some(lambda) = endpoint.scalar_correction {
                let t = endpoint.transfer[j];
                let free = t[0] * endpoint.q0[j]
                    + t[1] * endpoint.v0[j]
                    + t[2] * base_first
                    + t[3] * slope;
                let q = (free - t[2] * lambda * endpoint.q0[j]) / (1. - t[2] * lambda);
                endpoint.correction[j] = lambda * (q - endpoint.q0[j]);
            }
            let first = base_first + endpoint.correction[j];
            let q0 = endpoint.q0[j];
            let v0 = endpoint.v0[j];
            let (q1, v1, work, weighted) = if endpoint.micro_reference {
                // Independent replay of the same force ramp at every tick.
                let mut q = q0;
                let mut v = v0;
                let mut work = 0.;
                let mut weighted = 0.;
                for frame in 0..endpoint.frames {
                    let alpha = (frame as f64 + 0.5) / endpoint.frames as f64;
                    let g = (1. - alpha) * endpoint.start_force[j]
                        + alpha * force
                        + endpoint.correction[j];
                    let next = endpoint.c[j] * q + endpoint.s[j] * v - endpoint.gain[j] * g;
                    v = -self.omega2[j] * endpoint.s[j] * q + endpoint.c[j] * v - endpoint.s[j] * g;
                    work += g * (next - q);
                    weighted += alpha * (next - q);
                    q = next;
                }
                (q, v, work, weighted)
            } else {
                let t = endpoint.transfer[j];
                let q = t[0] * q0 + t[1] * v0 + t[2] * first + t[3] * slope;
                let v = -self.omega2[j] * t[1] * q0 + t[0] * v0 + t[4] * first + t[5] * slope;
                let sum = t[6] * q0 + t[7] * v0 + t[8] * first + t[9] * slope;
                let work =
                    (first + slope * (endpoint.frames - 1) as f64) * q - first * q0 - slope * sum;
                let weighted = (1. - 0.5 / endpoint.frames as f64) * q
                    - 0.5 / endpoint.frames as f64 * q0
                    - sum / endpoint.frames as f64;
                (q, v, work, weighted)
            };
            work_change += work - endpoint.coordinate_work[j];
            endpoint.coordinate_work[j] = work;
            endpoint.weighted_displacement[j] = weighted;
            endpoint.q[j] = q1;
            endpoint.v[j] = v1;
            endpoint.end_force[j] = force;
        }
        self.plan
            .synthesize(&endpoint.q[..nl], &self.ku, &mut self.fields[..n]);
        (self.point_from_fields(out, true), work_change)
    }
    pub fn nonlinear_avf(
        &mut self,
        a: &[f64],
        b: &[f64],
        bridge0: f64,
        bridge1: f64,
        out: &mut [f64],
    ) -> f64 {
        assert_eq!(a.len(), self.size());
        assert_eq!(b.len(), self.size());
        assert_eq!(out.len(), self.size());
        self.fields(a, bridge0, 0);
        self.avf_after_old_fields(b, bridge1, out)
    }
    fn avf_after_old_fields(&mut self, b: &[f64], bridge1: f64, out: &mut [f64]) -> f64 {
        self.fields(b, bridge1, 1);
        let n = self.nodes;
        let product = |x0: f64, x1: f64, y0: f64, y1: f64| {
            (2. * x0 * y0 + x0 * y1 + x1 * y0 + 2. * x1 * y1) / 6.
        };
        let square_product = |x0: f64, x1: f64, y0: f64, y1: f64| {
            (x0 * x0 * (3. * y0 + y1) + 2. * x0 * x1 * (y0 + y1) + x1 * x1 * (y0 + 3. * y1)) / 12.
        };
        let mut boundary = 0.;
        for j in 0..n {
            let (u0, v0, w0, u1, v1, w1) = (
                self.fields[j],
                self.fields[n + j],
                self.fields[2 * n + j],
                self.fields[3 * n + j],
                self.fields[4 * n + j],
                self.fields[5 * n + j],
            );
            let eta = (v0 * v0 + v0 * v1 + v1 * v1 + w0 * w0 + w0 * w1 + w1 * w1) / 6.;
            let pv = product(u0, u1, v0, v1)
                + (v0 + v1) * (v0 * v0 + v1 * v1) / 8.
                + square_product(w0, w1, v0, v1) / 2.;
            let pw = product(u0, u1, w0, w1)
                + (w0 + w1) * (w0 * w0 + w1 * w1) / 8.
                + square_product(v0, v1, w0, w1) / 2.;
            self.products[j] = eta;
            self.products[n + j] = pv;
            self.products[2 * n + j] = pw;
            boundary += pv;
        }
        let factor = self.ea * self.length / (2 * n) as f64;
        self.plan
            .analyze(&self.products[..n], &self.ku, factor, &mut out[..self.nl]);
        self.plan.analyze(
            &self.products[n..2 * n],
            &self.kv,
            factor,
            &mut out[self.nl..self.nl + self.nt],
        );
        self.plan.analyze(
            &self.products[2 * n..],
            &self.kv,
            factor,
            &mut out[self.nl + self.nt..],
        );
        self.ea / n as f64 * boundary
    }
}
#[derive(Clone, Copy, Default)]
struct Contact {
    force: f64,
    jacobian: f64,
    heat: f64,
}
fn felt(a: f64, b: f64, k: f64, p: f64, rate_loss: f64, dt: f64) -> Contact {
    let hi = a.max(b);
    let lo = a.min(b);
    if hi <= 0. {
        return Contact::default();
    }
    let delta = b - a;
    let elastic = if hi == lo {
        k * hi.powf(p)
    } else if lo > 0. {
        let relative = (hi - lo) / hi;
        k * hi.powf(p) * (-((p + 1.) * (-relative).ln_1p()).exp_m1()) / ((p + 1.) * relative)
    } else {
        k / (p + 1.) * hi.powf(p + 1.) / (hi - lo)
    };
    let jac = if hi == lo || (delta.abs() < 1e-5 * hi && lo > 0.) {
        0.5 * k * p * ((a + b) / 2.).powf(p - 1.)
    } else {
        (k * b.max(0.).powf(p) - elastic) / delta
    };
    let argument = rate_loss * delta / dt;
    let exponent = argument.clamp(-40., 40.);
    let factor = exponent.exp();
    Contact {
        force: elastic * factor,
        jacobian: factor
            * (jac
                + if argument > -40. && argument < 40. {
                    elastic * rate_loss / dt
                } else {
                    0.
                }),
        heat: elastic * exponent.exp_m1() * delta,
    }
}
fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
// Contiguous slices let LLVM vectorize carriers; avoid gathering every
// coordinate through an index array inside the implicit microtrajectory.
fn held_linear(
    q: &[f64],
    v: &[f64],
    c: &[f64],
    s: &[f64],
    gain: &[f64],
    force: &[f64],
    out: &mut [f64],
) {
    for ((((((out, q), v), c), s), gain), force) in out
        .iter_mut()
        .zip(q)
        .zip(v)
        .zip(c)
        .zip(s)
        .zip(gain)
        .zip(force)
    {
        *out = *c * *q + *s * *v - *gain * *force;
    }
}
fn advance_carriers(
    q: &mut [f64],
    v: &mut [f64],
    c: &[f64],
    s: &[f64],
    gain: &[f64],
    omega2: &[f64],
    force: &[f64],
) {
    for ((((((q, v), c), s), gain), omega2), force) in q
        .iter_mut()
        .zip(v)
        .zip(c)
        .zip(s)
        .zip(gain)
        .zip(omega2)
        .zip(force)
    {
        let next_q = *c * *q + *s * *v - *gain * *force;
        *v = -*omega2 * *s * *q + *c * *v - *s * *force;
        *q = next_q;
    }
}
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn dot_avx2(a: &[f64], b: &[f64]) -> f64 {
    use std::arch::x86_64::*;
    unsafe {
        let mut sum = _mm256_setzero_pd();
        let end = a.len() / 4 * 4;
        for j in (0..end).step_by(4) {
            sum = _mm256_add_pd(
                sum,
                _mm256_mul_pd(
                    _mm256_loadu_pd(a.as_ptr().add(j)),
                    _mm256_loadu_pd(b.as_ptr().add(j)),
                ),
            );
        }
        let mut lanes = [0.; 4];
        _mm256_storeu_pd(lanes.as_mut_ptr(), sum);
        let mut result = lanes.iter().sum::<f64>();
        for j in end..a.len() {
            result += a[j] * b[j];
        }
        result
    }
}
#[cfg(target_arch = "x86_64")]
fn dot_simd(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len());
    unsafe { dot_avx2(a, b) }
}
fn dot_dispatch() -> fn(&[f64], &[f64]) -> f64 {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        return dot_simd;
    }
    dot
}
fn norm(a: &[f64]) -> f64 {
    dot(a, a).sqrt()
}
#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub heat: f64,
    pub force: f64,
    pub iterations: usize,
}
fn anderson_update(
    iteration: usize,
    x: &mut [f64],
    fx: &[f64],
    history_x: &mut [f64],
    history_r: &mut [f64],
    delta_r: &mut [f64],
    delta_f: &mut [f64],
    gram_dot: fn(&[f64], &[f64]) -> f64,
) {
    let n = x.len();
    let slot = iteration % 4;
    history_x[slot * n..(slot + 1) * n].copy_from_slice(x);
    for j in 0..n {
        history_r[slot * n + j] = fx[j] - x[j];
    }
    let depth = iteration.min(3);
    let mut coefficients = [0.; 3];
    let mut matrix = [[0.; 3]; 3];
    if depth > 0 {
        for i in 0..depth {
            let before = (iteration - depth + i) % 4;
            let after = (iteration - depth + i + 1) % 4;
            let dr = &mut delta_r[i * n..(i + 1) * n];
            let df = &mut delta_f[i * n..(i + 1) * n];
            for (((((dr, df), xa), xb), ra), rb) in dr
                .iter_mut()
                .zip(df)
                .zip(&history_x[after * n..(after + 1) * n])
                .zip(&history_x[before * n..(before + 1) * n])
                .zip(&history_r[after * n..(after + 1) * n])
                .zip(&history_r[before * n..(before + 1) * n])
            {
                *dr = *ra - *rb;
                *df = *xa - *xb + *ra - *rb;
            }
            coefficients[i] = (gram_dot)(
                &delta_r[i * n..(i + 1) * n],
                &history_r[slot * n..(slot + 1) * n],
            );
        }
        for i in 0..depth {
            for j in i..depth {
                let value = (gram_dot)(&delta_r[i * n..(i + 1) * n], &delta_r[j * n..(j + 1) * n]);
                matrix[i][j] = value;
                matrix[j][i] = value;
            }
        }
        let ridge = (0..depth).map(|i| matrix[i][i]).fold(0f64, f64::max) * 1e-10;
        let mut valid = ridge > 0. && ridge.is_finite();
        for i in 0..depth {
            matrix[i][i] += ridge;
        }
        for column in 0..depth {
            let pivot = (column..depth)
                .max_by(|i, j| {
                    matrix[*i][column]
                        .abs()
                        .total_cmp(&matrix[*j][column].abs())
                })
                .unwrap();
            matrix.swap(column, pivot);
            coefficients.swap(column, pivot);
            if !matrix[column][column].is_finite() || matrix[column][column].abs() < 1e-300 {
                valid = false;
                break;
            }
            for row in column + 1..depth {
                let fraction = matrix[row][column] / matrix[column][column];
                for j in column..depth {
                    matrix[row][j] -= fraction * matrix[column][j];
                }
                coefficients[row] -= fraction * coefficients[column];
            }
        }
        if valid {
            for i in (0..depth).rev() {
                coefficients[i] = (coefficients[i]
                    - (i + 1..depth)
                        .map(|j| matrix[i][j] * coefficients[j])
                        .sum::<f64>())
                    / matrix[i][i];
            }
            valid = coefficients[..depth].iter().all(|v| v.is_finite())
                && coefficients[..depth].iter().map(|v| v.abs()).sum::<f64>() <= 8.;
        }
        if !valid {
            coefficients.fill(0.);
        }
    }
    x.copy_from_slice(fx);
    for i in 0..depth {
        for (value, difference) in x.iter_mut().zip(&delta_f[i * n..(i + 1) * n]) {
            *value -= coefficients[i] * difference;
        }
    }
}
pub struct Choir {
    wires: Vec<Wire>,
    width: usize,
    size: usize,
    dt: f64,
    mass: f64,
    stiffness: f64,
    exponent: f64,
    rate_loss: f64,
    shapes: Vec<f64>,
    q: Vec<f64>,
    v: Vec<f64>,
    c: Vec<f64>,
    s: Vec<f64>,
    gain: Vec<f64>,
    omega2: Vec<f64>,
    linear: Vec<f64>,
    candidate: Vec<f64>,
    gradient: Vec<f64>,
    trial_gradient: Vec<f64>,
    trial: Vec<f64>,
    residual: Vec<f64>,
    update: Vec<f64>,
    block_start_q: Vec<f64>,
    block_start_v: Vec<f64>,
    block_force: Vec<f64>,
    block_next_force: Vec<f64>,
    block_trace: Vec<f64>,
    contact_mobility: [[f64; 3]; 3],
    last_outer_iterations: usize,
    ramp_start_force: Vec<f64>,
    ramp_end_force: Vec<f64>,
    ramp_correction: Vec<f64>,
    ramp_next_correction: Vec<f64>,
    ramp_weighted_displacement: Vec<f64>,
    anderson_x: Vec<f64>,
    anderson_r: Vec<f64>,
    free_indices: Vec<usize>,
    ramp_transfer: Vec<[f64; 10]>,
    flight_bounds: Vec<[f64; 4]>,
    ramp_micro_reference: bool,
    last_certified_flight: bool,
    anderson_delta_r: Vec<f64>,
    anderson_delta_f: Vec<f64>,
    gram_dot: fn(&[f64], &[f64]) -> f64,
    endpoint_force: Vec<f64>,
    endpoint_potential: f64,
    endpoint_valid: bool,
    use_endpoint_cache: bool,
    cached_initial_gradients: usize,
    ramp_coordinate_work: Vec<f64>,
    use_condensed_axial: bool,
    canonical_correction: bool,
    solver_x: Vec<f64>,
    solver_next: Vec<f64>,
    transverse_indices: Vec<usize>,
    use_scalar_flight: bool,
    scalar_flight_blocks: usize,
    scalar_flight_fallbacks: usize,
}
impl Choir {
    fn new(
        wires: Vec<Wire>,
        dt: f64,
        mass: f64,
        stiffness: f64,
        exponent: f64,
        rate_loss: f64,
        shapes: Vec<f64>,
        q: Vec<f64>,
        v: Vec<f64>,
    ) -> Self {
        assert_eq!(wires.len(), 3);
        let width = wires[0].size();
        let size = 3 * width + 1;
        assert!(wires
            .iter()
            .all(|w| w.size() == width && w.nt == wires[0].nt));
        assert!(
            dt.is_finite()
                && dt > 0.
                && mass > 0.
                && stiffness > 0.
                && exponent >= 1.
                && rate_loss >= 0.
        );
        assert_eq!(q.len(), size);
        assert_eq!(v.len(), size);
        assert_eq!(shapes.len(), 3 * wires[0].nt);
        let mut omega2 = wires
            .iter()
            .flat_map(|w| w.omega2.iter().copied())
            .collect::<Vec<_>>();
        omega2.push(0.);
        let c: Vec<f64> = omega2.iter().map(|w| (w.sqrt() * dt).cos()).collect();
        let s: Vec<f64> = omega2
            .iter()
            .map(|w| {
                if *w == 0. {
                    dt
                } else {
                    (w.sqrt() * dt).sin() / w.sqrt()
                }
            })
            .collect();
        let gain: Vec<f64> = omega2
            .iter()
            .map(|w| {
                if *w == 0. {
                    dt * dt / 2.
                } else {
                    2. * (w.sqrt() * dt / 2.).sin().powi(2) / w
                }
            })
            .collect();
        let mut contact_mobility = [[gain[size - 1] / mass; 3]; 3];
        for i in 0..3 {
            let nt = wires[i].nt;
            let start = i * width + wires[i].nl;
            contact_mobility[i][i] += (0..nt)
                .map(|j| gain[start + j] * shapes[i * nt + j].powi(2))
                .sum::<f64>();
        }
        let contact_indices = (0..size)
            .filter(|j| {
                *j == size - 1
                    || (*j % width >= wires[0].nl && *j % width < wires[0].nl + wires[0].nt)
            })
            .collect::<Vec<_>>();
        let free_indices = (0..size - 1)
            .filter(|j| !contact_indices.contains(j))
            .collect::<Vec<_>>();
        // Exact composition of the original microstep map. No division by
        // modal resonance or sin(theta): near-aliasing modes remain defined.
        // Entries: C,S,qa,qb,va,vb, sumC,sumS,sumQa,sumQb. Sums exclude
        // the endpoint, as required by discrete summation by parts.
        let mut ramp_transfer = vec![[0.; 10]; 65 * size];
        let nt = wires[0].nt;
        let mut flight_bounds = vec![[0.; 4]; 65 * 3 * nt];
        for j in 0..size - 1 {
            let (mut cj, mut sj, mut qa, mut qb, mut va, mut vb) = (1., 0., 0., 0., 0., 0.);
            let (mut sc, mut ss, mut sa, mut sb) = (0., 0., 0., 0.);
            let mut bound: [f64; 4] = [1., 0., 0., 0.];
            for frame in 0..64 {
                let nq_a = c[j] * qa + s[j] * va - gain[j];
                let nq_b = c[j] * qb + s[j] * vb - gain[j] * frame as f64;
                va = -omega2[j] * s[j] * qa + c[j] * va - s[j];
                vb = -omega2[j] * s[j] * qb + c[j] * vb - s[j] * frame as f64;
                qa = nq_a;
                qb = nq_b;
                let nc = c[j] * cj - omega2[j] * s[j] * sj;
                sj = s[j] * cj + c[j] * sj;
                cj = nc;
                ramp_transfer[(frame + 1) * size + j] = [cj, sj, qa, qb, va, vb, sc, ss, sa, sb];
                let mode = j % width;
                if mode >= wires[0].nl && mode < wires[0].nl + nt {
                    for (maximum, value) in bound.iter_mut().zip([cj, sj, qa, qb]) {
                        *maximum = maximum.max(value.abs());
                    }
                    flight_bounds[((frame + 1) * 3 + j / width) * nt + mode - wires[0].nl] = bound;
                }

                sc += cj;
                ss += sj;
                sa += qa;
                sb += qb;
            }
        }
        let transverse_indices = (0..3)
            .flat_map(|i| (0..2 * nt).map(move |j| i * width + (width - 2 * nt) + j))
            .collect();
        Self {
            wires,
            width,
            size,
            dt,
            mass,
            stiffness,
            exponent,
            rate_loss,
            shapes,
            q,
            v,
            c,
            s,
            gain,
            omega2,
            linear: vec![0.; size],
            candidate: vec![0.; size],
            gradient: vec![0.; size],
            trial_gradient: vec![0.; size],
            trial: vec![0.; size],
            residual: vec![0.; size],
            update: vec![0.; size],
            block_start_q: vec![0.; size],
            block_start_v: vec![0.; size],
            block_force: vec![0.; size],
            block_next_force: vec![0.; size],
            block_trace: vec![0.; 64],
            contact_mobility,
            last_outer_iterations: 0,
            ramp_start_force: vec![0.; size],
            ramp_end_force: vec![0.; size],
            ramp_correction: vec![0.; size],
            ramp_next_correction: vec![0.; size],
            ramp_weighted_displacement: vec![0.; size],
            anderson_x: vec![0.; 8 * size],
            anderson_r: vec![0.; 8 * size],
            free_indices,
            ramp_transfer,
            flight_bounds,
            ramp_micro_reference: false,
            last_certified_flight: false,
            anderson_delta_r: vec![0.; 6 * size],
            anderson_delta_f: vec![0.; 6 * size],
            gram_dot: dot_dispatch(),
            endpoint_force: vec![0.; size],
            endpoint_potential: 0.,
            endpoint_valid: false,
            use_endpoint_cache: true,
            cached_initial_gradients: 0,
            ramp_coordinate_work: vec![0.; size],
            use_condensed_axial: true,
            canonical_correction: true,
            solver_x: vec![0.; 2 * size],
            solver_next: vec![0.; 2 * size],
            transverse_indices,
            use_scalar_flight: true,
            scalar_flight_blocks: 0,
            scalar_flight_fallbacks: 0,
        }
    }
    fn compression(&self, q: &[f64], wire: usize) -> f64 {
        let nt = self.wires[wire].nt;
        let start = wire * self.width + self.wires[wire].nl;
        q[self.size - 1] / self.mass.sqrt()
            + dot(
                &self.shapes[wire * nt..(wire + 1) * nt],
                &q[start..start + nt],
            )
    }
    fn direction_dot(&self, x: &[f64], wire: usize) -> f64 {
        self.compression(x, wire)
    }
    pub fn energy(&mut self) -> f64 {
        let mut energy = 0.5 * dot(&self.v, &self.v)
            + 0.5
                * self
                    .q
                    .iter()
                    .zip(&self.omega2)
                    .map(|(q, w)| q * q * w)
                    .sum::<f64>();
        for i in 0..3 {
            let compression = self.compression(&self.q, i).max(0.);
            energy += self.wires[i]
                .nonlinear_potential(&self.q[i * self.width..(i + 1) * self.width], 0.);
            energy += self.stiffness / (self.exponent + 1.) * compression.powf(self.exponent + 1.);
        }
        energy
    }
    pub fn launch(&mut self, speed: f64) -> Result<f64, &'static str> {
        if !speed.is_finite() || speed < 0. {
            return Err("invalid hammer speed");
        }
        if (0..3).any(|i| self.compression(&self.q, i) > 1e-12) {
            return Err("hammer must be separated before relaunch");
        }
        let before = self.energy();
        let head = self.size - 1;
        let mut position = f64::INFINITY;
        for i in 0..3 {
            let nt = self.wires[i].nt;
            let start = i * self.width + self.wires[i].nl;
            position = position.min(
                -dot(
                    &self.shapes[i * nt..(i + 1) * nt],
                    &self.q[start..start + nt],
                ) * self.mass.sqrt(),
            );
        }
        self.q[head] = position;
        self.v[head] = self.mass.sqrt() * speed;
        Ok(self.energy() - before)
    }
    fn remainder(&mut self, trial: bool) -> ([f64; 3], Stats) {
        let candidate = if trial { &self.trial } else { &self.candidate };
        let mut contacts = [Contact::default(); 3];
        for (i, contact) in contacts.iter_mut().enumerate() {
            *contact = felt(
                self.compression(&self.q, i),
                self.compression(candidate, i),
                self.stiffness,
                self.exponent,
                self.rate_loss,
                self.dt,
            );
        }
        let gradient = if trial {
            &mut self.trial_gradient
        } else {
            &mut self.gradient
        };
        gradient.fill(0.);
        for i in 0..3 {
            self.wires[i].avf_after_old_fields(
                &candidate[i * self.width..(i + 1) * self.width],
                0.,
                &mut gradient[i * self.width..(i + 1) * self.width],
            );
            let nt = self.wires[i].nt;
            let start = i * self.width + self.wires[i].nl;
            for j in 0..nt {
                gradient[start + j] += self.shapes[i * nt + j] * contacts[i].force;
            }
            gradient[self.size - 1] += contacts[i].force / self.mass.sqrt();
        }
        (
            [
                contacts[0].jacobian,
                contacts[1].jacobian,
                contacts[2].jacobian,
            ],
            Stats {
                heat: contacts.iter().map(|c| c.heat).sum(),
                force: contacts.iter().map(|c| c.force).sum(),
                iterations: 0,
            },
        )
    }
    /// Three-dimensional felt solve with a constant nonlinear string force.
    /// Every accepted microstep retains the original felt divided work/loss.
    fn contact_with_held_nonlinearity(
        &mut self,
        contact_only: bool,
    ) -> Result<Stats, &'static str> {
        if contact_only {
            for i in 0..3 {
                let start = i * self.width + self.wires[i].nl;
                let end = start + self.wires[i].nt;
                held_linear(
                    &self.q[start..end],
                    &self.v[start..end],
                    &self.c[start..end],
                    &self.s[start..end],
                    &self.gain[start..end],
                    &self.block_force[start..end],
                    &mut self.linear[start..end],
                );
            }
            let j = self.size - 1;
            self.linear[j] =
                self.c[j] * self.q[j] + self.s[j] * self.v[j] - self.gain[j] * self.block_force[j];
        } else {
            held_linear(
                &self.q,
                &self.v,
                &self.c,
                &self.s,
                &self.gain,
                &self.block_force,
                &mut self.linear,
            );
        }
        let old = [
            self.compression(&self.q, 0),
            self.compression(&self.q, 1),
            self.compression(&self.q, 2),
        ];
        let free = [
            self.compression(&self.linear, 0),
            self.compression(&self.linear, 1),
            self.compression(&self.linear, 2),
        ];
        let mut forces = [0.; 3];
        let mut cached: Option<[Contact; 3]> = None;
        for iteration in 0..24 {
            let contacts = if let Some(contacts) = cached.take() {
                contacts
            } else {
                let mut contacts = [Contact::default(); 3];
                for i in 0..3 {
                    let next = free[i] - dot(&self.contact_mobility[i], &forces);
                    contacts[i] = felt(
                        old[i],
                        next,
                        self.stiffness,
                        self.exponent,
                        self.rate_loss,
                        self.dt,
                    );
                }
                contacts
            };
            let residual = [
                forces[0] - contacts[0].force,
                forces[1] - contacts[1].force,
                forces[2] - contacts[2].force,
            ];
            let norm_residual = norm(&residual);
            let scale = norm(&forces)
                .max(
                    contacts
                        .iter()
                        .map(|c| c.force * c.force)
                        .sum::<f64>()
                        .sqrt(),
                )
                .max(1e-12);
            if !norm_residual.is_finite() {
                return Err("nonfinite reduced felt residual");
            }
            if norm_residual <= 2e-13 * scale {
                if !contact_only {
                    self.gradient.copy_from_slice(&self.block_force);
                }
                for i in 0..3 {
                    let nt = self.wires[i].nt;
                    let start = i * self.width + self.wires[i].nl;
                    let end = start + nt;
                    for ((g, f), shape) in self.gradient[start..end]
                        .iter_mut()
                        .zip(&self.block_force[start..end])
                        .zip(&self.shapes[i * nt..(i + 1) * nt])
                    {
                        *g = *f + *shape * forces[i];
                    }
                    if contact_only {
                        advance_carriers(
                            &mut self.q[start..end],
                            &mut self.v[start..end],
                            &self.c[start..end],
                            &self.s[start..end],
                            &self.gain[start..end],
                            &self.omega2[start..end],
                            &self.gradient[start..end],
                        );
                    }
                }
                let head = self.size - 1;
                self.gradient[head] = self.block_force[head];
                for force in forces {
                    self.gradient[head] += force / self.mass.sqrt();
                }
                if contact_only {
                    let j = head;
                    let q = self.c[j] * self.q[j] + self.s[j] * self.v[j]
                        - self.gain[j] * self.gradient[j];
                    self.v[j] = -self.omega2[j] * self.s[j] * self.q[j] + self.c[j] * self.v[j]
                        - self.s[j] * self.gradient[j];
                    self.q[j] = q;
                } else {
                    advance_carriers(
                        &mut self.q,
                        &mut self.v,
                        &self.c,
                        &self.s,
                        &self.gain,
                        &self.omega2,
                        &self.gradient,
                    );
                }
                return Ok(Stats {
                    heat: contacts.iter().map(|c| c.heat).sum(),
                    force: forces.iter().sum(),
                    iterations: iteration + 1,
                });
            }
            let mut matrix = [[0.; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    matrix[i][j] = contacts[i].jacobian * self.contact_mobility[i][j]
                        + if i == j { 1. } else { 0. };
                }
            }
            let mut update = residual;
            for column in 0..3 {
                let pivot = (column..3)
                    .max_by(|i, j| {
                        matrix[*i][column]
                            .abs()
                            .total_cmp(&matrix[*j][column].abs())
                    })
                    .unwrap();
                matrix.swap(column, pivot);
                update.swap(column, pivot);
                if matrix[column][column].abs() < 1e-30 {
                    return Err("singular reduced felt tangent");
                }
                for row in column + 1..3 {
                    let fraction = matrix[row][column] / matrix[column][column];
                    for j in column..3 {
                        matrix[row][j] -= fraction * matrix[column][j];
                    }
                    update[row] -= fraction * update[column];
                }
            }
            for i in (0..3).rev() {
                update[i] = (update[i] - (i + 1..3).map(|j| matrix[i][j] * update[j]).sum::<f64>())
                    / matrix[i][i];
            }
            let mut accepted = false;
            for power in 0..14 {
                let fraction = 2f64.powi(-power);
                let trial = [
                    forces[0] - fraction * update[0],
                    forces[1] - fraction * update[1],
                    forces[2] - fraction * update[2],
                ];
                let mut trial_residual = [0.; 3];
                let mut trial_contacts = [Contact::default(); 3];
                for i in 0..3 {
                    let next = free[i] - dot(&self.contact_mobility[i], &trial);
                    trial_contacts[i] = felt(
                        old[i],
                        next,
                        self.stiffness,
                        self.exponent,
                        self.rate_loss,
                        self.dt,
                    );
                    trial_residual[i] = trial[i] - trial_contacts[i].force;
                }
                if norm(&trial_residual) < norm_residual {
                    forces = trial;
                    cached = Some(trial_contacts);
                    accepted = true;
                    break;
                }
            }
            if !accepted {
                return Err("reduced felt line search failed");
            }
        }
        Err("reduced felt solve did not converge")
    }
    /// Hold only the nonlinear string gradient over a short implicit block.
    /// Linear carriers and felt remain at the microclock. Global string work
    /// closes at block boundaries; events must terminate a block first.
    pub fn step_block(&mut self, frames: usize) -> Result<Stats, &'static str> {
        self.endpoint_valid = false;
        if frames == 0 || frames > 64 {
            return Err("block must contain 1..64 microsteps");
        }
        self.block_start_q.copy_from_slice(&self.q);
        self.block_start_v.copy_from_slice(&self.v);
        let energy = self.energy().max(1e-30);
        self.block_force.fill(0.);
        for i in 0..3 {
            self.wires[i].avf_after_old_fields(
                &self.block_start_q[i * self.width..(i + 1) * self.width],
                0.,
                &mut self.block_force[i * self.width..(i + 1) * self.width],
            );
        }
        for outer in 0..32 {
            self.q.copy_from_slice(&self.block_start_q);
            self.v.copy_from_slice(&self.block_start_v);
            let mut stats = Stats::default();
            for frame in 0..frames {
                let micro = match self.contact_with_held_nonlinearity(false) {
                    Ok(v) => v,
                    Err(error) => {
                        self.q.copy_from_slice(&self.block_start_q);
                        self.v.copy_from_slice(&self.block_start_v);
                        return Err(error);
                    }
                };
                stats.heat += micro.heat;
                stats.force = stats.force.max(micro.force);
                stats.iterations = stats.iterations.max(micro.iterations);
                self.block_trace[frame] = micro.force;
            }
            self.block_next_force.fill(0.);
            for i in 0..3 {
                self.wires[i].avf_after_old_fields(
                    &self.q[i * self.width..(i + 1) * self.width],
                    0.,
                    &mut self.block_next_force[i * self.width..(i + 1) * self.width],
                );
            }
            let mut difference = 0.;
            let mut work_residual = 0.;
            for j in 0..self.size {
                let delta = self.block_next_force[j] - self.block_force[j];
                difference += delta * delta;
                work_residual += delta * (self.q[j] - self.block_start_q[j]);
            }
            let force_scale = norm(&self.block_next_force)
                .max(norm(&self.block_force))
                .max(1e-30);
            if difference.sqrt() <= 1e-11 * force_scale && work_residual.abs() <= 1e-13 * energy {
                self.last_outer_iterations = outer + 1;
                return Ok(stats);
            }
            self.block_force.copy_from_slice(&self.block_next_force);
        }
        self.q.copy_from_slice(&self.block_start_q);
        self.v.copy_from_slice(&self.block_start_v);
        Err("nonlinear block failed to converge; shorten block")
    }
    /// Certify zero contact over *every* microstep of a proposed ramp.
    /// Triangle bounds enclose each actual prepared carrier coefficient at
    /// all ticks, including the old state. A head moving toward the strings
    /// therefore cannot be skipped merely because it starts separated.
    fn certified_ramp_flight(&self, frames: usize) -> bool {
        let head = self.size - 1;
        let head0 = self.block_start_q[head] / self.mass.sqrt();
        let travel = frames as f64 * self.dt * self.block_start_v[head] / self.mass.sqrt();
        let head_max = head0.max(head0 + travel);
        if !head_max.is_finite() {
            return false;
        }
        for i in 0..3 {
            let nt = self.wires[i].nt;
            let start = i * self.width + self.wires[i].nl;
            let mut bound = 0.;
            for mode in 0..nt {
                let j = start + mode;
                let slope = (self.ramp_end_force[j] - self.ramp_start_force[j]) / frames as f64;
                let first = self.ramp_start_force[j] + 0.5 * slope + self.ramp_correction[j];
                let b = self.flight_bounds[(frames * 3 + i) * nt + mode];
                bound += self.shapes[i * nt + mode].abs()
                    * (b[0] * self.block_start_q[j].abs()
                        + b[1] * self.block_start_v[j].abs()
                        + b[2] * first.abs()
                        + b[3] * slope.abs());
            }
            // Include accumulated floating-point composition error. Values
            // too close to first touch take the original contact path.
            let margin = 256. * f64::EPSILON * (head0.abs() + travel.abs() + bound) + 1e-15;
            if !bound.is_finite() || head_max + bound >= -margin {
                return false;
            }
        }
        true
    }
    /// Solve the determined force-free block in transverse endpoint forces
    /// and one signed work coefficient. All axial q/v are still propagated.
    /// A failed contact certificate or unsafe iterate returns to the full
    /// determined solver with the initial physical state, never a shortened
    /// or partially accepted trajectory.
    fn try_scalar_flight(&mut self, frames: usize, potential0: f64, energy: f64) -> Option<Stats> {
        let head = self.size - 1;
        let h = frames as f64 * self.dt;
        let head0 = self.block_start_q[head] / self.mass.sqrt();
        let travel = h * self.block_start_v[head] / self.mass.sqrt();
        if !head0.max(head0 + travel).is_finite() || head0.max(head0 + travel) >= -1e-15 {
            return None;
        }
        let mut displacement_scale2 = 0.;
        for j in 0..head {
            let t = self.ramp_transfer[frames * self.size + j];
            let q = t[0] * self.block_start_q[j]
                + t[1] * self.block_start_v[j]
                + t[2] * self.ramp_start_force[j];
            displacement_scale2 += (q - self.block_start_q[j]).powi(2);
        }
        let scale = displacement_scale2
            .sqrt()
            .max(h * self.block_start_v[head].abs())
            .max(1e-12);
        let n = self.transverse_indices.len() + 1;
        for (row, &j) in self.transverse_indices.iter().enumerate() {
            self.solver_x[row] = self.ramp_start_force[j];
        }
        self.solver_x[n - 1] = 0.;
        for outer in 0..32 {
            let gamma = self.solver_x[n - 1];
            let lambda = gamma / scale;
            if !lambda.is_finite() {
                return None;
            }
            for j in 0..head {
                let denominator = 1. - self.ramp_transfer[frames * self.size + j][2] * lambda;
                if !denominator.is_finite() || denominator <= 1e-6 {
                    return None;
                }
            }
            self.q.copy_from_slice(&self.block_start_q);
            self.v.copy_from_slice(&self.block_start_v);
            self.ramp_end_force.copy_from_slice(&self.ramp_start_force);
            self.ramp_correction.fill(0.);
            self.ramp_coordinate_work.fill(0.);
            self.ramp_weighted_displacement.fill(0.);
            let mut work = 0.;
            for (row, &j) in self.transverse_indices.iter().enumerate() {
                let force = self.solver_x[row];
                self.ramp_end_force[j] = force;
                let t = self.ramp_transfer[frames * self.size + j];
                let q0 = self.block_start_q[j];
                let v0 = self.block_start_v[j];
                let slope = (force - self.ramp_start_force[j]) / frames as f64;
                let base = self.ramp_start_force[j] + 0.5 * slope;
                let free = t[0] * q0 + t[1] * v0 + t[2] * base + t[3] * slope;
                let projected = (free - t[2] * lambda * q0) / (1. - t[2] * lambda);
                let correction = lambda * (projected - q0);
                self.ramp_correction[j] = correction;
                let first = base + correction;
                let q = t[0] * q0 + t[1] * v0 + t[2] * first + t[3] * slope;
                let v = -self.omega2[j] * t[1] * q0 + t[0] * v0 + t[4] * first + t[5] * slope;
                let sum = t[6] * q0 + t[7] * v0 + t[8] * first + t[9] * slope;
                let local_work =
                    (first + slope * (frames - 1) as f64) * q - first * q0 - slope * sum;
                self.q[j] = q;
                self.v[j] = v;
                self.ramp_coordinate_work[j] = local_work;
                work += local_work;
                self.ramp_weighted_displacement[j] =
                    (1. - 0.5 / frames as f64) * q - 0.5 / frames as f64 * q0 - sum / frames as f64;
            }
            if !self.certified_ramp_flight(frames) {
                return None;
            }
            self.q[head] = self.block_start_q[head] + h * self.block_start_v[head];
            self.block_next_force.fill(0.);
            let mut potential1 = 0.;
            for i in 0..3 {
                let start = i * self.width;
                let end = (i + 1) * self.width;
                let endpoint = AxialEndpoint {
                    frames,
                    micro_reference: false,
                    q: &mut self.q[start..end],
                    v: &mut self.v[start..end],
                    q0: &self.block_start_q[start..end],
                    v0: &self.block_start_v[start..end],
                    start_force: &self.ramp_start_force[start..end],
                    end_force: &mut self.ramp_end_force[start..end],
                    correction: &mut self.ramp_correction[start..end],
                    scalar_correction: Some(lambda),
                    weighted_displacement: &mut self.ramp_weighted_displacement[start..end],
                    coordinate_work: &mut self.ramp_coordinate_work[start..end],
                    transfer: &self.ramp_transfer
                        [frames * self.size + start..frames * self.size + end],
                    c: &self.c[start..end],
                    s: &self.s[start..end],
                    gain: &self.gain[start..end],
                };
                let (potential, delta_work) = self.wires[i]
                    .point_condensed_axial(endpoint, &mut self.block_next_force[start..end]);
                potential1 += potential;
                work += delta_work;
            }
            let residual = potential1 - potential0 - work;
            let mut shifted = residual;
            let mut force_difference = 0.;
            let mut displacement2 = 0.;
            for (row, &j) in self.transverse_indices.iter().enumerate() {
                let delta = self.block_next_force[j] - self.ramp_end_force[j];
                force_difference += delta * delta;
                shifted -= delta * self.ramp_weighted_displacement[j];
                self.solver_next[row] = self.block_next_force[j];
            }
            for j in 0..head {
                displacement2 += (self.q[j] - self.block_start_q[j]).powi(2);
            }
            let next_gamma = if displacement2 > 1e-40 {
                gamma + shifted * scale / displacement2
            } else if shifted.abs() <= 1e-13 * energy.max(1e-30) {
                0.
            } else {
                return None;
            };
            let correction_difference = (next_gamma - gamma).abs() * displacement2.sqrt() / scale;
            let force_scale = norm(&self.block_next_force)
                .max(norm(&self.ramp_end_force))
                .max(norm(&self.ramp_correction))
                .max(1e-30);
            if !residual.is_finite() || !next_gamma.is_finite() || !force_difference.is_finite() {
                return None;
            }
            if force_difference.sqrt() <= 1e-11 * force_scale
                && correction_difference <= 1e-11 * force_scale
                && residual.abs() <= 1e-13 * energy.max(1e-30)
            {
                self.last_outer_iterations = outer + 1;
                self.last_certified_flight = true;
                self.endpoint_force.copy_from_slice(&self.block_next_force);
                self.endpoint_potential = potential1;
                self.endpoint_valid = true;
                self.block_trace[..frames].fill(0.);
                self.scalar_flight_blocks += 1;
                return Some(Stats {
                    heat: 0.,
                    force: 0.,
                    iterations: 1,
                });
            }
            self.solver_next[n - 1] = next_gamma;
            anderson_update(
                outer,
                &mut self.solver_x[..n],
                &self.solver_next[..n],
                &mut self.anderson_x,
                &mut self.anderson_r,
                &mut self.anderson_delta_r,
                &mut self.anderson_delta_f,
                self.gram_dot,
            );
        }
        None
    }
    /// Endpoint force interpolation plus an explicit discrete-work correction.
    /// The correction acts only on string coordinates, never on the hammer.
    /// It is solved jointly with the endpoint trajectory, not applied after it.
    pub fn step_ramp(&mut self, frames: usize) -> Result<Stats, &'static str> {
        if frames == 0 || frames > 64 {
            return Err("ramp block must contain 1..64 microsteps");
        }
        self.block_start_q.copy_from_slice(&self.q);
        self.block_start_v.copy_from_slice(&self.v);
        let potential0 = if self.use_endpoint_cache && self.endpoint_valid {
            self.ramp_start_force.copy_from_slice(&self.endpoint_force);
            self.cached_initial_gradients += 1;
            self.endpoint_potential
        } else {
            self.ramp_start_force.fill(0.);
            let mut potential = 0.;
            for i in 0..3 {
                potential += self.wires[i].point_nonlinearity(
                    &self.q[i * self.width..(i + 1) * self.width],
                    &mut self.ramp_start_force[i * self.width..(i + 1) * self.width],
                );
            }
            potential
        };
        let mut energy = 0.5 * dot(&self.v, &self.v)
            + 0.5
                * self
                    .q
                    .iter()
                    .zip(&self.omega2)
                    .map(|(q, w)| q * q * w)
                    .sum::<f64>()
            + potential0;
        for i in 0..3 {
            energy += self.stiffness / (self.exponent + 1.)
                * self
                    .compression(&self.q, i)
                    .max(0.)
                    .powf(self.exponent + 1.);
        }
        if self.use_scalar_flight
            && self.use_condensed_axial
            && self.canonical_correction
            && !self.ramp_micro_reference
        {
            if let Some(stats) = self.try_scalar_flight(frames, potential0, energy) {
                return Ok(stats);
            }
            self.scalar_flight_fallbacks += 1;
            self.q.copy_from_slice(&self.block_start_q);
            self.v.copy_from_slice(&self.block_start_v);
        }
        self.ramp_end_force.copy_from_slice(&self.ramp_start_force);
        self.ramp_correction.fill(0.);
        for outer in 0..32 {
            self.q.copy_from_slice(&self.block_start_q);
            self.v.copy_from_slice(&self.block_start_v);
            self.ramp_weighted_displacement.fill(0.);
            self.ramp_coordinate_work.fill(0.);
            let mut work = 0.;
            let mut stats = Stats::default();
            let certified_flight = !self.ramp_micro_reference && self.certified_ramp_flight(frames);
            if certified_flight {
                let head = self.size - 1;
                self.q[head] =
                    self.block_start_q[head] + frames as f64 * self.dt * self.block_start_v[head];
                self.block_trace[..frames].fill(0.);
                stats.iterations = 1;
            } else {
                for frame in 0..frames {
                    let alpha = (frame as f64 + 0.5) / frames as f64;
                    for i in 0..3 {
                        let start = if self.ramp_micro_reference {
                            i * self.width
                        } else {
                            i * self.width + self.wires[i].nl
                        };
                        let end = if self.ramp_micro_reference {
                            (i + 1) * self.width
                        } else {
                            start + self.wires[i].nt
                        };
                        for (((f, g0), g1), correction) in self.block_force[start..end]
                            .iter_mut()
                            .zip(&self.ramp_start_force[start..end])
                            .zip(&self.ramp_end_force[start..end])
                            .zip(&self.ramp_correction[start..end])
                        {
                            *f = (1. - alpha) * *g0 + alpha * *g1 + *correction;
                        }
                        self.candidate[start..end].copy_from_slice(&self.q[start..end]);
                    }
                    let head = self.size - 1;
                    self.block_force[head] = 0.;
                    self.candidate[head] = self.q[head];
                    let micro =
                        match self.contact_with_held_nonlinearity(!self.ramp_micro_reference) {
                            Ok(v) => v,
                            Err(error) => {
                                self.q.copy_from_slice(&self.block_start_q);
                                self.v.copy_from_slice(&self.block_start_v);
                                return Err(error);
                            }
                        };
                    for i in 0..3 {
                        let start = if self.ramp_micro_reference {
                            i * self.width
                        } else {
                            i * self.width + self.wires[i].nl
                        };
                        let end = if self.ramp_micro_reference {
                            (i + 1) * self.width
                        } else {
                            start + self.wires[i].nt
                        };
                        for ((((q, old), f), weighted), coordinate_work) in self.q[start..end]
                            .iter()
                            .zip(&self.candidate[start..end])
                            .zip(&self.block_force[start..end])
                            .zip(&mut self.ramp_weighted_displacement[start..end])
                            .zip(&mut self.ramp_coordinate_work[start..end])
                        {
                            let displacement = *q - *old;
                            work += *f * displacement;
                            *coordinate_work += *f * displacement;
                            *weighted += alpha * displacement;
                        }
                    }
                    stats.heat += micro.heat;
                    stats.force = stats.force.max(micro.force);
                    stats.iterations = stats.iterations.max(micro.iterations);
                    self.block_trace[frame] = micro.force;
                }
            }
            // Non-contact coordinates are independent linear oscillators
            // under the same ramp used in the microsteps. Compose them once.
            // Discrete summation by parts gives both force work and the
            // weighted displacement used by the endpoint correction.
            let carriers = if self.ramp_micro_reference {
                0
            } else if certified_flight {
                self.size - 1
            } else {
                self.free_indices.len()
            };
            for index in 0..carriers {
                let j = if certified_flight {
                    index
                } else {
                    self.free_indices[index]
                };
                // Axial coordinates will be composed once with their
                // directly determined endpoint force. They do not drive felt.
                if self.use_condensed_axial && j % self.width < self.wires[0].nl {
                    continue;
                }
                let slope = (self.ramp_end_force[j] - self.ramp_start_force[j]) / frames as f64;
                let first = self.ramp_start_force[j] + 0.5 * slope + self.ramp_correction[j];
                let t = self.ramp_transfer[frames * self.size + j];
                let q0 = self.block_start_q[j];
                let v0 = self.block_start_v[j];
                let q1 = t[0] * q0 + t[1] * v0 + t[2] * first + t[3] * slope;
                let v1 = -self.omega2[j] * t[1] * q0 + t[0] * v0 + t[4] * first + t[5] * slope;
                let sum = t[6] * q0 + t[7] * v0 + t[8] * first + t[9] * slope;
                let coordinate_work =
                    (first + slope * (frames - 1) as f64) * q1 - first * q0 - slope * sum;
                work += coordinate_work;
                self.ramp_coordinate_work[j] = coordinate_work;
                self.ramp_weighted_displacement[j] = (1. - 0.5 / frames as f64) * q1
                    - 0.5 / frames as f64 * q0
                    - sum / frames as f64;
                self.q[j] = q1;
                self.v[j] = v1;
            }
            self.block_next_force.fill(0.);
            let mut potential1 = 0.;
            for i in 0..3 {
                let start = i * self.width;
                let end = (i + 1) * self.width;
                if self.use_condensed_axial {
                    let endpoint = AxialEndpoint {
                        frames,
                        micro_reference: self.ramp_micro_reference,
                        q: &mut self.q[start..end],
                        v: &mut self.v[start..end],
                        q0: &self.block_start_q[start..end],
                        v0: &self.block_start_v[start..end],
                        start_force: &self.ramp_start_force[start..end],
                        end_force: &mut self.ramp_end_force[start..end],
                        correction: &mut self.ramp_correction[start..end],
                        scalar_correction: None,
                        weighted_displacement: &mut self.ramp_weighted_displacement[start..end],
                        coordinate_work: &mut self.ramp_coordinate_work[start..end],
                        transfer: &self.ramp_transfer
                            [frames * self.size + start..frames * self.size + end],
                        c: &self.c[start..end],
                        s: &self.s[start..end],
                        gain: &self.gain[start..end],
                    };
                    let (potential, changed_work) = self.wires[i]
                        .point_condensed_axial(endpoint, &mut self.block_next_force[start..end]);
                    potential1 += potential;
                    work += changed_work;
                } else {
                    potential1 += self.wires[i].point_nonlinearity(
                        &self.q[start..end],
                        &mut self.block_next_force[start..end],
                    );
                }
            }
            let work_residual = potential1 - potential0 - work;
            let mut shifted_work = work_residual;
            let mut force_difference = 0.;
            let mut displacement2 = 0.;
            for j in 0..self.size - 1 {
                let difference = self.block_next_force[j] - self.ramp_end_force[j];
                force_difference += difference * difference;
                shifted_work -= difference * self.ramp_weighted_displacement[j];
                displacement2 += (self.q[j] - self.block_start_q[j]).powi(2);
            }
            self.ramp_next_correction
                .copy_from_slice(&self.ramp_correction);
            if displacement2 > 1e-40 {
                if self.canonical_correction {
                    // A single work equation otherwise leaves an arbitrary
                    // orthogonal component depending on the iteration path.
                    let projection = (0..self.size - 1)
                        .map(|j| self.ramp_correction[j] * (self.q[j] - self.block_start_q[j]))
                        .sum::<f64>();
                    let magnitude = (projection + shifted_work) / displacement2;
                    for j in 0..self.size - 1 {
                        self.ramp_next_correction[j] =
                            magnitude * (self.q[j] - self.block_start_q[j]);
                    }
                    self.ramp_next_correction[self.size - 1] = 0.;
                } else {
                    for j in 0..self.size - 1 {
                        self.ramp_next_correction[j] +=
                            shifted_work * (self.q[j] - self.block_start_q[j]) / displacement2;
                    }
                }
            } else if shifted_work.abs() > 1e-13 * energy.max(1e-30) {
                self.q.copy_from_slice(&self.block_start_q);
                self.v.copy_from_slice(&self.block_start_v);
                return Err("degenerate ramp work correction; shorten block");
            } else if self.canonical_correction {
                self.ramp_next_correction.fill(0.);
            }
            let force_scale = norm(&self.block_next_force)
                .max(norm(&self.ramp_end_force))
                .max(norm(&self.ramp_next_correction))
                .max(1e-30);
            let correction_difference = self
                .ramp_next_correction
                .iter()
                .zip(&self.ramp_correction)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt();
            if force_difference.sqrt() <= 1e-11 * force_scale
                && correction_difference <= 1e-11 * force_scale
                && work_residual.abs() <= 1e-13 * energy.max(1e-30)
            {
                self.last_outer_iterations = outer + 1;
                self.last_certified_flight = certified_flight;
                self.endpoint_force.copy_from_slice(&self.block_next_force);
                self.endpoint_potential = potential1;
                self.endpoint_valid = true;
                return Ok(stats);
            }
            self.accelerate_ramp_fixed_point(outer);
        }
        self.q.copy_from_slice(&self.block_start_q);
        self.v.copy_from_slice(&self.block_start_v);
        Err("ramp endpoint/work solve did not converge; shorten block")
    }
    /// Depth-three Anderson acceleration changes the solver, not its target
    /// force/work equations or convergence tolerances. Storage is prepared.
    fn accelerate_ramp_fixed_point(&mut self, iteration: usize) {
        let n = 2 * self.size;
        self.solver_x[..self.size].copy_from_slice(&self.ramp_end_force);
        self.solver_x[self.size..n].copy_from_slice(&self.ramp_correction);
        self.solver_next[..self.size].copy_from_slice(&self.block_next_force);
        self.solver_next[self.size..n].copy_from_slice(&self.ramp_next_correction);
        anderson_update(
            iteration,
            &mut self.solver_x[..n],
            &self.solver_next[..n],
            &mut self.anderson_x,
            &mut self.anderson_r,
            &mut self.anderson_delta_r,
            &mut self.anderson_delta_f,
            self.gram_dot,
        );
        self.ramp_end_force
            .copy_from_slice(&self.solver_x[..self.size]);
        self.ramp_correction
            .copy_from_slice(&self.solver_x[self.size..n]);
    }
    pub fn step(&mut self) -> Result<Stats, &'static str> {
        self.endpoint_valid = false;
        for i in 0..3 {
            self.wires[i].fields(&self.q[i * self.width..(i + 1) * self.width], 0., 0);
        }
        for j in 0..self.size {
            self.linear[j] = self.c[j] * self.q[j] + self.s[j] * self.v[j];
        }
        self.candidate.copy_from_slice(&self.linear);
        let velocity_scale = self
            .s
            .iter()
            .zip(&self.v)
            .map(|(s, v)| (s * v).powi(2))
            .sum::<f64>()
            .sqrt();
        let scale = norm(&self.q).max(velocity_scale).max(1e-12);
        let mut cached = None;
        for iteration in 0..40 {
            let (jac, mut stats) = if let Some(v) = cached.take() {
                v
            } else {
                self.remainder(false)
            };
            for j in 0..self.size {
                self.residual[j] =
                    self.candidate[j] - self.linear[j] + self.gain[j] * self.gradient[j];
            }
            let residual_norm = norm(&self.residual);
            if !residual_norm.is_finite() {
                return Err("nonfinite string residual");
            }
            if residual_norm < 2e-14 * scale {
                for j in 0..self.size {
                    let v = -self.omega2[j] * self.s[j] * self.q[j] + self.c[j] * self.v[j]
                        - self.s[j] * self.gradient[j];
                    self.q[j] = self.candidate[j];
                    self.v[j] = v;
                }
                stats.iterations = iteration + 1;
                return Ok(stats);
            }
            let root = jac.map(|v| v.max(0.).sqrt());
            let mut matrix = [[0.; 3]; 3];
            let mut rhs = [0.; 3];
            let head_gain = self.gain[self.size - 1] / self.mass;
            for i in 0..3 {
                rhs[i] = root[i] * self.direction_dot(&self.residual, i);
                for k in 0..3 {
                    matrix[i][k] = root[i] * root[k] * head_gain + if i == k { 1. } else { 0. };
                }
                let nt = self.wires[i].nt;
                let start = i * self.width + self.wires[i].nl;
                matrix[i][i] += jac[i].max(0.)
                    * (0..nt)
                        .map(|j| self.gain[start + j] * self.shapes[i * nt + j].powi(2))
                        .sum::<f64>();
            }
            let mut lower = [[0.; 3]; 3];
            for i in 0..3 {
                for j in 0..=i {
                    let value =
                        matrix[i][j] - (0..j).map(|k| lower[i][k] * lower[j][k]).sum::<f64>();
                    lower[i][j] = if i == j {
                        value.sqrt()
                    } else {
                        value / lower[j][j]
                    };
                }
            }
            let mut solution = [0.; 3];
            for i in 0..3 {
                solution[i] =
                    (rhs[i] - (0..i).map(|j| lower[i][j] * solution[j]).sum::<f64>()) / lower[i][i];
            }
            for i in (0..3).rev() {
                solution[i] = (solution[i]
                    - (i + 1..3).map(|j| lower[j][i] * solution[j]).sum::<f64>())
                    / lower[i][i];
            }
            self.update.copy_from_slice(&self.residual);
            let mut correction = [0.; 3];
            for i in 0..3 {
                correction[i] = root[i] * solution[i];
            }
            for i in 0..3 {
                let nt = self.wires[i].nt;
                let start = i * self.width + self.wires[i].nl;
                for j in 0..nt {
                    self.update[start + j] -=
                        self.gain[start + j] * self.shapes[i * nt + j] * correction[i];
                }
                self.update[self.size - 1] -=
                    self.gain[self.size - 1] * correction[i] / self.mass.sqrt();
            }
            let mut accepted = false;
            for power in 0..14 {
                let fraction = 2f64.powi(-power);
                for j in 0..self.size {
                    self.trial[j] = self.candidate[j] - fraction * self.update[j];
                }
                let (tj, ts) = self.remainder(true);
                let norm_trial = (0..self.size)
                    .map(|j| {
                        (self.trial[j] - self.linear[j] + self.gain[j] * self.trial_gradient[j])
                            .powi(2)
                    })
                    .sum::<f64>()
                    .sqrt();
                if norm_trial < residual_norm {
                    self.candidate.copy_from_slice(&self.trial);
                    self.gradient.copy_from_slice(&self.trial_gradient);
                    cached = Some((tj, ts));
                    accepted = true;
                    break;
                }
            }
            if !accepted {
                return Err("string/contact line search failed; refine step");
            }
        }
        Err("string/contact did not converge; refine step")
    }
}
fn main() {
    let args = env::args().collect::<Vec<_>>();
    let bytes = fs::read(&args[1]).expect("choir coefficients");
    assert!(bytes.len() >= 20 && &bytes[..8] == b"RFSTR001");
    let u32_at = |o| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
    let (nt, nl, nodes) = (u32_at(8), u32_at(12), u32_at(16));
    assert!(nt > 0 && nt <= 256 && nl > 0 && nl <= 512 && nodes <= 2048);
    let width = nl + 2 * nt;
    let size = 3 * width + 1;
    assert_eq!(bytes.len(), 20 + 8 * (11 + 3 * nt + 9 + 3 * size));
    let mut offset = 20;
    let mut read = |count| {
        let result = bytes[offset..offset + 8 * count]
            .chunks_exact(8)
            .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        offset += 8 * count;
        assert!(result.iter().all(|v| v.is_finite()));
        result
    };
    // L, mu, Young, density, rate, head mass, total stiffness, exponent,
    // rate damping, and two endpoint displacements for the force-kernel test.
    let p = read(11);
    let shapes = read(3 * nt);
    let tensions = read(3);
    let launch = read(3);
    let durations = read(3);
    let q = read(size);
    let endpoint = read(size);
    let v = read(size);
    let area = p[1] / p[3];
    let ea = p[2] * area;
    let ei = p[2] * area * area / (4. * std::f64::consts::PI);
    let wires = tensions
        .iter()
        .map(|t| Wire::new(nt, nl, nodes, p[0], p[1], *t, ea, ei))
        .collect();
    let mut choir = Choir::new(wires, 1. / p[4], p[5], p[6] / 3., p[7], p[8], shapes, q, v);
    choir.ramp_micro_reference = args.iter().any(|arg| arg == "--micro-reference");
    choir.use_endpoint_cache = !args.iter().any(|arg| arg == "--uncached");
    choir.use_condensed_axial = !args.iter().any(|arg| arg == "--uncondensed");
    choir.canonical_correction = !args.iter().any(|arg| arg == "--legacy-correction");
    choir.use_scalar_flight = !args.iter().any(|arg| arg == "--vector-flight");
    if args.iter().any(|arg| arg == "--portable") {
        choir.gram_dot = dot;
        for wire in &mut choir.wires {
            wire.plan.kernel = fft_butterflies_portable;
            wire.plan.simd_fft = false;
        }
    }

    if args.get(2).map(String::as_str) == Some("kernel") {
        let mut gradients = vec![0.; 3 * width];
        let mut boundaries = [0.; 3];
        let mut potentials0 = [0.; 3];
        let mut potentials1 = [0.; 3];
        for i in 0..3 {
            let w = &mut choir.wires[i];
            let a = &choir.q[i * width..(i + 1) * width];
            let b = &endpoint[i * width..(i + 1) * width];
            potentials0[i] = w.nonlinear_potential(a, p[9]);
            potentials1[i] = w.nonlinear_potential(b, p[10]);
            boundaries[i] = w.nonlinear_avf(
                a,
                b,
                p[9],
                p[10],
                &mut gradients[i * width..(i + 1) * width],
            );
        }
        println!("{{\"gradient\":{:?},\"boundary_gradient\":{:?},\"potential0\":{:?},\"potential1\":{:?},\"cross_mass\":{:?},\"lift_mass\":{}}}",gradients,boundaries,potentials0,potentials1,choir.wires[0].cross,choir.wires[0].lift_mass);
        return;
    }
    let ramp_mode = args
        .get(2)
        .is_some_and(|v| v == "verify-ramp" || v == "bench-ramp");
    let block_mode = ramp_mode
        || args
            .get(2)
            .is_some_and(|v| v == "verify-block" || v == "bench-block");
    let micro_block = if block_mode {
        args.get(if args[2].starts_with("verify") { 3 } else { 4 })
            .map(|v| v.parse::<usize>().expect("microsteps per nonlinear block"))
            .unwrap_or(16)
    } else {
        1
    };
    assert!((1..=64).contains(&micro_block));
    if args
        .get(2)
        .is_some_and(|v| v == "verify" || v == "verify-block" || v == "verify-ramp")
    {
        let initial = choir.energy();
        let mut heat = 0.;
        let mut work = 0.;
        let mut peak = 0f64;
        let mut maximum_iterations = 0;
        let mut work_relative_max = 0f64;
        let mut maximum_outer_iterations = 0;
        let mut certified_flight_blocks = 0;
        let cached_before = choir.cached_initial_gradients;
        let zero_before = choir
            .wires
            .iter()
            .map(|w| w.plan.zero_transforms)
            .sum::<usize>();
        let total = (durations[2] * p[4]).round() as usize;
        let mut events = Vec::new();
        let mut checkpoints = Vec::new();
        let mut force_trace = Vec::new();
        let mut frame = 0;
        while frame < total {
            for event in 0..3 {
                if frame == (durations[event] * p[4]).round() as usize && launch[event] > 0. {
                    let before = choir.q[..size - 1].to_vec();
                    let velocity = choir.v[..size - 1].to_vec();
                    let w = choir.launch(launch[event]).expect("relaunch");
                    assert_eq!(before, choir.q[..size - 1]);
                    assert_eq!(velocity, choir.v[..size - 1]);
                    work += w;
                    events.push(w);
                }
            }
            let next_event = (0..2)
                .filter(|i| launch[*i] > 0.)
                .map(|i| (durations[i] * p[4]).round() as usize)
                .filter(|tick| *tick > frame)
                .min()
                .unwrap_or(total);
            let count = micro_block.min(total - frame).min(next_event - frame);
            let stats = if ramp_mode {
                choir.step_ramp(count)
            } else if block_mode {
                choir.step_block(count)
            } else {
                choir.step()
            }
            .expect("native step");
            maximum_outer_iterations = maximum_outer_iterations.max(choir.last_outer_iterations);
            if ramp_mode && choir.last_certified_flight {
                certified_flight_blocks += 1;
            }
            heat += stats.heat;
            peak = peak.max(stats.force);
            maximum_iterations = maximum_iterations.max(stats.iterations);
            assert!(stats.heat >= -1e-18);
            let energy = choir.energy();
            work_relative_max = work_relative_max.max(
                (energy + heat - initial - work).abs() / initial.abs().max(work.abs()).max(1e-30),
            );
            for offset in 0..count {
                if frame + offset < ((0.006 * p[4]).round() as usize) {
                    force_trace.push(if block_mode {
                        choir.block_trace[offset]
                    } else {
                        stats.force
                    });
                }
            }
            if frame % ((p[4] * 0.001).round() as usize).max(1) == 0 {
                checkpoints.push(energy);
            }
            frame += count;
        }
        let energy = choir.energy();
        println!("{{\"q\":{:?},\"v\":{:?},\"heat_j\":{},\"launch_work_j\":{:?},\"energy_j\":{},\"work_relative_max\":{},\"force_peak_n\":{},\"maximum_iterations\":{},\"force_trace\":{:?},\"energy_checkpoints\":{:?},\"micro_block\":{},\"maximum_outer_iterations\":{},\"certified_flight_blocks\":{},\"micro_reference\":{},\"simd_fft\":{},\"real_fft_complex_size\":{},\"exact_zero_transforms\":{},\"cached_initial_gradients\":{},\"condensed_axial\":{},\"endpoint_nonlinear_force\":{:?},\"canonical_correction\":{},\"scalar_flight_blocks\":{},\"scalar_flight_fallbacks\":{}}}",choir.q,choir.v,heat,events,energy,work_relative_max,peak,maximum_iterations,force_trace,checkpoints,micro_block,maximum_outer_iterations,certified_flight_blocks,choir.ramp_micro_reference,choir.wires[0].plan.simd_fft,choir.wires[0].plan.fft.len(),choir.wires.iter().map(|w|w.plan.zero_transforms).sum::<usize>()-zero_before,choir.cached_initial_gradients-cached_before,ramp_mode && choir.use_condensed_axial,choir.endpoint_force,ramp_mode && choir.canonical_correction,choir.scalar_flight_blocks,choir.scalar_flight_fallbacks);
        return;
    }
    choir.launch(launch[0]).expect("cold launch");
    let blocks = args
        .get(3)
        .map(|v| v.parse::<usize>().expect("benchmark buffer count"))
        .unwrap_or(512);
    assert!((8..=2048).contains(&blocks));
    let frames = 256;
    let mut times = Vec::with_capacity(blocks);
    // The first buffers include the physical impact, not just warm sustain.
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let mut peak = 0f64;
    let mut iterations = 0;
    let mut outer_iterations = 0;
    let mut certified_flight_blocks = 0;
    let cached_before = choir.cached_initial_gradients;
    let zero_before = choir
        .wires
        .iter()
        .map(|w| w.plan.zero_transforms)
        .sum::<usize>();
    let mut contact_buffers = 0;
    let mut contact_time_sum = 0.;
    let mut contact_time_max = 0f64;
    for _ in 0..blocks {
        let start = Instant::now();
        let mut completed = 0;
        let mut had_contact = false;
        while completed < frames {
            let count = micro_block.min(frames - completed);
            let stats = black_box(
                if ramp_mode {
                    choir.step_ramp(count)
                } else if block_mode {
                    choir.step_block(count)
                } else {
                    choir.step()
                }
                .expect("benchmark step"),
            );
            peak = peak.max(stats.force);
            had_contact |= stats.force > 0.;
            iterations = iterations.max(stats.iterations);
            outer_iterations = outer_iterations.max(choir.last_outer_iterations);
            if ramp_mode && choir.last_certified_flight {
                certified_flight_blocks += 1;
            }
            completed += count;
        }
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        times.push(elapsed);
        if had_contact {
            contact_buffers += 1;
            contact_time_sum += elapsed;
            contact_time_max = contact_time_max.max(elapsed);
        }
    }
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocations;
    let mean = times.iter().sum::<f64>() / blocks as f64;
    let first_buffer_ms = times[0];
    let worst_buffer_index = times
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0;
    let mean_contact_ms = if contact_buffers > 0 {
        contact_time_sum / contact_buffers as f64
    } else {
        0.
    };
    times.sort_by(f64::total_cmp);
    println!("{{\"rate_hz\":{},\"transverse\":{},\"axial\":{},\"nodes\":{},\"frames\":{},\"buffers\":{},\"mean_ms\":{},\"p99_ms\":{},\"max_ms\":{},\"deadline_ms\":{},\"allocations\":{},\"peak_force_n\":{},\"maximum_iterations\":{},\"micro_block\":{},\"maximum_outer_iterations\":{},\"certified_flight_blocks\":{},\"micro_reference\":{},\"simd_fft\":{},\"real_fft_complex_size\":{},\"exact_zero_transforms\":{},\"cached_initial_gradients\":{},\"first_buffer_ms\":{},\"worst_buffer_index\":{},\"buffers_with_contact\":{},\"mean_contact_buffer_ms\":{},\"max_contact_buffer_ms\":{},\"condensed_axial\":{},\"canonical_correction\":{},\"scalar_flight_blocks\":{},\"scalar_flight_fallbacks\":{}}}",p[4],nt,nl,nodes,frames,blocks,mean,times[blocks*99/100],times[blocks-1],frames as f64/p[4]*1000.,allocations,peak,iterations,micro_block,outer_iterations,certified_flight_blocks,choir.ramp_micro_reference,choir.wires[0].plan.simd_fft,choir.wires[0].plan.fft.len(),choir.wires.iter().map(|w|w.plan.zero_transforms).sum::<usize>()-zero_before,choir.cached_initial_gradients-cached_before,first_buffer_ms,worst_buffer_index,contact_buffers,mean_contact_ms,contact_time_max,ramp_mode && choir.use_condensed_axial,ramp_mode && choir.canonical_correction,choir.scalar_flight_blocks,choir.scalar_flight_fallbacks);
}
