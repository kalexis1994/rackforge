//! Native coefficient consumer / callback budget probe, outside the factory.
//! No allocation, solve, exp, file access or locks inside Body::step.
use std::{env, fs, hint::black_box, time::Instant};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountAlloc;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;

struct Body {
    n: usize,
    p: usize,
    dt: f64,
    e: Vec<f64>,
    g: Vec<f64>,
    l: Vec<f64>,
    t: Vec<f64>,
    b: Vec<f64>,
    state: Vec<f64>,
    next: Vec<f64>,
    integral: Vec<f64>,
}

impl Body {
    fn load(path: &str) -> Self {
        let bytes = fs::read(path).expect("coefficient file");
        assert!(bytes.len() >= 24 && &bytes[..8] == b"RFBDY001", "coefficient version");
        let n = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let p = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let dt = f64::from_le_bytes(bytes[16..24].try_into().unwrap());
        assert!(n > 0 && n <= 512 && p > 0 && p <= 128 && dt.is_finite() && dt > 0.);
        assert_eq!(bytes.len(), 24 + 8 * (2*n*n + 3*n*p));
        let mut cursor = 24;
        let mut read = |count| -> Vec<f64> {
            let result = bytes[cursor..cursor+8*count].chunks_exact(8)
                .map(|v| f64::from_le_bytes(v.try_into().unwrap())).collect::<Vec<_>>();
            assert!(result.iter().all(|v| v.is_finite()));
            cursor += 8*count;
            result
        };
        Self { n, p, dt, e: read(n*n), g: read(n*p), l: read(n*n),
            t: read(n*p), b: read(n*p), state: vec![0.; n],
            next: vec![0.; n], integral: vec![0.; n] }
    }

    fn step(&mut self, force: &[f64], velocity: &mut [f64]) {
        for row in 0..self.n {
            let mut next = 0.;
            let mut integral = 0.;
            for col in 0..self.n {
                next += self.e[row*self.n+col]*self.state[col];
                integral += self.l[row*self.n+col]*self.state[col];
            }
            for col in 0..self.p {
                next += self.g[row*self.p+col]*force[col];
                integral += self.t[row*self.p+col]*force[col];
            }
            self.next[row] = next;
            self.integral[row] = integral;
        }
        velocity.fill(0.);
        for row in 0..self.n {
            let average = self.integral[row]/self.dt;
            for col in 0..self.p {
                velocity[col] += self.b[row*self.p+col]*average;
            }
        }
        std::mem::swap(&mut self.state, &mut self.next);
    }

    fn energy(&self) -> f64 {
        self.state.iter().map(|v| v*v/2.).sum()
    }
}

fn force(frame: usize, port: usize) -> f64 {
    if frame < 400 { (((frame*17+port*13)%97) as f64-48.)/48. } else { 0. }
}

fn main() {
    let args = env::args().collect::<Vec<_>>();
    let mut body = Body::load(&args[1]);
    let mut f = vec![0.; body.p];
    let mut y = vec![0.; body.p];
    if args.get(2).map(String::as_str) == Some("verify") {
        let mut checksum = 0.;
        let mut work = 0.;
        let mut maximum_excess: f64 = 0.;
        let mut minimum_loss = f64::INFINITY;
        for frame in 0..1200 {
            for (port, value) in f.iter_mut().enumerate() { *value = force(frame, port); }
            let before = body.energy();
            body.step(&f, &mut y);
            let injected = body.dt*f.iter().zip(&y).map(|(f, y)| f*y).sum::<f64>();
            let loss = injected - (body.energy()-before);
            minimum_loss = minimum_loss.min(loss);
            maximum_excess = maximum_excess.max(-loss);
            work += injected;
            checksum += y.iter().enumerate().map(|(p, v)| (p+1) as f64*v).sum::<f64>();
        }
        println!("{{\"state\":{:?},\"checksum\":{:?},\"work_j\":{:?},\"energy_j\":{:?},\"minimum_step_loss_j\":{:?},\"maximum_energy_excess_j\":{:?}}}",
            body.state, checksum, work, body.energy(), minimum_loss, maximum_excess);
        return;
    }
    let frames = 256;
    let blocks = 2048;
    let inputs = (0..frames*body.p).map(|i| force(i/body.p, i%body.p)).collect::<Vec<_>>();
    for _ in 0..32 {
        for frame in 0..frames { body.step(&inputs[frame*body.p..(frame+1)*body.p], &mut y); }
    }
    let mut elapsed = Vec::with_capacity(blocks);
    let allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
    for _ in 0..blocks {
        let begun = Instant::now();
        for frame in 0..frames {
            body.step(black_box(&inputs[frame*body.p..(frame+1)*body.p]), black_box(&mut y));
        }
        black_box(&body.state);
        elapsed.push(begun.elapsed().as_secs_f64()*1000.);
    }
    let allocations = ALLOCATIONS.load(Ordering::Relaxed)-allocations_before;
    let mean = elapsed.iter().sum::<f64>()/blocks as f64;
    elapsed.sort_by(f64::total_cmp);
    println!("{{\"state_count\":{},\"ports\":{},\"rate_hz\":{},\"frames_per_buffer\":{},\"buffers\":{},\"deadline_ms\":{},\"mean_ms\":{},\"p50_ms\":{},\"p95_ms\":{},\"p99_ms\":{},\"max_ms\":{},\"callback_allocations\":{},\"checksum\":{:?}}}",
        body.n, body.p, 1./body.dt, frames, blocks, frames as f64*body.dt*1000., mean,
        elapsed[blocks/2], elapsed[blocks*95/100], elapsed[blocks*99/100], elapsed[blocks-1], allocations, body.energy());
}
