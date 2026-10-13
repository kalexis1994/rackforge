//! Experimental quadratic force convolution on exact free modal carriers.
//! This is a component kernel, not an accepted nonlinear time step.
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

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-4 {
        let y = x * x;
        1. + y * (-1. / 6. + y * (1. / 120. - y / 5040.))
    } else {
        x.sin() / x
    }
}

/// Positive convolution with sin(omega*(h-t))/omega and cos(omega*(h-t)).
/// Signed forcing frequencies are retained, including zero and resonance.
fn harmonic_response(omega: f64, nu: f64, h: f64) -> [[f64; 2]; 2] {
    if omega == 0. {
        let x = nu * h;
        let s = sinc(x / 2.);
        let q_sin = if x.abs() < 1e-3 {
            h * h * x * (1. / 6. + x * x * (-1. / 120. + x * x / 5040.))
        } else {
            h * h * (x - x.sin()) / (x * x)
        };
        return [
            [0.5 * h * h * s * s, q_sin],
            [h * sinc(x), 0.5 * h * x * s * s],
        ];
    }
    let a = h * sinc((nu - omega) * h / 2.);
    let b = h * sinc((nu + omega) * h / 2.);
    let (sp, cp) = ((omega + nu) * h / 2.).sin_cos();
    let (sm, cm) = ((omega - nu) * h / 2.).sin_cos();
    [
        [
            0.5 * (a * sp + b * sm) / omega,
            0.5 * (-a * cp + b * cm) / omega,
        ],
        [0.5 * (a * cp + b * cm), 0.5 * (a * sp - b * sm)],
    ]
}

#[derive(Clone)]
struct Term {
    left: usize,
    right: usize,
    output: usize,
    coefficient: f64,
    response: [[f64; 4]; 2],
}
impl Term {
    fn new(
        left: usize,
        right: usize,
        output: usize,
        coefficient: f64,
        omega: &[f64],
        h: f64,
    ) -> Self {
        let sum = harmonic_response(omega[output], omega[left] + omega[right], h);
        let diff = harmonic_response(omega[output], omega[left] - omega[right], h);
        let mut response = [[0.; 4]; 2];
        for row in 0..2 {
            response[row] = [
                0.5 * coefficient * (sum[row][0] + diff[row][0]),
                0.5 * coefficient * (sum[row][1] - diff[row][1]),
                0.5 * coefficient * (sum[row][1] + diff[row][1]),
                0.5 * coefficient * (diff[row][0] - sum[row][0]),
            ];
        }
        Self {
            left,
            right,
            output,
            coefficient,
            response,
        }
    }
}

pub struct PhaseQuadratic {
    nt: usize,
    omega: Vec<f64>,
    axial: Vec<Term>,
    transverse: Vec<Term>,
    amplitudes: Vec<f64>,
}
impl PhaseQuadratic {
    pub fn new(
        nt: usize,
        nl: usize,
        omega: Vec<f64>,
        ku: &[f64],
        kv: &[f64],
        ea_length: f64,
        h: f64,
    ) -> Self {
        assert!(nt > 0 && nl > 0 && omega.len() == nl + 2 * nt);
        assert!(ku.len() == nl && kv.len() == nt);
        assert!(h.is_finite() && h > 0. && ea_length.is_finite() && ea_length > 0.);
        assert!(omega.iter().all(|w| w.is_finite() && *w > 0.));
        let mut axial = Vec::with_capacity(nt * nt);
        let mut transverse = Vec::with_capacity(2 * nt * nt - nt);
        // Integral cos(j*pi*x/L) cos(k*pi*x/L) cos(l*pi*x/L)
        // is L/4 precisely when j=k+l or j=|k-l|>0, and zero otherwise.
        // The diagonal receives only one ordered pair in eta=0.5*(v*v+w*w).
        for k in 1..=nt {
            for l in k..=nt {
                for j in [k + l, l - k] {
                    if j == 0 || j > nl {
                        continue;
                    }
                    let coefficient = ea_length / 8.
                        * ku[j - 1]
                        * kv[k - 1]
                        * kv[l - 1]
                        * if k == l { 1. } else { 2. };
                    axial.push(Term::new(
                        nl + k - 1,
                        nl + l - 1,
                        j - 1,
                        coefficient,
                        &omega,
                        h,
                    ));
                    transverse.push(Term::new(
                        j - 1,
                        nl + l - 1,
                        nl + k - 1,
                        coefficient * if k == l { 2. } else { 1. },
                        &omega,
                        h,
                    ));
                    if k != l {
                        transverse.push(Term::new(
                            j - 1,
                            nl + k - 1,
                            nl + l - 1,
                            coefficient,
                            &omega,
                            h,
                        ));
                    }
                }
            }
        }
        let size = omega.len();
        Self {
            nt,
            omega,
            axial,
            transverse,
            amplitudes: vec![0.; size],
        }
    }
    pub fn terms(&self) -> usize {
        self.axial.len() + self.transverse.len()
    }
    pub fn prepared_bytes(&self) -> usize {
        (self.axial.capacity() + self.transverse.capacity()) * std::mem::size_of::<Term>()
            + (self.omega.capacity() + self.amplitudes.capacity()) * std::mem::size_of::<f64>()
    }
    /// Force-induced increments only: q(t)=q0*cos(w*t)+v0/w*sin(w*t).
    /// Quadratic part of the ORIGINAL gradient: gu=EA Du' eta,
    /// gv=EA Dv'(u*v), gw=EA Dw'(u*w). The cubic eta*v/w is excluded.
    /// Caller retains every physical q/v; all scratch is prepared here.
    pub fn apply(&mut self, q: &[f64], v: &[f64], dq: &mut [f64], dv: &mut [f64]) {
        let size = self.omega.len();
        assert!(q.len() == size && v.len() == size && dq.len() == size && dv.len() == size);
        dq.fill(0.);
        dv.fill(0.);
        for i in 0..size {
            self.amplitudes[i] = v[i] / self.omega[i];
        }
        let products = |left: usize, right: usize| {
            [
                q[left] * q[right],
                q[left] * self.amplitudes[right],
                self.amplitudes[left] * q[right],
                self.amplitudes[left] * self.amplitudes[right],
            ]
        };
        let dot4 = |a: &[f64; 4], b: &[f64; 4]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
        for term in &self.axial {
            let a = products(term.left, term.right);
            let b = products(term.left + self.nt, term.right + self.nt);
            let combined = [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]];
            dq[term.output] -= dot4(&term.response[0], &combined);
            dv[term.output] -= dot4(&term.response[1], &combined);
        }
        for term in &self.transverse {
            for axis in [0, self.nt] {
                let a = products(term.left, term.right + axis);
                dq[term.output + axis] -= dot4(&term.response[0], &a);
                dv[term.output + axis] -= dot4(&term.response[1], &a);
            }
        }
    }
    /// Point gradient for verifying the spatial contraction separately.
    fn point(&self, q: &[f64], out: &mut [f64]) {
        out.fill(0.);
        for term in &self.axial {
            out[term.output] += term.coefficient
                * (q[term.left] * q[term.right] + q[term.left + self.nt] * q[term.right + self.nt]);
        }
        for term in &self.transverse {
            for axis in [0, self.nt] {
                out[term.output + axis] += term.coefficient * q[term.left] * q[term.right + axis];
            }
        }
    }
}

fn apply_choir(plans: &mut [PhaseQuadratic], q: &[f64], v: &[f64], dq: &mut [f64], dv: &mut [f64]) {
    let width = plans[0].omega.len();
    for (i, plan) in plans.iter_mut().enumerate() {
        let range = i * width..(i + 1) * width;
        plan.apply(
            &q[range.clone()],
            &v[range.clone()],
            &mut dq[range.clone()],
            &mut dv[range],
        );
    }
}

fn main() {
    let args = env::args().collect::<Vec<_>>();
    if args.get(1).is_some_and(|arg| arg == "harmonic") {
        let values = args[2..]
            .iter()
            .map(|x| x.parse::<f64>().unwrap())
            .collect::<Vec<_>>();
        assert!(
            values.len() == 3
                && values[0] >= 0.
                && values[2] > 0.
                && values.iter().all(|x| x.is_finite())
        );
        println!("{:?}", harmonic_response(values[0], values[1], values[2]));
        return;
    }
    let bytes = fs::read(&args[1]).expect("RFSTR001 fixture");
    assert!(bytes.len() >= 20 && &bytes[..8] == b"RFSTR001");
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    let (nt, nl, nodes) = (u32_at(8), u32_at(12), u32_at(16));
    assert!(nt > 0 && nt <= 256 && nl > 0 && nl <= 512 && nodes > nl.max(2 * nt));
    let width = nl + 2 * nt;
    let size = 3 * width + 1;
    assert!(bytes.len() == 20 + 8 * (11 + 3 * nt + 9 + 3 * size));
    let data = bytes[20..]
        .chunks_exact(8)
        .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert!(data.iter().all(|x| x.is_finite()));
    let p = &data[..11];
    let tensions = &data[11 + 3 * nt..14 + 3 * nt];
    let state_start = 20 + 3 * nt;
    let q = &data[state_start..state_start + 3 * width];
    let v = &data[state_start + 2 * size..state_start + 2 * size + 3 * width];
    let frames = args
        .get(2)
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(16);
    assert!((1..=64).contains(&frames));
    assert!(
        256 % frames == 0,
        "benchmark buffer must contain whole prepared blocks"
    );
    let h = frames as f64 / p[4];
    let root = (p[1] * p[0] / 2.).sqrt();
    let area = p[1] / p[3];
    let ea = p[2] * area;
    let ei = p[2] * area * area / (4. * std::f64::consts::PI);
    let ku = (1..=nl)
        .map(|n| std::f64::consts::PI * n as f64 / p[0] / root)
        .collect::<Vec<_>>();
    let kv = (1..=nt)
        .map(|n| std::f64::consts::PI * n as f64 / p[0] / root)
        .collect::<Vec<_>>();
    let start = Instant::now();
    let mut plans = tensions
        .iter()
        .map(|t| {
            let axial = ku.iter().map(|k| (ea * p[0] / 2.).sqrt() * k);
            let transverse = kv
                .iter()
                .enumerate()
                .map(|(i, k)| {
                    (t * p[0] / 2. * k * k
                        + ei * p[0] / 2. * (std::f64::consts::PI * (i + 1) as f64 / p[0]).powi(4)
                            / (root * root))
                        .sqrt()
                })
                .collect::<Vec<_>>();
            PhaseQuadratic::new(
                nt,
                nl,
                axial
                    .chain(transverse.iter().copied())
                    .chain(transverse.iter().copied())
                    .collect(),
                &ku,
                &kv,
                ea * p[0],
                h,
            )
        })
        .collect::<Vec<_>>();
    let preparation_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut dq = vec![0.; 3 * width];
    let mut dv = dq.clone();
    apply_choir(&mut plans, q, v, &mut dq, &mut dv);
    let buffers = args
        .get(3)
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(64);
    assert!((8..=2048).contains(&buffers));
    let mut times = Vec::with_capacity(buffers);
    let allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
    for _ in 0..buffers {
        let start = Instant::now();
        for _ in 0..256 / frames {
            apply_choir(
                black_box(&mut plans),
                black_box(q),
                black_box(v),
                &mut dq,
                &mut dv,
            );
            black_box(&dq);
            black_box(&dv);
        }
        times.push(start.elapsed().as_secs_f64() * 1000.);
    }
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocations_before;
    let mean = times.iter().sum::<f64>() / buffers as f64;
    times.sort_by(f64::total_cmp);
    let mut end_force = vec![0.; 3 * width];
    let mut free_endpoint = q.to_vec();
    for (i, plan) in plans.iter().enumerate() {
        for j in 0..width {
            let (s, c) = (plan.omega[j] * h).sin_cos();
            free_endpoint[i * width + j] =
                c * q[i * width + j] + s * v[i * width + j] / plan.omega[j];
        }
        plan.point(
            &free_endpoint[i * width..(i + 1) * width],
            &mut end_force[i * width..(i + 1) * width],
        );
    }
    println!("{{\"dq\":{:?},\"dv\":{:?},\"endpoint_quadratic_gradient\":{:?},\"omega\":{:?},\"prepared_terms\":{},\"prepared_bytes\":{},\"preparation_ms\":{},\"block_seconds\":{},\"frames\":{},\"buffers\":{},\"mean_ms\":{},\"p99_ms\":{},\"max_ms\":{},\"allocations\":{},\"nonlinear_step\":false}}", dq,dv,end_force,plans.iter().flat_map(|p|p.omega.iter()).collect::<Vec<_>>(),plans.iter().map(|p|p.terms()).sum::<usize>(),plans.iter().map(|p|p.prepared_bytes()).sum::<usize>(),preparation_ms,h,frames,buffers,mean,times[buffers*99/100],times[buffers-1],allocations);
}
