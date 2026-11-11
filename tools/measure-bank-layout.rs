//! The split as it was actually made, timed: one 88-byte struct for every
//! bank, against a 32-byte sympathetic resonator and a 60-byte board mode.
//!
//! Same arithmetic in both cases -- the board ticks to two capsules through
//! a shape read from a sixteen-point transform, the sympathetic banks tick
//! to a pan -- and only the layout differs.
//!
//! Standalone on purpose: it has to run on the appliance, where the
//! workspace does not build in a minute.
//!
//!     scp tools/measure-bank-layout.rs pi:/tmp/
//!     ssh pi 'cd /tmp && nice -n 19 rustc -O -C target-cpu=native 
//!       measure-bank-layout.rs -o measure-bank-layout && nice -n 19 ./measure-bank-layout'

use std::time::Instant;

const POINTS: usize = 16;
const BOARD: usize = 256;
const SYMPATHETIC: usize = 192 + 256 + 40 + 14;

/// Everything in one struct, as it was: 88 bytes for every bank.
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Fat {
    y1: f32, y2: f32, a1: f32, a2: f32, drive: f32, velocity: f32,
    pan_left: f32, pan_right: f32,
    shape_q: usize, shape_a: f32, shape_b: f32, shape_c: f32,
    shape_theta: f32, shape_qy: f32, shape_theta_y: f32, omega: f32,
    out_y: [f32; 2], out_y1: [f32; 2], v1: f32,
}

/// The sympathetic resonator after the split.
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Slim {
    y1: f32, y2: f32, a1: f32, a2: f32, drive: f32, velocity: f32,
    pan_left: f32, pan_right: f32,
}

/// The board mode after the split.
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Board {
    y1: f32, y2: f32, a1: f32, a2: f32, drive: f32, velocity: f32,
    shape_q: u16, shape_a: f32, shape_b: f32, shape_c: f32,
    out_y: [f32; 2], out_y1: [f32; 2], v1: f32,
}

fn coefficients(index: usize, count: usize) -> (f32, f32, f32, f32, f32) {
    let t = index as f32 / count as f32;
    let hz = 45.0 * (8500.0f32 / 45.0).powf(t);
    let rate = 48_000.0f32;
    let r = (-6.907_755 / ((3.0 / (1.0 + 8.0 * t)) * rate)).exp();
    let omega = std::f32::consts::TAU * hz / rate;
    (2.0 * r * omega.cos(), -r * r, (1.0 - r) * 2.0 * omega.sin(),
     1.0 / (2.0 * (0.5 * omega).sin()), t)
}

macro_rules! fill {
    ($ty:ty, $count:expr, $extra:expr) => {{
        let mut v: Vec<$ty> = Vec::with_capacity($count);
        for i in 0..$count {
            let (a1, a2, drive, velocity, t) = coefficients(i, $count);
            let mut m = <$ty>::default();
            m.a1 = a1; m.a2 = a2; m.drive = drive; m.velocity = velocity;
            $extra(&mut m, t, i);
            v.push(m);
        }
        v
    }};
}

fn run(label: &str, samples: usize, fat: bool) -> f64 {
    let mut cos_t = [0.0f32; POINTS];
    let mut sin_t = [0.0f32; POINTS];
    for q in 0..POINTS {
        cos_t[q] = (q as f32 * 0.37).cos();
        sin_t[q] = (q as f32 * 0.37).sin();
    }
    let mut sink = 0.0f32;
    let bytes;
    let start;
    if fat {
        let mut board = fill!(Fat, BOARD, |m: &mut Fat, _t: f32, i: usize| {
            m.shape_q = i % POINTS; m.shape_a = 0.0; m.shape_b = 1.0; m.shape_c = 0.5;
            m.out_y = [0.7, 0.3];
        });
        let mut symp = fill!(Fat, SYMPATHETIC, |m: &mut Fat, t: f32, _i: usize| {
            m.pan_left = 1.0 - t; m.pan_right = t;
        });
        bytes = (board.len() + symp.len()) * std::mem::size_of::<Fat>();
        for _ in 0..2048 { step_fat(&mut board, &mut symp, &cos_t, &sin_t, 1.0, &mut sink); }
        start = Instant::now();
        for n in 0..samples {
            let x = if n % 512 == 0 { 1.0 } else { 0.0 };
            step_fat(&mut board, &mut symp, &cos_t, &sin_t, x, &mut sink);
        }
    } else {
        let mut board = fill!(Board, BOARD, |m: &mut Board, _t: f32, i: usize| {
            m.shape_q = (i % POINTS) as u16; m.shape_a = 0.0; m.shape_b = 1.0; m.shape_c = 0.5;
            m.out_y = [0.7, 0.3];
        });
        let mut symp = fill!(Slim, SYMPATHETIC, |m: &mut Slim, t: f32, _i: usize| {
            m.pan_left = 1.0 - t; m.pan_right = t;
        });
        bytes = board.len() * std::mem::size_of::<Board>()
            + symp.len() * std::mem::size_of::<Slim>();
        for _ in 0..2048 { step_slim(&mut board, &mut symp, &cos_t, &sin_t, 1.0, &mut sink); }
        start = Instant::now();
        for n in 0..samples {
            let x = if n % 512 == 0 { 1.0 } else { 0.0 };
            step_slim(&mut board, &mut symp, &cos_t, &sin_t, x, &mut sink);
        }
    }
    let each = start.elapsed().as_secs_f64() * 1e6 / (samples as f64 / 128.0);
    println!("  {label:<22} {:>6.1} KB   {each:>7.1} us por bloque de 128",
             bytes as f32 / 1024.0);
    if sink.is_nan() { println!("(imposible)"); }
    each
}

#[inline(always)]
fn step_fat(board: &mut [Fat], symp: &mut [Fat], cos_t: &[f32; POINTS],
            sin_t: &[f32; POINTS], x: f32, sink: &mut f32) {
    let (mut l, mut r) = (0.0f32, 0.0f32);
    for m in board.iter_mut() {
        let input = m.shape_a * cos_t[0] + m.shape_b * cos_t[m.shape_q] + m.shape_c * sin_t[m.shape_q];
        let y = m.a1 * m.y1 + m.a2 * m.y2 + m.drive * (input + x);
        let v = (y - m.y1) * m.velocity;
        l += v * m.out_y[0] + m.v1 * m.out_y1[0];
        r += v * m.out_y[1] + m.v1 * m.out_y1[1];
        m.y2 = m.y1; m.y1 = y; m.v1 = v;
    }
    for m in symp.iter_mut() {
        let y = m.a1 * m.y1 + m.a2 * m.y2 + m.drive * x;
        let v = (y - m.y1) * m.velocity;
        l += v * m.pan_left; r += v * m.pan_right;
        m.y2 = m.y1; m.y1 = y;
    }
    *sink += l + r;
}

#[inline(always)]
fn step_slim(board: &mut [Board], symp: &mut [Slim], cos_t: &[f32; POINTS],
             sin_t: &[f32; POINTS], x: f32, sink: &mut f32) {
    let (mut l, mut r) = (0.0f32, 0.0f32);
    for m in board.iter_mut() {
        let q = m.shape_q as usize;
        let input = m.shape_a * cos_t[0] + m.shape_b * cos_t[q] + m.shape_c * sin_t[q];
        let y = m.a1 * m.y1 + m.a2 * m.y2 + m.drive * (input + x);
        let v = (y - m.y1) * m.velocity;
        l += v * m.out_y[0] + m.v1 * m.out_y1[0];
        r += v * m.out_y[1] + m.v1 * m.out_y1[1];
        m.y2 = m.y1; m.y1 = y; m.v1 = v;
    }
    for m in symp.iter_mut() {
        let y = m.a1 * m.y1 + m.a2 * m.y2 + m.drive * x;
        let v = (y - m.y1) * m.velocity;
        l += v * m.pan_left; r += v * m.pan_right;
        m.y2 = m.y1; m.y1 = y;
    }
    *sink += l + r;
}

fn main() {
    let samples = 48_000 * 3;
    println!("{BOARD} modos de tabla + {SYMPATHETIC} simpaticos, un hilo\n");
    let fat = run("todo en 88 bytes", samples, true);
    let slim = run("60 tabla / 32 simpatia", samples, false);
    println!("\n  -> {:.2}x mas rapido, {:.0} us por bloque menos", fat / slim, fat - slim);
}
