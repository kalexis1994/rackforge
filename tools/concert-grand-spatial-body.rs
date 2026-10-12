// The producer expands the frozen modal consumer below, changing its
// leading documentation comment to an ordinary comment for inclusion.
mod backend {
    include!("concert-grand-modal-body.rs");

    pub fn spatial_main() {
        let args = env::args().collect::<Vec<_>>();
        let mut body = ModalBody::load(&args[1]);
        let bytes = fs::read(&args[2]).unwrap();
        assert!(&bytes[..8] == b"RFPRT001");
        let external = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let latent = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        assert!(external > 0 && external <= 128 && latent == body.p);
        assert_eq!(bytes.len(), 16+8*external*latent);
        let map = bytes[16..].chunks_exact(8).map(|v| f64::from_le_bytes(v.try_into().unwrap())).collect::<Vec<_>>();
        let mut transpose = vec![0.; map.len()];
        for row in 0..external { for col in 0..latent { transpose[col*external+row] = map[row*latent+col]; } }
        let mut f = vec![0.; external];
        let mut u = vec![0.; latent];
        let mut v = vec![0.; latent];
        let mut y = vec![0.; external];
        let step = |body: &mut ModalBody, f: &[f64], u: &mut [f64], v: &mut [f64], y: &mut [f64]| {
            for (value, row) in u.iter_mut().zip(transpose.chunks_exact(external)) { *value = dot(row, f); }
            body.step(u, v);
            for (value, row) in y.iter_mut().zip(map.chunks_exact(latent)) { *value = dot(row, v); }
        };
        if args.get(3).map(String::as_str) == Some("verify") {
            let mut checksum = 0.;
            let mut work = 0.;
            for frame in 0..1200 {
                for (p, value) in f.iter_mut().enumerate() { *value = force(frame, p); }
                step(&mut body, &f, &mut u, &mut v, &mut y);
                work += body.dt*dot(&f, &y);
                checksum += y.iter().enumerate().map(|(p, v)| (p+1) as f64*v).sum::<f64>();
            }
            println!("{{\"real\":{:?},\"imag\":{:?},\"checksum\":{:?},\"work_j\":{:?}}}", body.real, body.imag, checksum, work);
            return;
        }
        let frames = 256;
        let blocks = 2048;
        let input = (0..frames*external).map(|i| force(i/external, i%external)).collect::<Vec<_>>();
        for _ in 0..32 { for f in input.chunks_exact(external) { step(&mut body, f, &mut u, &mut v, &mut y); } }
        let mut times = Vec::with_capacity(blocks);
        let allocations = COUNT.load(Ordering::Relaxed);
        for _ in 0..blocks {
            let started = Instant::now();
            for f in input.chunks_exact(external) { step(&mut body, black_box(f), &mut u, &mut v, black_box(&mut y)); }
            black_box(&body.real);
            times.push(started.elapsed().as_secs_f64()*1000.);
        }
        let allocations = COUNT.load(Ordering::Relaxed)-allocations;
        let mean = times.iter().sum::<f64>()/blocks as f64;
        times.sort_by(f64::total_cmp);
        println!("{{\"resonators\":{},\"latent_ports\":{},\"external_ports\":{},\"rate_hz\":{},\"frames_per_buffer\":{},\"buffers\":{},\"deadline_ms\":{},\"mean_ms\":{},\"p50_ms\":{},\"p95_ms\":{},\"p99_ms\":{},\"max_ms\":{},\"callback_allocations\":{},\"checksum\":{:?}}}", body.n, latent, external, 1./body.dt, frames, blocks, frames as f64*body.dt*1000., mean, times[blocks/2], times[blocks*95/100], times[blocks*99/100], times[blocks-1], allocations, dot(&body.real, &body.real));
    }
}
fn main() { backend::spatial_main(); }
