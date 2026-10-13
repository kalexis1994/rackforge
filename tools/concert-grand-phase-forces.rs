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
#[repr(C)]
struct Term {
    response: [[f64; 4]; 2],
    coefficient: f64,
    left: u16,
    right: u16,
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
            left: u16::try_from(left).expect("bounded modal index"),
            right: u16::try_from(right).expect("bounded modal index"),
            coefficient,
            response,
        }
    }
}

/// Stable output grouping retains the scalar summation order within a mode.
/// Only prepared indices are compressed; every physical mode remains f64.
#[repr(C, align(32))]
struct Response([[f64; 4]; 2]);
struct TermTables {
    responses: Vec<Response>,
    indices: Vec<[u16; 2]>,
    // Used only by the point-gradient diagnostic, outside the timed callback.
    coefficients: Vec<f64>,
}
impl TermTables {
    fn len(&self) -> usize {
        self.responses.len()
    }
    fn bytes(&self) -> usize {
        self.responses.capacity() * std::mem::size_of::<Response>()
            + self.indices.capacity() * std::mem::size_of::<[u16; 2]>()
            + self.coefficients.capacity() * std::mem::size_of::<f64>()
    }
    fn hot_bytes(&self) -> usize {
        self.responses.capacity() * std::mem::size_of::<Response>()
            + self.indices.capacity() * std::mem::size_of::<[u16; 2]>()
    }
    fn range(&self, range: &[usize]) -> impl Iterator<Item = (&Response, &[u16; 2])> {
        self.responses[range[0]..range[1]]
            .iter()
            .zip(&self.indices[range[0]..range[1]])
    }
    fn points(&self, range: &[usize]) -> impl Iterator<Item = (&f64, &[u16; 2])> {
        self.coefficients[range[0]..range[1]]
            .iter()
            .zip(&self.indices[range[0]..range[1]])
    }
}
fn group_terms(
    mut terms: Vec<(usize, Term)>,
    first: usize,
    count: usize,
) -> (TermTables, Vec<usize>) {
    terms.sort_by_key(|(output, _)| *output);
    let mut offsets = vec![0; count + 1];
    let mut grouped = TermTables {
        responses: Vec::with_capacity(terms.len()),
        indices: Vec::with_capacity(terms.len()),
        coefficients: Vec::with_capacity(terms.len()),
    };
    for (output, term) in terms {
        assert!(output >= first && output < first + count);
        offsets[output - first + 1] += 1;
        grouped.responses.push(Response(term.response));
        grouped.indices.push([term.left, term.right]);
        grouped.coefficients.push(term.coefficient);
    }
    for i in 1..offsets.len() {
        offsets[i] += offsets[i - 1];
    }
    (grouped, offsets)
}

pub struct PhaseQuadratic {
    nt: usize,
    omega: Vec<f64>,
    axial: TermTables,
    transverse: TermTables,
    axial_offsets: Vec<usize>,
    transverse_offsets: Vec<usize>,
    amplitudes: Vec<f64>,
    simd: bool,
}
impl PhaseQuadratic {
    pub fn set_portable(&mut self) {
        self.simd = false;
    }
    /// All axial modes remain dynamic. Separate high-frequency homogeneous
    /// carriers from their slow forced part; low/resonant modes use sinc.
    pub fn axial_waveform(&self, times: &[f64]) -> AxialWaveform {
        let nl = self.axial_offsets.len() - 1;
        let maximum_transverse = self.omega[nl..nl + self.nt]
            .iter()
            .copied()
            .fold(0., f64::max);
        let high = (0..nl)
            .map(|j| self.omega[j] > 4. * maximum_transverse)
            .collect::<Vec<_>>();
        let mut initial_q = vec![[0.; 4]; self.axial.len()];
        let mut initial_v = initial_q.clone();
        let mut samples = Vec::with_capacity(times.len() * self.axial.len());
        for &t in times {
            for (output, range) in self.axial_offsets.windows(2).enumerate() {
                for k in range[0]..range[1] {
                    let [left, right] = self.axial.indices[k];
                    let a = self.omega[left as usize];
                    let b = self.omega[right as usize];
                    let sum = a + b;
                    let diff = a - b;
                    let coefficient = self.axial.coefficients[k];
                    let response;
                    if high[output] {
                        // omega_u > 2*max forcing frequency ensures both
                        // denominators exceed 3/4 omega_u^2. No near-resonance
                        // division or stiffness/mode removal occurs here.
                        let w2 = self.omega[output] * self.omega[output];
                        let ds = w2 - sum * sum;
                        let dd = w2 - diff * diff;
                        let (ss, cs) = (sum * t).sin_cos();
                        let (sd, cd) = (diff * t).sin_cos();
                        response = [
                            -0.5 * coefficient * (cs / ds + cd / dd),
                            -0.5 * coefficient * (ss / ds - sd / dd),
                            -0.5 * coefficient * (ss / ds + sd / dd),
                            -0.5 * coefficient * (cd / dd - cs / ds),
                        ];
                        initial_q[k] = [
                            -0.5 * coefficient * (1. / ds + 1. / dd),
                            0.,
                            0.,
                            -0.5 * coefficient * (1. / dd - 1. / ds),
                        ];
                        initial_v[k] = [
                            0.,
                            -0.5 * coefficient * (sum / ds - diff / dd),
                            -0.5 * coefficient * (sum / ds + diff / dd),
                            0.,
                        ];
                    } else {
                        let rs = harmonic_response(self.omega[output], sum, t)[0];
                        let rd = harmonic_response(self.omega[output], diff, t)[0];
                        response = [
                            -0.5 * coefficient * (rs[0] + rd[0]),
                            -0.5 * coefficient * (rs[1] - rd[1]),
                            -0.5 * coefficient * (rs[1] + rd[1]),
                            -0.5 * coefficient * (rd[0] - rs[0]),
                        ];
                    }
                    samples.push(response);
                }
            }
        }
        AxialWaveform {
            nt: self.nt,
            nl,
            omega: self.omega.clone(),
            indices: self.axial.indices.clone(),
            offsets: self.axial_offsets.clone(),
            initial_q,
            initial_v,
            samples,
            high,
            products: vec![[0.; 4]; self.axial.len()],
            amplitudes: vec![0.; self.omega.len()],
            simd: self.simd,
            sample_count: times.len(),
        }
    }
    pub fn new(
        nt: usize,
        nl: usize,
        omega: Vec<f64>,
        ku: &[f64],
        kv: &[f64],
        ea_length: f64,
        h: f64,
    ) -> Self {
        assert!(nt > 0 && nt <= 256 && nl > 0 && nl <= 512 && omega.len() == nl + 2 * nt);
        assert!(ku.len() == nl && kv.len() == nt);
        assert!(h.is_finite() && h > 0. && ea_length.is_finite() && ea_length > 0.);
        assert!(omega.iter().all(|w| w.is_finite() && *w > 0.));
        assert_eq!(&omega[nl..nl + nt], &omega[nl + nt..]);
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
                    axial.push((
                        j - 1,
                        Term::new(nl + k - 1, nl + l - 1, j - 1, coefficient, &omega, h),
                    ));
                    transverse.push((
                        nl + k - 1,
                        Term::new(
                            j - 1,
                            nl + l - 1,
                            nl + k - 1,
                            coefficient * if k == l { 2. } else { 1. },
                            &omega,
                            h,
                        ),
                    ));
                    if k != l {
                        transverse.push((
                            nl + l - 1,
                            Term::new(j - 1, nl + k - 1, nl + l - 1, coefficient, &omega, h),
                        ));
                    }
                }
            }
        }
        // Transverse terms share the axial left carrier across both axes.
        // Store [q_cc,q_cs,v_cc,v_cs] and [q_sc,q_ss,v_sc,v_ss] so SIMD
        // can contract the left carrier once, then apply each right axis.
        for (_, term) in &mut transverse {
            let r = term.response;
            term.response = [
                [r[0][0], r[0][1], r[1][0], r[1][1]],
                [r[0][2], r[0][3], r[1][2], r[1][3]],
            ];
        }
        let (axial, axial_offsets) = group_terms(axial, 0, nl);
        let (transverse, transverse_offsets) = group_terms(transverse, nl, nt);
        let size = omega.len();
        Self {
            nt,
            omega,
            axial,
            transverse,
            axial_offsets,
            transverse_offsets,
            amplitudes: vec![0.; size],
            simd: {
                #[cfg(target_arch = "x86_64")]
                {
                    std::arch::is_x86_feature_detected!("avx2")
                }
                #[cfg(not(target_arch = "x86_64"))]
                {
                    false
                }
            },
        }
    }
    pub fn terms(&self) -> usize {
        self.axial.len() + self.transverse.len()
    }
    pub fn prepared_bytes(&self) -> usize {
        self.axial.bytes()
            + self.transverse.bytes()
            + (self.omega.capacity() + self.amplitudes.capacity()) * std::mem::size_of::<f64>()
            + (self.axial_offsets.capacity() + self.transverse_offsets.capacity())
                * std::mem::size_of::<usize>()
    }
    pub fn callback_table_bytes(&self) -> usize {
        self.axial.hot_bytes()
            + self.transverse.hot_bytes()
            + (self.axial_offsets.capacity() + self.transverse_offsets.capacity())
                * std::mem::size_of::<usize>()
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
        #[cfg(target_arch = "x86_64")]
        if self.simd {
            // CPU dispatch is prepared in new(), never inferred from inputs.
            unsafe { self.contract_avx2(q, dq, dv) };
            return;
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
        for (output, range) in self.axial_offsets.windows(2).enumerate() {
            for (response, indices) in self.axial.range(range) {
                let (left, right) = (indices[0] as usize, indices[1] as usize);
                let a = products(left, right);
                let b = products(left + self.nt, right + self.nt);
                let combined = [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]];
                dq[output] -= dot4(&response.0[0], &combined);
                dv[output] -= dot4(&response.0[1], &combined);
            }
        }
        let nl = self.axial_offsets.len() - 1;
        for (mode, range) in self.transverse_offsets.windows(2).enumerate() {
            for (response, indices) in self.transverse.range(range) {
                let (left, right) = (indices[0] as usize, indices[1] as usize);
                // Restore the original expanded order for the portable control.
                let rq = [
                    response.0[0][0],
                    response.0[0][1],
                    response.0[1][0],
                    response.0[1][1],
                ];
                let rv = [
                    response.0[0][2],
                    response.0[0][3],
                    response.0[1][2],
                    response.0[1][3],
                ];
                for axis in [0, self.nt] {
                    let a = products(left, right + axis);
                    dq[nl + mode + axis] -= dot4(&rq, &a);
                    dv[nl + mode + axis] -= dot4(&rv, &a);
                }
            }
        }
    }
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn contract_avx2(&self, q: &[f64], dq: &mut [f64], dv: &mut [f64]) {
        use std::arch::x86_64::*;
        // apply() checks all state lengths. Private constructor-generated
        // indices satisfy: axial inputs in [nl,nl+nt), transverse left in
        // [0,nl), right in [nl,nl+nt); adding nt reaches the other axis.
        // Grouping copies these indices without altering them. Avoid checking
        // these established bounds separately for every product in the loop.
        #[inline(always)]
        unsafe fn state(q: &[f64], amplitudes: &[f64], index: usize) -> (f64, f64) {
            unsafe { (*q.get_unchecked(index), *amplitudes.get_unchecked(index)) }
        }
        #[target_feature(enable = "avx2")]
        unsafe fn pair(qi: f64, bi: f64, qj: f64, bj: f64) -> __m256d {
            _mm256_mul_pd(_mm256_set_pd(bi, bi, qi, qi), _mm256_set_pd(bj, qj, bj, qj))
        }
        #[target_feature(enable = "avx2")]
        unsafe fn reduce_pair(q: __m256d, v: __m256d) -> [f64; 2] {
            unsafe {
                // Pairwise sums may round differently from the scalar oracle.
                // Both products cover exactly four doubles without overread.
                let pairs = _mm256_hadd_pd(q, v);
                let total = _mm_add_pd(
                    _mm256_castpd256_pd128(pairs),
                    _mm256_extractf128_pd(pairs, 1),
                );
                let mut result = [0.; 2];
                _mm_storeu_pd(result.as_mut_ptr(), total);
                result
            }
        }
        unsafe {
            for (output, range) in self.axial_offsets.windows(2).enumerate() {
                // One horizontal reduction per output, rather than per term.
                // Response has 32-byte alignment and 64-byte stride: both
                // four-double rows are aligned and remain within the object.
                let mut sum_q = _mm256_setzero_pd();
                let mut sum_v = _mm256_setzero_pd();
                for (response, indices) in self.axial.range(range) {
                    let (left, right) = (indices[0] as usize, indices[1] as usize);
                    let (ql, bl) = state(q, &self.amplitudes, left);
                    let (qr, br) = state(q, &self.amplitudes, right);
                    let (wl, cl) = state(q, &self.amplitudes, left + self.nt);
                    let (wr, cr) = state(q, &self.amplitudes, right + self.nt);
                    let a = pair(ql, bl, qr, br);
                    let b = pair(wl, cl, wr, cr);
                    let products = _mm256_add_pd(a, b);
                    sum_q = _mm256_add_pd(
                        sum_q,
                        _mm256_mul_pd(_mm256_load_pd(response.0[0].as_ptr()), products),
                    );
                    sum_v = _mm256_add_pd(
                        sum_v,
                        _mm256_mul_pd(_mm256_load_pd(response.0[1].as_ptr()), products),
                    );
                }
                let result = reduce_pair(sum_q, sum_v);
                dq[output] -= result[0];
                dv[output] -= result[1];
            }
            let nl = self.axial_offsets.len() - 1;
            for (mode, range) in self.transverse_offsets.windows(2).enumerate() {
                let mut vertical = _mm256_setzero_pd();
                let mut horizontal = _mm256_setzero_pd();
                for (response, indices) in self.transverse.range(range) {
                    let (left, right) = (indices[0] as usize, indices[1] as usize);
                    let (ql, bl) = state(q, &self.amplitudes, left);
                    let factors = _mm256_add_pd(
                        _mm256_mul_pd(_mm256_load_pd(response.0[0].as_ptr()), _mm256_set1_pd(ql)),
                        _mm256_mul_pd(_mm256_load_pd(response.0[1].as_ptr()), _mm256_set1_pd(bl)),
                    );
                    let (qv, bv) = state(q, &self.amplitudes, right);
                    let (qw, bw) = state(q, &self.amplitudes, right + self.nt);
                    vertical = _mm256_add_pd(
                        vertical,
                        _mm256_mul_pd(factors, _mm256_set_pd(bv, qv, bv, qv)),
                    );
                    horizontal = _mm256_add_pd(
                        horizontal,
                        _mm256_mul_pd(factors, _mm256_set_pd(bw, qw, bw, qw)),
                    );
                }
                for (axis, products) in [(0, vertical), (self.nt, horizontal)] {
                    let total = _mm_hadd_pd(
                        _mm256_castpd256_pd128(products),
                        _mm256_extractf128_pd(products, 1),
                    );
                    let mut result = [0.; 2];
                    _mm_storeu_pd(result.as_mut_ptr(), total);
                    dq[nl + mode + axis] -= result[0];
                    dv[nl + mode + axis] -= result[1];
                }
            }
        }
    }
    /// Point gradient for verifying the spatial contraction separately.
    fn point(&self, q: &[f64], out: &mut [f64]) {
        out.fill(0.);
        for (output, range) in self.axial_offsets.windows(2).enumerate() {
            for (coefficient, indices) in self.axial.points(range) {
                let (left, right) = (indices[0] as usize, indices[1] as usize);
                out[output] +=
                    coefficient * (q[left] * q[right] + q[left + self.nt] * q[right + self.nt]);
            }
        }
        let nl = self.axial_offsets.len() - 1;
        for (mode, range) in self.transverse_offsets.windows(2).enumerate() {
            for (coefficient, indices) in self.transverse.points(range) {
                let (left, right) = (indices[0] as usize, indices[1] as usize);
                for axis in [0, self.nt] {
                    out[nl + mode + axis] += coefficient * q[left] * q[right + axis];
                }
            }
        }
    }
}

pub struct AxialWaveform {
    nt: usize,
    nl: usize,
    omega: Vec<f64>,
    indices: Vec<[u16; 2]>,
    offsets: Vec<usize>,
    initial_q: Vec<[f64; 4]>,
    initial_v: Vec<[f64; 4]>,
    samples: Vec<[f64; 4]>,
    high: Vec<bool>,
    products: Vec<[f64; 4]>,
    amplitudes: Vec<f64>,
    simd: bool,
    sample_count: usize,
}
impl AxialWaveform {
    pub fn prepared_bytes(&self) -> usize {
        (self.initial_q.capacity()
            + self.initial_v.capacity()
            + self.samples.capacity()
            + self.products.capacity())
            * std::mem::size_of::<[f64; 4]>()
            + self.omega.capacity() * 8
            + self.amplitudes.capacity() * 8
            + self.indices.capacity() * 4
            + self.offsets.capacity() * std::mem::size_of::<usize>()
            + self.high.capacity() * std::mem::size_of::<bool>()
    }
    pub fn high_modes(&self) -> usize {
        self.high.iter().filter(|x| **x).count()
    }
    pub fn set_portable(&mut self) {
        self.simd = false;
    }
    pub fn set_carriers(&mut self, q: &[f64], v: &[f64]) {
        assert!(q.len() == self.omega.len() && v.len() == q.len());
        // Each transverse carrier participates in many triads. Preserve the
        // same f64 division, but perform it once per mode, outside that loop.
        for j in self.nl..self.omega.len() {
            self.amplitudes[j] = v[j] / self.omega[j];
        }
        for (product, &[left, right]) in self.products.iter_mut().zip(&self.indices) {
            let (left, right) = (left as usize, right as usize);
            let (a, b) = (self.amplitudes[left], self.amplitudes[right]);
            let (c, d) = (
                self.amplitudes[left + self.nt],
                self.amplitudes[right + self.nt],
            );
            *product = [
                q[left] * q[right] + q[left + self.nt] * q[right + self.nt],
                q[left] * b + q[left + self.nt] * d,
                a * q[right] + c * q[right + self.nt],
                a * b + c * d,
            ];
        }
    }
    fn dot(a: &[f64; 4], b: &[f64; 4]) -> f64 {
        a.iter().zip(b).map(|(a, b)| a * b).sum()
    }
    pub fn initial_particular(&self, q: &mut [f64], v: &mut [f64]) {
        assert!(q.len() == self.nl && v.len() == self.nl);
        #[cfg(target_arch = "x86_64")]
        if self.simd {
            // simd is inherited from the prepared, runtime-checked phase plan.
            unsafe { self.initial_particular_avx2(q, v) };
            return;
        }
        q.fill(0.);
        v.fill(0.);
        for (j, range) in self.offsets.windows(2).enumerate() {
            for k in range[0]..range[1] {
                q[j] += Self::dot(&self.initial_q[k], &self.products[k]);
                v[j] += Self::dot(&self.initial_v[k], &self.products[k]);
            }
        }
    }
    pub fn sample(&self, index: usize, out: &mut [f64]) {
        assert!(out.len() == self.nl && index < self.sample_count);
        #[cfg(target_arch = "x86_64")]
        if self.simd {
            unsafe { self.sample_avx2(index, out) };
            return;
        }
        out.fill(0.);
        let start = index * self.products.len();
        for (j, range) in self.offsets.windows(2).enumerate() {
            for k in range[0]..range[1] {
                out[j] += Self::dot(&self.samples[start + k], &self.products[k]);
            }
        }
    }
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn initial_particular_avx2(&self, q: &mut [f64], v: &mut [f64]) {
        use std::arch::x86_64::*;
        unsafe {
            for (j, range) in self.offsets.windows(2).enumerate() {
                let mut sq = _mm256_setzero_pd();
                let mut sv = _mm256_setzero_pd();
                for k in range[0]..range[1] {
                    let p = _mm256_loadu_pd(self.products[k].as_ptr());
                    sq = _mm256_add_pd(
                        sq,
                        _mm256_mul_pd(p, _mm256_loadu_pd(self.initial_q[k].as_ptr())),
                    );
                    sv = _mm256_add_pd(
                        sv,
                        _mm256_mul_pd(p, _mm256_loadu_pd(self.initial_v[k].as_ptr())),
                    );
                }
                q[j] = Self::sum_lanes(sq);
                v[j] = Self::sum_lanes(sv);
            }
        }
    }
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn sample_avx2(&self, index: usize, out: &mut [f64]) {
        use std::arch::x86_64::*;
        let start = index * self.products.len();
        unsafe {
            for (j, range) in self.offsets.windows(2).enumerate() {
                let mut sum = _mm256_setzero_pd();
                for k in range[0]..range[1] {
                    sum = _mm256_add_pd(
                        sum,
                        _mm256_mul_pd(
                            _mm256_loadu_pd(self.products[k].as_ptr()),
                            _mm256_loadu_pd(self.samples[start + k].as_ptr()),
                        ),
                    );
                }
                out[j] = Self::sum_lanes(sum);
            }
        }
    }
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn sum_lanes(value: std::arch::x86_64::__m256d) -> f64 {
        use std::arch::x86_64::*;
        let pair = _mm_add_pd(
            _mm256_castpd256_pd128(value),
            _mm256_extractf128_pd(value, 1),
        );
        _mm_cvtsd_f64(_mm_hadd_pd(pair, pair))
    }
}

pub fn allocation_count() -> usize {
    ALLOCATIONS.load(Ordering::Relaxed)
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
    if args.iter().any(|arg| arg == "--portable") {
        for plan in &mut plans {
            plan.simd = false;
        }
    }
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
    println!("{{\"dq\":{:?},\"dv\":{:?},\"endpoint_quadratic_gradient\":{:?},\"omega\":{:?},\"prepared_terms\":{},\"prepared_bytes\":{},\"callback_table_bytes\":{},\"prepared_index_bits\":16,\"response_alignment_bytes\":32,\"grouped_outputs\":true,\"preparation_ms\":{},\"block_seconds\":{},\"frames\":{},\"buffers\":{},\"mean_ms\":{},\"p99_ms\":{},\"max_ms\":{},\"allocations\":{},\"simd\":{},\"nonlinear_step\":false}}", dq,dv,end_force,plans.iter().flat_map(|p|p.omega.iter()).collect::<Vec<_>>(),plans.iter().map(|p|p.terms()).sum::<usize>(),plans.iter().map(|p|p.prepared_bytes()).sum::<usize>(),plans.iter().map(|p|p.callback_table_bytes()).sum::<usize>(),preparation_ms,h,frames,buffers,mean,times[buffers*99/100],times[buffers-1],allocations,plans[0].simd);
}
