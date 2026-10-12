// Same RFRCM001 model, explicit AVX2 phase, no fast-math.
use std::{env, fs, hint::black_box, time::Instant};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::arch::x86_64::*;
struct CountAlloc;
static COUNT: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 { COUNT.fetch_add(1, Ordering::Relaxed); unsafe { System.alloc(l) } }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { unsafe { System.dealloc(p, l) } }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 { COUNT.fetch_add(1, Ordering::Relaxed); unsafe { System.realloc(p, l, n) } }
}
#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;
#[inline]
fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut sums = [0.; 4];
    for (a, b) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        for lane in 0..4 { sums[lane] = a[lane].mul_add(b[lane], sums[lane]); }
    }
    let mut result = (sums[0]+sums[1])+(sums[2]+sums[3]);
    for (a, b) in a.chunks_exact(4).remainder().iter().zip(b.chunks_exact(4).remainder()) { result = a.mul_add(*b, result); }
    result
}
struct ModalBody {
    n: usize, p: usize, dt: f64, c: Vec<f64>, br: Vec<f64>, bi: Vec<f64>,
    factor: Vec<f64>, real: Vec<f64>, imag: Vec<f64>,
    ur: Vec<f64>, ui: Vec<f64>, average_r: Vec<f64>, average_i: Vec<f64>,
}
impl ModalBody {
    fn load(path: &str) -> Self {
        assert!(is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma"), "native AVX2/FMA pilot required");
        let bytes = fs::read(path).unwrap();
        assert!(bytes.len() >= 24 && &bytes[..8] == b"RFRCM001");
        let n = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let p = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let dt = f64::from_le_bytes(bytes[16..24].try_into().unwrap());
        assert!(n > 0 && n <= 1212 && p > 0 && p <= 128 && dt.is_finite() && dt > 0.);
        assert_eq!(bytes.len(), 24+8*(9*n+2*n*p));
        let mut offset = 24;
        let mut read = |count| {
            let values = bytes[offset..offset+8*count].chunks_exact(8).map(|b| f64::from_le_bytes(b.try_into().unwrap())).collect::<Vec<_>>();
            assert!(values.iter().all(|v| v.is_finite()));
            offset += 8*count;
            values
        };
        let original_c = read(8*n);
        let original_br = read(n*p);
        let original_bi = read(n*p);
        let factor = read(n);
        let mut c = vec![0.; 8*n];
        let mut br = vec![0.; n*p];
        let mut bi = vec![0.; n*p];
        for mode in 0..n {
            for column in 0..8 { c[column*n+mode] = original_c[mode*8+column]; }
            for port in 0..p { br[port*n+mode] = original_br[mode*p+port]; bi[port*n+mode] = original_bi[mode*p+port]; }
        }
        Self { n, p, dt, c, br, bi, factor, real: vec![0.; n], imag: vec![0.; n],
               ur: vec![0.; n], ui: vec![0.; n], average_r: vec![0.; n], average_i: vec![0.; n] }
    }
    fn step(&mut self, force: &[f64], out: &mut [f64]) {
        // Feature support was checked at construction. Every pointer used
        // below belongs to this private object's fixed n-element buffers.
        unsafe { self.step_avx2(force, out); }
    }
    #[target_feature(enable="avx2,fma")]
    unsafe fn step_avx2(&mut self, force: &[f64], out: &mut [f64]) {
        self.ur.fill(0.);
        self.ui.fill(0.);
        // Long independent lanes; each modal input preserves port order.
        for ((br, bi), f) in self.br.chunks_exact(self.n).zip(self.bi.chunks_exact(self.n)).zip(force) {
            for (u, b) in self.ur.iter_mut().zip(br) { *u = b.mul_add(*f, *u); }
            for (u, b) in self.ui.iter_mut().zip(bi) { *u = b.mul_add(*f, *u); }
        }
        let c = &self.c;
        let n = self.n;
        let mut mode = 0;
        while mode+4 <= n {
            let load = |column: usize| _mm256_loadu_pd(c.as_ptr().add(column*n+mode));
            let r = _mm256_loadu_pd(self.real.as_ptr().add(mode));
            let i = _mm256_loadu_pd(self.imag.as_ptr().add(mode));
            let ur = _mm256_loadu_pd(self.ur.as_ptr().add(mode));
            let ui = _mm256_loadu_pd(self.ui.as_ptr().add(mode));
            let factor = _mm256_loadu_pd(self.factor.as_ptr().add(mode));
            let ar = _mm256_fnmadd_pd(load(7), ui, _mm256_fmadd_pd(load(6), ur,
                         _mm256_fnmadd_pd(load(3), i, _mm256_mul_pd(load(2), r))));
            let ai = _mm256_fmadd_pd(load(7), ur, _mm256_fmadd_pd(load(6), ui,
                         _mm256_fmadd_pd(load(3), r, _mm256_mul_pd(load(2), i))));
            let nr = _mm256_fnmadd_pd(load(5), ui, _mm256_fmadd_pd(load(4), ur,
                         _mm256_fnmadd_pd(load(1), i, _mm256_mul_pd(load(0), r))));
            let ni = _mm256_fmadd_pd(load(5), ur, _mm256_fmadd_pd(load(4), ui,
                         _mm256_fmadd_pd(load(1), r, _mm256_mul_pd(load(0), i))));
            _mm256_storeu_pd(self.average_r.as_mut_ptr().add(mode), _mm256_mul_pd(factor, ar));
            _mm256_storeu_pd(self.average_i.as_mut_ptr().add(mode), _mm256_mul_pd(factor, ai));
            _mm256_storeu_pd(self.real.as_mut_ptr().add(mode), nr);
            _mm256_storeu_pd(self.imag.as_mut_ptr().add(mode), ni);
            mode += 4;
        }
        for mode in mode..n {
            let r = self.real[mode];
            let i = self.imag[mode];
            let ur = self.ur[mode];
            let ui = self.ui[mode];
            self.average_r[mode] = self.factor[mode]*(c[2*n+mode]*r-c[3*n+mode]*i+c[6*n+mode]*ur-c[7*n+mode]*ui);
            self.average_i[mode] = self.factor[mode]*(c[2*n+mode]*i+c[3*n+mode]*r+c[6*n+mode]*ui+c[7*n+mode]*ur);
            self.real[mode] = c[mode]*r-c[n+mode]*i+c[4*n+mode]*ur-c[5*n+mode]*ui;
            self.imag[mode] = c[mode]*i+c[n+mode]*r+c[4*n+mode]*ui+c[5*n+mode]*ur;
        }
        for ((y, br), bi) in out.iter_mut().zip(self.br.chunks_exact(n)).zip(self.bi.chunks_exact(n)) {
            *y = dot(br, &self.average_r)-dot(bi, &self.average_i);
        }
    }
}
fn force(frame: usize, port: usize) -> f64 {
    if frame < 400 { (((frame*17+port*13)%97) as f64-48.)/48. } else { 0. }
}

