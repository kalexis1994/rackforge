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
    packing: Option<PreparedPacking>,
    fft: Vec<ComplexQuad>,
    fields: Vec<Quad>,
    products: Vec<Quad>,
    // Current sweep only: V, W and eta for each independent four-node group.
    transverse_cache: Vec<Quad>,
}
impl BatchCosine {
    fn new(plan: &CosinePlan, ku: &[f64], kv: &[f64], factor: f64) -> Self {
        let nodes = plan.nodes;
        Self {
            packing: Some(PreparedPacking::new(plan, ku, kv, factor)),
            fft: vec![ComplexQuad::default(); nodes / 2],
            fields: vec![Quad::default(); 3 * nodes],
            products: vec![Quad::default(); 3 * nodes],
            transverse_cache: Vec::new(),
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
        if let Some(packing) = &self.packing {
            for cell in &packing.synthesis[usize::from(slot != 0)] {
                let mut r = Quad::default();
                let mut i = Quad::default();
                for tap in 0..cell.count {
                    let value = Quad::gather(q, stride, cell.source[tap]);
                    r = r + value * Quad::splat(cell.weights[tap][0]);
                    i = i + value * Quad::splat(cell.weights[tap][1]);
                }
                self.fft[cell.target] = ComplexQuad { r, i };
            }
        } else {
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
        if let Some(packing) = &self.packing {
            for (index, cell) in packing.analysis[usize::from(slot != 0)].iter().enumerate() {
                let a = self.fft[cell.a];
                let b = self.fft[cell.b];
                let value = a.r * Quad::splat(cell.weights[0])
                    + a.i * Quad::splat(cell.weights[1])
                    + b.r * Quad::splat(cell.weights[2])
                    + b.i * Quad::splat(cell.weights[3]);
                for lane in 0..4 {
                    out[lane * stride + index] = value.0[lane];
                }
            }
            return;
        }
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
    pub(crate) fn axial_modes(&self) -> usize {
        self.nl
    }
    pub(crate) fn prepare_axial_elimination(&mut self, order: usize) {
        let b = self.batch4.as_mut().expect("prepared four-node workspace");
        b.transverse_cache = vec![Quad::default(); 3 * self.nodes * (order / 4)];
    }
    pub(crate) fn disable_axial_elimination_cache(&mut self) {
        if let Some(b) = &mut self.batch4 {
            b.transverse_cache = Vec::new();
        }
    }
    pub(crate) fn axial_force_batch4(
        &mut self,
        q: &[f64],
        stride: usize,
        out: &mut [f64],
        group: usize,
    ) {
        let b = self.batch4.as_mut().expect("prepared four-node workspace");
        b.synthesize(&self.plan, &q[self.nl..], stride, &self.kv, 1);
        b.synthesize(&self.plan, &q[self.nl + self.nt..], stride, &self.kv, 2);
        for j in 0..self.nodes {
            let v = b.fields[self.nodes + j];
            let w = b.fields[2 * self.nodes + j];
            b.products[j] = Quad::splat(0.5) * (v * v + w * w);
        }
        let cache = &mut b.transverse_cache[group * 3 * self.nodes..(group + 1) * 3 * self.nodes];
        cache[..2 * self.nodes].copy_from_slice(&b.fields[self.nodes..]);
        cache[2 * self.nodes..].copy_from_slice(&b.products[..self.nodes]);
        let factor = self.ea * self.length / (2 * self.nodes) as f64;
        b.analyze(&self.plan, 0, &self.ku, factor, out, stride);
    }
    pub(crate) fn transverse_force_batch4(
        &mut self,
        q: &[f64],
        stride: usize,
        out: &mut [f64],
        group: usize,
    ) {
        let b = self.batch4.as_mut().expect("prepared four-node workspace");
        b.synthesize(&self.plan, q, stride, &self.ku, 0);
        let cache = &b.transverse_cache[group * 3 * self.nodes..(group + 1) * 3 * self.nodes];
        for j in 0..self.nodes {
            let u = b.fields[j];
            let v = cache[j];
            let w = cache[self.nodes + j];
            let eta = cache[2 * self.nodes + j];
            b.products[self.nodes + j] = (u + eta) * v;
            b.products[2 * self.nodes + j] = (u + eta) * w;
        }
        let factor = self.ea * self.length / (2 * self.nodes) as f64;
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
    pub(crate) fn prepare_batch4(&mut self) {
        self.batch4 = Some(BatchCosine::new(
            &self.plan,
            &self.ku,
            &self.kv,
            self.ea * self.length / (2 * self.nodes) as f64,
        ));
    }
    pub(crate) fn legacy_batch_packing(&mut self) {
        if let Some(batch) = &mut self.batch4 {
            batch.packing = None;
        }
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
                    + b.packing.as_ref().map(|p| p.prepared_bytes()).unwrap_or(0)
                    + b.transverse_cache.capacity() * std::mem::size_of::<Quad>()
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

struct SynthesisCell {
    source: [usize; 4],
    weights: [[f64; 2]; 4],
    count: usize,
    target: usize,
}
struct AnalysisCell {
    a: usize,
    b: usize,
    weights: [f64; 4],
}
struct PreparedPacking {
    synthesis: [Vec<SynthesisCell>; 2],
    analysis: [Vec<AnalysisCell>; 2],
}
impl PreparedPacking {
    fn new(plan: &CosinePlan, ku: &[f64], kv: &[f64], factor: f64) -> Self {
        let synthesis = [Self::synthesis(plan, ku), Self::synthesis(plan, kv)];
        let analysis = [
            Self::analysis(plan, ku, factor),
            Self::analysis(plan, kv, factor),
        ];
        Self {
            synthesis,
            analysis,
        }
    }
    fn prepared_bytes(&self) -> usize {
        self.synthesis
            .iter()
            .map(|v| v.capacity() * std::mem::size_of::<SynthesisCell>())
            .sum::<usize>()
            + self
                .analysis
                .iter()
                .map(|v| v.capacity() * std::mem::size_of::<AnalysisCell>())
                .sum::<usize>()
    }
    fn synthesis(plan: &CosinePlan, scales: &[f64]) -> Vec<SynthesisCell> {
        let n = plan.nodes;
        let m = n / 2;
        let mut cells = Vec::with_capacity(m);
        for k in 0..m {
            // A real-FFT cell depends on at most four modal coordinates.
            // Duplicate indices (notably k=0) represent one basis vector.
            let mut source = [usize::MAX; 4];
            let mut count = 0;
            for mode in [k, n - k, k + m, m - k] {
                if mode > 0 && mode <= scales.len() && !source[..count].contains(&(mode - 1)) {
                    source[count] = mode - 1;
                    count += 1;
                }
            }
            let mut weights = [[0.; 2]; 4];
            for tap in 0..count {
                let spectrum = |index: usize| {
                    let a = if index > 0 && index <= scales.len() && index - 1 == source[tap] {
                        n as f64 * 0.5 * scales[index - 1]
                    } else {
                        0.
                    };
                    let mirror = n - index;
                    let b = if index > 0 && mirror <= scales.len() && mirror - 1 == source[tap] {
                        n as f64 * 0.5 * scales[mirror - 1]
                    } else {
                        0.
                    };
                    let p = plan.phase[index];
                    (a * p.r + b * p.i, a * p.i - b * p.r)
                };
                let x = spectrum(k);
                let y = spectrum(k + m);
                let w = plan.real_roots[k];
                let dr = x.0 - y.0;
                let di = x.1 - y.1;
                weights[tap] = [
                    0.5 * (x.0 + y.0 + w.i * dr - w.r * di),
                    0.5 * (x.1 + y.1 + w.r * dr + w.i * di),
                ];
            }
            cells.push(SynthesisCell {
                source,
                weights,
                count,
                target: plan.reverse[k],
            });
        }
        cells
    }
    fn analysis(plan: &CosinePlan, scales: &[f64], factor: f64) -> Vec<AnalysisCell> {
        let m = plan.nodes / 2;
        scales
            .iter()
            .enumerate()
            .map(|(index, &scale)| {
                let k = index + 1;
                let w = plan.real_roots[k];
                let p = plan.phase[k];
                let weights = std::array::from_fn(|component| {
                    let mut basis = [0.; 4];
                    basis[component] = 1.;
                    let [ar, ai, br, bi] = basis;
                    let xr = 0.5 * (ar + br + w.r * (ai + bi) + w.i * (ar - br));
                    let xi = 0.5 * (ai - bi + w.r * (br - ar) + w.i * (ai + bi));
                    2. * factor * scale * (xr * p.r + xi * p.i)
                });
                AnalysisCell {
                    a: k % m,
                    b: (m - k % m) % m,
                    weights,
                }
            })
            .collect()
    }
}
