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
#[derive(Clone, Copy, Default)]
struct Complex {
    r: f64,
    i: f64,
}
struct CosinePlan {
    nodes: usize,
    fft: Vec<Complex>,
    roots: Vec<Complex>,
    phase: Vec<Complex>,
    reverse: Vec<usize>,
}
impl CosinePlan {
    fn new(nodes: usize) -> Self {
        assert!(nodes.is_power_of_two());
        let n = 2 * nodes;
        let bits = n.trailing_zeros();
        Self {
            nodes,
            fft: vec![Complex::default(); n],
            roots: (0..n / 2)
                .map(|k| {
                    let a = -2. * std::f64::consts::PI * k as f64 / n as f64;
                    Complex {
                        r: a.cos(),
                        i: a.sin(),
                    }
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
        let mut width = 2;
        while width <= n {
            let half = width / 2;
            let stride = n / width;
            for first in (0..n).step_by(width) {
                for j in 0..half {
                    let mut w = self.roots[j * stride];
                    if inverse {
                        w.i = -w.i;
                    }
                    let v = self.fft[first + j + half];
                    let u = self.fft[first + j];
                    let r = v.r * w.r - v.i * w.i;
                    let i = v.r * w.i + v.i * w.r;
                    self.fft[first + j] = Complex {
                        r: u.r + r,
                        i: u.i + i,
                    };
                    self.fft[first + j + half] = Complex {
                        r: u.r - r,
                        i: u.i - i,
                    };
                }
            }
            width *= 2;
        }
        if inverse {
            for x in &mut self.fft {
                x.r /= n as f64;
                x.i /= n as f64;
            }
        }
    }
    fn synthesize(&mut self, q: &[f64], scales: &[f64], out: &mut [f64]) {
        assert_eq!(q.len(), scales.len());
        assert_eq!(out.len(), self.nodes);
        self.fft.fill(Complex::default());
        for (k, (q, s)) in q.iter().zip(scales).enumerate() {
            let k = k + 1;
            let a = self.nodes as f64 * q * s;
            let p = self.phase[k];
            self.fft[k] = Complex {
                r: a * p.r,
                i: a * p.i,
            };
            self.fft[2 * self.nodes - k] = Complex {
                r: a * p.r,
                i: -a * p.i,
            };
        }
        self.transform(true);
        for (x, v) in out.iter_mut().zip(&self.fft) {
            *x = v.r;
        }
    }
    fn analyze(&mut self, values: &[f64], scales: &[f64], factor: f64, out: &mut [f64]) {
        assert_eq!(values.len(), self.nodes);
        assert_eq!(scales.len(), out.len());
        for (j, x) in values.iter().enumerate() {
            self.fft[j] = Complex { r: *x, i: 0. };
            self.fft[2 * self.nodes - 1 - j] = Complex { r: *x, i: 0. };
        }
        self.transform(false);
        for (k, (value, scale)) in out.iter_mut().zip(scales).enumerate() {
            let k = k + 1;
            *value = factor
                * scale
                * (self.fft[k].r * self.phase[k].r + self.fft[k].i * self.phase[k].i);
        }
    }
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
fn norm(a: &[f64]) -> f64 {
    dot(a, a).sqrt()
}
#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub heat: f64,
    pub force: f64,
    pub iterations: usize,
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
        let c = omega2.iter().map(|w| (w.sqrt() * dt).cos()).collect();
        let s = omega2
            .iter()
            .map(|w| {
                if *w == 0. {
                    dt
                } else {
                    (w.sqrt() * dt).sin() / w.sqrt()
                }
            })
            .collect();
        let gain = omega2
            .iter()
            .map(|w| {
                if *w == 0. {
                    dt * dt / 2.
                } else {
                    2. * (w.sqrt() * dt / 2.).sin().powi(2) / w
                }
            })
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
            self.wires[i].nonlinear_avf(
                &self.q[i * self.width..(i + 1) * self.width],
                &candidate[i * self.width..(i + 1) * self.width],
                0.,
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
    pub fn step(&mut self) -> Result<Stats, &'static str> {
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
    if args.get(2).map(String::as_str) == Some("verify") {
        let initial = choir.energy();
        let mut heat = 0.;
        let mut work = 0.;
        let mut peak = 0f64;
        let mut maximum_iterations = 0;
        let mut work_relative_max = 0f64;
        let total = (durations[2] * p[4]).round() as usize;
        let mut events = Vec::new();
        let mut checkpoints = Vec::new();
        let mut force_trace = Vec::new();
        for frame in 0..total {
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
            let stats = choir.step().expect("native step");
            heat += stats.heat;
            peak = peak.max(stats.force);
            maximum_iterations = maximum_iterations.max(stats.iterations);
            assert!(stats.heat >= -1e-18);
            let energy = choir.energy();
            work_relative_max = work_relative_max.max(
                (energy + heat - initial - work).abs() / initial.abs().max(work.abs()).max(1e-30),
            );
            if frame < ((0.006 * p[4]).round() as usize) {
                force_trace.push(stats.force);
            }
            if frame % ((p[4] * 0.001).round() as usize).max(1) == 0 {
                checkpoints.push(energy);
            }
        }
        let energy = choir.energy();
        println!("{{\"q\":{:?},\"v\":{:?},\"heat_j\":{},\"launch_work_j\":{:?},\"energy_j\":{},\"work_relative_max\":{},\"force_peak_n\":{},\"maximum_iterations\":{},\"force_trace\":{:?},\"energy_checkpoints\":{:?}}}",choir.q,choir.v,heat,events,energy,work_relative_max,peak,maximum_iterations,force_trace,checkpoints);
        return;
    }
    choir.launch(launch[0]).expect("cold launch");
    let blocks = args.get(3).map(|v|v.parse::<usize>().expect("benchmark buffer count")).unwrap_or(512);
    assert!((8..=2048).contains(&blocks));
    let frames = 256;
    let mut times = Vec::with_capacity(blocks);
    // The first buffers include the physical impact, not just warm sustain.
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let mut peak = 0f64;
    let mut iterations = 0;
    for _ in 0..blocks {
        let start = Instant::now();
        for _ in 0..frames {
            let stats = black_box(choir.step().expect("benchmark step"));
            peak = peak.max(stats.force);
            iterations = iterations.max(stats.iterations);
        }
        times.push(start.elapsed().as_secs_f64() * 1000.);
    }
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocations;
    let mean = times.iter().sum::<f64>() / blocks as f64;
    times.sort_by(f64::total_cmp);
    println!("{{\"rate_hz\":{},\"transverse\":{},\"axial\":{},\"nodes\":{},\"frames\":{},\"buffers\":{},\"mean_ms\":{},\"p99_ms\":{},\"max_ms\":{},\"deadline_ms\":{},\"allocations\":{},\"peak_force_n\":{},\"maximum_iterations\":{}}}",p[4],nt,nl,nodes,frames,blocks,mean,times[blocks*99/100],times[blocks-1],frames as f64/p[4]*1000.,allocations,peak,iterations);
}
