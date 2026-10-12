//! Exact complex-pole realization, retaining nonproportional material loss.
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

// Four independent partial sums allow vectorization without fast-math.
#[inline]
fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut sums = [0.; 4];
    for (a, b) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        for lane in 0..4 { sums[lane] += a[lane]*b[lane]; }
    }
    let mut result = (sums[0]+sums[1])+(sums[2]+sums[3]);
    for (a, b) in a.chunks_exact(4).remainder().iter().zip(b.chunks_exact(4).remainder()) { result += a*b; }
    result
}
struct ModalBody {
    n: usize, p: usize, dt: f64, c: Vec<f64>, br: Vec<f64>, bi: Vec<f64>,
    cr: Vec<f64>, ci: Vec<f64>, real: Vec<f64>, imag: Vec<f64>,
}
impl ModalBody {
    fn load(path: &str) -> Self {
        let bytes = fs::read(path).unwrap();
        assert!(bytes.len() >= 24 && &bytes[..8] == b"RFMDL001");
        let n = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let p = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let dt = f64::from_le_bytes(bytes[16..24].try_into().unwrap());
        assert!(n > 0 && n <= 1212 && p > 0 && p <= 128 && dt.is_finite() && dt > 0.);
        assert_eq!(bytes.len(), 24+8*(8*n+4*n*p));
        let mut offset = 24;
        let mut read = |count| {
            let values = bytes[offset..offset+8*count].chunks_exact(8)
                .map(|b| f64::from_le_bytes(b.try_into().unwrap())).collect::<Vec<_>>();
            assert!(values.iter().all(|v| v.is_finite()));
            offset += 8*count;
            values
        };
        Self { n, p, dt, c: read(8*n), br: read(n*p), bi: read(n*p), cr: read(n*p), ci: read(n*p), real: vec![0.; n], imag: vec![0.; n] }
    }
    fn step(&mut self, force: &[f64], out: &mut [f64]) {
        out.fill(0.);
        for mode in 0..self.n {
            let span = mode*self.p..(mode+1)*self.p;
            let ur = dot(&self.br[span.clone()], force);
            let ui = dot(&self.bi[span.clone()], force);
            let c = &self.c[8*mode..8*(mode+1)];
            let r = self.real[mode];
            let i = self.imag[mode];
            let ar = c[2]*r-c[3]*i+c[6]*ur-c[7]*ui;
            let ai = c[2]*i+c[3]*r+c[6]*ui+c[7]*ur;
            self.real[mode] = c[0]*r-c[1]*i+c[4]*ur-c[5]*ui;
            self.imag[mode] = c[0]*i+c[1]*r+c[4]*ui+c[5]*ur;
            for ((y, cr), ci) in out.iter_mut().zip(&self.cr[span.clone()]).zip(&self.ci[span]) { *y += cr*ar-ci*ai; }
        }
    }
}
fn force(frame: usize, port: usize) -> f64 {
    if frame < 400 { (((frame*17+port*13)%97) as f64-48.)/48. } else { 0. }
}
fn main() {
    let args = env::args().collect::<Vec<_>>();
    let mut body = ModalBody::load(&args[1]);
    let mut f = vec![0.; body.p];
    let mut y = vec![0.; body.p];
    if args.get(2).map(String::as_str) == Some("verify") {
        let mut checksum = 0.;
        let mut work = 0.;
        for frame in 0..1200 {
            for (p, value) in f.iter_mut().enumerate() { *value = force(frame, p); }
            body.step(&f, &mut y);
            work += body.dt*dot(&f, &y);
            checksum += y.iter().enumerate().map(|(p, v)| (p+1) as f64*v).sum::<f64>();
        }
        println!("{{\"real\":{:?},\"imag\":{:?},\"checksum\":{:?},\"work_j\":{:?}}}", body.real, body.imag, checksum, work);
        return;
    }
    let frames = 256;
    let blocks = 2048;
    let inputs = (0..frames*body.p).map(|i| force(i/body.p, i%body.p)).collect::<Vec<_>>();
    for _ in 0..32 { for f in inputs.chunks_exact(body.p) { body.step(f, &mut y); } }
    let mut elapsed = Vec::with_capacity(blocks);
    let before = COUNT.load(Ordering::Relaxed);
    for _ in 0..blocks {
        let begin = Instant::now();
        for f in inputs.chunks_exact(body.p) { body.step(black_box(f), black_box(&mut y)); }
        black_box(&body.real);
        elapsed.push(begin.elapsed().as_secs_f64()*1000.);
    }
    let allocations = COUNT.load(Ordering::Relaxed)-before;
    let mean = elapsed.iter().sum::<f64>()/blocks as f64;
    elapsed.sort_by(f64::total_cmp);
    println!("{{\"resonators\":{},\"ports\":{},\"rate_hz\":{},\"frames_per_buffer\":{},\"buffers\":{},\"deadline_ms\":{},\"mean_ms\":{},\"p50_ms\":{},\"p95_ms\":{},\"p99_ms\":{},\"max_ms\":{},\"callback_allocations\":{},\"checksum\":{:?}}}", body.n, body.p, 1./body.dt, frames, blocks, frames as f64*body.dt*1000., mean, elapsed[blocks/2], elapsed[blocks*95/100], elapsed[blocks*99/100], elapsed[blocks-1], allocations, dot(&body.real, &body.real));
}
