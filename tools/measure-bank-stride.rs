//! Does sweeping the resonator banks cost arithmetic, or cache?
//!
//! Concert Grand keeps its modes as an array of structs and sweeps every one
//! of them every sample. `BodyMode` is 88 bytes and the banks together are
//! 65 KB, against a Cortex-A72's 32 KB of L1 -- but the fields the per-sample
//! loop actually reads come to 32 bytes, which would fit.
//!
//! This is that question with nothing else in it: the same arithmetic, the
//! same number of modes, the same access order, and only the STRIDE changed.
//! If the time tracks the stride the loop is paying for memory and packing
//! the structs is worth a refactor. If it is flat, it is paying for flops
//! and the packing buys nothing.
//!
//! Standalone on purpose: it has to run on the appliance, where the
//! workspace does not build in a minute.
//!
//!     scp tools/measure-bank-stride.rs pi:/tmp/
//!     ssh pi 'cd /tmp && nice -n 19 rustc -O -C target-cpu=native 
//!       measure-bank-stride.rs -o measure-bank-stride && nice -n 19 ./measure-bank-stride'

use std::time::Instant;

#[derive(Clone, Copy)]
#[repr(C)]
struct Mode<const PAD: usize> {
    y1: f32,
    y2: f32,
    a1: f32,
    a2: f32,
    drive: f32,
    velocity: f32,
    pan_left: f32,
    pan_right: f32,
    /// Bytes the loop never reads -- exactly what `BodyMode` carries today
    /// for the banks that do not use them.
    _cold: [f32; PAD],
}

impl<const PAD: usize> Mode<PAD> {
    fn new(index: usize, count: usize) -> Self {
        // A plausible bank: 45 Hz to 8.5 kHz, T60 falling with frequency.
        let t = index as f32 / count as f32;
        let hz = 45.0 * (8500.0f32 / 45.0).powf(t);
        let rate = 48_000.0f32;
        let t60 = 3.0 / (1.0 + 8.0 * t);
        let r = (-6.907_755 / (t60 * rate)).exp();
        let omega = std::f32::consts::TAU * hz / rate;
        Self {
            y1: 0.0,
            y2: 0.0,
            a1: 2.0 * r * omega.cos(),
            a2: -r * r,
            drive: (1.0 - r) * 2.0 * omega.sin(),
            velocity: 1.0 / (2.0 * (0.5 * omega).sin()),
            pan_left: 1.0 - t,
            pan_right: t,
            _cold: [0.0; PAD],
        }
    }
}

fn sweep<const PAD: usize>(count: usize, samples: usize) -> (f64, f32) {
    let mut bank: Vec<Mode<PAD>> = (0..count).map(|i| Mode::new(i, count)).collect();
    let stride = std::mem::size_of::<Mode<PAD>>();
    // Warm the caches, and give the resonators some state to carry.
    let mut sink = 0.0f32;
    for n in 0..4096 {
        let x = if n % 512 == 0 { 1.0 } else { 0.0 };
        for mode in bank.iter_mut() {
            let y = mode.a1 * mode.y1 + mode.a2 * mode.y2 + mode.drive * x;
            let v = (y - mode.y1) * mode.velocity;
            sink += v * mode.pan_left;
            mode.y2 = mode.y1;
            mode.y1 = y;
        }
    }
    let start = Instant::now();
    for n in 0..samples {
        let x = if n % 512 == 0 { 1.0 } else { 0.0 };
        let mut left = 0.0f32;
        let mut right = 0.0f32;
        for mode in bank.iter_mut() {
            let y = mode.a1 * mode.y1 + mode.a2 * mode.y2 + mode.drive * x;
            let v = (y - mode.y1) * mode.velocity;
            left += v * mode.pan_left;
            right += v * mode.pan_right;
            mode.y2 = mode.y1;
            mode.y1 = y;
        }
        sink += left + right;
    }
    let elapsed = start.elapsed().as_secs_f64();
    // Nanoseconds per mode per sample, and the sink so nothing is elided.
    let per = elapsed * 1e9 / (samples as f64 * count as f64);
    println!(
        "  {stride:>4} bytes/modo  {:>6.1} KB de banco  {per:>6.2} ns por modo y muestra",
        (stride * count) as f32 / 1024.0
    );
    (per, sink)
}

fn main() {
    // The banks Concert Grand sweeps: board 256, undamped 192, silent 256,
    // bed 40, open 14.
    let count = 256 + 192 + 256 + 40 + 14;
    let samples = 48_000 * 2;
    println!("{count} modos, {} muestras, un solo hilo", samples);
    println!();
    println!("lo que el lazo lee de verdad son 32 bytes; el resto es lastre:");
    let mut sink = 0.0f32;
    let (p0, s) = sweep::<0>(count, samples);   //  32 B  -- empaquetado
    sink += s;
    let (p1, s) = sweep::<6>(count, samples);   //  56 B
    sink += s;
    let (p2, s) = sweep::<14>(count, samples);  //  88 B  -- BodyMode hoy
    sink += s;
    let (p3, s) = sweep::<30>(count, samples);  // 152 B
    sink += s;
    println!();
    println!("{:>10} {:>12} {:>10}", "bytes", "ns/modo", "x contra 32");
    for (bytes, per) in [(32, p0), (56, p1), (88, p2), (152, p3)] {
        println!("{bytes:>10} {per:>12.2} {:>10.2}", per / p0);
    }
    println!();
    println!("  si el tiempo sigue a los bytes -> memoria, y empaquetar paga");
    println!("  si es plano -> flops, y empaquetar no compra nada");
    if sink.is_nan() {
        println!("(imposible)");
    }
}
