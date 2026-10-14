//! Four independent temporal nodes share transform coefficients. Lanes never
//! mix: ordinary f64 operations allow the compiler to vectorize across nodes.
use super::{CosinePlan, Wire};
use std::ops::{Add, Mul, Sub};

#[derive(Clone, Copy, Default)]
#[repr(C, align(32))]
struct Quad([f64; 4]);
impl Quad {
    #[inline(always)]
    fn splat(x: f64) -> Self {
        Self([x; 4])
    }
    #[inline(always)]
    fn gather(q: &[f64], stride: usize, k: usize) -> Self {
        Self([q[k], q[stride + k], q[2 * stride + k], q[3 * stride + k]])
    }
}
macro_rules! quad_op {
    ($trait:ident, $method:ident, $op:tt) => {
        impl $trait for Quad {
            type Output = Self;
            #[inline(always)]
            fn $method(self, b: Self) -> Self {
                let a = self.0; let b = b.0;
                Self([a[0] $op b[0], a[1] $op b[1], a[2] $op b[2], a[3] $op b[3]])
            }
        }
    };
}
quad_op!(Add, add, +);
quad_op!(Sub, sub, -);
quad_op!(Mul, mul, *);

#[derive(Clone, Copy, Default)]
struct ComplexQuad {
    r: Quad,
    i: Quad,
}

pub(super) struct BatchCosine {
    fft: Vec<ComplexQuad>,
    fields: Vec<Quad>,
    products: Vec<Quad>,
}
impl BatchCosine {
    fn new(nodes: usize) -> Self {
        Self {
            fft: vec![ComplexQuad::default(); nodes / 2],
            fields: vec![Quad::default(); 3 * nodes],
            products: vec![Quad::default(); 3 * nodes],
        }
    }
    fn transform(&mut self, plan: &CosinePlan, inverse: bool) {
        let mut width = 2;
        while width <= self.fft.len() {
            let half = width / 2;
            for chunk in self.fft.chunks_exact_mut(width) {
                let (left, right) = chunk.split_at_mut(half);
                for ((u, v), root) in left
                    .iter_mut()
                    .zip(right)
                    .zip(&plan.roots[half - 1..2 * half - 1])
                {
                    let wr = Quad::splat(root.r);
                    let wi = Quad::splat(if inverse { -root.i } else { root.i });
                    let r = v.r * wr - v.i * wi;
                    let i = v.r * wi + v.i * wr;
                    *v = ComplexQuad {
                        r: u.r - r,
                        i: u.i - i,
                    };
                    *u = ComplexQuad {
                        r: u.r + r,
                        i: u.i + i,
                    };
                }
            }
            width *= 2;
        }
    }
    fn spectrum(
        plan: &CosinePlan,
        q: &[f64],
        stride: usize,
        scales: &[f64],
        k: usize,
    ) -> ComplexQuad {
        let a = if k > 0 && k <= scales.len() {
            Quad::splat(plan.nodes as f64 * 0.5)
                * Quad::gather(q, stride, k - 1)
                * Quad::splat(scales[k - 1])
        } else {
            Quad::default()
        };
        let mirror = plan.nodes - k;
        let b = if k > 0 && mirror <= scales.len() {
            Quad::splat(plan.nodes as f64 * 0.5)
                * Quad::gather(q, stride, mirror - 1)
                * Quad::splat(scales[mirror - 1])
        } else {
            Quad::default()
        };
        let pr = Quad::splat(plan.phase[k].r);
        let pi = Quad::splat(plan.phase[k].i);
        ComplexQuad {
            r: a * pr + b * pi,
            i: a * pi - b * pr,
        }
    }
    fn synthesize(
        &mut self,
        plan: &CosinePlan,
        q: &[f64],
        stride: usize,
        scales: &[f64],
        slot: usize,
    ) {
        let n = plan.nodes;
        let m = n / 2;
        if (0..4).all(|lane| {
            q[lane * stride..lane * stride + scales.len()]
                .iter()
                .all(|x| *x == 0.)
        }) {
            self.fields[slot * n..(slot + 1) * n].fill(Quad::default());
            return;
        }
        for k in 0..m {
            let x = Self::spectrum(plan, q, stride, scales, k);
            let y = Self::spectrum(plan, q, stride, scales, k + m);
            let wr = Quad::splat(plan.real_roots[k].r);
            let wi = Quad::splat(plan.real_roots[k].i);
            let dr = x.r - y.r;
            let di = x.i - y.i;
            self.fft[plan.reverse[k]] = ComplexQuad {
                r: Quad::splat(0.5) * (x.r + y.r + wi * dr - wr * di),
                i: Quad::splat(0.5) * (x.i + y.i + wr * dr + wi * di),
            };
        }
        self.transform(plan, true);
        for j in 0..m {
            let a = 2 * j;
            let b = a + 1;
            let ia = if a < m { 2 * a } else { 2 * (n - 1 - a) + 1 };
            let ib = if b < m { 2 * b } else { 2 * (n - 1 - b) + 1 };
            // m is a power of two, so division equals this exact scaling.
            self.fields[slot * n + ia] = self.fft[j].r * Quad::splat(1. / m as f64);
            self.fields[slot * n + ib] = self.fft[j].i * Quad::splat(1. / m as f64);
        }
    }
    fn analyze(
        &mut self,
        plan: &CosinePlan,
        slot: usize,
        scales: &[f64],
        factor: f64,
        out: &mut [f64],
        stride: usize,
    ) {
        let n = plan.nodes;
        let m = n / 2;
        if self.products[slot * n..(slot + 1) * n]
            .iter()
            .all(|q| q.0.iter().all(|x| *x == 0.))
        {
            for lane in 0..4 {
                out[lane * stride..lane * stride + scales.len()].fill(0.);
            }
            return;
        }
        for j in 0..m {
            let a = 2 * j;
            let b = a + 1;
            let ia = if a < m { 2 * a } else { 2 * (n - 1 - a) + 1 };
            let ib = if b < m { 2 * b } else { 2 * (n - 1 - b) + 1 };
            self.fft[plan.reverse[j]] = ComplexQuad {
                r: self.products[slot * n + ia],
                i: self.products[slot * n + ib],
            };
        }
        self.transform(plan, false);
        for (index, &scale) in scales.iter().enumerate() {
            let k = index + 1;
            let a = self.fft[k % m];
            let b = self.fft[(m - k % m) % m];
            let wr = Quad::splat(plan.real_roots[k].r);
            let wi = Quad::splat(plan.real_roots[k].i);
            let xr = Quad::splat(0.5) * (a.r + b.r + wr * (a.i + b.i) + wi * (a.r - b.r));
            let xi = Quad::splat(0.5) * (a.i - b.i + wr * (b.r - a.r) + wi * (a.i + b.i));
            let result = Quad::splat(2. * factor * scale)
                * (xr * Quad::splat(plan.phase[k].r) + xi * Quad::splat(plan.phase[k].i));
            for lane in 0..4 {
                out[lane * stride + index] = result.0[lane];
            }
        }
    }
}
impl Wire {
    pub(crate) fn prepare_batch4(&mut self) {
        self.batch4 = Some(BatchCosine::new(self.nodes));
    }
    pub(crate) fn disable_batch4(&mut self) {
        self.batch4 = None;
    }
    pub(crate) fn batch_prepared_bytes(&self) -> usize {
        self.batch4
            .as_ref()
            .map(|b| {
                b.fft.capacity() * std::mem::size_of::<ComplexQuad>()
                    + (b.fields.capacity() + b.products.capacity()) * std::mem::size_of::<Quad>()
            })
            .unwrap_or(0)
    }
    pub(crate) fn point_nonlinearity_batch4(&mut self, q: &[f64], stride: usize, out: &mut [f64]) {
        assert!(
            stride >= self.size()
                && q.len() >= 3 * stride + self.size()
                && out.len() >= 3 * stride + self.size()
        );
        let b = self.batch4.as_mut().expect("prepared four-node workspace");
        b.synthesize(&self.plan, q, stride, &self.ku, 0);
        b.synthesize(&self.plan, &q[self.nl..], stride, &self.kv, 1);
        b.synthesize(&self.plan, &q[self.nl + self.nt..], stride, &self.kv, 2);
        for j in 0..self.nodes {
            let u = b.fields[j];
            let v = b.fields[self.nodes + j];
            let w = b.fields[2 * self.nodes + j];
            let eta = Quad::splat(0.5) * (v * v + w * w);
            b.products[j] = eta;
            b.products[self.nodes + j] = (u + eta) * v;
            b.products[2 * self.nodes + j] = (u + eta) * w;
        }
        let factor = self.ea * self.length / (2 * self.nodes) as f64;
        b.analyze(&self.plan, 0, &self.ku, factor, out, stride);
        b.analyze(&self.plan, 1, &self.kv, factor, &mut out[self.nl..], stride);
        b.analyze(
            &self.plan,
            2,
            &self.kv,
            factor,
            &mut out[self.nl + self.nt..],
            stride,
        );
    }
}
