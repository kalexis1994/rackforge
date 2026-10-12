// One reciprocal complex spatial row, with fused arithmetic (no fast-math).
use std::{env, fs, hint::black_box, time::Instant};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
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
}
impl ModalBody {
    fn load(path: &str) -> Self {
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
        Self { n, p, dt, c: read(8*n), br: read(n*p), bi: read(n*p), factor: read(n), real: vec![0.; n], imag: vec![0.; n] }
    }
    fn step(&mut self, force: &[f64], out: &mut [f64]) {
        out.fill(0.);
        for mode in 0..self.n {
            let span = mode*self.p..(mode+1)*self.p;
            let br = &self.br[span.clone()];
            let bi = &self.bi[span];
            let ur = dot(br, force);
            let ui = dot(bi, force);
            let c = &self.c[8*mode..8*(mode+1)];
            let r = self.real[mode];
            let i = self.imag[mode];
            let ar = c[2]*r-c[3]*i+c[6]*ur-c[7]*ui;
            let ai = c[2]*i+c[3]*r+c[6]*ui+c[7]*ur;
            self.real[mode] = c[0]*r-c[1]*i+c[4]*ur-c[5]*ui;
            self.imag[mode] = c[0]*i+c[1]*r+c[4]*ui+c[5]*ur;
            let ar = self.factor[mode]*ar;
            let ai = self.factor[mode]*ai;
            for ((y, br), bi) in out.iter_mut().zip(br).zip(bi) { *y = br.mul_add(ar, (-bi).mul_add(ai, *y)); }
        }
    }
}
fn force(frame: usize, port: usize) -> f64 {
    if frame < 400 { (((frame*17+port*13)%97) as f64-48.)/48. } else { 0. }
}
