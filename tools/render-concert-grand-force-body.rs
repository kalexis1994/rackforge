//! Offline one-way bridge-force observation through the existing loaded body.
#[allow(dead_code)]
#[path = "concert-grand-pressure-body.rs"]
mod body;
use std::{
    env, fs,
    io::{BufWriter, Write},
    time::Instant,
};
fn main() {
    let args = env::args().collect::<Vec<_>>();
    let mut body = body::PressureBody::load(&args[1]);
    let ports = args[3].parse::<usize>().unwrap();
    assert_eq!(ports, body.latent_ports());
    let input = fs::read(&args[2]).unwrap();
    assert!(ports > 0 && input.len() % (8 * ports) == 0);
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[4])
        .unwrap();
    let mut output = BufWriter::new(file);
    let mut force = vec![0.; ports];
    let mut velocity = force.clone();
    let start = Instant::now();
    let mut peak = 0f64;
    for frame in input.chunks_exact(8 * ports) {
        for (f, b) in force.iter_mut().zip(frame.chunks_exact(8)) {
            *f = f64::from_le_bytes(b.try_into().unwrap());
            assert!(f.is_finite());
        }
        let p = body.step_latent(&force, &mut velocity);
        for x in p {
            assert!(x.is_finite());
            peak = peak.max(x.abs());
            output.write_all(&x.to_le_bytes()).unwrap();
        }
    }
    output.flush().unwrap();
    println!(
        "{{\"frames\":{},\"rate_hz\":{},\"seconds\":{},\"peak_pa\":{},\"body_feedback\":false}}",
        input.len() / (8 * ports),
        body.sample_rate(),
        start.elapsed().as_secs_f64(),
        peak
    );
}
